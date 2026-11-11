// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearMocks } from "@tauri-apps/api/mocks";
import contract from "./fixtures/reader-contract.json";
import annotationsContract from "./fixtures/annotations-contract.json";
import pdfContract from "./fixtures/pdf-source-contract.json";
import study from "./fixtures/study-contract.json";
import { mountReader, type ReaderOptions } from "./reader";
import type { PrefStorage } from "./study-prefs";
import { backend, memoryStorage } from "./study-test-backend";
import { stubWideMatchMedia } from "./test-media";

let root: HTMLElement;
let restoreMedia: (() => void) | undefined;

beforeEach(() => {
  restoreMedia = stubWideMatchMedia();
  document.body.innerHTML = '<div id="app"></div>';
  root = document.querySelector<HTMLElement>("#app")!;
});

afterEach(() => {
  restoreMedia?.();
  clearMocks();
  document.getSelection()?.removeAllRanges();
});

const course = contract.list_courses[0];
const panelOf = () => root.querySelector<HTMLElement>("aside.margin .study-panel")!;
const section = (name: string) => root.querySelector<HTMLElement>(`aside.margin [data-section="${name}"]`)!;

async function mount(options: ReaderOptions = { storage: null }): Promise<void> {
  await mountReader(root, course, options);
}

/** Resolves once every section has left "loading". */
async function settled(): Promise<void> {
  await vi.waitFor(() => {
    for (const name of ["goals", "sources", "notes", "answers"]) expect(section(name).dataset.state).not.toBe("loading");
  });
}

describe("the panel for a written chapter", () => {
  it("asks for the open chapter's context and answers by course and chapter id", async () => {
    const { sent } = backend();
    await mount();
    await settled();
    expect(sent("chapter_context")).toEqual([{ courseId: "bio101", chapterId: "cells" }]);
    expect(sent("list_answers")).toEqual([{ courseId: "bio101", chapterId: "cells" }]);
  });

  it("stacks goals, sources, notes and answers, top to bottom, in the right column", async () => {
    backend();
    await mount();
    await settled();
    const names = Array.from(panelOf().querySelectorAll<HTMLElement>("[data-section]")).map((s) => s.dataset.section);
    expect(names).toEqual(["goals", "sources", "notes", "answers"]);
    expect(root.firstElementChild!.lastElementChild).toBe(root.querySelector("aside.margin"));
  });

  it("shows the chapter's summary and goals as written", async () => {
    backend();
    await mount();
    await settled();
    const goals = section("goals");
    expect(goals.dataset.state).toBe("ready");
    expect(goals.querySelector("h2")?.textContent).toBe("Goals");
    expect(goals.querySelector(".study-summary")?.textContent).toBe("What a cell is, and why a boundary matters.");
    expect(Array.from(goals.querySelectorAll("li")).map((li) => li.textContent)).toEqual(study.chapter_context.objectives);
  });

  it("shows each source as a chip, and only a source that is in the library is a button", async () => {
    backend();
    await mount();
    await settled();
    const chips = Array.from(section("sources").querySelectorAll<HTMLElement>(".chip"));
    expect(chips.map((c) => [c.tagName, c.textContent])).toEqual([
      ["BUTTON", "cell-notes.pdf"],
      ["SPAN", "membranes.md"],
    ]);
  });

  it("opens a PDF chip in the PDF view, where the marks of that source go in the right column", async () => {
    const { sent } = backend({ read_pdf_source: () => pdfContract.read_pdf_source, list_source_annotations: () => [] });
    await mount();
    await settled();
    section("sources").querySelector<HTMLButtonElement>("button.chip")!.click();
    await vi.waitFor(() => expect(root.querySelector(".pdf-page")).not.toBeNull());
    expect(sent("read_pdf_source")).toEqual([{ courseId: course.id, name: "cell-notes.pdf" }]);
    expect(root.querySelector("aside.margin .study-panel")).toBeNull();
    expect(root.querySelector<HTMLElement>(".reader")!.dataset.panel).toBeUndefined();
  });

  it("lists the questions newest first, with the passage, the rendered answer and what it cited", async () => {
    backend();
    await mount();
    await settled();
    const items = Array.from(section("answers").querySelectorAll<HTMLElement>("article.answer"));
    expect(items.map((a) => a.querySelector(".answer-question")?.textContent)).toEqual([
      "Why is the cell called the basic unit?",
      "What should I remember about this chapter?",
    ]);
    expect(items[0].querySelector(".answer-quote")?.textContent).toBe("basic unit of life");
    expect(items[1].querySelector(".answer-quote")).toBeNull();
    expect(items[0].querySelector(".answer-body strong")?.textContent).toBe("one or more cells");
    expect(Array.from(items[1].querySelectorAll(".answer-body li")).map((li) => li.textContent)).toHaveLength(3);
    expect(items[0].querySelector(".answer-sources")?.textContent).toContain("cell-notes.pdf");
    expect(items[1].querySelector(".answer-sources")).toBeNull();
  });
});

