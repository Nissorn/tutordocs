// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import contract from "./fixtures/reader-contract.json";
import wizardContract from "./fixtures/wizard-contract.json";
import generateContract from "./fixtures/generate-contract.json";
import { mountReader } from "./reader";
import { mountLibraryTools } from "./library-tools";

type Args = Record<string, unknown>;
type Handler = (args: Args) => unknown;

/** A promise the test settles by hand, to look at the page mid-job. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/** Sends one message down the Tauri channel the UI passed as `onProgress`. */
function emit(args: Args, index: number, message: unknown): void {
  const channel = args.onProgress as { id: number };
  const internals = (window as unknown as { __TAURI_INTERNALS__: { runCallback(id: number, data: unknown): void } })
    .__TAURI_INTERNALS__;
  internals.runCallback(channel.id, { index, message });
}

const calls: { cmd: string; args: Args }[] = [];

function backend(overrides: Record<string, Handler> = {}): void {
  const defaults: Record<string, Handler> = {
    list_courses: () => contract.list_courses,
    get_outline: () => wizardContract.outline_approved,
    list_chapters: () => contract.list_chapters,
    read_chapter: (args) =>
      args.chapterId === "cells" ? contract.read_chapter_ready : contract.read_chapter_placeholder,
    list_annotations: () => [],
    list_course_sources: () => [],
    save_annotation: (args) => ({ ...(args.mark as Args), createdAt: 1, updatedAt: 2 }),
  };
  mockIPC((cmd, raw) => {
    const args = (raw ?? {}) as Args;
    calls.push({ cmd, args });
    const handler = overrides[cmd] ?? defaults[cmd];
    if (!handler) throw new Error(`unexpected command ${cmd}`);
    return handler(args);
  });
}

function count(cmd: string): number {
  return calls.filter((c) => c.cmd === cmd).length;
}

let root: HTMLElement;

beforeEach(() => {
  document.body.innerHTML = '<div id="app"></div>';
  root = document.querySelector<HTMLElement>("#app")!;
  calls.length = 0;
});

afterEach(() => {
  clearMocks();
});

function doc(): HTMLElement {
  return root.querySelector<HTMLElement>("main.document")!;
}

function margin(): HTMLElement {
  return root.querySelector<HTMLElement>("aside.margin")!;
}

function chapterButton(id: string): HTMLButtonElement {
  return root.querySelector<HTMLButtonElement>(`nav button[data-chapter-id="${id}"]`)!;
}

function buttonNamed(name: string): HTMLButtonElement {
  const found = Array.from(root.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent === name);
  if (!found) throw new Error(`no button "${name}"`);
  return found;
}

const membranesReady = {
  id: "membranes",
  title: "Membranes",
  state: "ready",
  markdown: "# Membranes\n\nA membrane separates the cell from its surroundings.\n",
};

describe("opening a placeholder", () => {
  it("streams the job's progress, then renders the chapter and marks it written", async () => {
    const job = deferred<unknown>();
    let jobArgs: Args = {};
    backend({
      open_chapter: (args) => {
        jobArgs = args;
        return job.promise;
      },
    });
    const tools = mountLibraryTools(document.createElement("div"));
    await tools.ready;
    await mountReader(root, contract.list_courses[0], { tools });

    chapterButton("membranes").click();
    await vi.waitFor(() => expect(count("open_chapter")).toBe(1));
    expect(jobArgs).toMatchObject({ courseId: "bio101", chapterId: "membranes" });
    expect(doc().dataset.state).toBe("writing");

    const [started, tool] = generateContract.progress;
    emit(jobArgs, 0, started);
    emit(jobArgs, 1, tool);
    const status = doc().querySelector('[role="status"]')!;
    expect(status.textContent).toBe("Searching the library…");

    job.resolve(membranesReady);
    await vi.waitFor(() => expect(doc().dataset.state).toBe("ready"));
    expect(doc().querySelector(".prose")?.textContent).toContain("A membrane separates");
    expect(chapterButton("membranes").dataset.state).toBe("ready");
  });

  it("keeps the placeholder and says why when the job fails, and can try again", async () => {
    backend({
      open_chapter: () => {
        throw new Error("the agent job failed: job timed out; child killed");
      },
    });
    const tools = mountLibraryTools(document.createElement("div"));
    await tools.ready;
    await mountReader(root, contract.list_courses[0], { tools });

    chapterButton("membranes").click();
    await vi.waitFor(() => expect(doc().dataset.state).toBe("placeholder"));
    expect(doc().textContent).toContain("has not been written yet");
    expect(doc().querySelector('[role="alert"]')?.textContent).toContain("timed out");
    expect(chapterButton("membranes").dataset.state).toBe("placeholder");

    buttonNamed("Try again").click();
    await vi.waitFor(() => expect(count("open_chapter")).toBe(2));
  });

  it("never starts a job at launch, even when every chapter is a placeholder", async () => {
    backend({
      list_chapters: () => contract.list_chapters.map((c) => ({ ...c, placeholder: true })),
      open_chapter: () => membranesReady,
    });
    const tools = mountLibraryTools(document.createElement("div"));
    await tools.ready;
    await mountReader(root, contract.list_courses[0], { tools });

    expect(count("open_chapter")).toBe(0);
    expect(doc().dataset.state).toBe("empty");
  });
});

