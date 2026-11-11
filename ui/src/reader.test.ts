// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import contract from "./fixtures/reader-contract.json";
import quizContract from "./fixtures/quiz-contract.json";
import wizardContract from "./fixtures/wizard-contract.json";
import { mountReader } from "./reader";
import { mountLibraryTools } from "./library-tools";
import { stubWideMatchMedia } from "./test-media";

type Handler = (cmd: string, args: Record<string, unknown>) => unknown;

/** The Rust side of the contract: replays the shapes the Rust tests pin. */
function contractBackend(overrides: Partial<Record<string, Handler>> = {}): void {
  const defaults: Record<string, Handler> = {
    get_outline: () => wizardContract.outline_approved,
    list_chapters: () => contract.list_chapters,
    read_chapter: (_cmd, args) =>
      args.chapterId === "cells" ? contract.read_chapter_ready : contract.read_chapter_placeholder,
    list_annotations: () => [],
    list_course_sources: () => [],
    chapter_context: () => ({ objectives: [], summary: "", sources: [] }),
    list_answers: () => [],
  };
  mockIPC((cmd, args) => {
    const handler = overrides[cmd] ?? defaults[cmd];
    if (!handler) throw new Error(`unexpected command ${cmd}`);
    return handler(cmd, (args ?? {}) as Record<string, unknown>);
  });
}

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
});

function chapterButton(id: string): HTMLButtonElement {
  const button = root.querySelector<HTMLButtonElement>(`nav button[data-chapter-id="${id}"]`);
  if (!button) throw new Error(`no sidebar button for ${id}`);
  return button;
}

describe("three-column structure", () => {
  it("lays out sidebar, document and margin as three sibling columns", async () => {
    contractBackend();
    await mountReader(root, contract.list_courses[0]);
    const columns = Array.from(root.firstElementChild!.children).map((el) => el.tagName + "." + el.className);
    expect(columns).toEqual(["NAV.sidebar", "MAIN.document", "ASIDE.margin"]);
  });

  it("lists the course and its chapters from the command results, in order", async () => {
    contractBackend();
    await mountReader(root, contract.list_courses[0]);
    expect(root.querySelector("nav .course-title")?.textContent).toBe("Cell Biology");
    const rows = Array.from(root.querySelectorAll("nav button.chapter")).map((b) => b.textContent);
    expect(rows).toEqual(["Cells", "Membranes"]);
  });

  it("marks the placeholder chapter in the sidebar and keeps it clickable", async () => {
    contractBackend();
    await mountReader(root, contract.list_courses[0]);
    const placeholder = chapterButton("membranes");
    expect(placeholder.dataset.state).toBe("placeholder");
    expect(placeholder.disabled).toBe(false);
    expect(placeholder.title).toBe("Writes on open");
    expect(placeholder.getAttribute("aria-description")).toBe("Writes on open");
    expect(chapterButton("cells").dataset.state).toBe("ready");
  });
});

describe("opening a chapter", () => {
  it("opens the first written chapter on launch and renders its markdown", async () => {
    contractBackend();
    await mountReader(root, contract.list_courses[0]);
    const doc = root.querySelector<HTMLElement>("main.document")!;
    expect(doc.dataset.state).toBe("ready");
    expect(doc.querySelector("h1")?.textContent).toBe("Cells");
    expect(Array.from(doc.querySelectorAll("li")).map((li) => li.textContent)).toEqual(["Nucleus", "Membrane"]);
    expect(chapterButton("cells").getAttribute("aria-current")).toBe("page");
  });

  it("starts writing, not an error, when a placeholder chapter is clicked", async () => {
    contractBackend({ open_chapter: () => new Promise(() => {}) });
    await mountReader(root, contract.list_courses[0]);
    chapterButton("membranes").click();
    const doc = root.querySelector<HTMLElement>("main.document")!;
    await vi.waitFor(() => expect(doc.dataset.state).toBe("writing"));
    expect(doc.querySelector("h1")?.textContent).toBe("Membranes");
    expect(doc.querySelector('[role="status"]')?.textContent).toContain("Starting the agent");
    expect(doc.querySelector('[role="alert"]')).toBeNull();
    expect(chapterButton("membranes").getAttribute("aria-current")).toBe("page");
    expect(chapterButton("cells").hasAttribute("aria-current")).toBe(false);
  });
});

