import { number, record, str, strOrNull, strings } from "./api";

// The home page's data: one summary per course folder on the shelf, and the
// pure functions over a list of them (search, tag filter, order, wording).
// Nothing here touches the DOM or the backend.

export type OutlineStatus = "draft" | "approved";

export interface LibraryCourse {
  /** The folder name under courses/. Every course command takes this. */
  id: string;
  /** The id inside course.json. Job records carry this one as `courseId`. */
  storedId: string;
  title: string;
  tags: string[];
  objectives: string[];
  chapterCount: number;
  chapterTitles: string[];
  writtenChapterCount: number;
  /** Unix seconds, or 0 when unknown. */
  createdAt: number;
  /** Unix seconds, or null when the course was never opened. */
  openedAt: number | null;
  /** Why this course could not be read. It still opens. */
  error: string | null;
  /** Whether the outline is a draft or approved. */
  outlineStatus: OutlineStatus;
  /** Whether an outline job for this course is running. */
  planning: boolean;
}

export interface TrashEntry {
  trashId: string;
  courseId: string;
  title: string;
  trashedAt: string;
  chapterCount: number;
  writtenChapterCount: number;
  bytes: number;
}

/** Checks one summary as it crosses from Rust. */
export function parseCourse(raw: unknown): LibraryCourse {
  const r = record(raw, "course summary");
  const opened = r.openedAt;
  if (opened !== null && typeof opened !== "number") throw new Error(`expected "openedAt" to be a number or null`);
  return {
    id: str(r, "id"),
    storedId: str(r, "storedId"),
    title: str(r, "title"),
    tags: strings(r, "tags"),
    objectives: strings(r, "objectives"),
    chapterCount: number(r, "chapterCount"),
    chapterTitles: strings(r, "chapterTitles"),
    writtenChapterCount: number(r, "writtenChapterCount"),
    createdAt: number(r, "createdAt"),
    openedAt: opened,
    error: strOrNull(r, "error"),
    outlineStatus: r.outlineStatus === "draft" ? "draft" : "approved",
    planning: typeof r.planning === "boolean" ? r.planning : false,
  };
}

/** Checks one trash entry as it crosses from Rust. */
export function parseTrashEntry(raw: unknown): TrashEntry {
  const r = record(raw, "trash entry");
  return {
    trashId: str(r, "trashId"),
    courseId: str(r, "courseId"),
    title: str(r, "title"),
    trashedAt: str(r, "trashedAt"),
    chapterCount: number(r, "chapterCount"),
    writtenChapterCount: number(r, "writtenChapterCount"),
    bytes: number(r, "bytes"),
  };
}

export interface HomeFilter {
  query: string;
  /** Only courses carrying this tag, or null for all. */
  tag: string | null;
}

export const defaultFilter: HomeFilter = { query: "", tag: null };

export function encodeFilter(filter: HomeFilter): unknown {
  return {
    query: filter.query,
    tag: filter.tag,
  };
}

export function decodeFilter(raw: unknown): HomeFilter {
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) {
    return defaultFilter;
  }
  const f = raw as Record<string, unknown>;
  return {
    query: typeof f.query === "string" ? f.query : "",
    tag: typeof f.tag === "string" ? f.tag : null,
  };
}

const lower = (text: string): string => text.toLocaleLowerCase();

/**
 * Keeps the courses that carry the tag and whose title, tags, objectives or
 * chapter titles contain every word of the query, ignoring case.
 */
export function filterCourses(courses: LibraryCourse[], { query, tag }: HomeFilter): LibraryCourse[] {
  const words = lower(query).split(/\s+/).filter(Boolean);
  const wanted = tag === null ? null : lower(tag);
  return courses.filter((course) => {
    if (wanted !== null && !course.tags.some((t) => lower(t) === wanted)) return false;
    const haystack = lower([course.title, ...course.tags, ...course.objectives, ...course.chapterTitles].join("\n"));
    return words.every((word) => haystack.includes(word));
  });
}

/** Every tag in use once, in the spelling first seen, sorted ignoring case. */
export function allTags(courses: LibraryCourse[]): string[] {
  const seen = new Map<string, string>();
  for (const course of courses) for (const tag of course.tags) if (!seen.has(lower(tag))) seen.set(lower(tag), tag);
  return [...seen.values()].sort((a, b) => lower(a).localeCompare(lower(b)));
}

/** The text of a card's tag field as a tag list. Rust trims, dedupes and bounds each tag. */
export function parseTagInput(text: string): string[] {
  return text
    .split(",")
    .map((tag) => tag.trim())
    .filter(Boolean);
}

/** Most recently opened first, then courses never opened, newest created first. */
export function homeOrder(courses: LibraryCourse[]): LibraryCourse[] {
  return [...courses].sort((a, b) => {
    if (a.openedAt !== null || b.openedAt !== null) return (b.openedAt ?? -1) - (a.openedAt ?? -1);
    return b.createdAt - a.createdAt;
  });
}

const MINUTE = 60;
const HOUR = 3600;
const DAY = 86_400;

function plural(n: number, unit: string): string {
  return `${n} ${unit}${n === 1 ? "" : "s"}`;
}

/** "Opened 3 days ago", from unix seconds. */
export function describeOpened(openedAt: number | null, nowSeconds: number): string {
  if (openedAt === null) return "Not opened yet";
  const age = nowSeconds - openedAt;
  if (age < MINUTE) return "Opened just now";
  if (age < HOUR) return `Opened ${plural(Math.floor(age / MINUTE), "minute")} ago`;
  if (age < DAY) return `Opened ${plural(Math.floor(age / HOUR), "hour")} ago`;
  if (age < 2 * DAY) return "Opened yesterday";
  if (age < 30 * DAY) return `Opened ${plural(Math.floor(age / DAY), "day")} ago`;
  return `Opened ${new Date(openedAt * 1000).toISOString().slice(0, 10)}`;
}
