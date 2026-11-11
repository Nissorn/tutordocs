// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import contract from "./fixtures/wizard-contract.json";
import type { JobFeed } from "./job-feed";
import { mountJobDrawer } from "./job-drawer";
import { resetJobLabels } from "./job-label";
import type { Outline, VariantMeta } from "./outline-api";
import { type JobKind, type JobRecord } from "./jobs";
import { mountOutlineReview, type ReviewOptions } from "./outline-review";

type Args = Record<string, unknown>;
type Handler = (args: Args) => unknown;

const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;
const draftOutline = (): Outline => clone(contract.outline_draft) as Outline;
const approvedOutline = (): Outline => clone(contract.outline_approved_with_written) as Outline;

let root: HTMLElement;
let sent: Array<{ cmd: string; args: Args }>;
let handlers: Record<string, Handler>;
let options: ReviewOptions;
let controller: AbortController;

/** save_outline echoes what it was sent, as the backend does for a valid outline, giving added chapters an id. */
const echo: Handler = (a) => {
  const outline = clone(a.outline) as Outline;
  outline.chapters = outline.chapters.map((c, i) => ({ ...c, id: c.id ?? `new-${i + 1}` }));
  return outline;
};

function backend(over: Record<string, Handler> = {}): void {
  handlers = {
    save_outline: echo,
    revise_outline: () => draftOutline(),
    approve_outline: (a) => ({ ...draftOutline(), status: "approved", generate: a.generate }),
    cancel_chapter_queue: () => null,
    get_chapter_queue: () => ({ running: "cells", pending: ["membranes", "nucleus"] }),
    list_outline_variants: () => [
      {
        id: "current",
        label: "Current",
        parentId: null,
        createdAt: "2026-11-09T10:00:00Z",
        chapterCount: 3,
        writtenChapterCount: 1,
        current: true,
      },
    ],
    get_outline_variant: () => ({
      id: "current",
      label: "Current",
      parentId: null,
      createdAt: "2026-11-09T10:00:00Z",
      objectives: draftOutline().objectives,
      chapters: draftOutline().chapters,
    }),
    branch_outline: (a) => ({
      id: "draft-b",
      label: (a.label as string) || "Draft B",
      parentId: (a.fromVariantId as string) || "current",
      createdAt: "2026-11-09T10:05:00Z",
      objectives: draftOutline().objectives,
      chapters: draftOutline().chapters,
    }),
    set_current_variant: (a) => ({
      id: (a.variantId as string) || "current",
      label: "Current",
      parentId: null,
      createdAt: "2026-11-09T10:00:00Z",
      objectives: draftOutline().objectives,
      chapters: draftOutline().chapters,
    }),
    ...over,
  };
}

function mount(outline: Outline = draftOutline(), over: Partial<ReviewOptions> = {}): void {
  controller = new AbortController();
  options = {
    course: { id: "cell-biology", storedId: "cb-1" },
    jobs: null,
    onDone: vi.fn(),
    onChange: vi.fn(),
    signal: controller.signal,
    ...over,
  };
  mountOutlineReview(root, outline, options);
}

beforeEach(() => {
  document.body.innerHTML = '<div id="app"></div>';
  root = document.querySelector<HTMLElement>("#app")!;
  sent = [];
  backend();
  mockIPC((cmd, raw) => {
    const args = (raw ?? {}) as Args;
    sent.push({ cmd, args });
    const handler = handlers[cmd];
    if (!handler) throw new Error(`unexpected command ${cmd}`);
    return handler(args);
  });
});

afterEach(() => {
  controller?.abort();
  resetJobLabels();
  clearMocks();
});

const commands = (): string[] => sent.map((s) => s.cmd);
const rows = (): HTMLElement[] => Array.from(root.querySelectorAll<HTMLElement>("li.draft-chapter"));
const button = (name: string): HTMLButtonElement => {
  const found = Array.from(root.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent === name);
  if (!found) throw new Error(`no button "${name}"`);
  return found;
};
const hasButton = (name: string): boolean => Array.from(root.querySelectorAll("button")).some((b) => b.textContent === name);
const alertText = (): string => root.querySelector('[role="alert"]')?.textContent ?? "";
const titleOf = (row: HTMLElement): string => row.querySelector<HTMLInputElement>("input.draft-title")!.value;
const titles = (): string[] => rows().map(titleOf);
const settle = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 20));

function type(input: HTMLInputElement | HTMLTextAreaElement, text: string): void {
  input.value = text;
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

function gate(): { promise: Promise<unknown>; open: (value?: unknown) => void } {
  let release!: (value: unknown) => void;
  const promise = new Promise<unknown>((resolve) => (release = resolve));
  return { promise, open: (value = null) => release(value) };
}

function agentJob(id: string, kind: JobKind, courseId: string): JobRecord {
  return {
    id,
    courseId,
    kind,
    driver: "claude",
    model: null,
    effort: null,
    command: "claude -p <prompt, 12 chars>",
    startedAt: "2026-11-09T12:00:00Z",
    endedAt: null,
    status: "running",
    lines: [],
  };
}

function liveFeed(): JobFeed & { add(job: JobRecord): void } {
  let jobs: JobRecord[] = [];
  const listeners = new Set<() => void>();
  return {
    jobs: () => jobs,
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    async start() {},
    stop() {},
    async cancel() {},
    add(job) {
      jobs = [...jobs, job];
      for (const listener of [...listeners]) listener();
    },
  };
}

function key(target: HTMLElement, name: string): void {
  target.dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true, cancelable: true }));
}

describe("the draft on screen", () => {
  it("shows the objectives and every chapter as editable rows, marked as a draft", () => {
    mount();
    expect(root.querySelector(".review-status")!.textContent).toBe("Draft");
    expect(Array.from(root.querySelectorAll<HTMLInputElement>("input.draft-objective"), (i) => i.value)).toEqual(
      draftOutline().objectives,
    );
    expect(titles()).toEqual(["Cells as the unit of life", "Membranes and transport", "The nucleus and the genome"]);
    expect(rows()[1].querySelector<HTMLTextAreaElement>("textarea.draft-summary")!.value).toBe("The lipid bilayer, channels and pumps.");
  });

  it("shows each chapter's sources as chips, and says when it has none", () => {
    mount();
    expect(Array.from(rows()[1].querySelectorAll(".source-chip"), (c) => c.textContent)).toEqual(["membranes.pdf", "summary.md"]);
    expect(rows()[2].querySelector(".source-chip")).toBeNull();
    expect(rows()[2].querySelector(".source-none")!.textContent).toBe("No sources");
  });

  it("writes titles and sources as text, never as markup", () => {
    const outline = draftOutline();
    outline.chapters[0].title = "<img src=x onerror=alert(1)>";
    outline.chapters[0].sources = ["<b>evil</b>.pdf"];
    mount(outline);
    expect(root.querySelector("img")).toBeNull();
    expect(root.querySelector("b")).toBeNull();
    expect(titleOf(rows()[0])).toBe("<img src=x onerror=alert(1)>");
    expect(rows()[0].querySelector(".source-chip")!.textContent).toBe("<b>evil</b>.pdf");
  });
});