describe("a section before, and instead of, its content", () => {
  it("reads loading while the commands are out, then settles", async () => {
    let release: (value: unknown) => void = () => {};
    backend({ chapter_context: () => new Promise((resolve) => (release = resolve)), list_answers: () => [] });
    await mount();
    expect(section("goals").dataset.state).toBe("loading");
    expect(section("sources").dataset.state).toBe("loading");
    await vi.waitFor(() => expect(section("answers").dataset.state).toBe("empty"));
    release(study.chapter_context);
    await vi.waitFor(() => expect(section("goals").dataset.state).toBe("ready"));
    expect(section("sources").dataset.state).toBe("ready");
  });

  it("says so when there are no goals, no sources, no notes and no questions", async () => {
    backend({ chapter_context: () => ({ objectives: [], summary: "", sources: [] }), list_answers: () => [] });
    await mount();
    await settled();
    for (const name of ["goals", "sources", "notes", "answers"]) expect(section(name).dataset.state, name).toBe("empty");
    expect(section("goals").textContent).toContain("No goals");
    expect(section("sources").textContent).toContain("No sources");
    expect(section("notes").textContent).toContain("Select a passage");
    expect(section("answers").textContent).toContain("No questions yet");
  });

  it("shows why goals and sources failed, keeps the questions, and reads again on Try again", async () => {
    let healthy = false;
    const { sent } = backend({
      chapter_context: () => {
        if (!healthy) throw "no such chapter in the outline";
        return study.chapter_context;
      },
    });
    await mount();
    await settled();
    for (const name of ["goals", "sources"]) {
      expect(section(name).dataset.state).toBe("error");
      expect(section(name).querySelector('[role="alert"]')?.textContent).toContain("no such chapter in the outline");
    }
    expect(section("answers").dataset.state).toBe("ready");
    healthy = true;
    section("goals").querySelector<HTMLButtonElement>("button.retry")!.click();
    await vi.waitFor(() => expect(section("goals").dataset.state).toBe("ready"));
    expect(section("sources").dataset.state).toBe("ready");
    expect(sent("chapter_context")).toHaveLength(2);
  });

  it("shows why the questions failed without touching the goals", async () => {
    backend({
      list_answers: () => {
        throw new Error("the store is locked");
      },
    });
    await mount();
    await settled();
    expect(section("answers").dataset.state).toBe("error");
    expect(section("answers").querySelector('[role="alert"]')?.textContent).toContain("the store is locked");
    expect(section("goals").dataset.state).toBe("ready");
  });

  it("gives the notes their own state: empty with none, a count with some, and the error if they cannot load", async () => {
    backend();
    await mount();
    await settled();
    expect(section("notes").dataset.state).toBe("empty");
    clearMocks();
    document.body.innerHTML = '<div id="app"></div>';
    root = document.querySelector<HTMLElement>("#app")!;
    backend({ list_annotations: () => annotationsContract.list_annotations });
    await mount();
    await settled();
    expect(section("notes").dataset.state).toBe("ready");
    expect(section("notes").querySelectorAll(".note-card")).toHaveLength(1);
    clearMocks();
    document.body.innerHTML = '<div id="app"></div>';
    root = document.querySelector<HTMLElement>("#app")!;
    backend({
      list_annotations: () => {
        throw new Error("marks unavailable");
      },
    });
    await mount();
    await settled();
    expect(section("notes").dataset.state).toBe("error");
    expect(section("notes").textContent).toContain("marks unavailable");
  });
});

