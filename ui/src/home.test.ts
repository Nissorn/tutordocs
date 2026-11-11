// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { HomeFilter, LibraryCourse } from "./courses";
import { mountHome, type HomeHandle, type HomeOptions } from "./home";

const NOW = 1_800_000_000;

function course(over: Partial<LibraryCourse> & { id: string }): LibraryCourse {
  return {
    storedId: over.id,
    title: over.id,
    tags: [],
    objectives: [],
    chapterCount: 0,
    chapterTitles: [],
    writtenChapterCount: 0,
    createdAt: NOW - 10 * 86_400,
    openedAt: null,
    error: null,
    outlineStatus: "approved",
    planning: false,
    ...over,
  };
}

const biology = course({
  id: "cell-biology",
  title: "Cell Biology",
  tags: ["science", "exam"],
  objectives: ["Explain the membrane"],
  chapterCount: 7,
  writtenChapterCount: 3,
  chapterTitles: ["Cells", "Membranes and transport"],
  openedAt: NOW - 3 * 86_400,
});
const history = course({
  id: "roman-history",
  title: "Roman History",
  tags: ["humanities"],
  chapterCount: 4,
  writtenChapterCount: 4,
  chapterTitles: ["The Gracchi", "Caesar"],
  openedAt: NOW - 60,
});
const algebra = course({
  id: "linear-algebra",
  title: "Linear Algebra",
  tags: ["science"],
  chapterTitles: ["Vectors"],
});
const library = [biology, history, algebra];

let root: HTMLElement;
let home: HomeHandle;
let options: HomeOptions;
let sent: Array<{ cmd: string; args: Record<string, unknown> }>;
let reply: (cmd: string, args: Record<string, unknown>) => unknown;

function mount(courses: LibraryCourse[] = library, filter: HomeFilter = { query: "", tag: null }): void {
  options = {
    filter,
    onFilter: vi.fn(),
    onOpen: vi.fn(),
    onNewCourse: vi.fn(),
    onChange: vi.fn(),
    onTrash: vi.fn(),
    onRestore: vi.fn(),
    now: () => NOW,
  };
  home = mountHome(root, courses, options);
}

beforeEach(() => {
  document.body.innerHTML = '<div id="app"></div>';
  root = document.querySelector<HTMLElement>("#app")!;
  sent = [];
  reply = () => {
    throw new Error("no reply scripted");
  };
  mockIPC((cmd, raw) => {
    const args = (raw ?? {}) as Record<string, unknown>;
    sent.push({ cmd, args });
    return reply(cmd, args);
  });
});

afterEach(() => {
  clearMocks();
});

const cards = (): HTMLElement[] => Array.from(root.querySelectorAll<HTMLElement>("article.card"));
const titles = (): string[] => cards().map((c) => c.querySelector("h2")!.textContent!);

/** Present and not hidden. The page hides parts it does not need rather than removing them. */
const shown = (selector: string): boolean => {
  const node = root.querySelector<HTMLElement>(selector);
  return node !== null && !node.hidden;
};