describe("failure and race behavior", () => {
  it("says so in the document column when the course cannot be opened", async () => {
    contractBackend({
      list_chapters: () => {
        throw new Error("not a TutorDocs library at /nowhere: missing or unreadable course.json");
      },
    });
    await mountReader(root, contract.list_courses[0]);
    const doc = root.querySelector<HTMLElement>("main.document")!;
    expect(doc.dataset.state).toBe("error");
    expect(doc.querySelector('[role="alert"]')?.textContent).toContain("not a TutorDocs library");
  });

  it("shows a failed chapter read as an error, distinct from a placeholder", async () => {
    contractBackend({
      read_chapter: (_cmd, args) => {
        if (args.chapterId === "membranes") throw new Error("cannot read index.md");
        return contract.read_chapter_ready;
      },
    });
    await mountReader(root, contract.list_courses[0]);
    chapterButton("membranes").click();
    const doc = root.querySelector<HTMLElement>("main.document")!;
    await vi.waitFor(() => expect(doc.dataset.state).toBe("error"));
    expect(doc.querySelector('[role="alert"]')?.textContent).toContain("cannot read index.md");
    expect(doc.textContent).not.toContain("has not been written yet");
  });

  it("shows a malformed command response as an error instead of rendering it", async () => {
    contractBackend({
      read_chapter: (_cmd, args) =>
        args.chapterId === "cells" ? contract.read_chapter_ready : { id: "membranes", title: "Membranes", state: "ready" },
    });
    await mountReader(root, contract.list_courses[0]);
    chapterButton("membranes").click();
    const doc = root.querySelector<HTMLElement>("main.document")!;
    await vi.waitFor(() => expect(doc.dataset.state).toBe("error"));
  });

  it("keeps the latest click when an earlier chapter answers late", async () => {
    let releaseCells!: () => void;
    const cellsGate = new Promise<void>((resolve) => (releaseCells = resolve));
    let cellsCalls = 0;
    contractBackend({
      read_chapter: async (_cmd, args) => {
        if (args.chapterId === "cells") {
          cellsCalls += 1;
          if (cellsCalls > 1) await cellsGate;
          return contract.read_chapter_ready;
        }
        return contract.read_chapter_placeholder;
      },
    });
    await mountReader(root, contract.list_courses[0]);
    chapterButton("cells").click();
    chapterButton("membranes").click();
    const doc = root.querySelector<HTMLElement>("main.document")!;
    await vi.waitFor(() => expect(doc.dataset.state).toBe("placeholder"));
    releaseCells();
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(doc.dataset.state).toBe("placeholder");
    expect(doc.querySelector("h1")?.textContent).toBe("Membranes");
  });

  it("does not open any chapter on launch when every chapter is a placeholder", async () => {
    const reads = vi.fn();
    contractBackend({
      list_chapters: () => contract.list_chapters.map((c) => ({ ...c, placeholder: true })),
      read_chapter: reads,
    });
    await mountReader(root, contract.list_courses[0]);
    expect(reads).not.toHaveBeenCalled();
    expect(root.querySelector<HTMLElement>("main.document")!.dataset.state).toBe("empty");
  });

  it("never lets a link in a chapter navigate the app window", async () => {
    contractBackend({
      read_chapter: () => ({
        ...contract.read_chapter_ready,
        markdown: "A [source](https://example.com/paper) link.",
      }),
    });
    await mountReader(root, contract.list_courses[0]);
    const link = root.querySelector<HTMLAnchorElement>("main.document a")!;
    const click = new MouseEvent("click", { bubbles: true, cancelable: true });
    link.dispatchEvent(click);
    expect(click.defaultPrevented).toBe(true);
  });
});