describe("editing and saving", () => {
  it("sends the renamed chapter, the new summary and the edited objective", async () => {
    mount();
    type(rows()[0].querySelector("input.draft-title")!, "Cells");
    type(rows()[0].querySelector("textarea.draft-summary")!, "What a cell is.");
    type(root.querySelector("input.draft-objective")!, "Describe the bilayer");
    button("Save").click();
    await vi.waitFor(() => expect(commands()).toContain("save_outline"));
    const sentOutline = sent.find((s) => s.cmd === "save_outline")!.args.outline as Outline;
    expect(sent.find((s) => s.cmd === "save_outline")!.args.courseId).toBe("cell-biology");
    expect(sentOutline.chapters[0]).toMatchObject({ id: "cells", title: "Cells", summary: "What a cell is." });
    expect(sentOutline.objectives[0]).toBe("Describe the bilayer");
    expect(sentOutline.status).toBe("draft");
  });

  it("is clean as loaded, dirty after an edit, and clean again once saved", async () => {
    mount();
    expect(button("Save").disabled).toBe(true);
    expect(root.querySelector(".review-save")!.textContent).toBe("All changes saved");
    type(rows()[0].querySelector("input.draft-title")!, "Cells");
    expect(button("Save").disabled).toBe(false);
    expect(root.querySelector(".review-save")!.textContent).toBe("Unsaved changes");
    button("Save").click();
    await vi.waitFor(() => expect(root.querySelector(".review-save")!.textContent).toBe("All changes saved"));
    expect(button("Save").disabled).toBe(true);
    expect(options.onChange).toHaveBeenCalled();
  });

  it("keeps the user's edits and says why when saving is refused", async () => {
    backend({
      save_outline: () => {
        throw new Error('chapter id "zzz" is not in this course');
      },
    });
    mount();
    type(rows()[0].querySelector("input.draft-title")!, "Cells");
    button("Save").click();
    await vi.waitFor(() => expect(alertText()).toContain('chapter id "zzz" is not in this course'));
    expect(titleOf(rows()[0])).toBe("Cells");
    expect(button("Save").disabled).toBe(false);
  });

  it("will not save a chapter with no title, and says which", async () => {
    mount();
    button("Add a chapter").click();
    expect(rows()).toHaveLength(4);
    button("Save").click();
    await vi.waitFor(() => expect(alertText()).toBe("Chapter 4 needs a title."));
    await settle();
    expect(commands()).not.toContain("save_outline");
  });

  it("disables only action buttons during save, not the fieldset or text inputs", async () => {
    const pending = gate();
    backend({ save_outline: () => pending.promise });
    mount();
    const titleInput = rows()[0].querySelector<HTMLInputElement>("input.draft-title")!;
    type(titleInput, "Cells");
    button("Save").click();
    await vi.waitFor(() => expect(button("Save").disabled).toBe(true));
    expect(root.querySelector("fieldset.review-fields")!.hasAttribute("disabled")).toBe(false);
    expect(titleInput.disabled).toBe(false);
    expect(root.querySelector<HTMLButtonElement>("button.add-chapter")!.disabled).toBe(true);
    expect(root.querySelector<HTMLButtonElement>("button.add-objective")!.disabled).toBe(true);
    expect(rows()[1].querySelector<HTMLButtonElement>("button.draft-remove")!.disabled).toBe(true);
    pending.open(Object.assign(draftOutline(), { title: "Cell Biology" }));
    await vi.waitFor(() => expect(root.querySelector<HTMLButtonElement>("button.add-chapter")!.disabled).toBe(false));
  });

  it("saves unsaved edits when the screen is left", async () => {
    mount();
    type(rows()[0].querySelector("input.draft-title")!, "Cells");
    controller.abort();
    await vi.waitFor(() => expect(commands()).toContain("save_outline"));
  });

  it("saves nothing on leaving when nothing changed", async () => {
    mount();
    controller.abort();
    await settle();
    expect(commands()).not.toContain("save_outline");
  });
});

const handleOf = (row: HTMLElement): HTMLElement => {
  const handle = row.querySelector<HTMLElement>(".drag-handle");
  if (!handle) throw new Error("no drag handle");
  return handle;
};

