// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import agentsContract from "./fixtures/agents-contract.json";
import wizardContract from "./fixtures/wizard-contract.json";
import { mountApp } from "./app";

const NOW = Math.floor(Date.now() / 1000);

function wireCourse(id: string, title: string, over: Record<string, unknown> = {}) {
  return {
    id,
    storedId: id,
    title,
    tags: [],
    objectives: [],
    chapterCount: 2,
    chapterTitles: [`${title} one`, `${title} two`],
    writtenChapterCount: 2,
    createdAt: NOW - 86_400,
    openedAt: null as number | null,
    error: null as string | null,
    outlineStatus: "approved",
    planning: false,
    ...over,
  };
}

let shelf: ReturnType<typeof wireCourse>[];
let calls: Array<{ cmd: string; args: Record<string, unknown> }>;
let jobs: Record<string, ReturnType<typeof wireJob>>;
let root: HTMLElement;
let saved: Map<string, string>;

function wireJob(id: string, courseId: string, over: Record<string, unknown> = {}) {
  return {
    id,
    courseId,
    kind: "chapter",
    driver: "claude",
    model: "sonnet",
    effort: null,
    command: "claude -p <prompt, 99 chars> --model sonnet",
    startedAt: "2026-11-09T12:00:00Z",
    endedAt: null,
    status: "running",
    lines: [{ at: "2026-11-09T12:00:00Z", kind: "status", text: "running" }],
    ...over,
  };
}

const storage = {
  getItem: (key: string) => saved.get(key) ?? null,
  setItem: (key: string, value: string) => void saved.set(key, value),
};