function type(input: HTMLInputElement, text: string): void {
  input.value = text;
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

describe("the cards", () => {
  it("shows one card per course, most recently opened first", () => {
    mount();
    expect(titles()).toEqual(["Roman History", "Cell Biology", "Linear Algebra"]);
  });

  it("gives each card its tags, chapter progress and when it was opened", () => {
    mount();
    const card = cards().find((c) => c.dataset.courseId === "cell-biology")!;
    expect(Array.from(card.querySelectorAll(".tag-chip")).map((c) => c.textContent)).toEqual(["science", "exam"]);
    expect(card.querySelector(".card-progress")!.textContent).toBe("3 of 7 chapters written");
    expect(card.querySelector(".card-opened")!.textContent).toBe("Opened 3 days ago");
    const fresh = cards().find((c) => c.dataset.courseId === "linear-algebra")!;
    expect(fresh.querySelector(".card-progress")!.textContent).toBe("No chapters yet");
    expect(fresh.querySelector(".card-opened")!.textContent).toBe("Not opened yet");
  });

  it("opens the course from its title or from a click on the card", () => {
    mount();
    const card = cards().find((c) => c.dataset.courseId === "cell-biology")!;
    card.querySelector<HTMLButtonElement>("button.card-open")!.click();
    expect(options.onOpen).toHaveBeenLastCalledWith("cell-biology");
    card.querySelector<HTMLElement>(".card-meta")!.click();
    expect(options.onOpen).toHaveBeenCalledTimes(2);
  });

  it("shows a course's error on its card and still opens it", () => {
    mount([course({ id: "broken", error: "course.json is not valid JSON" })]);
    const card = cards()[0];
    expect(card.querySelector('[role="alert"]')!.textContent).toContain("course.json is not valid JSON");
    expect(card.querySelector("h2")!.textContent).toBe("broken");
    card.querySelector<HTMLButtonElement>("button.card-open")!.click();
    expect(options.onOpen).toHaveBeenCalledWith("broken");
  });
});

describe("search", () => {
  const search = (): HTMLInputElement => root.querySelector<HTMLInputElement>("input.home-search")!;

  it("narrows the cards as the user types, and says how many are left", () => {
    mount();
    type(search(), "memb");
    expect(titles()).toEqual(["Cell Biology"]);
    expect(root.querySelector(".home-count")!.textContent).toBe("1 of 3 courses");
    expect(options.onFilter).toHaveBeenLastCalledWith({ query: "memb", tag: null });
  });

  it("asks the web view not to autocorrect or capitalize what the user types", () => {
    mount();
    expect(search().spellcheck).toBe(false);
    expect(search().getAttribute("autocorrect")).toBe("off");
    expect(search().getAttribute("autocapitalize")).toBe("off");
  });

  it("finds a course by a chapter title, ignoring case", () => {
    mount();
    type(search(), "CAESAR");
    expect(titles()).toEqual(["Roman History"]);
  });

  it("says so when nothing matches, and brings the cards back when the search is cleared", () => {
    mount();
    type(search(), "quantum");
    expect(cards()).toHaveLength(0);
    expect(shown(".home-empty")).toBe(true);
    expect(root.querySelector(".home-empty")!.textContent).toContain("No course matches");
    type(search(), "");
    expect(cards()).toHaveLength(3);
    expect(shown(".home-empty")).toBe(false);
  });

  it("starts from the search the user had before they left Home", () => {
    mount(library, { query: "algebra", tag: null });
    expect(search().value).toBe("algebra");
    expect(titles()).toEqual(["Linear Algebra"]);
  });
});

describe("tag filter", () => {
  const pressed = (): string[] =>
    Array.from(root.querySelectorAll<HTMLButtonElement>(".tag-bar button[aria-pressed='true']")).map((b) => b.textContent!);

  it("lists every tag once in a bar above the cards", () => {
    mount();
    expect(Array.from(root.querySelectorAll(".tag-bar button")).map((b) => b.textContent)).toEqual([
      "exam",
      "humanities",
      "science",
    ]);
  });

  it("filters by the tag on a card when its chip is clicked, and clears on a second click", () => {
    mount();
    const science = cards()
      .find((c) => c.dataset.courseId === "linear-algebra")!
      .querySelector<HTMLButtonElement>(".tag-chip")!;
    science.click();
    expect(titles()).toEqual(["Cell Biology", "Linear Algebra"]);
    expect(pressed()).toEqual(["science"]);
    expect(options.onFilter).toHaveBeenLastCalledWith({ query: "", tag: "science" });
    root.querySelector<HTMLButtonElement>(".tag-bar button[aria-pressed='true']")!.click();
    expect(titles()).toHaveLength(3);
    expect(pressed()).toEqual([]);
  });

  it("combines with the search", () => {
    mount();
    root.querySelector<HTMLButtonElement>(".tag-bar button")!.click();
    type(root.querySelector<HTMLInputElement>("input.home-search")!, "roman");
    expect(cards()).toHaveLength(0);
  });

  it("has no tag bar when no course has a tag", () => {
    mount([course({ id: "plain" })]);
    expect(shown(".tag-bar")).toBe(false);
  });
});

describe("editing tags on a card", () => {
  const card = (): HTMLElement => cards().find((c) => c.dataset.courseId === "cell-biology")!;
  const edit = (): HTMLInputElement => card().querySelector<HTMLInputElement>("input.tag-input")!;

  it("saves what was typed when Enter is pressed, then shows the new chips", async () => {
    mount();
    reply = (cmd) => (cmd === "set_course_tags" ? { ...biology, tags: ["science", "finals"] } : null);
    card().querySelector<HTMLButtonElement>("button.card-edit-tags")!.click();
    expect(edit().value).toBe("science, exam");
    type(edit(), "science, finals");
    edit().dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(options.onChange).toHaveBeenCalled());
    expect(sent.find((s) => s.cmd === "set_course_tags")!.args).toEqual({
      courseId: "cell-biology",
      tags: ["science", "finals"],
    });
    expect(options.onChange).toHaveBeenCalledWith({ ...biology, tags: ["science", "finals"] });
  });

  it("drops the edit on Escape without saving", () => {
    mount();
    card().querySelector<HTMLButtonElement>("button.card-edit-tags")!.click();
    type(edit(), "something else");
    edit().dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(sent).toHaveLength(0);
    expect(card().querySelector("input.tag-input")).toBeNull();
    expect(Array.from(card().querySelectorAll(".tag-chip")).map((c) => c.textContent)).toEqual(["science", "exam"]);
  });

  it("keeps the field open and says why when the backend refuses the tags", async () => {
    mount();
    reply = () => {
      throw new Error("a tag may be at most 40 characters");
    };
    card().querySelector<HTMLButtonElement>("button.card-edit-tags")!.click();
    type(edit(), "x".repeat(41));
    edit().dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(card().querySelector('[role="alert"]')).not.toBeNull());
    expect(card().querySelector('[role="alert"]')!.textContent).toContain("at most 40 characters");
    expect(options.onChange).not.toHaveBeenCalled();
    expect(card().querySelector("input.tag-input")).not.toBeNull();
  });

  it("offers Add tags on a card that has none", () => {
    mount([course({ id: "plain", title: "Plain" })]);
    expect(cards()[0].querySelector("button.card-edit-tags")!.textContent).toBe("Add tags");
  });
});