describe("reordering", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  const pointer = (type: string, target: HTMLElement, x: number, y: number): void => {
    target.dispatchEvent(
      new PointerEvent(type, {
        bubbles: true,
        cancelable: true,
        pointerId: 1,
        button: 0,
        buttons: type === "pointerup" ? 0 : 1,
        clientX: x,
        clientY: y,
      }),
    );
  };

  /** happy-dom does no hit testing; point the implementation at a chosen row. */
  const pointAt = (row: HTMLElement | null, top = 0, height = 100): void => {
    vi.spyOn(document, "elementFromPoint").mockReturnValue(row);
    if (row) {
      vi.spyOn(row, "getBoundingClientRect").mockReturnValue({
        top,
        bottom: top + height,
        left: 0,
        right: 500,
        width: 500,
        height,
        x: 0,
        y: top,
        toJSON: () => ({}),
      });
    }
  };

  /** Drag a row's handle onto another row. half "top" drops before it, "bottom" after it. */
  const dragRow = (from: number, to: number, half: "top" | "bottom" = "top"): void => {
    const handle = handleOf(rows()[from]);
    pointer("pointerdown", handle, 10, 10);
    pointAt(rows()[to], 100, 100);
    pointer("pointermove", handle, 60, half === "top" ? 120 : 180);
    pointer("pointerup", handle, 60, half === "top" ? 120 : 180);
  };

  it("uses a non-form handle with a 44px hit target and no HTML5 drag", () => {
    mount();
    const handle = handleOf(rows()[0]);
    expect(handle.tagName).toBe("DIV");
    expect(root.querySelector("button.drag-handle")).toBeNull();
    expect(handle.closest("button, input, textarea, select, a")).toBeNull();
    expect(handle.getAttribute("role")).toBe("button");
    expect(handle.tabIndex).toBe(0);
    expect(handle.getAttribute("draggable")).not.toBe("true");
    expect(handle.getAttribute("aria-label")).toContain("arrow");
    expect(parseFloat(handle.style.minWidth)).toBeGreaterThanOrEqual(44);
    expect(parseFloat(handle.style.minHeight)).toBeGreaterThanOrEqual(44);
  });

  it("moves a chapter down from the keyboard and keeps focus on its handle", async () => {
    mount();
    const handle = handleOf(rows()[0]);
    handle.focus();
    key(handle, "ArrowDown");
    expect(titles()).toEqual(["Membranes and transport", "Cells as the unit of life", "The nucleus and the genome"]);
    expect(document.activeElement).toBe(handleOf(rows()[1]));
    key(document.activeElement as HTMLElement, "ArrowUp");
    expect(titles()[0]).toBe("Cells as the unit of life");
  });

  it("does not move the first chapter up or the last down", () => {
    mount();
    key(handleOf(rows()[0]), "ArrowUp");
    key(handleOf(rows()[2]), "ArrowDown");
    expect(titles()).toEqual(["Cells as the unit of life", "Membranes and transport", "The nucleus and the genome"]);
  });

  it("does not reorder from the keyboard or a pointer drag while a save is in flight", async () => {
    const pending = gate();
    backend({ save_outline: () => pending.promise });
    mount();
    const before = titles();
    type(rows()[0].querySelector("input.draft-title")!, "Cells");
    button("Save").click();
    await vi.waitFor(() => expect(button("Save").disabled).toBe(true));
    expect(root.querySelector("fieldset.review-fields")!.hasAttribute("disabled")).toBe(false);
    const handle = handleOf(rows()[0]);
    expect(handle.getAttribute("aria-disabled")).toBe("true");
    expect(handle.tabIndex).toBe(-1);
    key(handle, "ArrowDown");
    dragRow(0, 2);
    expect(titles().map((title, i) => (i === 0 ? "Cells" : title))).toEqual(["Cells", ...before.slice(1)]);
    expect(root.querySelector(".drag-placeholder")).toBeNull();
    pending.open(draftOutline());
    await vi.waitFor(() => expect(root.querySelector("fieldset.review-fields")!.hasAttribute("disabled")).toBe(false));
  });

  it("moves a chapter to where it is dropped and saves the new order", async () => {
    mount();
    dragRow(2, 0);
    expect(titles()).toEqual(["The nucleus and the genome", "Cells as the unit of life", "Membranes and transport"]);
    expect(document.activeElement).toBe(handleOf(rows()[0]));
    button("Save").click();
    await vi.waitFor(() => expect(commands()).toContain("save_outline"));
    const ids = ((sent.find((s) => s.cmd === "save_outline")!.args.outline as Outline).chapters).map((c) => c.id);
    expect(ids).toEqual(["nucleus", "cells", "membranes"]);
  });

  it("drops after a row when the pointer is in its lower half", () => {
    mount();
    dragRow(0, 1, "bottom");
    expect(titles()).toEqual(["Membranes and transport", "Cells as the unit of life", "The nucleus and the genome"]);
  });

  it("shows a placeholder and a ghost while dragging, and removes both on drop", () => {
    mount();
    const handle = handleOf(rows()[2]);
    pointer("pointerdown", handle, 10, 10);
    pointAt(rows()[0]);
    pointer("pointermove", handle, 60, 120);
    expect(root.querySelector(".drag-placeholder")).not.toBeNull();
    expect(document.body.querySelector(".drag-ghost")).not.toBeNull();
    expect(rows()[2].classList.contains("dragging")).toBe(true);
    pointer("pointerup", handle, 60, 120);
    expect(root.querySelector(".drag-placeholder")).toBeNull();
    expect(document.body.querySelector(".drag-ghost")).toBeNull();
    expect(root.querySelector(".draft-chapter.dragging")).toBeNull();
  });

  it("does not reorder on a click without movement", () => {
    mount();
    const before = titles();
    const handle = handleOf(rows()[1]);
    pointer("pointerdown", handle, 10, 10);
    pointer("pointerup", handle, 10, 10);
    expect(titles()).toEqual(before);
    expect(root.querySelector(".drag-placeholder")).toBeNull();
    expect(document.body.querySelector(".drag-ghost")).toBeNull();
  });

  it("cancels the drag without reordering on pointercancel", () => {
    mount();
    const before = titles();
    const handle = handleOf(rows()[2]);
    pointer("pointerdown", handle, 10, 10);
    pointAt(rows()[0]);
    pointer("pointermove", handle, 60, 120);
    expect(root.querySelector(".drag-placeholder")).not.toBeNull();
    pointer("pointercancel", handle, 60, 120);
    expect(titles()).toEqual(before);
    expect(root.querySelector(".drag-placeholder")).toBeNull();
    expect(document.body.querySelector(".drag-ghost")).toBeNull();
  });

  it("returns a chapter to its place when dropped back on its own slot", () => {
    mount();
    const before = titles();
    const handle = handleOf(rows()[1]);
    pointer("pointerdown", handle, 10, 10);
    pointAt(rows()[2], 100, 100);
    pointer("pointermove", handle, 60, 180);
    pointer("pointermove", handle, 60, 120);
    pointer("pointerup", handle, 60, 120);
    expect(titles()).toEqual(before);
  });
});