function backend(extra: Record<string, (args: Record<string, unknown>) => unknown> = {}): void {
  mockIPC(
    (cmd, raw) => {
      const args = (raw ?? {}) as Record<string, unknown>;
      calls.push({ cmd, args });
      if (extra[cmd]) return extra[cmd](args);
      switch (cmd) {
        case "list_library_courses":
          return shelf;
        case "mark_course_opened": {
          const course = shelf.find((c) => c.id === args.courseId);
          if (!course) throw new Error(`no course ${String(args.courseId)}`);
          course.openedAt = NOW;
          return course;
        }
        case "get_outline":
          return wizardContract.outline_approved;
        case "list_chapters":
          return [
            { id: "one", title: "One", placeholder: false },
            { id: "two", title: "Two", placeholder: false },
          ];
        case "read_chapter":
          return {
            id: args.chapterId,
            title: String(args.chapterId),
            state: "ready",
            markdown: `# ${String(args.courseId)} ${String(args.chapterId)}\n\nBody.\n`,
          };
        case "list_annotations":
        case "list_course_sources":
        case "course_progress":
          return [];
        case "list_agent_drivers":
          return agentsContract.list_agent_drivers;
        case "driver_tuning":
          return { model: null, effort: null };
        case "list_agent_jobs":
          return Object.values(jobs);
        case "agent_job":
          return jobs[String(args.id)];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    },
    { shouldMockEvents: true },
  );
}

beforeEach(() => {
  document.body.innerHTML = '<div id="app"></div>';
  root = document.querySelector<HTMLElement>("#app")!;
  saved = new Map();
  calls = [];
  jobs = {};
  shelf = [
    wireCourse("cell-biology", "Cell Biology", { storedId: "bio101", tags: ["science"] }),
    wireCourse("roman-history", "Roman History", { tags: ["humanities"] }),
    wireCourse("linear-algebra", "Linear Algebra", { tags: ["science"] }),
  ];
  backend();
});

afterEach(() => {
  clearMocks();
  vi.useRealTimers();
});

const tabs = (): string[] => Array.from(root.querySelectorAll("[role='tab']")).map((t) => t.textContent!);
const tab = (label: string): HTMLElement => {
  const found = Array.from(root.querySelectorAll<HTMLElement>("[role='tab']")).find((t) => t.textContent!.startsWith(label));
  if (!found) throw new Error(`no tab ${label}`);
  return found;
};
const stageKind = (): string => (root.querySelector(".home") ? "home" : root.querySelector(".reader") ? "reader" : "other");
const heading = (): string => root.querySelector("main.document h1")?.textContent ?? "";
const doc = (): HTMLElement => root.querySelector<HTMLElement>("main.document")!;
const order = (cmd: string): number => calls.findIndex((c) => c.cmd === cmd);

async function openFromHome(title: string): Promise<void> {
  const card = Array.from(root.querySelectorAll<HTMLElement>("article.card")).find(
    (c) => c.querySelector("h2")!.textContent === title,
  )!;
  card.querySelector<HTMLButtonElement>("button.card-open")!.click();
  await vi.waitFor(() => expect(root.querySelector<HTMLElement>("main.document")?.dataset.state).toBe("ready"));
}

async function goHome(): Promise<void> {
  tab("Home").click();
  await vi.waitFor(() => expect(stageKind()).toBe("home"));
}

describe("launch", () => {
  it("starts on Home with every course as a card and no course tab", async () => {
    await mountApp(root, { storage });
    expect(stageKind()).toBe("home");
    expect(tabs()).toEqual(["Home"]);
    expect(root.querySelectorAll("article.card")).toHaveLength(3);
  });

  it("says so when the library cannot be opened", async () => {
    backend({
      list_library_courses: () => {
        throw new Error("not a TutorDocs library");
      },
    });
    await mountApp(root, { storage });
    expect(root.querySelector('[role="alert"]')!.textContent).toContain("not a TutorDocs library");
    expect(tabs()).toEqual(["Home"]);
  });
});

describe("opening courses", () => {
  it("opens a course in a new tab, marks it opened before loading it, and shows its reader", async () => {
    await mountApp(root, { storage });
    await openFromHome("Cell Biology");
    expect(tabs()).toEqual(["Home", "Cell Biology"]);
    expect(tab("Cell Biology").getAttribute("aria-selected")).toBe("true");
    expect(heading()).toBe("cell-biology one");
    expect(order("mark_course_opened")).toBeLessThan(order("list_chapters"));
    expect(calls.find((c) => c.cmd === "list_chapters")!.args).toEqual({ courseId: "cell-biology" });
  });

  it("focuses a course's tab instead of opening a second one", async () => {
    await mountApp(root, { storage });
    await openFromHome("Cell Biology");
    await goHome();
    await openFromHome("Cell Biology");
    expect(tabs()).toEqual(["Home", "Cell Biology"]);
  });

  it("keeps two courses open and switches between them, marking each opened as it comes to the front", async () => {
    await mountApp(root, { storage });
    await openFromHome("Cell Biology");
    await goHome();
    await openFromHome("Roman History");
    expect(tabs()).toEqual(["Home", "Cell Biology", "Roman History"]);
    expect(heading()).toBe("roman-history one");

    calls.length = 0;
    tab("Cell Biology").click();
    await vi.waitFor(() => expect(heading()).toBe("cell-biology one"));
    expect(calls.find((c) => c.cmd === "mark_course_opened")!.args).toEqual({ courseId: "cell-biology" });
  });

  it("with two course tabs sharing a source name, the focused tab reads its own file", async () => {
    backend({
      list_course_sources: (args) => [
        {
          name: "notes.md",
          path: `/library/courses/${String(args.courseId)}/sources/notes.md`,
          kind: "markdown",
          url: null,
          fetchedAt: null,
          bytes: 100,
        },
      ],
      read_text_source: (args) => ({
        name: "notes.md",
        kind: "markdown",
        text: `# ${String(args.courseId)} notes\n`,
      }),
    });
    await mountApp(root, { storage });
    await openFromHome("Cell Biology");
    await goHome();
    await openFromHome("Roman History");
    expect(tabs()).toEqual(["Home", "Cell Biology", "Roman History"]);

    const romanSourceBtn = root.querySelector<HTMLButtonElement>('button[data-source-name="notes.md"]');
    expect(romanSourceBtn).not.toBeNull();
    romanSourceBtn!.click();
    await vi.waitFor(() => expect(root.querySelector(".source-text h1")?.textContent).toBe("roman-history notes"));
    const romanRead = calls.find((c) => c.cmd === "read_text_source");
    expect(romanRead?.args).toEqual({ courseId: "roman-history", name: "notes.md" });

    calls.length = 0;
    tab("Cell Biology").click();
    await vi.waitFor(() => expect(heading()).toBe("cell-biology one"));

    const bioSourceBtn = root.querySelector<HTMLButtonElement>('button[data-source-name="notes.md"]');
    expect(bioSourceBtn).not.toBeNull();
    bioSourceBtn!.click();
    await vi.waitFor(() => expect(root.querySelector(".source-text h1")?.textContent).toBe("cell-biology notes"));
    const bioRead = calls.find((c) => c.cmd === "read_text_source");
    expect(bioRead?.args).toEqual({ courseId: "cell-biology", name: "notes.md" });
  });

  it("puts one sidebar footer in whichever reader is showing, and none on Home", async () => {
    await mountApp(root, { storage });
    await openFromHome("Cell Biology");
    const footer = root.querySelector(".library-tools");
    expect(footer?.parentElement?.tagName).toBe("NAV");
    await goHome();
    expect(root.querySelector("nav.sidebar")).toBeNull();
    expect(root.querySelectorAll(".library-tools")).toHaveLength(1);
    expect(root.querySelector(".library-tools")!.isConnected).toBe(true);
    await openFromHome("Roman History");
    expect(root.querySelector(".library-tools")).toBe(footer);
    expect(root.querySelectorAll(".library-tools")).toHaveLength(1);
  });

  it("opens a course whose summary carries an error, and shows the error on its card", async () => {
    shelf.push(wireCourse("broken", "", { error: "course.json is not valid JSON", chapterCount: 0, writtenChapterCount: 0 }));
    await mountApp(root, { storage });
    const card = Array.from(root.querySelectorAll<HTMLElement>("article.card")).find((c) => c.dataset.courseId === "broken")!;
    expect(card.textContent).toContain("course.json is not valid JSON");
    card.querySelector<HTMLButtonElement>("button.card-open")!.click();
    await vi.waitFor(() => expect(tabs()).toContain("broken"));
  });
});

describe("each tab keeps its own place", () => {
  it("returns to the chapter and scroll position the reader left", async () => {
    await mountApp(root, { storage });
    await openFromHome("Cell Biology");
    root.querySelector<HTMLButtonElement>('button.chapter[data-chapter-id="two"]')!.click();
    await vi.waitFor(() => expect(heading()).toBe("cell-biology two"));
    doc().scrollTop = 180;
    doc().dispatchEvent(new Event("scroll"));

    await goHome();
    await openFromHome("Roman History");
    expect(heading()).toBe("roman-history one");
    expect(doc().scrollTop).toBe(0);

    tab("Cell Biology").click();
    await vi.waitFor(() => expect(heading()).toBe("cell-biology two"));
    expect(doc().scrollTop).toBe(180);
    expect(root.querySelector('button.chapter[data-chapter-id="two"]')!.getAttribute("aria-current")).toBe("page");
  });

  it("remembers a quiz in progress with typed draft answers across tab switch", async () => {
    const genQuiz = vi.fn(() => ({
      chapterId: "one",
      questions: [
        {
          id: "q1",
          objective: "Cells",
          prompt: "What is a cell?",
          referenceAnswer: "Unit",
          combines: [],
        },
      ],
    }));
    backend({ generate_quiz: genQuiz });

    await mountApp(root, { storage });
    await openFromHome("Cell Biology");

    // Start quiz
    const quizBtn = [...root.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Quiz me")!;
    quizBtn.click();
    await vi.waitFor(() => expect(root.querySelector("textarea.quiz-answer")).not.toBeNull());
    expect(genQuiz).toHaveBeenCalledTimes(1);

    // Type a draft answer
    const textarea = root.querySelector<HTMLTextAreaElement>("textarea.quiz-answer")!;
    textarea.value = "A biological building block";
    textarea.dispatchEvent(new Event("input"));

    // Switch to another course tab
    await goHome();
    await openFromHome("Roman History");
    expect(heading()).toBe("roman-history one");

    // Switch back to Cell Biology
    tab("Cell Biology").click();
    await vi.waitFor(() => expect(root.querySelector("textarea.quiz-answer")).not.toBeNull());

    // Draft answer survived without re-running quiz generation
    expect(root.querySelector<HTMLTextAreaElement>("textarea.quiz-answer")!.value).toBe("A biological building block");
    expect(genQuiz).toHaveBeenCalledTimes(1);
  });

  it("remembers a PDF view with zoom across tab switch", async () => {
    backend({
      list_course_sources: () => [
        { name: "guide.pdf", path: "/guide.pdf", kind: "pdf", url: null, fetchedAt: null, bytes: 1 },
      ],
      read_pdf_source: () => ({
        name: "guide.pdf",
        glyphHeightFactor: 1,
        pages: [
          { page: 1, text: "p1", words: [], width: 100, height: 100, caveat: null },
          { page: 2, text: "p2", words: [], width: 100, height: 100, caveat: null },
        ],
      }),
      list_source_annotations: () => [],
    });

    await mountApp(root, { storage });
    await openFromHome("Cell Biology");

    const pdfBtn = root.querySelector<HTMLButtonElement>('button[data-source-name="guide.pdf"]')!;
    pdfBtn.click();
    await vi.waitFor(() => expect(root.querySelector(".pdf-pages")).not.toBeNull());

    // Zoom in
    root.querySelector<HTMLButtonElement>('button[aria-label="Zoom in"]')!.click();
    expect(root.querySelector<HTMLElement>(".pdf-pages")!.style.getPropertyValue("--zoom")).toBe("1.25");

    // Switch away and back
    await goHome();
    await openFromHome("Roman History");
    expect(heading()).toBe("roman-history one");

    tab("Cell Biology").click();
    await vi.waitFor(() => expect(root.querySelector(".pdf-pages")).not.toBeNull());
    expect(root.querySelector("main.document h1")?.textContent).toBe("guide.pdf");
    expect(root.querySelector<HTMLElement>(".pdf-pages")!.style.getPropertyValue("--zoom")).toBe("1.25");
  });

  it("remembers outline review across tab switch", async () => {
    await mountApp(root, { storage });
    await openFromHome("Cell Biology");

    const editOutlineBtn = [...root.querySelectorAll<HTMLButtonElement>("button")].find(
      (b) => b.textContent === "Edit outline",
    )!;
    editOutlineBtn.click();
    await vi.waitFor(() => expect(root.querySelector(".review-host")).not.toBeNull());

    // Switch away and back
    await goHome();
    await openFromHome("Roman History");
    expect(heading()).toBe("roman-history one");

    tab("Cell Biology").click();
    await vi.waitFor(() => expect(root.querySelector(".review-host")).not.toBeNull());
  });
});

describe("closing tabs", () => {
  it("closes the active tab, shows the one beside it, and reopens fresh from Home", async () => {
    await mountApp(root, { storage });
    await openFromHome("Cell Biology");
    root.querySelector<HTMLButtonElement>('button.chapter[data-chapter-id="two"]')!.click();
    await vi.waitFor(() => expect(heading()).toBe("cell-biology two"));
    await goHome();
    await openFromHome("Roman History");

    root.querySelector<HTMLButtonElement>("button[aria-label='Close Roman History']")!.click();
    await vi.waitFor(() => expect(tabs()).toEqual(["Home", "Cell Biology"]));
    expect(tab("Cell Biology").getAttribute("aria-selected")).toBe("true");
    await vi.waitFor(() => expect(heading()).toBe("cell-biology two"));

    root.querySelector<HTMLButtonElement>("button[aria-label='Close Cell Biology']")!.click();
    await vi.waitFor(() => expect(stageKind()).toBe("home"));
    expect(tabs()).toEqual(["Home"]);

    await openFromHome("Cell Biology");
    expect(heading()).toBe("cell-biology one");
  });

  it("leaves the shown course alone when a background tab closes", async () => {
    await mountApp(root, { storage });
    await openFromHome("Cell Biology");
    await goHome();
    await openFromHome("Roman History");
    const before = root.querySelector(".reader");
    root.querySelector<HTMLButtonElement>("button[aria-label='Close Cell Biology']")!.click();
    expect(tabs()).toEqual(["Home", "Roman History"]);
    expect(root.querySelector(".reader")).toBe(before);
  });
});

describe("remembering tabs across restarts", () => {
  it("writes the open tabs and the active one to storage", async () => {
    await mountApp(root, { storage });
    await openFromHome("Cell Biology");
    await goHome();
    await openFromHome("Roman History");
    const remembered = JSON.parse(saved.get("tutordocs.tabs.v1")!);
    expect(remembered.open.map((t: { courseId: string }) => t.courseId)).toEqual(["cell-biology", "roman-history"]);
    expect(remembered.active).toBe("roman-history");
  });

  it("reopens the tabs and the one that was active, at its chapter and scroll", async () => {
    saved.set(
      "tutordocs.tabs.v1",
      JSON.stringify({
        open: [
          { courseId: "cell-biology", chapterId: "two", scrollTop: 75 },
          { courseId: "roman-history", chapterId: null, scrollTop: 0 },
        ],
        active: "cell-biology",
      }),
    );
    await mountApp(root, { storage });
    expect(tabs()).toEqual(["Home", "Cell Biology", "Roman History"]);
    expect(tab("Cell Biology").getAttribute("aria-selected")).toBe("true");
    expect(heading()).toBe("cell-biology two");
    expect(doc().scrollTop).toBe(75);
    expect(calls.find((c) => c.cmd === "mark_course_opened")!.args).toEqual({ courseId: "cell-biology" });
  });

  it("drops a remembered course that no longer exists, and lands on Home when it was the active one", async () => {
    saved.set(
      "tutordocs.tabs.v1",
      JSON.stringify({
        open: [
          { courseId: "gone-course", chapterId: null, scrollTop: 0 },
          { courseId: "roman-history", chapterId: null, scrollTop: 0 },
        ],
        active: "gone-course",
      }),
    );
    await mountApp(root, { storage });
    expect(tabs()).toEqual(["Home", "Roman History"]);
    expect(stageKind()).toBe("home");
  });

  it("restores Home search and tag filter across relaunches", async () => {
    saved.set(
      "tutordocs.tabs.v1",
      JSON.stringify({
        open: [],
        active: null,
        filter: { query: "Cell", tag: "science" },
      }),
    );
    await mountApp(root, { storage });
    const searchInput = root.querySelector<HTMLInputElement>("input.home-search")!;
    expect(searchInput.value).toBe("Cell");
  });

  it("works when storage throws on every read and write", async () => {
    const hostile = {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("full");
      },
    };
    await mountApp(root, { storage: hostile });
    await openFromHome("Cell Biology");
    expect(tabs()).toEqual(["Home", "Cell Biology"]);
  });

  it("works with no storage at all, and with garbage in it", async () => {
    saved.set("tutordocs.tabs.v1", "{not json");
    await mountApp(root, { storage });
    expect(tabs()).toEqual(["Home"]);
    document.body.innerHTML = '<div id="app"></div>';
    root = document.querySelector<HTMLElement>("#app")!;
    await mountApp(root, { storage: null });
    expect(tabs()).toEqual(["Home"]);
  });
});

describe("Home remembers the search while a course is open", () => {
  it("keeps the search text and the narrowed cards after a visit to a tab", async () => {
    await mountApp(root, { storage });
    const search = root.querySelector<HTMLInputElement>("input.home-search")!;
    search.value = "science";
    search.dispatchEvent(new Event("input", { bubbles: true }));
    await openFromHome("Cell Biology");
    await goHome();
    expect(root.querySelector<HTMLInputElement>("input.home-search")!.value).toBe("science");
    expect(root.querySelectorAll("article.card")).toHaveLength(2);
  });
});

describe("a new course", () => {
  const wizardButton = (name: string): HTMLButtonElement => {
    const found = Array.from(root.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent === name);
    if (!found) throw new Error(`no button "${name}"`);
    return found;
  };
  const step = (): string => root.querySelector('.wizard-steps [aria-current="step"]')?.textContent ?? "";

  function wizardBackend(extra: Record<string, (args: Record<string, unknown>) => unknown> = {}): { planned: () => boolean } {
    let planned = false;
    backend({
      list_course_templates: () => wizardContract.list_course_templates,
      create_course: (args) => {
        const made = wireCourse("genetics", String(args.title), { tags: args.tags, chapterCount: 0, writtenChapterCount: 0 });
        shelf.push(made);
        return made;
      },
      set_course_setup: (args) => args.setup,
      generate_outline: () => {
        planned = true;
        return null;
      },
      get_outline: (args) => (planned && args.courseId === "genetics" ? wizardContract.outline_draft : wizardContract.outline_approved),
      list_chapters: (args) =>
        args.courseId !== "genetics"
          ? [{ id: "one", title: "One", placeholder: false }]
          : planned
            ? [{ id: "cells", title: "Cells", placeholder: true }]
            : [],
      ...extra,
    });
    return { planned: () => planned };
  }

  async function runWizard(title: string): Promise<void> {
    root.querySelector<HTMLButtonElement>("button.home-new")!.click();
    await vi.waitFor(() => expect(root.querySelector('[aria-label="Course title"]')).not.toBeNull());
    const input = root.querySelector<HTMLInputElement>('[aria-label="Course title"]')!;
    input.value = title;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    wizardButton("Next").click();
    await vi.waitFor(() => expect(step()).toContain("Materials"));
    wizardButton("Next").click();
    await vi.waitFor(() => expect(step()).toContain("Guide"));
    wizardButton("Plan the course").click();
  }

  it("starts the first course in the wizard from an empty library, then opens its draft outline for review", async () => {
    shelf = [];
    wizardBackend();
    await mountApp(root, { storage });
    expect(root.querySelector(".home-welcome")!.textContent).toContain("first course");
    await runWizard("Genetics");
    await vi.waitFor(() => expect(tabs()).toEqual(["Home", "Genetics"]));
    await vi.waitFor(() => expect(doc().dataset.state).toBe("review"));
    expect(doc().querySelectorAll("li.draft-chapter")).toHaveLength(3);
    expect(calls.map((c) => c.cmd)).not.toContain("open_chapter");
  });

  it("replaces the cards with the wizard, and Cancel brings them back", async () => {
    wizardBackend();
    await mountApp(root, { storage });
    root.querySelector<HTMLButtonElement>("button.home-new")!.click();
    await vi.waitFor(() => expect(root.querySelector('[aria-label="Course title"]')).not.toBeNull());
    expect(root.querySelector(".home")).toBeNull();
    wizardButton("Cancel").click();
    await vi.waitFor(() => expect(root.querySelectorAll("article.card")).toHaveLength(3));
    expect(root.querySelector(".wizard")).toBeNull();
  });

  it("brings the cards back from the Home tab too", async () => {
    wizardBackend();
    await mountApp(root, { storage });
    root.querySelector<HTMLButtonElement>("button.home-new")!.click();
    await vi.waitFor(() => expect(root.querySelector(".wizard")).not.toBeNull());
    await goHome();
    await vi.waitFor(() => expect(root.querySelectorAll("article.card")).toHaveLength(3));
  });

  it("does not pull the user away from another tab when a plan finishes later", async () => {
    let release!: () => void;
    const held = new Promise<null>((resolve) => (release = () => resolve(null)));
    wizardBackend({ generate_outline: () => held });
    await mountApp(root, { storage });
    await runWizard("Genetics");
    await vi.waitFor(() => expect(root.querySelector(".job-status")).not.toBeNull());
    await goHome();
    await openFromHome("Roman History");
    expect(tabs()).toEqual(["Home", "Roman History"]);
    release();
    await vi.waitFor(() => expect(calls.filter((c) => c.cmd === "list_library_courses").length).toBeGreaterThan(2));
    expect(tabs()).toEqual(["Home", "Roman History"]);
    expect(tab("Roman History").getAttribute("aria-selected")).toBe("true");
  });

  it("shows a completion notice on Home when a plan finishes while the user is on Home", async () => {
    let release!: () => void;
    const held = new Promise<null>((resolve) => (release = () => resolve(null)));
    wizardBackend({ generate_outline: () => held });
    await mountApp(root, { storage });
    await runWizard("Genetics");
    await vi.waitFor(() => expect(root.querySelector(".job-status")).not.toBeNull());
    const leaveBtn = Array.from(root.querySelectorAll<HTMLButtonElement>("button")).find(
      (b) => b.textContent === "Back to Home, plan keeps running",
    )!;
    leaveBtn.click();
    await vi.waitFor(() => expect(root.querySelector(".home")).not.toBeNull());

    release();
    await vi.waitFor(() => expect(root.querySelector(".home-notice")).not.toBeNull());
    expect(root.querySelector(".home-notice")!.textContent).toContain("Genetics");
  });

  it("closes open tab when course is trashed from Home", async () => {
    backend({
      trash_course: (args) => {
        shelf = shelf.filter((c) => c.id !== args.courseId);
        return {
          trashId: `${args.courseId}-1700000000`,
          courseId: String(args.courseId),
          title: "Cell Biology",
          trashedAt: "2026-11-09T12:00:00Z",
          chapterCount: 2,
          writtenChapterCount: 2,
          bytes: 100,
        };
      },
    });
    await mountApp(root, { storage });
    await openFromHome("Cell Biology");
    expect(tabs()).toEqual(["Home", "Cell Biology"]);
    await goHome();

    const card = () => root.querySelector<HTMLElement>('article.card[data-course-id="cell-biology"]')!;
    card().querySelector<HTMLButtonElement>("button.card-menu-button")!.click();
    card().querySelector<HTMLButtonElement>("button.card-action-trash")!.click();
    card().querySelector<HTMLButtonElement>("button.confirm-trash")!.click();

    await vi.waitFor(() => expect(tabs()).toEqual(["Home"]));
  });
});

describe("agent jobs", () => {
  it("marks the tab of a course whose job is running, even when another tab is showing", async () => {
    await mountApp(root, { storage });
    await openFromHome("Cell Biology");
    await goHome();
    await openFromHome("Roman History");
    expect(root.querySelector<HTMLElement>("[data-course-id='cell-biology']")!.dataset.busy).toBe("false");

    jobs["job-1"] = wireJob("job-1", "bio101");
    await emit("agent-job-status", { jobId: "job-1", status: "running" });
    await vi.waitFor(() =>
      expect(root.querySelector<HTMLElement>("[data-course-id='cell-biology']")!.dataset.busy).toBe("true"),
    );
    expect(root.querySelector<HTMLElement>("[data-course-id='roman-history']")!.dataset.busy).toBe("false");
    expect(root.querySelector("button.jobs-toggle")!.getAttribute("aria-label")).toBe("Agent jobs, 1 running");

    jobs["job-1"] = wireJob("job-1", "bio101", { status: "succeeded", endedAt: "2026-11-09T12:00:30Z" });
    await emit("agent-job-status", { jobId: "job-1", status: "succeeded" });
    await vi.waitFor(() =>
      expect(root.querySelector<HTMLElement>("[data-course-id='cell-biology']")!.dataset.busy).toBe("false"),
    );
    expect(root.querySelector("button.jobs-toggle")!.getAttribute("aria-label")).toBe("Agent jobs");
  });

  it("refreshes the course list when a job ends, so Home shows the new chapter count", async () => {
    await mountApp(root, { storage });
    jobs["job-1"] = wireJob("job-1", "roman-history");
    await emit("agent-job-status", { jobId: "job-1", status: "running" });
    await vi.waitFor(() => expect(root.querySelector("button.jobs-toggle")!.getAttribute("aria-label")).toContain("1 running"));
    shelf[1] = { ...shelf[1], chapterCount: 5, writtenChapterCount: 4 };
    jobs["job-1"] = wireJob("job-1", "roman-history", { status: "succeeded", endedAt: "2026-11-09T12:00:30Z" });
    await emit("agent-job-status", { jobId: "job-1", status: "succeeded" });
    await vi.waitFor(() => {
      const card = root.querySelector("article[data-course-id='roman-history']")!;
      expect(card.querySelector(".card-progress")!.textContent).toBe("4 of 5 chapters written");
    });
  });

  it("opens the job log from the tab bar, shows a job's command and follows its lines, over the reader", async () => {
    jobs["job-1"] = wireJob("job-1", "bio101");
    await mountApp(root, { storage });
    await openFromHome("Cell Biology");
    const drawer = root.querySelector<HTMLElement>("aside.job-drawer")!;
    expect(drawer.hidden).toBe(true);

    root.querySelector<HTMLButtonElement>("button.jobs-toggle")!.click();
    expect(drawer.hidden).toBe(false);
    expect(root.querySelector("button.jobs-toggle")!.getAttribute("aria-expanded")).toBe("true");
    drawer.querySelector<HTMLButtonElement>("button.job-row")!.click();
    expect(drawer.querySelector("pre.job-command")!.textContent).toBe("claude -p <prompt, 99 chars> --model sonnet");

    await emit("agent-job-line", { jobId: "job-1", line: { at: "2026-11-09T12:00:07Z", kind: "tool", text: "Read sources/notes.md" } });
    await vi.waitFor(() =>
      expect(Array.from(drawer.querySelectorAll(".job-line")).map((l) => l.textContent)).toEqual([
        "running",
        "Read sources/notes.md",
      ]),
    );
    expect(drawer.closest(".stage")).toBeNull();
    expect(root.querySelector(".reader")!.contains(drawer)).toBe(false);

    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(drawer.hidden).toBe(true);
    expect(root.querySelector("button.jobs-toggle")!.getAttribute("aria-expanded")).toBe("false");
  });

  it("rejoins a chapter job that kept running while the tab was in the background", async () => {
    shelf[0] = { ...shelf[0], writtenChapterCount: 1 };
    const opening = vi.fn(() => new Promise(() => {}));
    backend({
      list_chapters: () => [
        { id: "one", title: "One", placeholder: false },
        { id: "two", title: "Two", placeholder: true },
      ],
      read_chapter: (args) =>
        args.chapterId === "two"
          ? { id: "two", title: "Two", state: "placeholder" }
          : { id: "one", title: "One", state: "ready", markdown: "# One\n" },
      open_chapter: opening,
    });
    await mountApp(root, { storage });
    await openFromHome("Cell Biology");
    root.querySelector<HTMLButtonElement>('button.chapter[data-chapter-id="two"]')!.click();
    await vi.waitFor(() => expect(opening).toHaveBeenCalledTimes(1));
    jobs["job-1"] = wireJob("job-1", "bio101");
    await emit("agent-job-status", { jobId: "job-1", status: "running" });
    await goHome();
    await openFromHome("Roman History");

    tab("Cell Biology").click();
    await vi.waitFor(() => expect(opening).toHaveBeenCalledTimes(2));
    expect(doc().dataset.state).toBe("writing");
    expect(doc().querySelector("h1")!.textContent).toBe("Two");
  });

  it("shows an attention dot on the Jobs toggle when a job fails, and clears it when the drawer opens", async () => {
    await mountApp(root, { storage });
    jobs["job-fail"] = wireJob("job-fail", "cell-biology", { status: "running" });
    await emit("agent-job-status", { jobId: "job-fail", status: "running" });
    expect(root.querySelector("button.jobs-toggle .jobs-dot")).toBeNull();

    jobs["job-fail"] = wireJob("job-fail", "cell-biology", { status: "failed", endedAt: "2026-11-09T12:01:00Z" });
    await emit("agent-job-status", { jobId: "job-fail", status: "failed" });
    await vi.waitFor(() => expect(root.querySelector("button.jobs-toggle .jobs-dot")).not.toBeNull());

    root.querySelector<HTMLButtonElement>("button.jobs-toggle")!.click();
    await vi.waitFor(() => expect(root.querySelector("button.jobs-toggle .jobs-dot")).toBeNull());
  });
});

describe("settings", () => {
  const gear = (): HTMLButtonElement => root.querySelector<HTMLButtonElement>("button.settings-toggle")!;
  const scrim = (): HTMLElement => root.querySelector<HTMLElement>(".settings-scrim")!;
  const theme = (value: string): HTMLButtonElement =>
    root.querySelector<HTMLButtonElement>(`[aria-labelledby="settings-theme"] [data-value="${value}"]`)!;
  const press = (init: KeyboardEventInit): void => {
    document.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init }));
  };

  afterEach(() => {
    const html = document.documentElement;
    delete html.dataset.theme;
    for (const name of ["--text-scale", "--line-width", "--reading-font"]) html.style.removeProperty(name);
  });

  it("puts a gear next to Jobs that opens and closes the Settings dialog", async () => {
    await mountApp(root, { storage });
    expect(gear().nextElementSibling).toBe(root.querySelector("button.jobs-toggle"));
    expect(gear().getAttribute("aria-label")).toBe("Settings");
    expect(scrim().hidden).toBe(true);
    gear().click();
    expect(scrim().hidden).toBe(false);
    expect(gear().getAttribute("aria-expanded")).toBe("true");
    press({ key: "Escape" });
    expect(scrim().hidden).toBe(true);
    expect(gear().getAttribute("aria-expanded")).toBe("false");
  });

  it("opens with Cmd+comma and closes with a second press", async () => {
    await mountApp(root, { storage });
    press({ key: ",", metaKey: true });
    expect(scrim().hidden).toBe(false);
    press({ key: ",", metaKey: true });
    expect(scrim().hidden).toBe(true);
    press({ key: "," });
    expect(scrim().hidden).toBe(true);
  });

  it("applies a theme the moment it is chosen and remembers it for the next launch", async () => {
    await mountApp(root, { storage });
    gear().click();
    theme("dark").click();
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(JSON.parse(saved.get("tutordocs.settings.v1")!)).toMatchObject({ theme: "dark" });

    delete document.documentElement.dataset.theme;
    await mountApp(root, { storage });
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("opens a reader with a separator on each side of the document", async () => {
    await mountApp(root, { storage });
    await openFromHome("Cell Biology");
    const separators = Array.from(root.querySelectorAll(".reader [role='separator']"));
    expect(separators.map((s) => s.getAttribute("aria-label"))).toEqual(["Resize the sidebar", "Resize the study panel"]);
  });

  it("points a chapter's figure at the palette of the forced theme and back to macOS", async () => {
    backend({
      read_chapter: (args) => ({
        id: args.chapterId,
        title: "One",
        state: "ready",
        markdown: '# One\n\n![Power path](visuals/path.svg "Power takes one route.")\n',
      }),
    });
    await mountApp(root, { storage });
    await openFromHome("Cell Biology");
    const figure = (): string => root.querySelector<HTMLImageElement>("img")!.getAttribute("src")!;
    expect(figure()).toBe("visual://localhost/one/path.svg");
    gear().click();
    theme("dark").click();
    expect(figure()).toBe("visual://localhost/one/path.svg?theme=dark");
    theme("light").click();
    expect(figure()).toBe("visual://localhost/one/path.svg?theme=light");
    theme("system").click();
    expect(figure()).toBe("visual://localhost/one/path.svg");
  });
});