describe("a new course", () => {
  it("asks the page for the wizard from the New course button, and makes nothing itself", () => {
    mount();
    root.querySelector<HTMLButtonElement>("button.home-new")!.click();
    expect(options.onNewCourse).toHaveBeenCalledTimes(1);
    expect(sent).toHaveLength(0);
    expect(root.querySelector("form")).toBeNull();
  });

  it("puts one clear call to start the first course on an empty library", () => {
    mount([]);
    expect(cards()).toHaveLength(0);
    expect(shown(".home-welcome")).toBe(true);
    expect(root.querySelector(".home-welcome")!.textContent).toContain("first course");
    expect(shown("input.home-search")).toBe(false);
    expect(shown("button.home-new")).toBe(true);
    root.querySelector<HTMLButtonElement>("button.home-new")!.click();
    expect(options.onNewCourse).toHaveBeenCalledTimes(1);
  });

  it("brings the cards back, and hides the welcome, once the first course arrives", () => {
    mount([]);
    home.update([biology]);
    expect(cards()).toHaveLength(1);
    expect(shown(".home-welcome")).toBe(false);
    expect(shown("input.home-search")).toBe(true);
  });
});

describe("staying current", () => {
  it("re-renders the cards from a new list without losing the search text", () => {
    mount();
    const search = root.querySelector<HTMLInputElement>("input.home-search")!;
    type(search, "science");
    home.update([{ ...biology, writtenChapterCount: 5 }, history, algebra]);
    expect(root.querySelector<HTMLInputElement>("input.home-search")).toBe(search);
    expect(search.value).toBe("science");
    expect(cards().find((c) => c.dataset.courseId === "cell-biology")!.querySelector(".card-progress")!.textContent).toBe(
      "5 of 7 chapters written",
    );
  });

  it("keeps a tag edit that is in progress when the list refreshes", () => {
    mount();
    const card = () => cards().find((c) => c.dataset.courseId === "cell-biology")!;
    card().querySelector<HTMLButtonElement>("button.card-edit-tags")!.click();
    type(card().querySelector<HTMLInputElement>("input.tag-input")!, "science, half-typed");
    home.update([biology, history, algebra]);
    expect(card().querySelector<HTMLInputElement>("input.tag-input")!.value).toBe("science, half-typed");
  });
});