describe("adding and removing", () => {
  it("adds a chapter, focuses its title, and sends it with no id", async () => {
    mount();
    button("Add a chapter").click();
    expect(document.activeElement).toBe(rows()[3].querySelector("input.draft-title"));
    type(rows()[3].querySelector("input.draft-title")!, "Mitochondria");
    button("Save").click();
    await vi.waitFor(() => expect(commands()).toContain("save_outline"));
    const added = (sent.find((s) => s.cmd === "save_outline")!.args.outline as Outline).chapters[3];
    expect(added).toMatchObject({ id: null, title: "Mitochondria", written: false });
  });

  it("removes an unwritten chapter", () => {
    mount();
    rows()[1].querySelector<HTMLButtonElement>("button.draft-remove")!.click();
    expect(titles()).toEqual(["Cells as the unit of life", "The nucleus and the genome"]);
  });

  it("cannot remove a written chapter, and shows why beside it", () => {
    mount(approvedOutline());
    const written = rows()[0];
    const remove = written.querySelector<HTMLButtonElement>("button.draft-remove")!;
    expect(remove.disabled).toBe(true);
    expect(written.querySelector(".draft-reason")!.textContent).toBe('"Cells as the unit of life" is already written, so it cannot be removed.');
    expect(written.querySelector(".written-badge")!.textContent).toBe("Written");
    remove.click();
    expect(rows()).toHaveLength(2);
    expect(rows()[1].querySelector<HTMLButtonElement>("button.draft-remove")!.disabled).toBe(false);
    expect(rows()[1].querySelector(".draft-reason")).toBeNull();
  });

  it("adds, edits and removes objectives", async () => {
    mount();
    button("Add an objective").click();
    const inputs = root.querySelectorAll<HTMLInputElement>("input.draft-objective");
    expect(inputs).toHaveLength(3);
    type(inputs[2], "Name the organelles");
    root.querySelector<HTMLButtonElement>("li.draft-objective-row button.draft-remove-objective")!.click();
    button("Save").click();
    await vi.waitFor(() => expect(commands()).toContain("save_outline"));
    expect((sent.find((s) => s.cmd === "save_outline")!.args.outline as Outline).objectives).toEqual([
      "Compare diffusion and active transport",
      "Name the organelles",
    ]);
  });
});

describe("comments and revising", () => {
  it("offers a comment box on every chapter and keeps Revise off until one has text", () => {
    mount();
    expect(root.querySelectorAll("textarea.draft-comment")).toHaveLength(3);
    expect(button("Revise with my comments").disabled).toBe(true);
    expect(root.querySelector(".review-hint")!.textContent).toContain("comment");
    type(rows()[2].querySelector("textarea.draft-comment")!, "Too short. Add the nucleolus.");
    expect(button("Revise with my comments").disabled).toBe(false);
    expect(root.querySelector(".review-hint")).toBeNull();
  });

  it("saves the comments, then revises, and shows the revised chapters with the comments cleared", async () => {
    const revised = draftOutline();
    revised.chapters[2].title = "The nucleus, nucleolus and genome";
    backend({ revise_outline: () => revised });
    mount();
    type(rows()[2].querySelector("textarea.draft-comment")!, "Too short. Add the nucleolus.");
    button("Revise with my comments").click();
    await vi.waitFor(() => expect(titles()[2]).toBe("The nucleus, nucleolus and genome"));
    expect(commands().filter((c) => c === "save_outline" || c === "revise_outline")).toEqual(["save_outline", "revise_outline"]);
    const saved = sent.find((s) => s.cmd === "save_outline")!.args.outline as Outline;
    expect(saved.chapters[2].comment).toBe("Too short. Add the nucleolus.");
    expect(sent.find((s) => s.cmd === "revise_outline")!.args).toEqual({ courseId: "cell-biology" });
    expect(rows()[2].querySelector<HTMLTextAreaElement>("textarea.draft-comment")!.value).toBe("");
    expect(root.querySelector(".review-notice")!.textContent).toContain("comments were applied");
  });

  it("shows the revise job while it runs and locks the edits", async () => {
    const job = gate();
    backend({ revise_outline: () => job.promise });
    mount();
    type(rows()[0].querySelector("textarea.draft-comment")!, "More on prokaryotes.");
    button("Revise with my comments").click();
    await vi.waitFor(() => expect(root.querySelector(".job-status")).not.toBeNull());
    expect(root.querySelector(".job-status")!.textContent).toContain("Revising the outline");
    expect(root.querySelector("fieldset.review-fields")!.hasAttribute("disabled")).toBe(false);
    expect(root.querySelector<HTMLButtonElement>("button.add-chapter")!.disabled).toBe(true);
    expect(rows()[0].querySelector<HTMLTextAreaElement>("textarea.draft-comment")!.disabled).toBe(false);
    job.open(draftOutline());
    await vi.waitFor(() => expect(root.querySelector(".job-status")).toBeNull());
    expect(root.querySelector("fieldset.review-fields")!.hasAttribute("disabled")).toBe(false);
  });

  it("reads the revise job from the feed the app holds", async () => {
    const feed = {
      jobs: () => [
        {
          id: "job-9",
          courseId: "cb-1",
          kind: "outline",
          driver: "claude",
          model: null,
          effort: null,
          command: "claude -p",
          startedAt: new Date().toISOString(),
          endedAt: null,
          status: "running",
          lines: [{ at: "x", kind: "text", text: "Reworking chapter 3." }],
        },
      ],
      subscribe: () => () => undefined,
    } as unknown as JobFeed;
    const job = gate();
    backend({ revise_outline: () => job.promise });
    mount(draftOutline(), { jobs: feed });
    type(rows()[0].querySelector("textarea.draft-comment")!, "More on prokaryotes.");
    button("Revise with my comments").click();
    await vi.waitFor(() => expect(root.querySelector(".job-status-lines li")?.textContent).toBe("Reworking chapter 3."));
    job.open(draftOutline());
  });

  it("labels the revise job Outline revise in the drawer, matched by the id inside course.json", async () => {
    const feed = liveFeed();
    const job = gate();
    backend({ revise_outline: () => job.promise });
    mount(draftOutline(), { jobs: feed });
    type(rows()[0].querySelector("textarea.draft-comment")!, "More on prokaryotes.");
    button("Revise with my comments").click();
    await vi.waitFor(() => expect(commands()).toContain("revise_outline"));
    feed.add(agentJob("job-4", "outline", "cb-1"));
    const drawer = mountJobDrawer(root, feed, { courses: () => [], onClose: () => {} });
    drawer.open();
    expect(root.querySelector("aside.job-drawer .job-kind")!.textContent).toBe("Outline revise");
    job.open(draftOutline());
    await vi.waitFor(() => expect(root.querySelector(".review-notice")?.textContent).toContain("comments were applied"));
  });

  it("does not leave Outline revise on a later outline job when revise fails before a job starts", async () => {
    const feed = liveFeed();
    backend({
      revise_outline: () => {
        throw new Error("no agent");
      },
    });
    mount(draftOutline(), { jobs: feed });
    type(rows()[0].querySelector("textarea.draft-comment")!, "More on prokaryotes.");
    button("Revise with my comments").click();
    await vi.waitFor(() => expect(alertText()).toContain("no agent"));
    feed.add(agentJob("job-5", "outline", "cb-1"));
    const drawer = mountJobDrawer(root, feed, { courses: () => [], onClose: () => {} });
    drawer.open();
    expect(root.querySelector("aside.job-drawer .job-kind")!.textContent).toBe("Outline");
  });

  it("keeps the comments and says why when the revision fails", async () => {
    backend({
      revise_outline: () => {
        throw new Error("the agent's outline was not usable");
      },
    });
    mount();
    type(rows()[0].querySelector("textarea.draft-comment")!, "More on prokaryotes.");
    button("Revise with my comments").click();
    await vi.waitFor(() => expect(alertText()).toContain("the agent's outline was not usable"));
    expect(rows()[0].querySelector<HTMLTextAreaElement>("textarea.draft-comment")!.value).toBe("More on prokaryotes.");
    expect(button("Revise with my comments").disabled).toBe(false);
  });

  it("does not revise when the comments could not be saved first", async () => {
    backend({
      save_outline: () => {
        throw new Error("disk is full");
      },
    });
    mount();
    type(rows()[0].querySelector("textarea.draft-comment")!, "More on prokaryotes.");
    button("Revise with my comments").click();
    await vi.waitFor(() => expect(alertText()).toContain("disk is full"));
    expect(commands()).not.toContain("revise_outline");
  });
});

