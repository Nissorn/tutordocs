// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearMocks } from "@tauri-apps/api/mocks";
import contract from "./fixtures/reader-contract.json";
import study from "./fixtures/study-contract.json";
import type { JobFeed } from "./job-feed";
import { mountJobDrawer } from "./job-drawer";
import { resetJobLabels } from "./job-label";
import type { JobRecord } from "./jobs";
import { mountReader, type ReaderOptions } from "./reader";
import { openSelectionPopup, type SelectionActions, type SelectionPopup } from "./selection-popup";
import { backend, memoryStorage } from "./study-test-backend";

let root: HTMLElement;

beforeEach(() => {
  document.body.innerHTML = '<div id="app"></div>';
  root = document.querySelector<HTMLElement>("#app")!;
});

afterEach(() => {
  resetJobLabels();
  clearMocks();
  document.getSelection()?.removeAllRanges();
});

const prose = () => root.querySelector<HTMLElement>("main.document .prose")!;
const popup = () => root.querySelector<HTMLElement>(".mark-popover");
const action = (name: "fix" | "ask") => root.querySelector<HTMLButtonElement>(`.mark-popover button[data-action="${name}"]`)!;
const field = () => root.querySelector<HTMLTextAreaElement>(".mark-popover textarea")!;
const status = () => root.querySelector<HTMLElement>(".mark-popover .popup-status")!;
const answers = () => Array.from(root.querySelectorAll<HTMLElement>('aside.margin [data-section="answers"] article.answer'));