describe("clutter budget", () => {
  it("has no controls beyond one per chapter, one to edit the outline, the overflow actions menu, and the study panel's toggle and question box", async () => {
    contractBackend();
    const tools = mountLibraryTools(document.createElement("div"));
    await tools.ready;
    await mountReader(root, contract.list_courses[0], { storage: null, tools });
    await vi.waitFor(() => expect(root.querySelector('aside.margin [data-section="answers"]')?.getAttribute("data-state")).toBe("empty"));
    const selector = "button, a[href], input, select, textarea, summary, [role='button'], [tabindex]";
    const controls = Array.from(root.querySelectorAll<HTMLElement>(selector))
      .filter((c) => c.closest("[hidden]") === null)
      .map((c) => c.getAttribute("aria-label") || (c as HTMLTextAreaElement).placeholder || c.textContent);
    expect(controls).toEqual([
      "Edit outline",
      "Cells",
      "Membranes",
      "Chapter and course actions",
      "Rewrite chapter",
      "Hide the study panel",
      "Ask about this chapter",
      "Ask",
    ]);
  });

  it("keeps the right column to the study panel on a written chapter, and empty on a chapter still to write", async () => {
    contractBackend({ read_chapter: () => contract.read_chapter_placeholder });
    await mountReader(root, contract.list_courses[0], { storage: null, restore: { chapterId: "membranes", scrollTop: 0, view: "chapter", rejoin: false } });
    expect(root.querySelector("aside.margin")!.children.length).toBe(0);
  });
});

describe("the visible Rewrite shortcut", () => {
  const shortcut = () => root.querySelector<HTMLButtonElement>(".chapter-tools button.chapter-rewrite");

  it("shows a visible Rewrite chapter button in .chapter-tools on a written chapter", async () => {
    contractBackend();
    await mountReader(root, contract.list_courses[0]);
    const button = shortcut();
    expect(button).not.toBeNull();
    expect(button!.textContent).toBe("Rewrite chapter");
    expect(button!.closest("[hidden]")).toBeNull();
  });

  it("opens the same confirm box as the menu entry", async () => {
    contractBackend();
    const tools = mountLibraryTools(document.createElement("div"));
    await tools.ready;
    await mountReader(root, contract.list_courses[0], { tools });

    shortcut()!.click();
    const box = root.querySelector(".rewrite-confirm-box");
    expect(box).not.toBeNull();
    expect(box?.querySelector(".rewrite-confirm-prompt")?.textContent).toContain("Rewrite this chapter?");

    root.querySelector<HTMLButtonElement>("button.overflow-menu-button")!.click();
    root.querySelector<HTMLButtonElement>("button.menu-action-rewrite")!.click();
    expect(root.querySelectorAll(".rewrite-confirm-box")).toHaveLength(1);
    expect(root.querySelector(".rewrite-confirm-box")).toBe(box);
  });

  it("adds no second confirm box when clicked again while the box is open", async () => {
    contractBackend();
    await mountReader(root, contract.list_courses[0]);
    shortcut()!.click();
    shortcut()!.click();
    expect(root.querySelectorAll(".rewrite-confirm-box")).toHaveLength(1);
  });

  it("has no such button on the quiz view", async () => {
    contractBackend({ generate_quiz: () => quizContract.generate_quiz });
    const tools = mountLibraryTools(document.createElement("div"));
    await tools.ready;
    await mountReader(root, contract.list_courses[0], { tools });
    const quiz = [...root.querySelectorAll("button")].find((b) => b.textContent === "Quiz me");
    if (!quiz) throw new Error("no Quiz me button");
    quiz.click();
    const doc = root.querySelector<HTMLElement>("main.document")!;
    await vi.waitFor(() => expect(doc.dataset.state).toBe("quiz"));
    expect(shortcut()).toBeNull();
  });

  it("has no such button on the PDF view", async () => {
    contractBackend({
      list_course_sources: () => [{ name: "notes.pdf", path: "/s/notes.pdf", kind: "pdf", url: null, fetchedAt: null, bytes: 1 }],
      read_pdf_source: () => ({
        name: "notes.pdf",
        glyphHeightFactor: 1,
        pages: [{ page: 1, text: "Page 1", words: [], width: 100, height: 100, caveat: null }],
      }),
      list_source_annotations: () => [],
    });
    await mountReader(root, contract.list_courses[0], {
      restore: { chapterId: null, scrollTop: 0, view: "pdf", pdf: { sourceName: "notes.pdf", page: 1, zoom: 1 }, rejoin: false },
    });
    expect(shortcut()).toBeNull();
  });
});