describe("approving", () => {
  it("asks how the chapters should be written, with writing on open chosen first", () => {
    mount();
    button("Approve").click();
    const choices = Array.from(root.querySelectorAll<HTMLInputElement>('input[name="generate"]'));
    expect(choices.map((c) => [c.value, c.checked])).toEqual([
      ["on_open", true],
      ["all", false],
    ]);
    expect(root.querySelector(".approve-panel")!.textContent).toContain("Write each chapter when I open it");
    expect(root.querySelector(".approve-panel")!.textContent).toContain("Write all chapters now");
    expect(commands()).not.toContain("approve_outline");
  });

  it("approves to write each chapter on open, then leaves for the course", async () => {
    mount();
    button("Approve").click();
    button("Approve outline").click();
    await vi.waitFor(() => expect(options.onDone).toHaveBeenCalled());
    expect(sent.find((s) => s.cmd === "approve_outline")!.args).toEqual({ courseId: "cell-biology", generate: "on_open" });
    expect(options.onChange).toHaveBeenCalled();
  });

  it("approves to write every chapter now, says so, and offers to stop the queue", async () => {
    mount();
    button("Approve").click();
    root.querySelector<HTMLInputElement>('input[name="generate"][value="all"]')!.click();
    button("Approve outline").click();
    await vi.waitFor(() => expect(root.querySelector(".approved-panel")).not.toBeNull());
    expect(sent.find((s) => s.cmd === "approve_outline")!.args).toEqual({ courseId: "cell-biology", generate: "all" });
    expect(root.querySelector(".review-status")!.textContent).toBe("Approved");
    expect(root.querySelector(".approved-panel")!.textContent).toContain("one at a time");
    expect(options.onDone).not.toHaveBeenCalled();

    button("Stop after the running chapter").click();
    await vi.waitFor(() => expect(commands()).toContain("cancel_chapter_queue"));
    expect(sent.find((s) => s.cmd === "cancel_chapter_queue")!.args).toEqual({ courseId: "cell-biology" });
    expect(button("Stop after the running chapter").disabled).toBe(true);

    button("Open the course").click();
    expect(options.onDone).toHaveBeenCalled();
  });

  it("shows the running chapter and count left, then settles on All chapters written when the queue drains", async () => {
    let queue = { running: "cells" as string | null, pending: ["membranes", "nucleus"] };
    let listener: (() => void) | null = null;
    const feed = {
      jobs: () => [],
      subscribe: (fn: () => void) => {
        listener = fn;
        return () => {
          listener = null;
        };
      },
    } as unknown as JobFeed;
    backend({ get_chapter_queue: () => queue });
    mount(draftOutline(), { jobs: feed });
    button("Approve").click();
    root.querySelector<HTMLInputElement>('input[name="generate"][value="all"]')!.click();
    button("Approve outline").click();
    await vi.waitFor(() =>
      expect(root.querySelector(".approved-panel .approve-detail")?.textContent).toContain(
        "Cells as the unit of life",
      ),
    );
    expect(root.querySelector(".approved-panel .approve-detail")!.textContent).toContain("2 left");

    queue = { running: "membranes", pending: ["nucleus"] };
    listener!();
    await vi.waitFor(() =>
      expect(root.querySelector(".approved-panel .approve-detail")?.textContent).toContain("Membranes and transport"),
    );
    expect(root.querySelector(".approved-panel .approve-detail")!.textContent).toContain("1 left");

    queue = { running: null, pending: [] };
    listener!();
    await vi.waitFor(() =>
      expect(root.querySelector(".approved-panel .approve-detail")?.textContent).toContain("All chapters written"),
    );
    expect(button("Stop after the running chapter").disabled).toBe(true);
  });

  it("settles on All chapters written when the queue drains with no later job event", async () => {
    let queue = { running: "cells" as string | null, pending: [] as string[] };
    const feed = {
      jobs: () => [],
      subscribe: () => () => {},
    } as unknown as JobFeed;
    backend({ get_chapter_queue: () => queue });
    mount(draftOutline(), { jobs: feed });
    button("Approve").click();
    root.querySelector<HTMLInputElement>('input[name="generate"][value="all"]')!.click();
    button("Approve outline").click();
    await vi.waitFor(() =>
      expect(root.querySelector(".approved-panel .approve-detail")?.textContent).toContain("Cells as the unit of life"),
    );
    queue = { running: null, pending: [] };
    await vi.waitFor(() =>
      expect(root.querySelector(".approved-panel .approve-detail")?.textContent).toContain("All chapters written"),
    );
  });

  it("settles on Stopped after <chapter> once the running chapter finishes after stopping the queue", async () => {
    let queue = { running: "cells" as string | null, pending: ["membranes", "nucleus"] };
    let listener: (() => void) | null = null;
    const feed = {
      jobs: () => [],
      subscribe: (fn: () => void) => {
        listener = fn;
        return () => {
          listener = null;
        };
      },
    } as unknown as JobFeed;
    backend({
      get_chapter_queue: () => queue,
      cancel_chapter_queue: () => {
        queue = { running: "cells", pending: [] };
        return null;
      },
    });
    mount(draftOutline(), { jobs: feed });
    button("Approve").click();
    root.querySelector<HTMLInputElement>('input[name="generate"][value="all"]')!.click();
    button("Approve outline").click();
    await vi.waitFor(() =>
      expect(root.querySelector(".approved-panel .approve-detail")?.textContent).toContain(
        "Cells as the unit of life",
      ),
    );

    button("Stop after the running chapter").click();
    await vi.waitFor(() =>
      expect(root.querySelector(".review-notice")?.textContent).toContain("stop after the chapter that is running"),
    );
    expect(commands()).toContain("cancel_chapter_queue");

    queue = { running: null, pending: [] };
    listener!();
    await vi.waitFor(() =>
      expect(root.querySelector(".approved-panel .approve-detail")?.textContent).toContain(
        "Stopped after Cells as the unit of life",
      ),
    );
    expect(root.querySelector(".review-notice")).toBeNull();
  });

  it("saves unsaved edits before approving", async () => {
    mount();
    type(rows()[0].querySelector("input.draft-title")!, "Cells");
    button("Approve").click();
    button("Approve outline").click();
    await vi.waitFor(() => expect(commands()).toContain("approve_outline"));
    expect(commands().filter((c) => c === "save_outline" || c === "approve_outline")).toEqual(["save_outline", "approve_outline"]);
  });

  it("can be called off from the choice", () => {
    mount();
    button("Approve").click();
    button("Cancel").click();
    expect(root.querySelector(".approve-panel")).toBeNull();
    expect(commands()).not.toContain("approve_outline");
  });

  it("does not offer approval for an outline with no chapters or an untitled one", () => {
    const empty = draftOutline();
    empty.chapters = [];
    mount(empty);
    button("Approve").click();
    expect(alertText()).toBe("Add at least one chapter first.");
    expect(root.querySelector(".approve-panel")).toBeNull();
  });

  it("shows why approval failed and keeps the screen", async () => {
    backend({
      approve_outline: () => {
        throw new Error("another approval is running");
      },
    });
    mount();
    button("Approve").click();
    button("Approve outline").click();
    await vi.waitFor(() => expect(alertText()).toContain("another approval is running"));
    expect(options.onDone).not.toHaveBeenCalled();
    expect(button("Approve outline")).toBeTruthy();
  });
});

