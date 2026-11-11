// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import readerContract from "./fixtures/reader-contract.json";
import wizardContract from "./fixtures/wizard-contract.json";
import { mountReader, type ReaderCourse, type ReaderOptions } from "./reader";
import { stubWideMatchMedia } from "./test-media";

// The reader's side of tickets 31 and 32: a draft course opens on the outline
// review, an approved one can come back to it, a course with no outline is set
// up in the wizard, and a refused chapter sends the user to the review.

type Args = Record<string, unknown>;
type Handler = (args: Args) => unknown;

const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;
const course: ReaderCourse = { id: "cell-biology", storedId: "bio101", title: "Cell Biology", tags: ["science"] };

let root: HTMLElement;
let sent: Array<{ cmd: string; args: Args }>;
let handlers: Record<string, Handler>;
let controller: AbortController;
let onChanged: ReturnType<typeof vi.fn>;
let restoreMedia: (() => void) | undefined;

function backend(over: Record<string, Handler> = {}): void {
  handlers = {
    list_chapters: () => readerContract.list_chapters,
    read_chapter: (a) => (a.chapterId === "cells" ? readerContract.read_chapter_ready : readerContract.read_chapter_placeholder),
    list_annotations: () => [],
    course_progress: () => [],
    get_outline: () => ({ ...clone(wizardContract.outline_draft), status: "approved" }),
    save_outline: (a) => a.outline,
    list_course_templates: () => wizardContract.list_course_templates,
    get_course_setup: () => null,
    list_course_sources: () => [],
    chapter_context: () => ({ objectives: [], summary: "", sources: [] }),
    list_answers: () => [],
    ...over,
  };
}

async function mount(over: Partial<ReaderOptions> = {}, who: ReaderCourse = course): Promise<void> {
  controller = new AbortController();
  onChanged = vi.fn();
  await mountReader(root, who, { signal: controller.signal, onChanged: onChanged as unknown as () => void, ...over });
}

beforeEach(() => {
  restoreMedia = stubWideMatchMedia();
  document.body.innerHTML = '<div id="app"></div>';
  root = document.querySelector<HTMLElement>("#app")!;
  sent = [];
  backend();
  mockWindows("main");
  mockIPC(
    (cmd, raw) => {
      const args = (raw ?? {}) as Args;
      sent.push({ cmd, args });
      const handler = handlers[cmd];
      if (!handler) throw new Error(`unexpected command ${cmd}`);
      return handler(args);
    },
    { shouldMockEvents: true },
  );
});

afterEach(() => {
  restoreMedia?.();
  controller?.abort();
  clearMocks();
});

const commands = (): string[] => sent.map((s) => s.cmd);
const doc = (): HTMLElement => root.querySelector<HTMLElement>("main.document")!;
const button = (name: string): HTMLButtonElement => {
  const found = Array.from(root.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent === name);
  if (!found) throw new Error(`no button "${name}"`);
  return found;
};
const hasButton = (name: string): boolean => Array.from(root.querySelectorAll("button")).some((b) => b.textContent === name);
const sidebarButtons = (): string[] => Array.from(root.querySelectorAll("nav button.chapter"), (b) => b.textContent ?? "");

describe("a course whose outline is still a draft", () => {
  beforeEach(() => backend({ get_outline: () => clone(wizardContract.outline_draft) }));

  it("opens on the review screen in place of a chapter, and reads no chapter", async () => {
    await mount();
    expect(doc().dataset.state).toBe("review");
    expect(doc().querySelector(".review-status")!.textContent).toBe("Draft");
    expect(doc().querySelectorAll("li.draft-chapter")).toHaveLength(3);
    expect(commands()).not.toContain("read_chapter");
    expect(commands()).not.toContain("open_chapter");
  });

  it("keeps the sidebar footer and drops the chapter list, which cannot be opened yet", async () => {
    await mount();
    expect(root.querySelector("nav .course-title")!.textContent).toBe("Cell Biology");
    expect(sidebarButtons()).toEqual([]);
    expect(root.querySelector("nav .sidebar-note")!.textContent).toContain("approve");
    expect(root.querySelector(".reader")!.getAttribute("data-mode")).toBe("review");
  });

  it("goes to the course once the outline is approved to write on open", async () => {
    let approved = false;
    backend({
      get_outline: () => ({ ...clone(wizardContract.outline_draft), status: approved ? "approved" : "draft" }),
      approve_outline: () => {
        approved = true;
        return { ...clone(wizardContract.outline_draft), status: "approved" };
      },
    });
    await mount();
    button("Approve").click();
    button("Approve outline").click();
    await vi.waitFor(() => expect(sidebarButtons()).toEqual(["Cells", "Membranes"]));
    expect(doc().dataset.state).not.toBe("review");
    expect(onChanged).toHaveBeenCalled();
  });

  it("updates the sidebar from Draft outline to the chapters as soon as approve-all returns, before Open the course is pressed", async () => {
    backend({
      get_outline: () => clone(wizardContract.outline_draft),
      approve_outline: () => ({ ...clone(wizardContract.outline_draft), status: "approved" }),
    });
    await mount();
    expect(root.querySelector("nav .sidebar-note")?.textContent).toContain("Draft outline");
    button("Approve").click();
    doc().querySelector<HTMLInputElement>('input[name="generate"][value="all"]')!.click();
    button("Approve outline").click();
    await vi.waitFor(() => expect(doc().querySelector(".approved-panel")).not.toBeNull());
    expect(doc().dataset.state).toBe("review");
    expect(root.querySelector("nav .sidebar-note")).toBeNull();
    expect(sidebarButtons()).toEqual(["Cells", "Membranes"]);
    expect(hasButton("Edit outline")).toBe(true);
  });
});