describe("lifecycle states", () => {
  it("shows Planning badge when planning is true", () => {
    mount([course({ id: "planning-course", title: "Planning Course", planning: true })]);
    const card = cards()[0];
    expect(card.querySelector(".card-status")!.textContent).toBe("Planning");
  });

  it("shows Setup not finished and offers Continue and Discard on zero-chapter course", () => {
    mount([course({ id: "shell", title: "Empty Shell", chapterCount: 0 })]);
    const card = cards()[0];
    expect(card.querySelector(".card-status")!.textContent).toBe("Setup not finished");
    const cont = card.querySelector<HTMLButtonElement>("button.card-continue");
    const discard = card.querySelector<HTMLButtonElement>("button.card-discard");
    expect(cont).not.toBeNull();
    expect(discard).not.toBeNull();

    cont!.click();
    expect(options.onOpen).toHaveBeenCalledWith("shell");
  });

  it("disables Discard with reason when zero-chapter course is planning", () => {
    mount([course({ id: "shell", title: "Empty Shell", chapterCount: 0, planning: true })]);
    const card = cards()[0];
    const discard = card.querySelector<HTMLButtonElement>("button.card-discard")!;
    expect(discard).not.toBeNull();
    expect(discard.disabled).toBe(true);
    const reason = card.querySelector<HTMLElement>(".card-discard-reason")!;
    expect(reason).not.toBeNull();
    expect(reason.textContent).toBe("Planning. Discard when it finishes.");
  });

  it("shows Draft badge on a course with draft outline and chapters", () => {
    mount([course({ id: "draft-c", title: "Draft Course", chapterCount: 3, outlineStatus: "draft" })]);
    const card = cards()[0];
    expect(card.querySelector(".card-status")!.textContent).toBe("Draft");
  });

  it("shows Approved badge on an approved course with chapters", () => {
    mount([course({ id: "appr-c", title: "Approved Course", chapterCount: 3, outlineStatus: "approved" })]);
    const card = cards()[0];
    expect(card.querySelector(".card-status")!.textContent).toBe("Approved");
  });
});

describe("zero-chapter discard", () => {
  it("discard prompts confirm and calls trash_course on confirm", async () => {
    const shell = course({ id: "shell", title: "Empty Shell", chapterCount: 0 });
    reply = (cmd) => {
      if (cmd === "trash_course") {
        return {
          trashId: "shell-1700000000",
          courseId: "shell",
          title: "Empty Shell",
          trashedAt: "2026-11-09T12:00:00Z",
          chapterCount: 0,
          writtenChapterCount: 0,
          bytes: 100,
        };
      }
      throw new Error(`unexpected command ${cmd}`);
    };
    mount([shell]);
    const card = () => cards()[0];
    card().querySelector<HTMLButtonElement>("button.card-discard")!.click();
    expect(card().querySelector(".confirm-prompt")).not.toBeNull();
    expect(sent.filter((s) => s.cmd === "trash_course")).toHaveLength(0);

    card().querySelector<HTMLButtonElement>("button.confirm-discard")!.click();
    await vi.waitFor(() => expect(sent.filter((s) => s.cmd === "trash_course")).toHaveLength(1));
    expect(sent.find((s) => s.cmd === "trash_course")!.args).toEqual({ courseId: "shell" });
    await vi.waitFor(() => expect(options.onTrash).toHaveBeenCalledWith("shell"));
  });

  it("discard cancel drops prompt without trashing", () => {
    mount([course({ id: "shell", title: "Empty Shell", chapterCount: 0 })]);
    const card = () => cards()[0];
    card().querySelector<HTMLButtonElement>("button.card-discard")!.click();
    expect(card().querySelector(".confirm-prompt")).not.toBeNull();

    card().querySelector<HTMLButtonElement>("button.cancel-discard")!.click();
    expect(card().querySelector(".confirm-prompt")).toBeNull();
    expect(sent.filter((s) => s.cmd === "trash_course")).toHaveLength(0);
  });
});

