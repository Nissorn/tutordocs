import { describe, expect, it } from "vitest";
import {
  applyLine,
  applyStatus,
  busyCourses,
  describeDuration,
  newestFirst,
  parseJob,
  type JobLine,
  type JobRecord,
} from "./jobs";
import type { LibraryCourse } from "./courses";

function job(over: Partial<JobRecord> & { id: string }): JobRecord {
  return {
    courseId: "bio101",
    kind: "chapter",
    driver: "claude",
    model: null,
    effort: null,
    command: "claude -p <prompt, 10 chars>",
    startedAt: "2026-11-09T12:00:00Z",
    endedAt: null,
    status: "running",
    lines: [{ at: "2026-11-09T12:00:00Z", kind: "status", text: "running" }],
    ...over,
  };
}

const line = (text: string, kind: JobLine["kind"] = "text"): JobLine => ({ at: "2026-11-09T12:00:05Z", kind, text });

describe("parsing a job record", () => {
  const wire = {
    id: "job-1",
    courseId: "lib",
    kind: "outline",
    driver: "claude",
    model: "sonnet",
    effort: null,
    command: "claude -p <prompt, 4213 chars> --model sonnet",
    startedAt: "2026-11-09T12:00:00Z",
    endedAt: null,
    status: "running",
    lines: [{ at: "2026-11-09T12:00:00Z", kind: "status", text: "running" }],
  };

  it("reads the shape ticket 25 documents", () => {
    expect(parseJob(wire)).toEqual(wire);
    expect(parseJob({ ...wire, kind: "ask" }).kind).toBe("ask");
  });

  it("refuses a status or line kind it does not know", () => {
    expect(() => parseJob({ ...wire, status: "paused" })).toThrow(/status/);
    expect(() => parseJob({ ...wire, lines: [{ at: "x", kind: "poem", text: "y" }] })).toThrow(/kind/);
  });
});

describe("live lines", () => {
  it("appends a line to its job and leaves the others alone", () => {
    const a = job({ id: "a" });
    const b = job({ id: "b" });
    const next = applyLine([a, b], "a", line("Drafting"));
    expect(next[0].lines.map((l) => l.text)).toEqual(["running", "Drafting"]);
    expect(next[1]).toBe(b);
  });

  it("replaces the last line when Rust says the log was truncated", () => {
    const full = job({ id: "a", lines: [line("one"), line("two")] });
    const next = applyLine([full], "a", line("truncated", "status"));
    expect(next[0].lines.map((l) => l.text)).toEqual(["one", "truncated"]);
  });

  it("does not invent a job for a line it has no record of", () => {
    const jobs = [job({ id: "a" })];
    expect(applyLine(jobs, "ghost", line("x"))).toBe(jobs);
  });
});

describe("status changes", () => {
  it("moves a running job to its end state", () => {
    const next = applyStatus([job({ id: "a" })], "a", "succeeded");
    expect(next[0].status).toBe("succeeded");
  });

  it("ignores a status word it does not know", () => {
    const jobs = [job({ id: "a" })];
    expect(applyStatus(jobs, "a", "paused")).toBe(jobs);
  });
});

describe("order", () => {
  it("lists the newest job first without reordering the caller's array", () => {
    const jobs = [job({ id: "job-1" }), job({ id: "job-2" }), job({ id: "job-3" })];
    expect(newestFirst(jobs).map((j) => j.id)).toEqual(["job-3", "job-2", "job-1"]);
    expect(jobs[0].id).toBe("job-1");
  });
});

describe("duration", () => {
  const at = (iso: string) => Date.parse(iso);
  it("counts from start to end for a finished job", () => {
    const done = job({ id: "a", status: "succeeded", endedAt: "2026-11-09T12:01:05Z" });
    expect(describeDuration(done, at("2026-11-09T13:00:00Z"))).toBe("1m 05s");
  });
  it("counts from start to now for a running job", () => {
    expect(describeDuration(job({ id: "a" }), at("2026-11-09T12:00:12Z"))).toBe("12s");
  });
  it("shows hours past an hour and never goes negative", () => {
    const long = job({ id: "a", status: "failed", endedAt: "2026-11-09T14:03:00Z" });
    expect(describeDuration(long, 0)).toBe("2h 03m");
    expect(describeDuration(job({ id: "a" }), at("2026-11-09T11:59:00Z"))).toBe("0s");
  });
});

describe("which courses are busy", () => {
  const course = (id: string, storedId: string): LibraryCourse => ({
    id,
    storedId,
    title: id,
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
  });

  it("matches a running job to the course by folder id or by the id stored in course.json", () => {
    const courses = [course("biology", "bio101"), course("history", "history")];
    const jobs = [job({ id: "a", courseId: "bio101" }), job({ id: "b", courseId: "history", status: "succeeded" })];
    expect([...busyCourses(jobs, courses)]).toEqual(["biology"]);
    expect([...busyCourses([job({ id: "c", courseId: "history" })], courses)]).toEqual(["history"]);
  });

  it("is empty when nothing runs", () => {
    expect(busyCourses([job({ id: "a", status: "failed" })], [course("biology", "bio101")]).size).toBe(0);
  });
});
