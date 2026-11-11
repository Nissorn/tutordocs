import { list, oneOf, record, str, strOrNull } from "./api";
import type { LibraryCourse } from "./courses";

// Agent jobs as src-tauri/src/jobs.rs reports them, and the pure functions the
// drawer and the tab bar need over a list of them.

export type JobStatus = "running" | "succeeded" | "failed" | "cancelled";
export type LineKind = "text" | "tool" | "error" | "status";
export type JobKind = "outline" | "chapter" | "rewrite" | "url" | "quiz" | "grade" | "ask";

export interface JobLine {
  at: string;
  kind: LineKind;
  text: string;
}

export interface JobRecord {
  id: string;
  /** The id inside course.json, not the folder name. */
  courseId: string;
  kind: JobKind;
  driver: string;
  model: string | null;
  effort: string | null;
  /** The binary and argv as spawned, with the prompt replaced by its length. */
  command: string;
  startedAt: string;
  endedAt: string | null;
  status: JobStatus;
  lines: JobLine[];
}

const STATUSES: readonly JobStatus[] = ["running", "succeeded", "failed", "cancelled"];
const LINE_KINDS: readonly LineKind[] = ["text", "tool", "error", "status"];
const JOB_KINDS: readonly JobKind[] = ["outline", "chapter", "rewrite", "url", "quiz", "grade", "ask"];

export function parseLine(raw: unknown): JobLine {
  const r = record(raw, "job line");
  return { at: str(r, "at"), kind: oneOf(r, "kind", LINE_KINDS), text: str(r, "text") };
}

export function parseJob(raw: unknown): JobRecord {
  const r = record(raw, "job");
  return {
    id: str(r, "id"),
    courseId: str(r, "courseId"),
    kind: oneOf(r, "kind", JOB_KINDS),
    driver: str(r, "driver"),
    model: strOrNull(r, "model"),
    effort: strOrNull(r, "effort"),
    command: str(r, "command"),
    startedAt: str(r, "startedAt"),
    endedAt: strOrNull(r, "endedAt"),
    status: oneOf(r, "status", STATUSES),
    lines: list(r.lines, "job lines").map(parseLine),
  };
}

/** Appends a live line to its job. `truncated` replaces the last line, as Rust documents. */
export function applyLine(jobs: JobRecord[], jobId: string, line: JobLine): JobRecord[] {
  if (!jobs.some((j) => j.id === jobId)) return jobs;
  const replacing = line.kind === "status" && line.text === "truncated";
  return jobs.map((j) =>
    j.id !== jobId ? j : { ...j, lines: replacing ? [...j.lines.slice(0, -1), line] : [...j.lines, line] },
  );
}

export function applyStatus(jobs: JobRecord[], jobId: string, status: string): JobRecord[] {
  if (!(STATUSES as readonly string[]).includes(status) || !jobs.some((j) => j.id === jobId)) return jobs;
  return jobs.map((j) => (j.id === jobId ? { ...j, status: status as JobStatus } : j));
}

function sequence(id: string): number {
  return Number(/(\d+)$/.exec(id)?.[1] ?? 0);
}

/** Newest first, by start time and then by the registry's own counter. */
export function newestFirst(jobs: JobRecord[]): JobRecord[] {
  return [...jobs].sort(
    (a, b) => Date.parse(b.startedAt) - Date.parse(a.startedAt) || sequence(b.id) - sequence(a.id),
  );
}

/** "12s", "1m 05s", "2h 03m". A running job counts up to `nowMs`. */
export function describeDuration(job: JobRecord, nowMs: number): string {
  const end = job.endedAt === null ? nowMs : Date.parse(job.endedAt);
  const seconds = Math.max(0, Math.floor((end - Date.parse(job.startedAt)) / 1000));
  const pad = (n: number) => String(n).padStart(2, "0");
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${pad(seconds % 60)}s`;
  return `${Math.floor(seconds / 3600)}h ${pad(Math.floor((seconds % 3600) / 60))}m`;
}

/** Folder ids of the courses that have a running job. A job names its course by the id in course.json. */
export function busyCourses(jobs: JobRecord[], courses: LibraryCourse[]): Set<string> {
  const running = new Set(jobs.filter((j) => j.status === "running").map((j) => j.courseId));
  return new Set(courses.filter((c) => running.has(c.id) || running.has(c.storedId)).map((c) => c.id));
}