describe("rewriting a chapter", () => {
  const mark = (id: string, quote: string) => ({
    annotationId: id,
    kind: "highlight",
    color: "yellow",
    quote,
    note: "",
    selectors: [{ type: "TextQuoteSelector", exact: quote, prefix: "", suffix: "" }],
    resolution: "resolved",
    locator: 1,
    rects: [],
    createdAt: 1,
    updatedAt: 1,
  });
  const rewritten = {
    ...contract.read_chapter_ready,
    markdown: "# Cells\n\nThe cell is the basic unit of life, wrapped in a membrane.\n",
  };

  it("re-anchors the marks against the new text and lists the ones that no longer fit", async () => {
    const job = deferred<unknown>();
    const saved: Args[] = [];
    backend({
      list_annotations: () => [mark("kept", "basic unit"), mark("lost", "Nucleus")],
      save_annotation: (args) => {
        saved.push(args.mark as Args);
        return { ...(args.mark as Args), createdAt: 1, updatedAt: 2 };
      },
      regenerate_chapter: () => job.promise,
    });
    const tools = mountLibraryTools(document.createElement("div"));
    await tools.ready;
    await mountReader(root, contract.list_courses[0], { tools });
    await vi.waitFor(() => expect(root.querySelectorAll("mark.hl").length).toBe(2));

    buttonNamed("Rewrite chapter").click();
    expect(doc().querySelector(".rewrite-confirm-prompt")?.textContent).toContain("Rewrite this chapter?");
    buttonNamed("Cancel").click();
    expect(count("regenerate_chapter")).toBe(0);

    buttonNamed("Rewrite chapter").click();
    const docPage = root.querySelector<HTMLElement>("main.document")!;
    docPage.scrollTop = 150;
    buttonNamed("Confirm rewrite").click();
    await vi.waitFor(() => expect(count("regenerate_chapter")).toBe(1));
    expect(doc().querySelector(".prose")?.textContent).toContain("Nucleus");

    job.resolve(rewritten);
    await vi.waitFor(() => expect(doc().querySelector(".prose")?.textContent).toContain("wrapped in a membrane"));
    expect(docPage.scrollTop).toBe(150);
    await vi.waitFor(() => expect(margin().textContent).toContain("Needs attention"));
    const painted = Array.from(root.querySelectorAll<HTMLElement>("mark.hl"));
    expect(painted.map((m) => m.dataset.annotationId)).toEqual(["kept"]);
    expect(margin().querySelector(".margin-conflicts")?.textContent).toContain("Nucleus");
    const lastSaved = (id: string) => saved.filter((m) => m.annotationId === id).at(-1)?.resolution;
    await vi.waitFor(() => expect(lastSaved("lost")).toBe("unresolved"));
    expect(lastSaved("kept")).toBe("resolved");
    expect(count("delete_annotation")).toBe(0);
  });

  describe("with the learner's instruction", () => {
    const SIMPLER =
      "Rewrite for a reader who found this chapter hard. Use plainer words, shorter sentences and one concrete example per idea, and define each term the first time it appears.";

    async function openRewriteBox(): Promise<void> {
      backend({ regenerate_chapter: () => rewritten });
      const tools = mountLibraryTools(document.createElement("div"));
      await tools.ready;
      await mountReader(root, contract.list_courses[0], { tools });
      buttonNamed("Rewrite chapter").click();
    }

    const sentArgs = (): Args => calls.find((c) => c.cmd === "regenerate_chapter")!.args;

    it("shows four chips, the note box and the old buttons", async () => {
      await openRewriteBox();
      const chips = Array.from(doc().querySelectorAll<HTMLButtonElement>(".rewrite-chip")).map((c) => c.textContent);
      expect(chips).toEqual(["Explain it more simply", "Add more examples", "Make it shorter", "Go deeper"]);
      expect(doc().querySelector(".rewrite-note-label")?.textContent).toBe("Tell the AI what to change (optional)");
      expect(doc().querySelector(".rewrite-confirm-prompt")?.textContent).toContain("Rewrite this chapter?");
      expect(buttonNamed("Confirm rewrite")).toBeTruthy();
      expect(buttonNamed("Cancel")).toBeTruthy();
    });

    it("sends the chosen chip's sentence and the note as one instruction", async () => {
      await openRewriteBox();
      buttonNamed("Explain it more simply").click();
      doc().querySelector<HTMLTextAreaElement>(".rewrite-note-input")!.value = "  I know some biology. ";
      buttonNamed("Confirm rewrite").click();
      await vi.waitFor(() => expect(count("regenerate_chapter")).toBe(1));
      expect(sentArgs().instruction).toBe(`${SIMPLER}\n\nI know some biology.`);
      expect(sentArgs().courseId).toBe(contract.list_courses[0].id);
    });

    it("sends no instruction at all for an empty request", async () => {
      await openRewriteBox();
      buttonNamed("Confirm rewrite").click();
      await vi.waitFor(() => expect(count("regenerate_chapter")).toBe(1));
      expect("instruction" in sentArgs()).toBe(false);
    });

    it("sends no instruction after a chip was chosen and cleared", async () => {
      await openRewriteBox();
      buttonNamed("Go deeper").click();
      buttonNamed("Go deeper").click();
      buttonNamed("Confirm rewrite").click();
      await vi.waitFor(() => expect(count("regenerate_chapter")).toBe(1));
      expect("instruction" in sentArgs()).toBe(false);
    });

    it("keeps the request in the box when the rewrite fails, so the learner can retry", async () => {
      backend({
        regenerate_chapter: () => {
          throw new Error("the agent finished without writing the chapter.");
        },
      });
      const tools = mountLibraryTools(document.createElement("div"));
      await tools.ready;
      await mountReader(root, contract.list_courses[0], { tools });
      buttonNamed("Rewrite chapter").click();
      buttonNamed("Make it shorter").click();
      buttonNamed("Confirm rewrite").click();
      await vi.waitFor(() => expect(doc().querySelector(".chapter-tools .error")).not.toBeNull());
      expect(buttonNamed("Make it shorter").disabled).toBe(false);
      expect(buttonNamed("Make it shorter").getAttribute("aria-pressed")).toBe("true");
      expect(doc().querySelector<HTMLTextAreaElement>(".rewrite-note-input")!.disabled).toBe(false);
    });
  });

  it("keeps the old chapter and says why when the rewrite fails", async () => {
    backend({
      regenerate_chapter: () => {
        throw new Error("the agent finished without writing the chapter: no index.md was written.");
      },
    });
    const tools = mountLibraryTools(document.createElement("div"));
    await tools.ready;
    await mountReader(root, contract.list_courses[0], { tools });

    buttonNamed("Rewrite chapter").click();
    buttonNamed("Confirm rewrite").click();
    await vi.waitFor(() => expect(doc().querySelector(".chapter-tools .error")).not.toBeNull());
    expect(doc().querySelector(".chapter-tools .error")?.textContent).toContain("unchanged");
    expect(doc().querySelector(".prose")?.textContent).toContain("basic unit of life");
    expect(buttonNamed("Confirm rewrite").disabled).toBe(false);
  });
});
