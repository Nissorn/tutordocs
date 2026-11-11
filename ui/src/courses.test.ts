import { describe, expect, it } from "vitest";
import readerContract from "./fixtures/reader-contract.json";
import {
  allTags,
  describeOpened,
  encodeFilter,
  decodeFilter,
  filterCourses,
  homeOrder,
  parseCourse,
  parseTagInput,
  parseTrashEntry,
  type LibraryCourse,
} from "./courses";

function course(over: Partial<LibraryCourse> & { id: string }): LibraryCourse {
  return {
    storedId: over.id,
    title: over.id,
    tags: [],
    objectives: [],
    chapterCount: 0,
    chapterTitles: [],
    writtenChapterCount: 0,
    createdAt: 0,
    openedAt: null,
    error: null,
    outlineStatus: "approved",
    planning: false,
    ...over,
  };
}

const biology = course({
  id: "biology",
  title: "Cell Biology",
  tags: ["science", "Exam"],
  objectives: ["Explain how a membrane separates a cell"],
  chapterTitles: ["Cells", "Membranes and transport"],
});
const history = course({
  id: "history",
  title: "Roman History",
  tags: ["humanities"],
  objectives: ["Trace the fall of the republic"],
  chapterTitles: ["The Gracchi", "Caesar"],
});
const maths = course({ id: "maths", title: "Linear Algebra", tags: ["science"], chapterTitles: ["Vectors"] });
const all = [biology, history, maths];

describe("search", () => {
  const ids = (query: string, tag: string | null = null) =>
    filterCourses(all, { query, tag }).map((c) => c.id);

  it("shows everything for an empty or blank query", () => {
    expect(ids("")).toEqual(["biology", "history", "maths"]);
    expect(ids("   ")).toEqual(["biology", "history", "maths"]);
  });

  it("matches the title, ignoring case", () => {
    expect(ids("ROMAN")).toEqual(["history"]);
  });

  it("matches a tag", () => {
    expect(ids("humanities")).toEqual(["history"]);
  });

  it("matches an objective", () => {
    expect(ids("republic")).toEqual(["history"]);
  });

  it("matches a chapter title", () => {
    expect(ids("transport")).toEqual(["biology"]);
    expect(ids("vectors")).toEqual(["maths"]);
  });

  it("matches inside a word, as the user types", () => {
    expect(ids("memb")).toEqual(["biology"]);
  });

  it("needs every word to match somewhere in the same course", () => {
    expect(ids("cell exam")).toEqual(["biology"]);
    expect(ids("cell caesar")).toEqual([]);
  });

  it("returns nothing when no course matches", () => {
    expect(ids("quantum")).toEqual([]);
  });
});

describe("tag filter", () => {
  it("keeps only courses carrying the tag, ignoring case", () => {
    expect(filterCourses(all, { query: "", tag: "EXAM" }).map((c) => c.id)).toEqual(["biology"]);
    expect(filterCourses(all, { query: "", tag: "science" }).map((c) => c.id)).toEqual(["biology", "maths"]);
  });

  it("combines with the query", () => {
    expect(filterCourses(all, { query: "linear", tag: "science" }).map((c) => c.id)).toEqual(["maths"]);
    expect(filterCourses(all, { query: "roman", tag: "science" })).toEqual([]);
  });

  it("lists every tag once, in first-seen spelling, sorted", () => {
    expect(allTags(all)).toEqual(["Exam", "humanities", "science"]);
    expect(allTags([course({ id: "a", tags: ["Math"] }), course({ id: "b", tags: ["math"] })])).toEqual(["Math"]);
  });
});

describe("tag text typed on a card", () => {
  it("splits on commas, trims, and drops blanks", () => {
    expect(parseTagInput(" science, exam ,, ")).toEqual(["science", "exam"]);
    expect(parseTagInput("")).toEqual([]);
  });
});

describe("home order", () => {
  it("puts the most recently opened first, then never-opened by newest created", () => {
    const a = course({ id: "a", openedAt: 100 });
    const b = course({ id: "b", openedAt: 300 });
    const c = course({ id: "c", createdAt: 50 });
    const d = course({ id: "d", createdAt: 90 });
    expect(homeOrder([a, c, b, d]).map((x) => x.id)).toEqual(["b", "a", "d", "c"]);
  });

  it("does not reorder the caller's array", () => {
    const input = [course({ id: "a" }), course({ id: "b", openedAt: 5 })];
    homeOrder(input);
    expect(input.map((c) => c.id)).toEqual(["a", "b"]);
  });
});

