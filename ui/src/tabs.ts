// The set of open tabs: what the user did, and nothing derived from jobs. Home
// is always there and is not in the list, so `active: null` is Home. A tab is
// keyed by the course's folder id, the same id every course command takes.

import { parseGraded, parseQuiz, type Graded, type Quiz, type QuizTabState } from "./quiz";

export type { QuizTabState } from "./quiz";

export interface PdfTabState {
  sourceName: string;
  page: number;
  zoom: number;
}

export type ReaderViewState =
  | { view: "chapter" }
  | { view: "review" }
  | { view: "quiz"; quiz: QuizTabState }
  | { view: "pdf"; pdf: PdfTabState };

export type ReaderView = ReaderViewState["view"];

export type TabPosition = {
  /** The chapter on screen, or null when the tab has not shown one yet. */
  chapterId: string | null;
  scrollTop: number;
} & ReaderViewState;

export type CourseTab = TabPosition & {
  courseId: string;
};

export interface TabsState {
  /** Left to right as the tab bar shows them. */
  open: CourseTab[];
  /** The course id of the active tab, always one of `open`, or null for Home. */
  active: string | null;
}

export const emptyTabs: TabsState = { open: [], active: null };

export function tabFor(state: TabsState, courseId: string): CourseTab | undefined {
  return state.open.find((tab) => tab.courseId === courseId);
}

/** Adds the course as the last tab, or focuses its tab when it is already open. */
export function openCourse(state: TabsState, courseId: string): TabsState {
  if (tabFor(state, courseId)) return { ...state, active: courseId };
  return {
    open: [...state.open, { courseId, chapterId: null, scrollTop: 0, view: "chapter" }],
    active: courseId,
  };
}

/** Shows Home (null) or an open course. A course with no tab changes nothing. */
export function activate(state: TabsState, courseId: string | null): TabsState {
  if (courseId !== null && !tabFor(state, courseId)) return state;
  return state.active === courseId ? state : { ...state, active: courseId };
}

/** Closing the active tab shows the tab that slid into its place, else the one before it, else Home. */
export function closeTab(state: TabsState, courseId: string): TabsState {
  const at = state.open.findIndex((tab) => tab.courseId === courseId);
  if (at < 0) return state;
  const open = state.open.filter((tab) => tab.courseId !== courseId);
  if (state.active !== courseId) return { ...state, open };
  return { ...state, open, active: (open[at] ?? open[at - 1])?.courseId ?? null };
}

function encodePosition(pos: TabPosition): string {
  const out: Record<string, unknown> = {
    chapterId: pos.chapterId,
    scrollTop: pos.scrollTop,
    view: pos.view,
  };
  if (pos.view === "quiz") out.quiz = pos.quiz;
  if (pos.view === "pdf") out.pdf = pos.pdf;
  return JSON.stringify(out);
}

/** Keeps a tab's chapter, view mode, quiz/pdf anchors, and scroll. Returns the same state when nothing changed. */
export function remember(state: TabsState, courseId: string, position: TabPosition): TabsState {
  const tab = tabFor(state, courseId);
  if (!tab || encodePosition(tab) === encodePosition(position)) return state;
  return {
    ...state,
    open: state.open.map((t) => (t.courseId === courseId ? { courseId: t.courseId, ...position } : t)),
  };
}

/** Drops the tabs of courses that are gone. Home takes over when the active one was dropped. */
export function dropMissing(state: TabsState, existing: Iterable<string>): TabsState {
  const keep = new Set(existing);
  const open = state.open.filter((tab) => keep.has(tab.courseId));
  if (open.length === state.open.length) return state;
  return {
    open,
    active: state.active !== null && keep.has(state.active) ? state.active : null,
  };
}

export function encodeTabs(state: TabsState): unknown {
  return {
    open: state.open.map((tab) => {
      const out: Record<string, unknown> = {
        courseId: tab.courseId,
        chapterId: tab.chapterId,
        scrollTop: tab.scrollTop,
        view: tab.view,
      };
      if (tab.view === "quiz") out.quiz = tab.quiz;
      if (tab.view === "pdf") out.pdf = tab.pdf;
      return out;
    }),
    active: state.active,
  };
}

/** Reads what `encodeTabs` wrote, and treats anything else as a fresh start. */
export function decodeTabs(raw: unknown): TabsState {
  if (typeof raw !== "object" || raw === null) return emptyTabs;
  const { open, active } = raw as { open?: unknown; active?: unknown };
  if (!Array.isArray(open)) return emptyTabs;
  const tabs: CourseTab[] = [];
  for (const item of open) {
    if (typeof item !== "object" || item === null) continue;
    const { courseId, chapterId, scrollTop, view, quiz, pdf } = item as Record<string, unknown>;
    if (typeof courseId !== "string" || courseId === "" || tabs.some((t) => t.courseId === courseId)) continue;

    let parsedQuizTab: QuizTabState | undefined;
    if (typeof quiz === "object" && quiz !== null && !Array.isArray(quiz)) {
      try {
        const q = quiz as Record<string, unknown>;
        const parsedQuiz = q.quiz !== undefined ? parseQuiz(q.quiz) : undefined;
        const parsedGrades = q.grades !== undefined ? parseGraded(q.grades) : undefined;
        const answers: Record<string, string> = {};
        if (typeof q.answers === "object" && q.answers !== null && !Array.isArray(q.answers)) {
          for (const [k, v] of Object.entries(q.answers)) {
            if (typeof v === "string") answers[k] = v;
            else throw new Error("malformed answer");
          }
        }
        parsedQuizTab = {
          ...(typeof q.chapterId === "string" ? { chapterId: q.chapterId } : {}),
          ...(parsedQuiz ? { quiz: parsedQuiz } : {}),
          answers,
          ...(parsedGrades ? { grades: parsedGrades } : {}),
        };
      } catch {
        parsedQuizTab = undefined;
      }
    }

    let parsedPdfTab: PdfTabState | undefined;
    if (typeof pdf === "object" && pdf !== null && !Array.isArray(pdf)) {
      const p = pdf as Record<string, unknown>;
      if (typeof p.sourceName === "string" && p.sourceName !== "") {
        const page = typeof p.page === "number" && Number.isFinite(p.page) && p.page >= 1 ? Math.floor(p.page) : 1;
        const zoom = typeof p.zoom === "number" && Number.isFinite(p.zoom) && p.zoom > 0 ? p.zoom : 1;
        parsedPdfTab = {
          sourceName: p.sourceName,
          page,
          zoom,
        };
      }
    }

    let viewState: ReaderViewState;
    if (view === "quiz" && parsedQuizTab) {
      viewState = { view: "quiz", quiz: parsedQuizTab };
    } else if (view === "pdf" && parsedPdfTab) {
      viewState = { view: "pdf", pdf: parsedPdfTab };
    } else if (view === "review") {
      viewState = { view: "review" };
    } else {
      viewState = { view: "chapter" };
    }

    tabs.push({
      courseId,
      chapterId: typeof chapterId === "string" ? chapterId : null,
      scrollTop: typeof scrollTop === "number" && Number.isFinite(scrollTop) && scrollTop > 0 ? scrollTop : 0,
      ...viewState,
    });
  }
  return {
    open: tabs,
    active: typeof active === "string" && tabs.some((t) => t.courseId === active) ? active : null,
  };
}