describe("coming back to a tab", () => {
  const twoWritten = () => contract.list_chapters.map((c) => ({ ...c, placeholder: false }));
  const readById = (_cmd: string, args: Record<string, unknown>) => ({
    id: String(args.chapterId),
    title: String(args.chapterId) === "membranes" ? "Membranes" : "Cells",
    state: "ready",
    markdown: `# ${String(args.chapterId) === "membranes" ? "Membranes" : "Cells"}\n\nBody of ${String(args.chapterId)}.\n`,
  });

  it("opens the chapter the reader was on and scrolls to where they were", async () => {
    contractBackend({ list_chapters: twoWritten, read_chapter: readById });
    await mountReader(root, contract.list_courses[0], {
      restore: { chapterId: "membranes", scrollTop: 240, view: "chapter", rejoin: false },
    });
    const doc = root.querySelector<HTMLElement>("main.document")!;
    expect(doc.querySelector("h1")?.textContent).toBe("Membranes");
    expect(chapterButton("membranes").getAttribute("aria-current")).toBe("page");
    expect(doc.scrollTop).toBe(240);
  });

  it("falls back to the first written chapter when the remembered one is gone", async () => {
    contractBackend();
    await mountReader(root, contract.list_courses[0], {
      restore: { chapterId: "deleted-chapter", scrollTop: 50, view: "chapter", rejoin: false },
    });
    expect(root.querySelector("main.document h1")?.textContent).toBe("Cells");
  });

  it("does not start a job for a remembered chapter that is still a placeholder", async () => {
    const open = vi.fn(() => new Promise(() => {}));
    contractBackend({ open_chapter: open });
    await mountReader(root, contract.list_courses[0], {
      restore: { chapterId: "membranes", scrollTop: 0, view: "chapter", rejoin: false },
    });
    expect(open).not.toHaveBeenCalled();
    expect(root.querySelector("main.document h1")?.textContent).toBe("Cells");
  });

  it("rejoins a chapter job that kept running while the tab was in the background", async () => {
    const open = vi.fn(() => new Promise(() => {}));
    contractBackend({ open_chapter: open });
    void mountReader(root, contract.list_courses[0], {
      restore: { chapterId: "membranes", scrollTop: 0, view: "chapter", rejoin: true },
    });
    await vi.waitFor(() => expect(open).toHaveBeenCalledTimes(1));
    expect(root.querySelector<HTMLElement>("main.document")!.dataset.state).toBe("writing");
  });

  it("restores the quiz view with typed answers and grades instead of regenerating", async () => {
    const generateQuiz = vi.fn();
    contractBackend({
      list_chapters: twoWritten,
      read_chapter: readById,
      generate_quiz: generateQuiz,
    });
    const quizData = {
      chapterId: "cells",
      questions: [
        {
          id: "q1",
          objective: "Cell basics",
          prompt: "What is a cell?",
          referenceAnswer: "Basic unit of life.",
          combines: [],
        },
      ],
    };
    await mountReader(root, contract.list_courses[0], {
      restore: {
        chapterId: "cells",
        scrollTop: 0,
        view: "quiz",
        quiz: {
          chapterId: "cells",
          quiz: quizData,
          answers: { q1: "My remembered draft" },
          grades: [
            {
              questionId: "q1",
              objective: "Cell basics",
              result: "correct",
              final: true,
              confidence: 0.99,
              feedback: "Accurate.",
            },
          ],
        },
        rejoin: false,
      },
    });

    const doc = root.querySelector<HTMLElement>("main.document")!;
    expect(doc.dataset.state).toBe("quiz");
    expect(generateQuiz).not.toHaveBeenCalled();
    const answer = doc.querySelector<HTMLTextAreaElement>("textarea.quiz-answer")!;
    expect(answer.value).toBe("My remembered draft");
    expect(doc.querySelector(".quiz-verdict")?.textContent).toContain("Correct");
  });

  it("restores the PDF view with page, zoom, and source name", async () => {
    contractBackend({
      list_course_sources: () => [{ name: "notes.pdf", path: "/s/notes.pdf", kind: "pdf", url: null, fetchedAt: null, bytes: 1 }],
      read_pdf_source: () => ({
        name: "notes.pdf",
        glyphHeightFactor: 1,
        pages: [
          { page: 1, text: "Page 1", words: [], width: 100, height: 100, caveat: null },
          { page: 2, text: "Page 2", words: [], width: 100, height: 100, caveat: null },
        ],
      }),
      list_source_annotations: () => [],
    });

    await mountReader(root, contract.list_courses[0], {
      restore: {
        chapterId: null,
        scrollTop: 0,
        view: "pdf",
        pdf: { sourceName: "notes.pdf", page: 2, zoom: 1.25 },
        rejoin: false,
      },
    });

    const doc = root.querySelector<HTMLElement>("main.document")!;
    expect(doc.querySelector(".pdf-source")).not.toBeNull();
    expect(doc.querySelector<HTMLInputElement>("input.pdf-page-input")?.value).toBe("2");
    expect(doc.querySelector<HTMLElement>(".pdf-pages")?.style.getPropertyValue("--zoom")).toBe("1.25");
  });

  it("restores the outline review view mode", async () => {
    contractBackend({
      list_chapters: twoWritten,
      read_chapter: readById,
      get_outline: () => wizardContract.outline_approved,
    });

    await mountReader(root, contract.list_courses[0], {
      restore: {
        chapterId: null,
        scrollTop: 0,
        view: "review",
        rejoin: false,
      },
    });

    const doc = root.querySelector<HTMLElement>("main.document")!;
    expect(doc.dataset.state).toBe("review");
  });
});