async function mount(options: ReaderOptions = { storage: null }): Promise<void> {
  await mountReader(root, contract.list_courses[0], options);
  await vi.waitFor(() => expect(root.querySelector('aside.margin [data-section="answers"]')?.getAttribute("data-state")).toBe("ready"));
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

/** Selects "basic unit" in the chapter and releases the mouse, as a reader would. */
function selectPassage(text = "basic unit"): void {
  const walker = document.createTreeWalker(prose(), NodeFilter.SHOW_TEXT);
  let line: Text | null = null;
  for (let node = walker.nextNode(); node && !line; node = walker.nextNode()) {
    if ((node as Text).data.includes(text)) line = node as Text;
  }
  const range = document.createRange();
  range.setStart(line!, line!.data.indexOf(text));
  range.setEnd(line!, line!.data.indexOf(text) + text.length);
  const selection = document.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
  prose().dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
}

/** What the focus moving into the box does to the page's selection. */
function loseSelection(): void {
  document.getSelection()!.removeAllRanges();
  document.dispatchEvent(new Event("selectionchange"));
}

function type(box: HTMLTextAreaElement, text: string): void {
  box.value = text;
  box.dispatchEvent(new Event("input", { bubbles: true }));
}

function submit(): void {
  root.querySelector<HTMLFormElement>(".mark-popover form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
}

describe("the popup on a selection", () => {
  it("offers five highlight colors, then Fix this section and Ask", async () => {
    backend();
    await mount();
    selectPassage();
    const buttons = Array.from(popup()!.querySelectorAll("button"));
    expect(buttons.map((b) => b.dataset.color ?? b.textContent)).toEqual([
      "yellow",
      "green",
      "blue",
      "pink",
      "purple",
      "Fix this section",
      "Ask",
    ]);
    expect(popup()!.getAttribute("role")).toBe("toolbar");
  });

  it("closes when the selection goes while only the toolbar is showing", async () => {
    backend();
    await mount();
    selectPassage();
    loseSelection();
    expect(popup()).toBeNull();
  });
});

describe("Fix this section", () => {
  it("opens a box that names the passage and asks what should change", async () => {
    backend();
    await mount();
    selectPassage();
    action("fix").click();
    expect(popup()!.getAttribute("role")).toBe("dialog");
    expect(popup()!.getAttribute("aria-label")).toBe("Fix this section");
    expect(popup()!.querySelector(".popup-quote")?.textContent).toBe("basic unit");
    expect(field().getAttribute("aria-label")).toBe("What should change in this section?");
    expect(popup()!.querySelector("button[data-color]")).toBeNull();
  });

  it("stays open when the selection goes, because the box has taken the focus", async () => {
    backend();
    await mount();
    selectPassage();
    action("fix").click();
    loseSelection();
    expect(popup()).not.toBeNull();
  });

  it("sends the passage and the instruction, shows the job running, and shows the revised chapter on success", async () => {
    let release: (value: unknown) => void = () => {};
    const { sent } = backend({ revise_section: () => new Promise((resolve) => (release = resolve)) });
    await mount();
    prose().closest<HTMLElement>("main.document")!.scrollTop = 240;
    selectPassage();
    action("fix").click();
    loseSelection();
    type(field(), "  Add an example of a cell  ");
    submit();
    expect(sent("revise_section")).toEqual([
      { courseId: "bio101", chapterId: "cells", quote: "basic unit", instruction: "Add an example of a cell" },
    ]);
    expect(popup()!.dataset.phase).toBe("running");
    expect(status().textContent).toContain("Revising this section");
    expect(field().disabled).toBe(true);
    expect(field().value).toBe("Add an example of a cell");
    submit();
    expect(sent("revise_section")).toHaveLength(1);

    release(study.revise_section);
    await vi.waitFor(() => expect(prose().textContent).toContain("A membrane keeps the inside different"));
    expect(popup()).toBeNull();
    expect(prose().closest<HTMLElement>("main.document")!.scrollTop).toBe(240);
    await vi.waitFor(() => expect(root.querySelectorAll('aside.margin [data-section="goals"]')).toHaveLength(1));
    expect(sent("chapter_context")).toHaveLength(2);
  });

  it("labels that job Section revise in the drawer and leaves its kind as rewrite", async () => {
    const feed = liveFeed();
    let release: (value: unknown) => void = () => {};
    backend({ revise_section: () => new Promise((resolve) => (release = resolve)) });
    await mount({ storage: null, jobs: feed });
    selectPassage();
    action("fix").click();
    type(field(), "Add an example");
    submit();
    feed.add({
      id: "job-7",
      courseId: "bio101",
      kind: "rewrite",
      driver: "claude",
      model: null,
      effort: null,
      command: "claude -p <prompt, 20 chars>",
      startedAt: "2026-11-09T12:00:00Z",
      endedAt: null,
      status: "running",
      lines: [],
    });
    const drawer = mountJobDrawer(root, feed, { courses: () => [], onClose: () => {} });
    drawer.open();
    const shown = root.querySelector("aside.job-drawer .job-kind")!;
    expect(shown.textContent).toBe("Section revise");
    expect(feed.jobs()[0].kind).toBe("rewrite");
    release(study.revise_section);
    await vi.waitFor(() => expect(popup()).toBeNull());
  });

  it("keeps the chapter and the instruction when the job fails, and sends again on Try again", async () => {
    let healthy = false;
    const { sent } = backend({
      revise_section: () => {
        if (!healthy) throw "the agent exited with status 1";
        return study.revise_section;
      },
    });
    await mount();
    selectPassage();
    action("fix").click();
    type(field(), "Shorter");
    submit();
    await vi.waitFor(() => expect(popup()!.dataset.phase).toBe("failed"));
    expect(status().querySelector('[role="alert"]')?.textContent).toContain("the agent exited with status 1");
    expect(prose().textContent).toContain("The cell is the basic unit of life.");
    expect(field().value).toBe("Shorter");
    expect(popup()!.querySelector<HTMLButtonElement>(".popup-submit")!.textContent).toBe("Try again");
    healthy = true;
    submit();
    await vi.waitFor(() => expect(prose().textContent).toContain("A membrane keeps the inside different"));
    expect(sent("revise_section")).toHaveLength(2);
  });

  it("does not send an empty instruction", async () => {
    const { sent } = backend();
    await mount();
    selectPassage();
    action("fix").click();
    type(field(), "   ");
    submit();
    expect(sent("revise_section")).toEqual([]);
    expect(popup()!.dataset.phase).toBe("editing");
  });

  it("closes on Cancel and on Escape, but not while the job runs", async () => {
    let release: (value: unknown) => void = () => {};
    backend({ revise_section: () => new Promise((resolve) => (release = resolve)) });
    await mount();
    selectPassage();
    action("fix").click();
    popup()!.querySelector<HTMLButtonElement>(".popup-cancel")!.click();
    expect(popup()).toBeNull();

    selectPassage();
    action("fix").click();
    field().dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(popup()).toBeNull();

    selectPassage();
    action("fix").click();
    type(field(), "Go");
    submit();
    popup()!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(popup()).not.toBeNull();
    selectPassage();
    expect(root.querySelectorAll(".mark-popover")).toHaveLength(1);
    expect(popup()!.dataset.phase).toBe("running");
    release(study.revise_section);
    await vi.waitFor(() => expect(popup()).toBeNull());
  });

  it("submits on Enter and takes Shift+Enter as a new line", async () => {
    const { sent } = backend();
    await mount();
    selectPassage();
    action("fix").click();
    type(field(), "Add a diagram");
    const shift = new KeyboardEvent("keydown", { key: "Enter", shiftKey: true, bubbles: true, cancelable: true });
    field().dispatchEvent(shift);
    expect(shift.defaultPrevented).toBe(false);
    expect(sent("revise_section")).toEqual([]);
    field().dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    expect(sent("revise_section")).toHaveLength(1);
  });
});

describe("Ask about a passage", () => {
  it("opens a box that names the passage and asks for the question", async () => {
    backend();
    await mount();
    selectPassage();
    action("ask").click();
    expect(popup()!.getAttribute("aria-label")).toBe("Ask about this passage");
    expect(popup()!.querySelector(".popup-quote")?.textContent).toBe("basic unit");
    expect(field().getAttribute("aria-label")).toBe("Your question about this passage");
  });

  it("sends the passage and the question, then shows the rendered answer here and files it in the panel", async () => {
    const { sent } = backend({ ask_about: () => study.answers[0] });
    await mount();
    selectPassage();
    action("ask").click();
    loseSelection();
    type(field(), "Why is the cell called the basic unit?");
    submit();
    expect(sent("ask_about")).toEqual([
      { courseId: "bio101", chapterId: "cells", quote: "basic unit", question: "Why is the cell called the basic unit?" },
    ]);
    expect(popup()!.dataset.phase).toBe("running");
    expect(status().textContent).toContain("Asking");
    await vi.waitFor(() => expect(popup()!.dataset.phase).toBe("done"));
    expect(popup()!.querySelector(".popup-answer strong")?.textContent).toBe("one or more cells");
    expect(popup()!.querySelector(".popup-sources")?.textContent).toContain("cell-notes.pdf");
    expect(answers()[0].querySelector(".answer-question")?.textContent).toBe("Why is the cell called the basic unit?");
    expect(answers()).toHaveLength(2);
    popup()!.querySelector<HTMLButtonElement>(".popup-close")!.click();
    expect(popup()).toBeNull();
  });

  it("renders the answer only through the markdown renderer", async () => {
    backend({
      ask_about: () => ({ ...study.answers[0], answer: '<img src=x onerror="window.__pwned = true">\n\n[go](javascript:window.__pwned=true)' }),
    });
    await mount();
    selectPassage();
    action("ask").click();
    type(field(), "Anything?");
    submit();
    await vi.waitFor(() => expect(popup()!.dataset.phase).toBe("done"));
    const answer = popup()!.querySelector(".popup-answer")!;
    expect(answer.querySelector("img")).toBeNull();
    expect(answer.textContent).toContain("<img");
    expect(answer.querySelector("a")).toBeNull();
    expect((window as unknown as { __pwned?: boolean }).__pwned).toBeUndefined();
  });

  it("shows why the job failed, keeps the question, and offers another try", async () => {
    backend({
      ask_about: () => {
        throw new Error("no sources could be read");
      },
    });
    await mount();
    selectPassage();
    action("ask").click();
    type(field(), "Will it fail?");
    submit();
    await vi.waitFor(() => expect(popup()!.dataset.phase).toBe("failed"));
    expect(status().querySelector('[role="alert"]')?.textContent).toContain("no sources could be read");
    expect(field().value).toBe("Will it fail?");
    expect(answers()).toHaveLength(2);
  });

  it("asks another from the answer, with the same passage", async () => {
    const { sent } = backend();
    await mount();
    selectPassage();
    action("ask").click();
    type(field(), "First?");
    submit();
    await vi.waitFor(() => expect(popup()!.dataset.phase).toBe("done"));
    popup()!.querySelector<HTMLButtonElement>(".popup-again")!.click();
    expect(popup()!.dataset.phase).toBe("editing");
    expect(popup()!.querySelector(".popup-quote")?.textContent).toBe("basic unit");
    type(field(), "Second?");
    submit();
    await vi.waitFor(() => expect(sent("ask_about")).toHaveLength(2));
    expect(sent("ask_about")[1].quote).toBe("basic unit");
  });
});

describe("anchoring beside the passage", () => {
  let host: HTMLElement;
  let handle: SelectionPopup | null = null;
  let boxHeight = 34;

  beforeEach(() => {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 1280 });
    Object.defineProperty(window, "innerHeight", { configurable: true, value: 800 });
    host = document.createElement("div");
    document.body.append(host);
    boxHeight = 34;
  });

  afterEach(() => {
    handle?.close();
    handle = null;
    host.remove();
  });

  /** Opens the popup on a known rect and stubs the layout happy-dom does not do. */
  function open(rect: DOMRect, actions?: Partial<SelectionActions>): HTMLElement {
    handle = openSelectionPopup({
      host,
      rect,
      quote: "basic unit",
      onColor: () => {},
      actions: { fix: async () => {}, ask: async () => study.answers[0], ...actions },
      onClose: () => {},
    });
    const node = host.querySelector<HTMLElement>(".mark-popover")!;
    Object.defineProperty(node, "offsetWidth", { configurable: true, value: 360 });
    Object.defineProperty(node, "offsetHeight", { configurable: true, get: () => boxHeight });
    return node;
  }

  const askButton = (node: HTMLElement) => node.querySelector<HTMLButtonElement>('button[data-action="ask"]')!;
  const askField = (node: HTMLElement) => node.querySelector<HTMLTextAreaElement>("textarea")!;
  const submitForm = (node: HTMLElement) =>
    node.querySelector<HTMLFormElement>("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));

  it("keeps a tall answer beside the passage instead of jumping to the viewport top", async () => {
    let release!: (answer: typeof study.answers[0]) => void;
    const node = open(new DOMRect(400, 300, 160, 20), { ask: () => new Promise((resolve) => (release = resolve)) });
    boxHeight = 220;
    askButton(node).click();
    askField(node).value = "Why?";
    askField(node).dispatchEvent(new Event("input", { bubbles: true }));
    submitForm(node);
    boxHeight = 600;
    release(study.answers[0]);
    await vi.waitFor(() => expect(node.dataset.phase).toBe("done"));
    expect(node.style.top).toBe("326px");
  });

  it("stays below the passage when neither side fits, never clamped to the viewport top", () => {
    Object.defineProperty(window, "innerHeight", { configurable: true, value: 400 });
    const node = open(new DOMRect(400, 150, 160, 20));
    boxHeight = 220;
    askButton(node).click();
    expect(node.style.top).toBe("176px");
  });

  it("flips above the passage only when that side fits", () => {
    Object.defineProperty(window, "innerHeight", { configurable: true, value: 400 });
    const node = open(new DOMRect(400, 300, 160, 20));
    boxHeight = 120;
    askButton(node).click();
    expect(node.style.top).toBe("174px");
  });

  it("moves with the document and ignores a scroll inside the answer", async () => {
    host.style.overflow = "auto";
    host.style.height = "200px";
    host.style.width = "200px";
    const spacer = document.createElement("div");
    spacer.style.height = "2000px";
    spacer.style.width = "2000px";
    host.append(spacer);

    let release!: (answer: typeof study.answers[0]) => void;
    const node = open(new DOMRect(400, 300, 160, 20), { ask: () => new Promise((resolve) => (release = resolve)) });
    boxHeight = 220;
    askButton(node).click();
    askField(node).value = "Why?";
    askField(node).dispatchEvent(new Event("input", { bubbles: true }));
    submitForm(node);
    expect(handle!.busy()).toBe(true);

    host.scrollTop = 80;
    host.scrollLeft = 30;
    host.dispatchEvent(new Event("scroll"));
    expect(node.isConnected).toBe(true);
    expect(node.style.top).toBe("246px");
    expect(node.style.left).toBe("370px");

    boxHeight = 600;
    release(study.answers[0]);
    await vi.waitFor(() => expect(node.dataset.phase).toBe("done"));
    // The answer grew. The popup stays where the scroll left it.
    expect(node.style.top).toBe("246px");

    const answer = node.querySelector<HTMLElement>(".popup-answer")!;
    answer.style.overflow = "auto";
    answer.style.height = "40px";
    const inner = document.createElement("div");
    inner.style.height = "800px";
    answer.append(inner);
    answer.scrollTop = 40;
    answer.dispatchEvent(new Event("scroll", { bubbles: true }));
    expect(node.style.top).toBe("246px");
    expect(node.style.left).toBe("370px");
  });
});

describe("the actions without the panel", () => {
  it("asks and shows the answer in the popup while the panel is collapsed", async () => {
    const storage = memoryStorage({ "tutordocs.studyPanel.v1": "collapsed" });
    const { sent } = backend();
    await mount({ storage });
    expect(root.querySelector<HTMLElement>(".study-body")!.hidden).toBe(true);
    selectPassage();
    action("ask").click();
    type(field(), "Still works?");
    submit();
    await vi.waitFor(() => expect(popup()!.dataset.phase).toBe("done"));
    expect(popup()!.querySelector(".popup-answer")?.textContent).toContain("Its membrane controls what crosses");
    expect(sent("ask_about")).toHaveLength(1);
  });

  it("fixes a section while the whole column is hidden, as it is below 1080px", async () => {
    backend();
    await mount();
    root.querySelector<HTMLElement>("aside.margin")!.style.display = "none";
    selectPassage();
    action("fix").click();
    type(field(), "Add an example");
    submit();
    await vi.waitFor(() => expect(prose().textContent).toContain("A membrane keeps the inside different"));
  });

  it("renders disabled Ask and visible reason text next to it when askDisabledReason is set", () => {
    const host = document.createElement("div");
    root.append(host);
    const rect = { left: 100, top: 100, bottom: 120, right: 200, width: 100, height: 20 } as DOMRect;
    const p = openSelectionPopup({
      host,
      rect,
      quote: "sample text",
      onColor: () => {},
      actions: {},
      askDisabledReason: "No chapter is written yet",
      onClose: () => {},
    });
    const askBtn = root.querySelector<HTMLButtonElement>('button[data-action="ask"]')!;
    expect(askBtn).not.toBeNull();
    expect(askBtn.disabled).toBe(true);
    const reason = root.querySelector<HTMLElement>(".popover-disabled-reason")!;
    expect(reason).not.toBeNull();
    expect(reason.textContent).toBe("No chapter is written yet");
    p.close();
  });

  it("shows only swatches without divider, fix or ask when actions are absent and no disabled reason is set", () => {
    const host = document.createElement("div");
    root.append(host);
    const rect = { left: 100, top: 100, bottom: 120, right: 200, width: 100, height: 20 } as DOMRect;
    const p = openSelectionPopup({
      host,
      rect,
      quote: "sample text",
      onColor: () => {},
      actions: {},
      onClose: () => {},
    });
    expect(root.querySelector(".popover-divider")).toBeNull();
    expect(root.querySelector('button[data-action="fix"]')).toBeNull();
    expect(root.querySelector('button[data-action="ask"]')).toBeNull();
    expect(root.querySelectorAll("button.swatch")).toHaveLength(5);
    p.close();
  });
});
