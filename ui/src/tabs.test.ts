import { describe, expect, it } from "vitest";
import {
  activate,
  closeTab,
  decodeTabs,
  dropMissing,
  emptyTabs,
  encodeTabs,
  openCourse,
  remember,
  tabFor,
} from "./tabs";

function openAll(...ids: string[]) {
  return ids.reduce(openCourse, emptyTabs);
}

describe("opening a course", () => {
  it("starts on Home with no course tabs", () => {
    expect(emptyTabs).toEqual({ open: [], active: null });
  });

  it("adds a tab at the end and makes it active", () => {
    const state = openAll("biology", "history");
    expect(state.open.map((t) => t.courseId)).toEqual(["biology", "history"]);
    expect(state.active).toBe("history");
  });

  it("focuses the tab that is already open instead of adding a second", () => {
    const state = openCourse(openAll("biology", "history"), "biology");
    expect(state.open.map((t) => t.courseId)).toEqual(["biology", "history"]);
    expect(state.active).toBe("biology");
  });

  it("starts a new tab with no chapter and no scroll", () => {
    expect(tabFor(openAll("biology"), "biology")).toEqual({
      courseId: "biology",
      chapterId: null,
      scrollTop: 0,
      view: "chapter",
    });
  });
});

describe("switching", () => {
  it("goes Home and back without touching the open tabs", () => {
    const home = activate(openAll("biology", "history"), null);
    expect(home.active).toBeNull();
    expect(home.open).toHaveLength(2);
    expect(activate(home, "biology").active).toBe("biology");
  });

  it("ignores a course that has no tab, so the active tab is always an open one", () => {
    const state = openAll("biology");
    expect(activate(state, "ghost")).toBe(state);
  });
});

describe("closing", () => {
  it("activates the tab that slides into the closed one's place", () => {
    const state = activate(openAll("a", "b", "c"), "b");
    expect(closeTab(state, "b").active).toBe("c");
  });

  it("falls back to the tab on the left when the last tab closes", () => {
    expect(closeTab(openAll("a", "b", "c"), "c").active).toBe("b");
  });

  it("falls back to Home when the only tab closes", () => {
    expect(closeTab(openAll("a"), "a")).toEqual({ open: [], active: null });
  });

  it("keeps the active tab when a different tab closes", () => {
    const state = activate(openAll("a", "b", "c"), "a");
    const next = closeTab(state, "c");
    expect(next.active).toBe("a");
    expect(next.open.map((t) => t.courseId)).toEqual(["a", "b"]);
  });

  it("forgets the closed tab's position, so reopening starts fresh", () => {
    const scrolled = remember(openAll("a"), "a", { chapterId: "cells", scrollTop: 300, view: "chapter" });
    const reopened = openCourse(closeTab(scrolled, "a"), "a");
    expect(tabFor(reopened, "a")).toEqual({ courseId: "a", chapterId: null, scrollTop: 0, view: "chapter" });
  });
});