describe("answers are agent text, rendered only through the markdown renderer", () => {
  const hostile = {
    ...study.answers[0],
    question: "<b>q</b>",
    quote: "<i>quote</i>",
    answer:
      'Plain **bold**.\n\n<script>window.__pwned = true</script><img src=x onerror="window.__pwned = true">\n\n[click](javascript:window.__pwned=true) and [ok](https://example.com)',
    sources: ["<u>x.md</u>"],
  };

  it("escapes raw html, drops a javascript: link, and shows question, quote and source names as text", async () => {
    backend({ list_answers: () => [hostile] });
    await mount();
    await settled();
    const body = section("answers").querySelector<HTMLElement>(".answer-body")!;
    expect(body.querySelector("strong")?.textContent).toBe("bold");
    expect(body.querySelector("script, img")).toBeNull();
    expect(body.textContent).toContain("<script>");
    const hrefs = Array.from(body.querySelectorAll("a")).map((a) => a.getAttribute("href"));
    expect(hrefs).toEqual(["https://example.com"]);
    expect(section("answers").querySelector(".answer-question")?.textContent).toBe("<b>q</b>");
    expect(section("answers").querySelector(".answer-quote")?.textContent).toBe("<i>quote</i>");
    expect(section("answers").querySelector(".answer-sources")?.textContent).toContain("<u>x.md</u>");
    expect(section("answers").querySelector("b, i, u")).toBeNull();
    expect((window as unknown as { __pwned?: boolean }).__pwned).toBeUndefined();
  });
});