describe("card actions menu (rename and trash)", () => {
  it("renames course through rename_course from card menu", async () => {
    reply = (cmd, args) => {
      if (cmd === "rename_course") {
        return { ...biology, title: args.title as string };
      }
      throw new Error(`unexpected ${cmd}`);
    };
    mount([biology]);
    const card = () => cards()[0];
    card().querySelector<HTMLButtonElement>("button.card-menu-button")!.click();
    expect(card().querySelector(".card-menu")).not.toBeNull();

    card().querySelector<HTMLButtonElement>("button.card-action-rename")!.click();
    const input = card().querySelector<HTMLInputElement>("input.card-rename-input")!;
    expect(input.value).toBe("Cell Biology");
    type(input, "Advanced Cell Biology");
    card().querySelector<HTMLButtonElement>("button.card-rename-save")!.click();

    await vi.waitFor(() => expect(sent.filter((s) => s.cmd === "rename_course")).toHaveLength(1));
    expect(sent.find((s) => s.cmd === "rename_course")!.args).toEqual({
      courseId: "cell-biology",
      title: "Advanced Cell Biology",
    });
    await vi.waitFor(() => expect(options.onChange).toHaveBeenCalledWith(expect.objectContaining({ title: "Advanced Cell Biology" })));
  });

  it("trashes course with confirmation from card menu", async () => {
    reply = (cmd) => {
      if (cmd === "trash_course") {
        return {
          trashId: "cell-biology-1700000000",
          courseId: "cell-biology",
          title: "Cell Biology",
          trashedAt: "2026-11-09T12:00:00Z",
          chapterCount: 7,
          writtenChapterCount: 3,
          bytes: 5000,
        };
      }
      throw new Error(`unexpected ${cmd}`);
    };
    mount([biology]);
    const card = () => cards()[0];
    card().querySelector<HTMLButtonElement>("button.card-menu-button")!.click();
    card().querySelector<HTMLButtonElement>("button.card-action-trash")!.click();

    expect(card().querySelector(".confirm-prompt")).not.toBeNull();
    expect(sent.filter((s) => s.cmd === "trash_course")).toHaveLength(0);

    card().querySelector<HTMLButtonElement>("button.confirm-trash")!.click();
    await vi.waitFor(() => expect(sent.filter((s) => s.cmd === "trash_course")).toHaveLength(1));
    expect(sent.find((s) => s.cmd === "trash_course")!.args).toEqual({ courseId: "cell-biology" });
    await vi.waitFor(() => expect(options.onTrash).toHaveBeenCalledWith("cell-biology"));
  });
});