describe("per-tab position", () => {
  it("keeps each tab's chapter and scroll apart", () => {
    let state = openAll("a", "b");
    state = remember(state, "a", { chapterId: "cells", scrollTop: 120, view: "chapter" });
    state = remember(state, "b", { chapterId: "wars", scrollTop: 40, view: "chapter" });
    expect(tabFor(state, "a")).toMatchObject({ chapterId: "cells", scrollTop: 120, view: "chapter" });
    expect(tabFor(state, "b")).toMatchObject({ chapterId: "wars", scrollTop: 40, view: "chapter" });
  });

  it("returns the same state when the position did not change, so a scroll handler stays cheap", () => {
    const state = remember(openAll("a"), "a", { chapterId: "cells", scrollTop: 120, view: "chapter" });
    expect(remember(state, "a", { chapterId: "cells", scrollTop: 120, view: "chapter" })).toBe(state);
  });

  it("updates state when view mode changes between chapter, quiz, pdf, and review", () => {
    const base = remember(openAll("a"), "a", { chapterId: "cells", scrollTop: 120, view: "chapter" });
    const quizState = remember(base, "a", {
      chapterId: "cells",
      scrollTop: 0,
      view: "quiz",
      quiz: { answers: {}, grades: [] },
    });
    expect(quizState).not.toBe(base);
    expect(tabFor(quizState, "a")?.view).toBe("quiz");

    const reviewState = remember(quizState, "a", { chapterId: null, scrollTop: 0, view: "review" });
    expect(reviewState).not.toBe(quizState);
    expect(tabFor(reviewState, "a")?.view).toBe("review");

    const pdfState = remember(reviewState, "a", {
      chapterId: null,
      scrollTop: 0,
      view: "pdf",
      pdf: { sourceName: "doc.pdf", page: 1, zoom: 1 },
    });
    expect(pdfState).not.toBe(reviewState);
    expect(tabFor(pdfState, "a")?.view).toBe("pdf");
    const pTab = tabFor(pdfState, "a");
    if (pTab?.view === "pdf") {
      expect(pTab.pdf.sourceName).toBe("doc.pdf");
    }
  });

  it("updates state when quiz answers or grades change", () => {
    const q1 = {
      id: "q1",
      objective: "Cell theory",
      prompt: "What is a cell?",
      referenceAnswer: "Unit",
      combines: [],
    };
    const s1 = remember(openAll("a"), "a", {
      chapterId: "cells",
      scrollTop: 0,
      view: "quiz",
      quiz: { chapterId: "cells", quiz: { chapterId: "cells", questions: [q1] }, answers: { q1: "first" } },
    });
    const s2 = remember(s1, "a", {
      chapterId: "cells",
      scrollTop: 0,
      view: "quiz",
      quiz: { chapterId: "cells", quiz: { chapterId: "cells", questions: [q1] }, answers: { q1: "second" } },
    });
    expect(s2).not.toBe(s1);
    const tabA = tabFor(s2, "a");
    expect(tabA?.view).toBe("quiz");
    if (tabA?.view === "quiz") {
      expect(tabA.quiz.answers?.q1).toBe("second");
    }

    // Identical quiz state returns same state reference
    const s3 = remember(s2, "a", {
      chapterId: "cells",
      scrollTop: 0,
      view: "quiz",
      quiz: { chapterId: "cells", quiz: { chapterId: "cells", questions: [q1] }, answers: { q1: "second" } },
    });
    expect(s3).toBe(s2);
  });

  it("updates state when pdf page or zoom changes", () => {
    const s1 = remember(openAll("a"), "a", {
      chapterId: null,
      scrollTop: 0,
      view: "pdf",
      pdf: { sourceName: "doc.pdf", page: 1, zoom: 1 },
    });
    const s2 = remember(s1, "a", {
      chapterId: null,
      scrollTop: 0,
      view: "pdf",
      pdf: { sourceName: "doc.pdf", page: 2, zoom: 1 },
    });
    expect(s2).not.toBe(s1);
    const tabPdf2 = tabFor(s2, "a");
    expect(tabPdf2?.view).toBe("pdf");
    if (tabPdf2?.view === "pdf") {
      expect(tabPdf2.pdf.page).toBe(2);
    }

    const s3 = remember(s2, "a", {
      chapterId: null,
      scrollTop: 0,
      view: "pdf",
      pdf: { sourceName: "doc.pdf", page: 2, zoom: 1.25 },
    });
    expect(s3).not.toBe(s2);
    const tabPdf3 = tabFor(s3, "a");
    expect(tabPdf3?.view).toBe("pdf");
    if (tabPdf3?.view === "pdf") {
      expect(tabPdf3.pdf.zoom).toBe(1.25);
    }

    // Identical pdf state returns same state
    expect(
      remember(s3, "a", {
        chapterId: null,
        scrollTop: 0,
        view: "pdf",
        pdf: { sourceName: "doc.pdf", page: 2, zoom: 1.25 },
      }),
    ).toBe(s3);
  });

  it("ignores a position for a tab that is not open", () => {
    const state = openAll("a");
    expect(remember(state, "ghost", { chapterId: "x", scrollTop: 1, view: "chapter" })).toBe(state);
  });
});

describe("a course that no longer exists", () => {
  it("is dropped, and the active tab falls back to Home when it was the active one", () => {
    const state = activate(openAll("a", "gone", "c"), "gone");
    const next = dropMissing(state, ["a", "c"]);
    expect(next.open.map((t) => t.courseId)).toEqual(["a", "c"]);
    expect(next.active).toBeNull();
  });

  it("keeps an active tab whose course still exists", () => {
    const state = activate(openAll("a", "gone", "c"), "c");
    expect(dropMissing(state, ["a", "c"]).active).toBe("c");
  });
});