describe("asking about the whole chapter from the panel", () => {
  const box = () => panelOf().querySelector<HTMLFormElement>("form.ask-box")!;
  const field = () => box().querySelector<HTMLTextAreaElement>("textarea")!;
  const status = () => box().querySelector<HTMLElement>(".ask-status")!;

  function type(text: string): void {
    field().value = text;
    field().dispatchEvent(new Event("input", { bubbles: true }));
  }

  function submit(): void {
    box().dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  }

  it("sends the question with a null quote, shows the answer first, and clears the box", async () => {
    const { sent } = backend();
    await mount();
    await settled();
    type("How does a cell stay different from its surroundings?");
    submit();
    expect(field().disabled).toBe(true);
    expect(status().textContent).toContain("Asking");
    await vi.waitFor(() => expect(section("answers").querySelectorAll("article.answer")).toHaveLength(3));
    expect(sent("ask_about")).toEqual([
      {
        courseId: "bio101",
        chapterId: "cells",
        quote: null,
        question: "How does a cell stay different from its surroundings?",
      },
    ]);
    const first = section("answers").querySelector("article.answer")!;
    expect(first.querySelector(".answer-question")?.textContent).toBe("How does a cell stay different from its surroundings?");
    expect(first.querySelector(".answer-quote")).toBeNull();
    expect(field().value).toBe("");
    expect(field().disabled).toBe(false);
  });

  it("starts the list from the first answer when the chapter had none", async () => {
    backend({ list_answers: () => [] });
    await mount();
    await settled();
    expect(section("answers").dataset.state).toBe("empty");
    type("Anything?");
    submit();
    await vi.waitFor(() => expect(section("answers").dataset.state).toBe("ready"));
    expect(section("answers").querySelectorAll("article.answer")).toHaveLength(1);
  });

  it("does not send an empty question", async () => {
    const { sent } = backend();
    await mount();
    await settled();
    type("   ");
    submit();
    expect(sent("ask_about")).toEqual([]);
    expect(field().disabled).toBe(false);
  });

  it("keeps the question and shows why when the job fails, and sends it again on the next submit", async () => {
    let healthy = false;
    const { sent } = backend({
      ask_about: () => {
        if (!healthy) throw "the agent exited with status 1";
        return study.ask_about_whole_chapter;
      },
    });
    await mount();
    await settled();
    type("Will this fail?");
    submit();
    await vi.waitFor(() => expect(status().querySelector('[role="alert"]')).not.toBeNull());
    expect(status().textContent).toContain("the agent exited with status 1");
    expect(field().value).toBe("Will this fail?");
    expect(field().disabled).toBe(false);
    expect(section("answers").querySelectorAll("article.answer")).toHaveLength(2);
    healthy = true;
    submit();
    await vi.waitFor(() => expect(section("answers").querySelectorAll("article.answer")).toHaveLength(3));
    expect(sent("ask_about")).toHaveLength(2);
    expect(field().value).toBe("");
  });

  it("sends one job for a double submit", async () => {
    let release: (value: unknown) => void = () => {};
    const { sent } = backend({ ask_about: () => new Promise((resolve) => (release = resolve)) });
    await mount();
    await settled();
    type("Once only");
    submit();
    submit();
    expect(sent("ask_about")).toHaveLength(1);
    release(study.ask_about_whole_chapter);
    await vi.waitFor(() => expect(field().disabled).toBe(false));
  });

  it("submits on Enter, and leaves Shift+Enter for a new line", async () => {
    const { sent } = backend();
    await mount();
    await settled();
    type("Enter sends");
    const shift = new KeyboardEvent("keydown", { key: "Enter", shiftKey: true, bubbles: true, cancelable: true });
    field().dispatchEvent(shift);
    expect(shift.defaultPrevented).toBe(false);
    expect(sent("ask_about")).toEqual([]);
    const plain = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
    field().dispatchEvent(plain);
    expect(plain.defaultPrevented).toBe(true);
    expect(sent("ask_about")).toHaveLength(1);
  });

  it("prepends only the new answer node without rebuilding existing cards and scrolls it into view", async () => {
    backend({
      ask_about: (args) => ({
        ...study.ask_about_whole_chapter,
        id: 999,
        question: args.question as string,
      }),
    });
    await mount();
    await settled();
    const existingCards = Array.from(section("answers").querySelectorAll<HTMLElement>("article.answer"));
    expect(existingCards).toHaveLength(2);
    const scrollSpy = vi.fn();
    HTMLElement.prototype.scrollIntoView = scrollSpy;

    type("How does active transport differ?");
    submit();
    await vi.waitFor(() => expect(section("answers").querySelectorAll("article.answer")).toHaveLength(3));

    const updatedCards = Array.from(section("answers").querySelectorAll<HTMLElement>("article.answer"));
    expect(updatedCards[0].querySelector(".answer-question")?.textContent).toBe("How does active transport differ?");
    expect(updatedCards[1]).toBe(existingCards[0]);
    expect(updatedCards[2]).toBe(existingCards[1]);
    expect(scrollSpy).toHaveBeenCalled();
  });
});

describe("collapsing sections", () => {
  it("allows panel sections to collapse and expand", async () => {
    backend();
    await mount();
    await settled();
    const goalsSection = section("goals");
    const head = goalsSection.querySelector<HTMLElement>(".section-head")!;
    const body = goalsSection.querySelector<HTMLElement>(".section-body")!;
    expect(goalsSection.dataset.collapsed).toBeUndefined();
    expect(body.hidden).toBe(false);

    head.click();
    expect(goalsSection.dataset.collapsed).toBe("true");
    expect(body.hidden).toBe(true);

    head.click();
    expect(goalsSection.dataset.collapsed).toBe("false");
    expect(body.hidden).toBe(false);
  });
});