describe("telling the tab where the reader is", () => {
  it("reports the chapter that opened, then each scroll, then the next chapter", async () => {
    contractBackend({ open_chapter: () => new Promise(() => {}) });
    const seen: Array<{ chapterId: string | null; scrollTop: number }> = [];
    await mountReader(root, contract.list_courses[0], { onPosition: (p) => seen.push(p) });
    expect(seen.at(-1)).toEqual({ chapterId: "cells", scrollTop: 0, view: "chapter" });

    const doc = root.querySelector<HTMLElement>("main.document")!;
    doc.scrollTop = 90;
    doc.dispatchEvent(new Event("scroll"));
    expect(seen.at(-1)).toEqual({ chapterId: "cells", scrollTop: 90, view: "chapter" });

    chapterButton("membranes").click();
    expect(seen.at(-1)).toEqual({ chapterId: "membranes", scrollTop: 0, view: "chapter" });
  });
});

describe("leaving a tab", () => {
  it("stops a background chapter job from painting into the reader, and from reporting a position", async () => {
    let finish!: (value: unknown) => void;
    const written = new Promise((resolve) => (finish = resolve));
    contractBackend({ open_chapter: () => written });
    const seen: unknown[] = [];
    const leave = new AbortController();
    await mountReader(root, contract.list_courses[0], { onPosition: (p) => seen.push(p), signal: leave.signal });
    chapterButton("membranes").click();
    const doc = root.querySelector<HTMLElement>("main.document")!;
    await vi.waitFor(() => expect(doc.dataset.state).toBe("writing"));
    const reports = seen.length;

    leave.abort();
    finish(contract.read_chapter_ready);
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(doc.dataset.state).toBe("writing");
    doc.dispatchEvent(new Event("scroll"));
    expect(seen).toHaveLength(reports);
  });

  it("puts the footer it was given at the end of the sidebar, before anything has loaded", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => (release = resolve));
    contractBackend({ list_chapters: async () => (await gate, contract.list_chapters) });
    const footer = document.createElement("div");
    footer.className = "library-tools";
    const tools = { footer, ready: Promise.resolve(), updateActions: vi.fn() };
    const mounting = mountReader(root, contract.list_courses[0], { tools });
    expect(root.querySelector("nav.sidebar")?.lastElementChild).toBe(footer);
    release();
    await mounting;
    expect(root.querySelector("nav.sidebar")?.lastElementChild).toBe(footer);
  });
});

describe("telling the page the course changed", () => {
  it("reports a chapter that was just written, even after the reader was left", async () => {
    let finish!: (value: unknown) => void;
    contractBackend({ open_chapter: () => new Promise((resolve) => (finish = resolve)) });
    const onChanged = vi.fn();
    const leave = new AbortController();
    await mountReader(root, contract.list_courses[0], { onChanged, signal: leave.signal });
    chapterButton("membranes").click();
    await vi.waitFor(() => expect(finish).toBeDefined());
    leave.abort();
    finish({ id: "membranes", title: "Membranes", state: "ready", markdown: "# Membranes\n" });
    await vi.waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
  });
});