describe("what is remembered across restarts", () => {
  it("round-trips the open tabs, the active one and each position", () => {
    let state = openAll("a", "b");
    state = remember(state, "a", { chapterId: "cells", scrollTop: 120, view: "chapter" });
    state = activate(state, "a");
    expect(decodeTabs(JSON.parse(JSON.stringify(encodeTabs(state))))).toEqual(state);
  });

  it("reads anything unexpected as an empty set instead of throwing", () => {
    for (const raw of [null, undefined, 7, "x", [], {}, { open: "no" }, { open: [{ courseId: 4 }], active: 1 }]) {
      expect(decodeTabs(raw)).toEqual(emptyTabs);
    }
  });

  it("drops a malformed tab but keeps the good ones, and never lets two tabs share a course", () => {
    const raw = {
      open: [{ courseId: "a" }, { courseId: "a", chapterId: "x", scrollTop: 5 }, { nope: 1 }, { courseId: "b", scrollTop: -3 }],
      active: "b",
    };
    const state = decodeTabs(raw);
    expect(state.open.map((t) => t.courseId)).toEqual(["a", "b"]);
    expect(tabFor(state, "b")?.scrollTop).toBe(0);
    expect(state.active).toBe("b");
  });

  it("reads an active tab that is not in the list as Home", () => {
    expect(decodeTabs({ open: [{ courseId: "a" }], active: "z" }).active).toBeNull();
  });

  it("round-trips quiz view with draft answers and grades", () => {
    let state = openAll("bio");
    state = remember(state, "bio", {
      chapterId: "cells",
      scrollTop: 0,
      view: "quiz",
      quiz: {
        chapterId: "cells",
        quiz: {
          chapterId: "cells",
          questions: [
            {
              id: "q1",
              objective: "Understand cells",
              prompt: "What is a cell?",
              referenceAnswer: "The unit of life",
              combines: [],
            },
          ],
        },
        answers: { q1: "A living unit" },
        grades: [
          {
            questionId: "q1",
            objective: "Understand cells",
            result: "correct",
            final: true,
            confidence: 0.95,
            feedback: "Spot on.",
          },
        ],
      },
    });
    const encoded = JSON.parse(JSON.stringify(encodeTabs(state)));
    const decoded = decodeTabs(encoded);
    expect(decoded).toEqual(state);
    const tabBio = tabFor(decoded, "bio");
    expect(tabBio?.view).toBe("quiz");
    if (tabBio?.view === "quiz") {
      expect(tabBio.quiz.answers).toEqual({ q1: "A living unit" });
      expect(tabBio.quiz.grades?.[0]?.result).toBe("correct");
    }
  });

  it("round-trips pdf view with source name, page and zoom", () => {
    let state = openAll("history");
    state = remember(state, "history", {
      chapterId: null,
      scrollTop: 150,
      view: "pdf",
      pdf: {
        sourceName: "lecture1.pdf",
        page: 3,
        zoom: 1.25,
      },
    });
    const encoded = JSON.parse(JSON.stringify(encodeTabs(state)));
    const decoded = decodeTabs(encoded);
    expect(decoded).toEqual(state);
    const tabHist = tabFor(decoded, "history");
    expect(tabHist?.view).toBe("pdf");
    if (tabHist?.view === "pdf") {
      expect(tabHist.pdf).toEqual({
        sourceName: "lecture1.pdf",
        page: 3,
        zoom: 1.25,
      });
    }
  });

  it("round-trips review view mode", () => {
    let state = openAll("math");
    state = remember(state, "math", {
      chapterId: null,
      scrollTop: 0,
      view: "review",
    });
    const encoded = JSON.parse(JSON.stringify(encodeTabs(state)));
    const decoded = decodeTabs(encoded);
    expect(decoded).toEqual(state);
    expect(tabFor(decoded, "math")?.view).toBe("review");
  });

  it("sanitizes malformed view, quiz, and pdf structures in decodeTabs", () => {
    const raw = {
      open: [
        {
          courseId: "a",
          chapterId: "c1",
          scrollTop: 10,
          view: "unsupported_view",
          quiz: "not an object",
          pdf: { sourceName: 123, page: "not a number", zoom: -5 },
        },
        {
          courseId: "b",
          chapterId: null,
          scrollTop: 0,
          view: "quiz",
          quiz: {
            chapterId: 456,
            answers: "bad",
            grades: [{ questionId: 1, result: "unknown" }],
          },
        },
      ],
      active: "a",
    };
    const state = decodeTabs(raw);
    const tabA = tabFor(state, "a");
    expect(tabA?.view).toBe("chapter");

    const tabB = tabFor(state, "b");
    expect(tabB?.view).toBe("chapter");
  });

  it("decodes a stored tab with no view as chapter view, preserving legacy saved tabs", () => {
    const raw = {
      open: [{ courseId: "legacy", chapterId: "intro", scrollTop: 250 }],
      active: "legacy",
    };
    const state = decodeTabs(raw);
    expect(tabFor(state, "legacy")).toEqual({
      courseId: "legacy",
      chapterId: "intro",
      scrollTop: 250,
      view: "chapter",
    });
    expect(state.active).toBe("legacy");
  });

  it("drops corrupt quiz drafts without blocking opening the tab", () => {
    const raw = {
      open: [
        {
          courseId: "corrupt-quiz",
          chapterId: "chap1",
          scrollTop: 40,
          view: "quiz",
          quiz: {
            quiz: { chapterId: "chap1", questions: [{ id: "q1", prompt: 123 }] },
            answers: { q1: "test" },
          },
        },
        {
          courseId: "corrupt-grades",
          chapterId: "chap2",
          scrollTop: 0,
          view: "quiz",
          quiz: {
            quiz: {
              chapterId: "chap2",
              questions: [{ id: "q1", objective: "obj", prompt: "p", referenceAnswer: "r", combines: [] }],
            },
            grades: [{ questionId: "q1", result: "not-a-valid-outcome" }],
          },
        },
      ],
      active: "corrupt-quiz",
    };
    const state = decodeTabs(raw);
    expect(tabFor(state, "corrupt-quiz")).toEqual({
      courseId: "corrupt-quiz",
      chapterId: "chap1",
      scrollTop: 40,
      view: "chapter",
    });
    expect(tabFor(state, "corrupt-grades")).toEqual({
      courseId: "corrupt-grades",
      chapterId: "chap2",
      scrollTop: 0,
      view: "chapter",
    });
  });
});