describe("last opened in words", () => {
  const now = 1_700_000_000;
  it("says so when a course was never opened", () => {
    expect(describeOpened(null, now)).toBe("Not opened yet");
  });
  it("reads minutes, hours and days", () => {
    expect(describeOpened(now - 20, now)).toBe("Opened just now");
    expect(describeOpened(now - 5 * 60, now)).toBe("Opened 5 minutes ago");
    expect(describeOpened(now - 3600, now)).toBe("Opened 1 hour ago");
    expect(describeOpened(now - 3 * 86_400, now)).toBe("Opened 3 days ago");
    expect(describeOpened(now - 86_400, now)).toBe("Opened yesterday");
  });
  it("falls back to the date after a month, and survives a clock that is behind", () => {
    expect(describeOpened(now - 40 * 86_400, now)).toMatch(/^Opened \d{4}-\d{2}-\d{2}$/);
    expect(describeOpened(now + 500, now)).toBe("Opened just now");
  });
});

describe("parsing what Rust sends", () => {
  const wire = {
    id: "cell-biology",
    storedId: "bio101",
    title: "Cell Biology",
    tags: ["biology"],
    objectives: ["Explain the membrane"],
    chapterCount: 2,
    chapterTitles: ["Cells", "Membranes"],
    writtenChapterCount: 1,
    createdAt: 1_700_000_000,
    openedAt: null,
    error: null,
    outlineStatus: "approved" as const,
    planning: false,
  };

  it("reads a full summary", () => {
    expect(parseCourse(wire)).toEqual(wire);
  });

  it("reads the summary the Rust test pins in the shared fixture", () => {
    const parsed = parseCourse(readerContract.list_library_courses[0]);
    expect(parsed.chapterTitles).toEqual(["Cells", "Membranes"]);
    expect(parsed.outlineStatus).toBe("approved");
    expect(parsed.planning).toBe(false);
  });

  it("defaults missing outlineStatus to approved and missing planning to false", () => {
    const { outlineStatus: _o, planning: _p, ...older } = wire;
    const parsed = parseCourse(older);
    expect(parsed.outlineStatus).toBe("approved");
    expect(parsed.planning).toBe(false);
  });

  it("reads draft outline status and planning true", () => {
    const draft = { ...wire, outlineStatus: "draft", planning: true };
    const parsed = parseCourse(draft);
    expect(parsed.outlineStatus).toBe("draft");
    expect(parsed.planning).toBe(true);
  });

  it("reads an error row with no title", () => {
    const broken = { ...wire, title: "", error: "course.json is not valid", chapterTitles: [], storedId: "" };
    expect(parseCourse(broken).error).toBe("course.json is not valid");
  });

  it("refuses a summary with a missing field, so a contract drift fails loudly", () => {
    const { chapterTitles: _gone, ...short } = wire;
    expect(() => parseCourse(short)).toThrow(/chapterTitles/);
  });
});

describe("parseTrashEntry", () => {
  it("parses a valid trash entry matching the contract", () => {
    const raw = {
      trashId: "biology-1700000000",
      courseId: "biology",
      title: "Cell Biology",
      trashedAt: "2026-11-09T12:00:00Z",
      chapterCount: 5,
      writtenChapterCount: 3,
      bytes: 123456,
    };
    expect(parseTrashEntry(raw)).toEqual(raw);
  });

  it("refuses a malformed trash entry", () => {
    expect(() => parseTrashEntry({ trashId: "bio" })).toThrow();
  });
});

describe("filter encoding and decoding", () => {
  it("encodes and decodes a valid filter", () => {
    const filter = { query: "cell", tag: "science" };
    expect(decodeFilter(encodeFilter(filter))).toEqual(filter);
  });

  it("decodes null or empty tags", () => {
    const filter = { query: "history", tag: null };
    expect(decodeFilter(encodeFilter(filter))).toEqual(filter);
  });

  it("handles malformed raw values safely", () => {
    expect(decodeFilter(null)).toEqual({ query: "", tag: null });
    expect(decodeFilter(undefined)).toEqual({ query: "", tag: null });
    expect(decodeFilter(123)).toEqual({ query: "", tag: null });
    expect(decodeFilter("query")).toEqual({ query: "", tag: null });
    expect(decodeFilter([])).toEqual({ query: "", tag: null });
    expect(decodeFilter({ query: 42, tag: true })).toEqual({ query: "", tag: null });
  });
});