describe("sources that are not PDFs", () => {
  const source = (name: string, kind: string) => ({
    name,
    path: `/library/sources/${name}`,
    kind,
    url: kind === "url" ? "https://example.com/a" : null,
    fetchedAt: kind === "url" ? "2026-11-08T11:13:20Z" : null,
    bytes: 12,
  });
  const library = [
    source("lecture.pdf", "pdf"),
    source("notes.md", "markdown"),
    source("syllabus.txt", "text"),
    source("example-com-a.md", "url"),
    source("clip.png", "other"),
  ];
  const sourceButton = (name: string) => root.querySelector<HTMLButtonElement>(`nav button[data-source-name="${name}"]`);
  const doc = () => root.querySelector<HTMLElement>("main.document")!;

  it("lists every source the reader can show, whatever its kind, and none it cannot", async () => {
    contractBackend({ list_course_sources: () => library });
    await mountReader(root, contract.list_courses[0]);
    const listed = Array.from(root.querySelectorAll<HTMLButtonElement>("nav button[data-source-name]")).map(
      (b) => b.dataset.sourceName,
    );
    expect(listed).toEqual(["lecture.pdf", "notes.md", "syllabus.txt", "example-com-a.md"]);
  });

  it("asks for the sources of the course it was opened on", async () => {
    const asked: unknown[] = [];
    contractBackend({
      list_course_sources: (_cmd, args) => {
        asked.push(args);
        return [];
      },
    });
    await mountReader(root, contract.list_courses[0]);
    expect(asked).toEqual([{ courseId: contract.list_courses[0].id }]);
  });

  it("opens a markdown source as rendered, read-only text", async () => {
    const read: unknown[] = [];
    contractBackend({
      list_course_sources: () => library,
      read_text_source: (_cmd, args) => {
        read.push(args);
        return { name: "notes.md", kind: "markdown", text: "# My notes\n\n- one\n- two\n" };
      },
    });
    await mountReader(root, contract.list_courses[0]);
    sourceButton("notes.md")!.click();
    await vi.waitFor(() => expect(doc().querySelector(".source-text")).not.toBeNull());
    expect(read).toEqual([{ courseId: contract.list_courses[0].id, name: "notes.md" }]);
    expect(doc().dataset.state).toBe("ready");
    expect(doc().querySelector(".source-text h1")?.textContent).toBe("My notes");
    expect(Array.from(doc().querySelectorAll(".source-text li")).map((li) => li.textContent)).toEqual(["one", "two"]);
    expect(doc().querySelector("textarea, input, [contenteditable]")).toBeNull();
    expect(sourceButton("notes.md")!.getAttribute("aria-current")).toBe("page");
    expect(root.querySelector("aside.margin")!.children).toHaveLength(0);
  });

  it("opens a captured web page as markdown too", async () => {
    contractBackend({
      list_course_sources: () => library,
      read_text_source: () => ({ name: "example-com-a.md", kind: "markdown", text: "# A page\n" }),
    });
    await mountReader(root, contract.list_courses[0]);
    sourceButton("example-com-a.md")!.click();
    await vi.waitFor(() => expect(doc().querySelector(".source-text h1")?.textContent).toBe("A page"));
  });

  it("opens a text source as preformatted text, never as markup", async () => {
    const text = "Week 1\n  # not a heading\n<b>not bold</b>\n";
    contractBackend({
      list_course_sources: () => library,
      read_text_source: () => ({ name: "syllabus.txt", kind: "text", text }),
    });
    await mountReader(root, contract.list_courses[0]);
    sourceButton("syllabus.txt")!.click();
    await vi.waitFor(() => expect(doc().querySelector("pre.source-text")).not.toBeNull());
    const pre = doc().querySelector("pre.source-text")!;
    expect(pre.textContent).toBe(text);
    expect(pre.querySelector("h1, b")).toBeNull();
  });

  it("says why a source could not be opened, and leaves the sidebar usable", async () => {
    contractBackend({
      list_course_sources: () => library,
      read_text_source: () => {
        throw new Error('could not read "notes.md": No such file');
      },
    });
    await mountReader(root, contract.list_courses[0]);
    sourceButton("notes.md")!.click();
    await vi.waitFor(() => expect(doc().dataset.state).toBe("error"));
    expect(doc().querySelector('[role="alert"]')?.textContent).toContain("No such file");
    expect(sourceButton("syllabus.txt")).not.toBeNull();
  });

  it("makes a markdown or text chip in the study panel a button that opens the source", async () => {
    contractBackend({
      list_course_sources: () => library,
      chapter_context: () => ({ objectives: [], summary: "", sources: ["notes.md", "syllabus.txt", "gone.md"] }),
      read_text_source: () => ({ name: "notes.md", kind: "markdown", text: "# My notes\n" }),
    });
    await mountReader(root, contract.list_courses[0]);
    await vi.waitFor(() => expect(root.querySelectorAll("aside.margin .chip").length).toBe(3));
    const chips = Array.from(root.querySelectorAll<HTMLElement>("aside.margin .chip"));
    expect(chips.map((c) => [c.tagName, c.textContent])).toEqual([
      ["BUTTON", "notes.md"],
      ["BUTTON", "syllabus.txt"],
      ["SPAN", "gone.md"],
    ]);
    chips[0].click();
    await vi.waitFor(() => expect(doc().querySelector(".source-text h1")?.textContent).toBe("My notes"));
  });
});