describe("an approved course", () => {
  it("offers Edit outline in the sidebar where New outline used to be", async () => {
    await mount();
    expect(hasButton("Edit outline")).toBe(true);
    expect(hasButton("New outline")).toBe(false);
    expect(doc().dataset.state).toBe("ready");
  });

  it("comes back to the outline from the sidebar and returns to the chapters", async () => {
    await mount();
    button("Edit outline").click();
    await vi.waitFor(() => expect(doc().dataset.state).toBe("review"));
    expect(doc().querySelector(".review-status")!.textContent).toBe("Approved");
    expect(commands().filter((c) => c === "get_outline")).toHaveLength(2);
    button("Back to the chapters").click();
    await vi.waitFor(() => expect(sidebarButtons()).toEqual(["Cells", "Membranes"]));
    expect(doc().dataset.state).not.toBe("review");
  });

  it("drops the study panel's layout from the page while the outline shows, so a collapsed panel cannot squeeze it", async () => {
    await mount({ storage: null });
    await vi.waitFor(() => expect(root.querySelector(".reader")!.getAttribute("data-panel")).toBe("open"));
    button("Edit outline").click();
    await vi.waitFor(() => expect(doc().dataset.state).toBe("review"));
    expect(root.querySelector(".reader")!.hasAttribute("data-panel")).toBe(false);
    expect(root.querySelector(".reader")!.getAttribute("data-mode")).toBe("review");
  });

  it("saves edits to an approved outline once when the user opens a chapter instead of going back", async () => {
    await mount();
    button("Edit outline").click();
    await vi.waitFor(() => expect(doc().dataset.state).toBe("review"));
    const title = doc().querySelector<HTMLInputElement>("input.draft-title")!;
    title.value = "Cells, renamed";
    title.dispatchEvent(new Event("input", { bubbles: true }));
    root.querySelector<HTMLButtonElement>('nav button[data-chapter-id="cells"]')!.click();
    await vi.waitFor(() => expect(commands()).toContain("save_outline"));
    expect(commands().filter((c) => c === "save_outline")).toHaveLength(1);
  });

  it("shows why the outline could not be read when Edit outline fails", async () => {
    await mount();
    handlers.get_outline = () => {
      throw new Error("course.json is unreadable");
    };
    button("Edit outline").click();
    await vi.waitFor(() => expect(doc().querySelector('[role="alert"]')?.textContent).toContain("course.json is unreadable"));
  });
});

describe("a course the backend cannot say the outline status of", () => {
  it("still reads its chapters, as every course did before outlines had a status", async () => {
    backend({
      get_outline: () => {
        throw new Error("unexpected command get_outline");
      },
    });
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    await mount();
    expect(sidebarButtons()).toEqual(["Cells", "Membranes"]);
    expect(doc().dataset.state).toBe("ready");
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
  });

  it("sends the user to the review when opening a chapter is refused because the outline is a draft", async () => {
    backend({
      get_outline: (() => {
        let calls = 0;
        return () => {
          calls++;
          if (calls === 1) throw new Error("not available yet");
          return clone(wizardContract.outline_draft);
        };
      })(),
      open_chapter: () => {
        throw new Error("outline is a draft");
      },
    });
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    await mount();
    root.querySelector<HTMLButtonElement>('nav button[data-chapter-id="membranes"]')!.click();
    await vi.waitFor(() => expect(doc().dataset.state).toBe("review"));
    expect(doc().querySelector(".review-status")!.textContent).toBe("Draft");
    warn.mockRestore();
  });
});

describe("a course with no outline yet", () => {
  beforeEach(() => backend({ list_chapters: () => [] }));

  it("sets it up in the wizard in the document column, not in a bare topic box", async () => {
    await mount();
    await vi.waitFor(() => expect(doc().querySelector('[aria-label="Course title"]')).not.toBeNull());
    expect(doc().dataset.state).toBe("setup");
    expect(doc().querySelector(".wizard")).not.toBeNull();
    expect(doc().querySelector("textarea.outline-topic")).toBeNull();
    expect(doc().querySelector<HTMLInputElement>('[aria-label="Course title"]')!.value).toBe("Cell Biology");
    expect(doc().querySelector<HTMLInputElement>('[aria-label="Course title"]')!.disabled).toBe(false);
    expect(hasButton("Edit outline")).toBe(false);
  });

  it("opens the review on the draft the plan wrote", async () => {
    let planned = false;
    backend({
      list_chapters: () => (planned ? readerContract.list_chapters : []),
      get_outline: () => (planned ? clone(wizardContract.outline_draft) : { ...clone(wizardContract.outline_draft), status: "approved", chapters: [] }),
      mark_course_opened: () => readerContract.list_library_courses[0],
      set_course_setup: (a) => a.setup,
      generate_outline: () => {
        planned = true;
        return null;
      },
    });
    await mount();
    await vi.waitFor(() => expect(hasButton("Next")).toBe(true));
    button("Next").click();
    await vi.waitFor(() => expect(doc().textContent).toContain("Drop files"));
    button("Next").click();
    await vi.waitFor(() => expect(hasButton("Plan the course")).toBe(true));
    button("Plan the course").click();
    await vi.waitFor(() => expect(doc().dataset.state).toBe("review"));
    expect(commands()).toContain("generate_outline");
    expect(onChanged).toHaveBeenCalled();
  });
});