describe("an approved course coming back to the outline", () => {
  it("marks it approved, offers no approval, and goes back to the chapters", async () => {
    mount(approvedOutline());
    expect(root.querySelector(".review-status")!.textContent).toBe("Approved");
    expect(hasButton("Approve")).toBe(false);
    button("Back to the chapters").click();
    await vi.waitFor(() => expect(options.onDone).toHaveBeenCalled());
    expect(commands()).not.toContain("save_outline");
  });

  it("saves added chapters before going back", async () => {
    mount(approvedOutline());
    button("Add a chapter").click();
    type(rows()[2].querySelector("input.draft-title")!, "Cell division");
    button("Back to the chapters").click();
    await vi.waitFor(() => expect(options.onDone).toHaveBeenCalled());
    const outline = sent.find((s) => s.cmd === "save_outline")!.args.outline as Outline;
    expect(outline.status).toBe("approved");
    expect(outline.chapters.map((c) => c.title)).toEqual(["Cells as the unit of life", "Membranes and transport", "Cell division"]);
  });

  it("stays on the outline and says why when the save before leaving fails", async () => {
    backend({
      save_outline: () => {
        throw new Error("disk is full");
      },
    });
    mount(approvedOutline());
    type(rows()[1].querySelector("input.draft-title")!, "Changed");
    button("Back to the chapters").click();
    await vi.waitFor(() => expect(alertText()).toContain("disk is full"));
    expect(options.onDone).not.toHaveBeenCalled();
  });
});