describe("leaving a quiz", () => {
  function named(label: string): HTMLButtonElement {
    const found = [...root.querySelectorAll("button")].find((node) => node.textContent === label);
    if (!found) throw new Error(`no button ${label}`);
    return found;
  }

  it("returns to the chapter at the scroll it had when Quiz me was clicked", async () => {
    contractBackend({ generate_quiz: () => quizContract.generate_quiz });
    const tools = mountLibraryTools(document.createElement("div"));
    await tools.ready;
    await mountReader(root, contract.list_courses[0], { tools });
    const doc = root.querySelector<HTMLElement>("main.document")!;
    doc.scrollTop = 180;
    named("Quiz me").click();
    await vi.waitFor(() => expect(doc.dataset.state).toBe("quiz"));
    doc.scrollTop = 0;

    named("Back to the chapter").click();
    await vi.waitFor(() => expect(doc.querySelector("h1")?.textContent).toBe("Cells"));
    expect(doc.scrollTop).toBe(180);
  });

  it("collapses the study column while the quiz is open and restores it when the chapter returns", async () => {
    contractBackend({ generate_quiz: () => quizContract.generate_quiz });
    const tools = mountLibraryTools(document.createElement("div"));
    await tools.ready;
    await mountReader(root, contract.list_courses[0], { tools });
    const reader = root.querySelector<HTMLElement>(".reader")!;
    expect(reader.dataset.mode).toBeUndefined();

    named("Quiz me").click();
    await vi.waitFor(() => expect(reader.dataset.mode).toBe("quiz"));

    named("Back to the chapter").click();
    const doc = root.querySelector<HTMLElement>("main.document")!;
    await vi.waitFor(() => expect(doc.querySelector("h1")?.textContent).toBe("Cells"));
    expect(reader.dataset.mode).toBeUndefined();
  });

  it("opens the outline from a finished quiz", async () => {
    contractBackend({
      generate_quiz: () => quizContract.generate_quiz,
      grade_quiz: () => quizContract.grade_quiz,
    });
    const tools = mountLibraryTools(document.createElement("div"));
    await tools.ready;
    await mountReader(root, contract.list_courses[0], { tools });
    named("Quiz me").click();
    const doc = root.querySelector<HTMLElement>("main.document")!;
    await vi.waitFor(() => expect(doc.querySelector("textarea.quiz-answer")).not.toBeNull());
    doc.querySelector<HTMLTextAreaElement>("textarea.quiz-answer")!.value = "ownership moved";
    doc.querySelector("form")!.requestSubmit();
    await vi.waitFor(() => expect(named("Back to the outline")).toBeTruthy());
    named("Back to the outline").click();
    await vi.waitFor(() => expect(doc.dataset.state).toBe("review"));
    expect(doc.querySelector("section.review h1")?.textContent).toBe("Cell Biology");
  });
});