describe("trash view", () => {
  const fakeTrash = [
    {
      trashId: "biology-1700000000",
      courseId: "cell-biology",
      title: "Cell Biology",
      trashedAt: "2026-11-09T12:00:00Z",
      chapterCount: 7,
      writtenChapterCount: 3,
      bytes: 12345,
    },
  ];

  it("opens trash view, lists entries keyed by trashId, and returns to courses", async () => {
    reply = (cmd) => {
      if (cmd === "list_trash") return fakeTrash;
      throw new Error(`unexpected ${cmd}`);
    };
    mount();
    root.querySelector<HTMLButtonElement>("button.home-trash")!.click();
    await vi.waitFor(() => expect(sent.filter((s) => s.cmd === "list_trash")).toHaveLength(1));
    await vi.waitFor(() => expect(root.querySelector('[data-trash-id="biology-1700000000"]')).not.toBeNull());

    expect(root.querySelector(".trash-view")).not.toBeNull();
    const row = root.querySelector<HTMLElement>('[data-trash-id="biology-1700000000"]')!;
    expect(row).not.toBeNull();
    expect(row.textContent).toContain("Cell Biology");
    expect(row.textContent).toContain("12 KB");
    const timeSpan = row.querySelector<HTMLSpanElement>("span.trash-time")!;
    expect(timeSpan).not.toBeNull();
    expect(timeSpan.title).toBe("2026-11-09T12:00:00Z");

    root.querySelector<HTMLButtonElement>("button.home-back-courses")!.click();
    expect(root.querySelector(".trash-view")).toBeNull();
    expect(root.querySelector(".cards")).not.toBeNull();
  });

  it("restores a course from trash via restore_course", async () => {
    reply = (cmd) => {
      if (cmd === "list_trash") return fakeTrash;
      if (cmd === "restore_course") return biology;
      throw new Error(`unexpected ${cmd}`);
    };
    mount();
    root.querySelector<HTMLButtonElement>("button.home-trash")!.click();
    await vi.waitFor(() => expect(root.querySelector('[data-trash-id="biology-1700000000"]')).not.toBeNull());

    root.querySelector<HTMLButtonElement>('[data-trash-id="biology-1700000000"] button.trash-restore')!.click();
    await vi.waitFor(() => expect(sent.filter((s) => s.cmd === "restore_course")).toHaveLength(1));
    expect(sent.find((s) => s.cmd === "restore_course")!.args).toEqual({ trashId: "biology-1700000000" });
    await vi.waitFor(() => expect(options.onRestore).toHaveBeenCalledWith(biology));
  });

  it("empties a trash entry only after explicit confirmation step", async () => {
    reply = (cmd) => {
      if (cmd === "list_trash") return fakeTrash;
      if (cmd === "empty_trash") return null;
      throw new Error(`unexpected ${cmd}`);
    };
    mount();
    root.querySelector<HTMLButtonElement>("button.home-trash")!.click();
    await vi.waitFor(() => expect(root.querySelector('[data-trash-id="biology-1700000000"]')).not.toBeNull());

    const row = () => root.querySelector<HTMLElement>('[data-trash-id="biology-1700000000"]')!;
    row().querySelector<HTMLButtonElement>("button.trash-empty")!.click();

    expect(row().querySelector(".confirm-prompt")).not.toBeNull();
    expect(sent.filter((s) => s.cmd === "empty_trash")).toHaveLength(0);

    row().querySelector<HTMLButtonElement>("button.cancel-empty")!.click();
    expect(row().querySelector(".confirm-prompt")).toBeNull();
    expect(sent.filter((s) => s.cmd === "empty_trash")).toHaveLength(0);

    row().querySelector<HTMLButtonElement>("button.trash-empty")!.click();
    row().querySelector<HTMLButtonElement>("button.confirm-empty")!.click();

    await vi.waitFor(() => expect(sent.filter((s) => s.cmd === "empty_trash")).toHaveLength(1));
    expect(sent.find((s) => s.cmd === "empty_trash")!.args).toEqual({ trashId: "biology-1700000000" });
  });
});

describe("completion notice", () => {
  it("displays completion notice on Home and opens course from it", () => {
    mount([biology]);
    home.showCompletionNotice(biology);
    const notice = root.querySelector<HTMLElement>(".home-notice");
    expect(notice).not.toBeNull();
    expect(notice!.textContent).toContain("Cell Biology");

    notice!.querySelector<HTMLButtonElement>("button.notice-open")!.click();
    expect(options.onOpen).toHaveBeenCalledWith("cell-biology");
    expect(root.querySelector(".home-notice")).toBeNull();
  });

  it("dismisses completion notice without opening", () => {
    mount([biology]);
    home.showCompletionNotice(biology);
    const notice = root.querySelector<HTMLElement>(".home-notice")!;
    notice.querySelector<HTMLButtonElement>("button.notice-dismiss")!.click();
    expect(root.querySelector(".home-notice")).toBeNull();
    expect(options.onOpen).not.toHaveBeenCalled();
  });
});