describe("outline variants and compare", () => {
  const metaCurrent: VariantMeta = {
    id: "current",
    label: "Current",
    parentId: null,
    createdAt: "2026-11-09T10:00:00Z",
    chapterCount: 3,
    writtenChapterCount: 1,
    current: true,
  };
  const metaDraftB: VariantMeta = {
    id: "draft-b",
    label: "Draft B",
    parentId: "current",
    createdAt: "2026-11-09T10:05:00Z",
    chapterCount: 2,
    writtenChapterCount: 1,
    current: false,
  };
  const variantDraftB = {
    id: "draft-b",
    label: "Draft B",
    parentId: "current",
    createdAt: "2026-11-09T10:05:00Z",
    objectives: ["Learn Rust"],
    chapters: [
      {
        id: "intro",
        title: "Introduction to Rust",
        summary: "Getting started",
        sources: ["guide.md"],
        comment: "",
        written: true,
      },
      {
        id: "borrows",
        title: "Borrowing and lifetimes",
        summary: "References and safety",
        sources: [],
        comment: "Explain aliasing",
        written: false,
      },
    ],
  };

  it("lists variants and shows written badge on written chapters with Remove disabled", async () => {
    backend({
      list_outline_variants: () => [metaCurrent],
    });
    mount(approvedOutline());
    await vi.waitFor(() => expect(root.querySelector(".variant-select")).toBeTruthy());
    const writtenRows = rows().filter((r) => r.dataset.written === "true");
    expect(writtenRows.length).toBeGreaterThan(0);
    for (const r of writtenRows) {
      expect(r.querySelector(".written-badge")?.textContent).toBe("Written");
      expect(r.querySelector(".pinned-badge")).toBeNull();
      expect(r.querySelector<HTMLButtonElement>("button.draft-remove")?.disabled).toBe(true);
    }
  });

  it("branches a new variant with label, shows written chapters note, and switches to it", async () => {
    let variantsList = [metaCurrent];
    backend({
      list_outline_variants: () => variantsList,
      branch_outline: (a) => {
        const newMeta = {
          id: "draft-b",
          label: (a.label as string) || "Draft B",
          parentId: (a.fromVariantId as string) || "current",
          createdAt: "2026-11-09T10:05:00Z",
          chapterCount: 3,
          writtenChapterCount: 1,
          current: false,
        };
        variantsList = [...variantsList, newMeta];
        return {
          id: "draft-b",
          label: newMeta.label,
          parentId: newMeta.parentId,
          createdAt: newMeta.createdAt,
          objectives: draftOutline().objectives,
          chapters: draftOutline().chapters,
        };
      },
      get_outline_variant: () => ({
        id: "draft-b",
        label: "Draft B",
        parentId: "current",
        createdAt: "2026-11-09T10:05:00Z",
        objectives: draftOutline().objectives,
        chapters: draftOutline().chapters,
      }),
    });
    mount();
    await vi.waitFor(() => expect(root.querySelector(".variant-select")).toBeTruthy());
    button("Branch draft").click();
    expect(root.textContent).toContain("Written chapters stay in every draft.");
    const labelInput = root.querySelector<HTMLInputElement>("input.variant-branch-label")!;
    expect(labelInput).toBeTruthy();
    type(labelInput, "Draft B");
    button("Create branch").click();
    await vi.waitFor(() => {
      const call = sent.find((s) => s.cmd === "branch_outline");
      expect(call?.args).toEqual({
        courseId: "cell-biology",
        fromVariantId: "current",
        label: "Draft B",
      });
    });
    await vi.waitFor(() => {
      const select = root.querySelector<HTMLSelectElement>(".variant-select")!;
      expect(select.value).toBe("draft-b");
    });
  });

  it("switches to another variant and saves / revises on that variant", async () => {
    backend({
      list_outline_variants: () => [metaCurrent, metaDraftB],
      get_outline_variant: (a) => {
        if (a.variantId === "draft-b") return variantDraftB;
        return {
          id: "current",
          label: "Current",
          parentId: null,
          createdAt: "2026-11-09T10:00:00Z",
          objectives: draftOutline().objectives,
          chapters: draftOutline().chapters,
        };
      },
    });
    mount();
    await vi.waitFor(() => expect(root.querySelectorAll(".variant-select option").length).toBe(2));
    const select = root.querySelector<HTMLSelectElement>(".variant-select")!;
    select.value = "draft-b";
    select.dispatchEvent(new Event("change", { bubbles: true }));
    await vi.waitFor(() => {
      expect(titles()).toEqual(["Introduction to Rust", "Borrowing and lifetimes"]);
    });

    // Save on draft-b
    type(rows()[1].querySelector("input.draft-title")!, "Borrowing and aliasing");
    button("Save").click();
    await vi.waitFor(() => {
      const saveCall = sent.find((s) => s.cmd === "save_outline" && s.args.variantId === "draft-b");
      expect(saveCall).toBeTruthy();
    });

    // Revise on draft-b
    await vi.waitFor(() => expect(button("Revise with my comments").disabled).toBe(false));
    button("Revise with my comments").click();
    await vi.waitFor(() => {
      const reviseCall = sent.find((s) => s.cmd === "revise_outline" && s.args.variantId === "draft-b");
      expect(reviseCall).toBeTruthy();
    });
  });

  it("selects and sets current variant", async () => {
    backend({
      list_outline_variants: () => [metaCurrent, metaDraftB],
      get_outline_variant: () => variantDraftB,
      set_current_variant: (a) => ({ ...variantDraftB, id: a.variantId as string }),
    });
    mount();
    await vi.waitFor(() => expect(root.querySelectorAll(".variant-select option").length).toBe(2));
    const select = root.querySelector<HTMLSelectElement>(".variant-select")!;
    select.value = "draft-b";
    select.dispatchEvent(new Event("change", { bubbles: true }));
    await vi.waitFor(() => expect(titles()).toEqual(["Introduction to Rust", "Borrowing and lifetimes"]));

    button("Set as current").click();
    await vi.waitFor(() => {
      const setCall = sent.find((s) => s.cmd === "set_current_variant");
      expect(setCall?.args).toEqual({ courseId: "cell-biology", variantId: "draft-b" });
    });
  });

  it("compares two variants side by side and approves the picked one without regenerating", async () => {
    backend({
      list_outline_variants: () => [metaCurrent, metaDraftB],
      get_outline_variant: (a) => {
        if (a.variantId === "draft-b") return variantDraftB;
        return {
          id: "current",
          label: "Current",
          parentId: null,
          createdAt: "2026-11-09T10:00:00Z",
          objectives: draftOutline().objectives,
          chapters: draftOutline().chapters,
        };
      },
      approve_outline: () => ({
        ...variantDraftB,
        status: "approved",
        title: "Cell Biology",
      }),
    });
    mount();
    await vi.waitFor(() => expect(root.querySelector(".variant-compare")).toBeTruthy());

    button("Compare").click();
    await vi.waitFor(() => expect(root.querySelector(".compare-view")).toBeTruthy());

    // Both columns exist with titles and summaries
    const columns = root.querySelectorAll(".compare-column");
    expect(columns.length).toBe(2);
    expect(columns[0].textContent).toContain("Cells as the unit of life");
    expect(columns[1].textContent).toContain("Introduction to Rust");
    expect(columns[1].textContent).toContain("Borrowing and lifetimes");

    // Approve the picked variant (Draft B)
    const approveB = columns[1].querySelector<HTMLButtonElement>("button.compare-approve")!;
    expect(approveB).toBeTruthy();
    approveB.click();

    // Confirm approval
    button("Approve outline").click();
    await vi.waitFor(() => {
      const approveCall = sent.find((s) => s.cmd === "approve_outline");
      expect(approveCall?.args).toEqual({
        courseId: "cell-biology",
        generate: "on_open",
        variantId: "draft-b",
      });
    });
    await vi.waitFor(() => expect(options.onDone).toHaveBeenCalled());
  });

  it("hides the variant bar and leaves it childless in the compare view", async () => {
    backend({
      list_outline_variants: () => [metaCurrent, metaDraftB],
      get_outline_variant: (a) => (a.variantId === "draft-b" ? variantDraftB : { ...variantDraftB, id: "current" }),
    });
    mount();
    await vi.waitFor(() => expect(root.querySelector(".variant-compare")).toBeTruthy());
    button("Compare").click();
    await vi.waitFor(() => expect(root.querySelector(".compare-view")).toBeTruthy());
    const bar = root.querySelector<HTMLElement>(".variant-bar")!;
    expect(bar.hidden).toBe(true);
    expect(bar.children.length).toBe(0);
  });

  it("identifies current variant from current: true when current is not the oldest", async () => {
    const metaOldA: VariantMeta = {
      id: "draft-a",
      label: "Draft A",
      parentId: null,
      createdAt: "2026-01-01T10:00:00Z",
      chapterCount: 2,
      writtenChapterCount: 0,
      current: false,
    };
    const metaNewB: VariantMeta = {
      id: "draft-b",
      label: "Draft B",
      parentId: "draft-a",
      createdAt: "2026-01-02T10:00:00Z",
      chapterCount: 2,
      writtenChapterCount: 0,
      current: true,
    };
    backend({
      list_outline_variants: () => [metaOldA, metaNewB],
      get_outline_variant: (a) => {
        if (a.variantId === "draft-a") {
          return {
            id: "draft-a",
            label: "Draft A",
            parentId: null,
            createdAt: "2026-01-01T10:00:00Z",
            objectives: ["Objective A"],
            chapters: [
              {
                id: "c1",
                title: "Chapter 1 in A",
                summary: "Summary A",
                sources: [],
                comment: "",
                written: false,
              },
            ],
          };
        }
        return {
          id: "draft-b",
          label: "Draft B",
          parentId: "draft-a",
          createdAt: "2026-01-02T10:00:00Z",
          objectives: ["Objective B"],
          chapters: [
            {
              id: "c1",
              title: "Chapter 1 in B",
              summary: "Summary B",
              sources: [],
              comment: "",
              written: false,
            },
          ],
        };
      },
      save_outline: () => contract.outline_draft,
    });
    mount();
    await vi.waitFor(() => expect(root.querySelectorAll(".variant-select option").length).toBe(2));
    const select = root.querySelector<HTMLSelectElement>(".variant-select")!;
    // Draft B is active despite Draft A being list[0] (oldest)
    const activeOption = [...select.options].find((o) => o.textContent?.includes("(Active)"));
    expect(activeOption?.value).toBe("draft-b");

    // Switch to Draft A
    select.value = "draft-a";
    select.dispatchEvent(new Event("change", { bubbles: true }));
    await vi.waitFor(() => expect(titles()).toEqual(["Chapter 1 in A"]));

    // Editing and saving Draft A must pass variantId: "draft-a"
    type(rows()[0].querySelector("input.draft-title")!, "Chapter 1 in A (edited)");
    button("Save").click();
    await vi.waitFor(() => {
      const saveCall = sent.find((s) => s.cmd === "save_outline" && s.args.variantId === "draft-a");
      expect(saveCall).toBeTruthy();
    });
  });

  it("renders variant bar only after list loads, and shows load failure as error notice", async () => {
    backend({
      list_outline_variants: () => {
        throw new Error("backend failed to list variants");
      },
    });
    mount();
    await vi.waitFor(() => expect(alertText()).toContain("backend failed to list variants"));
    const bar = root.querySelector<HTMLElement>(".variant-bar")!;
    expect(bar.hidden).toBe(true);
  });

  it("persists dirty draft before entering compare and refuses if save fails", async () => {
    backend({
      list_outline_variants: () => [metaCurrent, metaDraftB],
      get_outline_variant: (a) => (a.variantId === "draft-b" ? variantDraftB : { ...variantDraftB, id: "current" }),
      save_outline: () => {
        throw new Error("disk write error");
      },
    });
    mount();
    await vi.waitFor(() => expect(root.querySelector(".variant-compare")).toBeTruthy());

    type(rows()[0].querySelector("input.draft-title")!, "New Unsaved Title");
    button("Compare").click();

    await vi.waitFor(() => expect(alertText()).toContain("disk write error"));
    expect(root.querySelector(".compare-view")).toBeNull();
  });

  it("marks differences in compare view with Changed and Only in this draft", async () => {
    const leftVariant = {
      id: "draft-a",
      label: "Draft A",
      parentId: null,
      createdAt: "2026-01-01T10:00:00Z",
      objectives: ["Objectives"],
      chapters: [
        {
          id: "shared-ch",
          title: "Title A",
          summary: "Summary unchanged",
          sources: [],
          comment: "",
          written: false,
        },
        {
          id: "left-only-ch",
          title: "Left Only",
          summary: "Summary Left",
          sources: [],
          comment: "",
          written: false,
        },
        {
          id: "identical-ch",
          title: "Identical Title",
          summary: "Identical Summary",
          sources: [],
          comment: "",
          written: false,
        },
      ],
    };

    const rightVariant = {
      id: "draft-b",
      label: "Draft B",
      parentId: "draft-a",
      createdAt: "2026-01-02T10:00:00Z",
      objectives: ["Objectives"],
      chapters: [
        {
          id: "shared-ch",
          title: "Title B (Changed)",
          summary: "Summary unchanged",
          sources: [],
          comment: "",
          written: false,
        },
        {
          id: "right-only-ch",
          title: "Right Only",
          summary: "Summary Right",
          sources: [],
          comment: "",
          written: false,
        },
        {
          id: "identical-ch",
          title: "Identical Title",
          summary: "Identical Summary",
          sources: [],
          comment: "",
          written: false,
        },
      ],
    };

    backend({
      list_outline_variants: () => [
        { ...metaCurrent, id: "draft-a", label: "Draft A" },
        { ...metaDraftB, id: "draft-b", label: "Draft B" },
      ],
      get_outline_variant: (a) => (a.variantId === "draft-b" ? rightVariant : leftVariant),
    });
    mount();
    await vi.waitFor(() => expect(root.querySelector(".variant-compare")).toBeTruthy());
    button("Compare").click();
    await vi.waitFor(() => expect(root.querySelector(".compare-view")).toBeTruthy());

    const columns = root.querySelectorAll(".compare-column");
    expect(columns.length).toBe(2);

    // Left column checks
    const leftRows = columns[0].querySelectorAll(".compare-chapter");
    expect(leftRows[0].querySelector(".compare-diff-badge")?.textContent).toBe("Changed");
    expect(leftRows[1].querySelector(".compare-diff-badge")?.textContent).toBe("Only in this draft");
    expect(leftRows[2].querySelector(".compare-diff-badge")).toBeNull();

    // Right column checks
    const rightRows = columns[1].querySelectorAll(".compare-chapter");
    expect(rightRows[0].querySelector(".compare-diff-badge")?.textContent).toBe("Changed");
    expect(rightRows[1].querySelector(".compare-diff-badge")?.textContent).toBe("Only in this draft");
    expect(rightRows[2].querySelector(".compare-diff-badge")).toBeNull();
  });
});