describe("collapsing the panel", () => {
  const toggle = () => panelOf().querySelector<HTMLButtonElement>("button.panel-toggle")!;
  const body = () => panelOf().querySelector<HTMLElement>(".study-body")!;

  it("starts open, hides the body and keeps only the toggle when collapsed, and opens again", async () => {
    backend();
    await mount();
    await settled();
    expect(body().hidden).toBe(false);
    expect(toggle().getAttribute("aria-expanded")).toBe("true");
    expect(root.querySelector<HTMLElement>(".reader")!.dataset.panel).toBe("open");
    toggle().click();
    expect(body().hidden).toBe(true);
    expect(toggle().getAttribute("aria-expanded")).toBe("false");
    expect(toggle().getAttribute("aria-label")).toBe("Show the study panel");
    expect(root.querySelector<HTMLElement>(".reader")!.dataset.panel).toBe("collapsed");
    toggle().click();
    expect(body().hidden).toBe(false);
    expect(toggle().getAttribute("aria-label")).toBe("Hide the study panel");
  });

  it("remembers the choice for the next chapter and the next mount", async () => {
    const storage = memoryStorage();
    backend();
    await mount({ storage });
    await settled();
    toggle().click();
    expect(storage.held.size).toBe(1);
    root.querySelector<HTMLButtonElement>('nav button[data-chapter-id="membranes"]')!.click();
    await vi.waitFor(() => expect(root.querySelector("aside.margin .study-panel")).not.toBeNull());
    await vi.waitFor(() => expect(body().hidden).toBe(true));
    clearMocks();
    document.body.innerHTML = '<div id="app"></div>';
    root = document.querySelector<HTMLElement>("#app")!;
    backend();
    await mount({ storage });
    expect(body().hidden).toBe(true);
    expect(toggle().getAttribute("aria-expanded")).toBe("false");
  });

  it("still collapses when storage throws on every read and write", async () => {
    const blocked: PrefStorage = {
      getItem: () => {
        throw new Error("SecurityError");
      },
      setItem: () => {
        throw new Error("QuotaExceededError");
      },
    };
    backend();
    await mount({ storage: blocked });
    await settled();
    toggle().click();
    expect(body().hidden).toBe(true);
  });
});

describe("moving between chapters", () => {
  it("loads the new chapter's context and answers, and ignores a late reply for the one left", async () => {
    let releaseCells: (value: unknown) => void = () => {};
    const { sent } = backend({
      chapter_context: (args) =>
        args.chapterId === "cells"
          ? new Promise((resolve) => (releaseCells = resolve))
          : { objectives: ["Describe diffusion"], summary: "", sources: [] },
      list_answers: (args) => (args.chapterId === "cells" ? [] : study.answers),
    });
    await mount();
    root.querySelector<HTMLButtonElement>('nav button[data-chapter-id="membranes"]')!.click();
    await vi.waitFor(() => expect(sent("chapter_context")).toHaveLength(2));
    await vi.waitFor(() => expect(section("goals").dataset.state).toBe("ready"));
    releaseCells(study.chapter_context);
    await new Promise((r) => setTimeout(r, 0));
    expect(Array.from(section("goals").querySelectorAll("li")).map((li) => li.textContent)).toEqual(["Describe diffusion"]);
    expect(sent("chapter_context")[1]).toEqual({ courseId: "bio101", chapterId: "membranes" });
  });

  it("has no panel while a chapter is loading, and none on the empty or placeholder views", async () => {
    backend({ list_chapters: () => contract.list_chapters, read_chapter: () => contract.read_chapter_placeholder });
    await mount();
    expect(root.querySelector("aside.margin")!.children.length).toBe(0);
  });
});
