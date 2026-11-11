import type { JobFeed } from "./job-feed";
import type { JobKind, JobRecord } from "./jobs";

// Drawer words for a job. The wire kind stays outline, rewrite, and the rest.
// Planning, outline revise and section revise share kinds, so the screen that
// starts the job says which word to use. A job that nobody named keeps the
// kind's own word.

const KIND_WORDS: Record<JobKind, string> = {
  outline: "Outline",
  chapter: "Chapter",
  rewrite: "Rewrite",
  url: "URL",
  quiz: "Quiz",
  grade: "Grading",
  ask: "Ask",
};

interface Note {
  courseIds: readonly string[];
  kind: JobKind;
  label: string;
  /** Job ids already in the feed when the note was made. Those keep their word. */
  already: ReadonlySet<string>;
  stop(): void;
}

const pending: Note[] = [];
const bound = new Map<string, string>();

function sequence(id: string): number {
  return Number(/(\d+)$/.exec(id)?.[1] ?? 0);
}

/** Attach each pending note to the oldest job that started after the note. */
export function assignJobLabels(jobs: JobRecord[]): void {
  const ordered = [...jobs].sort(
    (a, b) => Date.parse(a.startedAt) - Date.parse(b.startedAt) || sequence(a.id) - sequence(b.id),
  );
  for (const job of ordered) {
    if (bound.has(job.id)) continue;
    const index = pending.findIndex(
      (note) => note.kind === job.kind && note.courseIds.includes(job.courseId) && !note.already.has(job.id),
    );
    if (index < 0) continue;
    const note = pending[index];
    bound.set(job.id, note.label);
    pending.splice(index, 1);
  }
}

/** The drawer's word for one job. */
export function jobLabel(job: JobRecord): string {
  return bound.get(job.id) ?? KIND_WORDS[job.kind];
}

export interface LabelClaim {
  /** Drop the note when the call failed and no new job of that kind took it. */
  release(jobs: JobRecord[]): void;
}

/**
 * The next new job of `kind` for one of `courseIds` shows `label`.
 * A second note for the same kind and courses replaces one no job has taken.
 */
export function expectJobLabel(
  feed: JobFeed | null,
  courseIds: readonly string[],
  kind: JobKind,
  label: string,
): LabelClaim {
  const ids = [...new Set(courseIds.filter((id) => id !== ""))];
  const already = new Set((feed?.jobs() ?? []).map((job) => job.id));
  let off: (() => void) | null = null;
  const note: Note = {
    courseIds: ids,
    kind,
    label,
    already,
    stop() {
      off?.();
      off = null;
      const index = pending.indexOf(note);
      if (index >= 0) pending.splice(index, 1);
    },
  };
  for (const old of [...pending]) {
    if (old.kind === kind && old.courseIds.some((id) => ids.includes(id))) old.stop();
  }
  pending.push(note);
  const watch = (): void => {
    if (!feed) return;
    assignJobLabels(feed.jobs());
    if (!pending.includes(note)) note.stop();
  };
  if (feed) {
    off = feed.subscribe(watch);
    watch();
  }
  return {
    release(jobs) {
      assignJobLabels(jobs);
      note.stop();
    },
  };
}

/** Test isolation. Drops notes and labels from earlier cases. */
export function resetJobLabels(): void {
  for (const note of [...pending]) note.stop();
  pending.length = 0;
  bound.clear();
}
