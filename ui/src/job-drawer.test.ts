// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LibraryCourse } from "./courses";
import type { JobFeed } from "./job-feed";
import { mountJobDrawer, type DrawerHandle } from "./job-drawer";
import { expectJobLabel, resetJobLabels } from "./job-label";
import { applyLine, applyStatus, type JobLine, type JobRecord } from "./jobs";

const T0 = Date.parse("2026-11-09T12:00:00Z");

function job(over: Partial<JobRecord> & { id: string }): JobRecord {
  return {
    courseId: "bio101",
    kind: "chapter",
    driver: "claude",
    model: "sonnet",
    effort: "low",
    command: "claude -p <prompt, 812 chars> --model sonnet --effort low",
    startedAt: "2026-11-09T12:00:00Z",
    endedAt: null,
    status: "running",
    lines: [{ at: "2026-11-09T12:00:00Z", kind: "status", text: "running" }],
    ...over,
  };
}

const line = (text: string, kind: JobLine["kind"] = "text"): JobLine => ({ at: "2026-11-09T12:00:05Z", kind, text });

/** The same surface as the real feed, driven by the test instead of by Rust events. */
function scriptedFeed(initial: JobRecord[]): JobFeed & {
  push(jobId: string, line: JobLine): void;
  finish(jobId: string, status: string, endedAt: string): void;
  add(record: JobRecord): void;
  remove(jobId: string): void;
  cancelled: string[];
} {
  let jobs = initial;
  const listeners = new Set<() => void>();
  const tell = () => listeners.forEach((l) => l());
  return {
    cancelled: [],
    jobs: () => jobs,
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    async start() {},
    stop() {},
    async cancel(id) {
      this.cancelled.push(id);
    },
    push(jobId, next) {
      jobs = applyLine(jobs, jobId, next);
      tell();
    },
    finish(jobId, status, endedAt) {
      jobs = applyStatus(jobs, jobId, status).map((j) => (j.id === jobId ? { ...j, endedAt } : j));
      tell();
    },
    add(record) {
      jobs = [...jobs, record];
      tell();
    },
    remove(jobId) {
      jobs = jobs.filter((j) => j.id !== jobId);
      tell();
    },
  };
}

const biology = {
  id: "cell-biology",
  storedId: "bio101",
  title: "Cell Biology",
} as LibraryCourse;

let root: HTMLElement;
let feed: ReturnType<typeof scriptedFeed>;
let drawer: DrawerHandle;
let now: number;
let closed: () => void;

function mount(jobs: JobRecord[]): void {
  feed = scriptedFeed(jobs);
  now = T0 + 12_000;
  closed = vi.fn();
  drawer = mountJobDrawer(root, feed, { courses: () => [biology], now: () => now, onClose: closed });
}

beforeEach(() => {
  resetJobLabels();
  document.body.innerHTML = '<div id="app"></div>';
  root = document.querySelector<HTMLElement>("#app")!;
});

afterEach(() => {
  vi.useRealTimers();
});

const panel = (): HTMLElement => root.querySelector<HTMLElement>("aside.job-drawer")!;
const rows = (): HTMLElement[] => Array.from(root.querySelectorAll<HTMLElement>("button.job-row"));
const lines = (): string[] => Array.from(root.querySelectorAll(".job-line")).map((l) => l.textContent!);

describe("closed", () => {
  it("is in the page but hidden until opened, and adds no controls to the reading area", () => {
    mount([job({ id: "job-1" })]);
    expect(panel().hidden).toBe(true);
    expect(panel().id).toBe("job-drawer");
    drawer.open();
    expect(panel().hidden).toBe(false);
    drawer.close();
    expect(panel().hidden).toBe(true);
    expect(drawer.isOpen()).toBe(false);
  });

  it("closes on Escape and tells the page", () => {
    mount([]);
    drawer.open();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(panel().hidden).toBe(true);
    expect(closed).toHaveBeenCalled();
  });
});

describe("the list", () => {
  it("says so when no job has run", () => {
    mount([]);
    drawer.open();
    expect(panel().textContent).toContain("No agent jobs yet");
  });

  it("shows the newest job first, with kind, course, agent, model, effort, status and duration", () => {
    mount([
      job({ id: "job-1", kind: "outline", status: "succeeded", endedAt: "2026-11-09T12:00:41Z", model: null, effort: null }),
      job({ id: "job-2", startedAt: "2026-11-09T12:00:02Z" }),
    ]);
    drawer.open();
    expect(rows().map((r) => r.dataset.jobId)).toEqual(["job-2", "job-1"]);
    const newest = rows()[0].textContent!;
    expect(newest).toContain("Chapter");
    expect(newest).toContain("Cell Biology");
    expect(newest).toContain("claude · sonnet · low");
    expect(newest).toContain("running");
    expect(newest).toContain("10s");
    const oldest = rows()[1].textContent!;
    expect(oldest).toContain("claude · default model");
    expect(oldest).toContain("succeeded");
    expect(oldest).toContain("41s");
  });

  it("names a course it does not know by the id the job carries", () => {
    mount([job({ id: "job-1", courseId: "someone-else" })]);
    drawer.open();
    expect(rows()[0].textContent).toContain("someone-else");
  });

  it("adds a row when a job starts while the drawer is open", () => {
    mount([job({ id: "job-1" })]);
    drawer.open();
    feed.add(job({ id: "job-2", kind: "quiz", startedAt: "2026-11-09T12:00:08Z" }));
    expect(rows()).toHaveLength(2);
    expect(rows()[0].textContent).toContain("Quiz");
  });
});

describe("labels", () => {
  const kindOf = (id: string): string => rows().find((row) => row.dataset.jobId === id)!.querySelector(".job-kind")!.textContent!;

  it("keeps the wire kind and shows Outline, Outline revise, and Section revise", () => {
    const planning = job({ id: "job-1", kind: "outline", courseId: "bio101", startedAt: "2026-11-09T12:00:01Z" });
    const revising = job({ id: "job-2", kind: "outline", courseId: "bio101", startedAt: "2026-11-09T12:00:02Z" });
    const section = job({ id: "job-3", kind: "rewrite", courseId: "bio101", startedAt: "2026-11-09T12:00:03Z" });
    const chapter = job({ id: "job-4", kind: "rewrite", courseId: "bio101", startedAt: "2026-11-09T12:00:04Z" });
    mount([]);
    expectJobLabel(feed, ["cell-biology", "bio101"], "outline", "Outline");
    feed.add(planning);
    expectJobLabel(feed, ["cell-biology", "bio101"], "outline", "Outline revise");
    feed.add(revising);
    expectJobLabel(feed, ["cell-biology", "bio101"], "rewrite", "Section revise");
    feed.add(section);
    feed.add(chapter);
    drawer.open();
    expect(kindOf("job-1")).toBe("Outline");
    expect(kindOf("job-2")).toBe("Outline revise");
    expect(kindOf("job-3")).toBe("Section revise");
    expect(kindOf("job-4")).toBe("Rewrite");
    expect([planning.kind, revising.kind, section.kind]).toEqual(["outline", "outline", "rewrite"]);
    rows().find((row) => row.dataset.jobId === "job-2")!.click();
    expect(root.querySelector("h2")!.textContent).toContain("Outline revise");
  });

  it("does not give one course's label to another course's job", () => {
    mount([]);
    expectJobLabel(feed, ["bio101"], "outline", "Outline revise");
    feed.add(job({ id: "job-1", kind: "outline", courseId: "other" }));
    drawer.open();
    expect(rows()[0].querySelector(".job-kind")!.textContent).toBe("Outline");
  });

  it("leaves a rewrite job that already finished labeled Rewrite when a section revise starts", () => {
    mount([job({ id: "job-1", kind: "rewrite", courseId: "bio101", startedAt: "2026-11-09T12:00:01Z", status: "succeeded", endedAt: "2026-11-09T12:00:20Z" })]);
    expectJobLabel(feed, ["bio101"], "rewrite", "Section revise");
    feed.add(job({ id: "job-2", kind: "rewrite", courseId: "bio101", startedAt: "2026-11-09T12:00:30Z" }));
    drawer.open();
    expect(kindOf("job-1")).toBe("Rewrite");
    expect(kindOf("job-2")).toBe("Section revise");
  });

  it("lets a later expectation replace one that no job took", () => {
    mount([]);
    expectJobLabel(feed, ["bio101"], "outline", "Outline revise");
    expectJobLabel(feed, ["bio101"], "outline", "Outline");
    feed.add(job({ id: "job-1", kind: "outline", courseId: "bio101" }));
    drawer.open();
    expect(kindOf("job-1")).toBe("Outline");
  });
});

describe("one job", () => {
  it("shows the real command line and the log lines, and goes back to the list", () => {
    mount([
      job({
        id: "job-1",
        lines: [line("running", "status"), line("Read sources/notes.md", "tool"), line("Drafting the chapter")],
      }),
    ]);
    drawer.open();
    rows()[0].click();
    expect(root.querySelector("pre.job-command")!.textContent).toBe(
      "claude -p <prompt, 812 chars> --model sonnet --effort low",
    );
    expect(Array.from(root.querySelectorAll(".job-arg")).map((a) => a.textContent)).toEqual([
      "claude",
      "-p <prompt, 812 chars>",
      "--model sonnet",
      "--effort low",
    ]);
    expect(lines()).toEqual(["running", "Read sources/notes.md", "Drafting the chapter"]);
    expect(root.querySelectorAll(".job-line")[1].getAttribute("data-kind")).toBe("tool");
    root.querySelector<HTMLButtonElement>("button.job-back")!.click();
    expect(rows()).toHaveLength(1);
    expect(root.querySelector("pre.job-command")).toBeNull();
  });

  it("opens on the list rather than the last viewed detail view", () => {
    mount([job({ id: "job-1" }), job({ id: "job-2" })]);
    drawer.open();
    rows()[0].click();
    expect(root.querySelector("pre.job-command")).not.toBeNull();
    drawer.close();
    drawer.open();
    expect(root.querySelector("pre.job-command")).toBeNull();
    expect(rows()).toHaveLength(2);
  });

  it("follows the job live: new lines appear, then the status and duration settle", () => {
    mount([job({ id: "job-1" })]);
    drawer.open();
    rows()[0].click();
    feed.push("job-1", line("Read sources/notes.md", "tool"));
    feed.push("job-1", line("Drafting the chapter"));
    expect(lines()).toEqual(["running", "Read sources/notes.md", "Drafting the chapter"]);
    expect(root.querySelector(".job-chip")!.textContent).toBe("running");

    feed.push("job-1", line("succeeded", "status"));
    feed.finish("job-1", "succeeded", "2026-11-09T12:00:37Z");
    expect(root.querySelector(".job-chip")!.textContent).toBe("succeeded");
    expect(root.querySelector(".job-duration")!.textContent).toBe("37s");
    expect(root.querySelector("button.job-stop")).toBeNull();
  });

  it("replaces the last line when the log was truncated, instead of growing past it", () => {
    mount([job({ id: "job-1", lines: [line("running", "status"), line("old tail")] })]);
    drawer.open();
    rows()[0].click();
    feed.push("job-1", line("truncated", "status"));
    expect(lines()).toEqual(["running", "truncated"]);
  });

  it("counts a running job's duration up while the drawer is open", () => {
    vi.useFakeTimers();
    mount([job({ id: "job-1" })]);
    drawer.open();
    rows()[0].click();
    expect(root.querySelector(".job-duration")!.textContent).toBe("12s");
    now += 3000;
    vi.advanceTimersByTime(1000);
    expect(root.querySelector(".job-duration")!.textContent).toBe("15s");
    drawer.close();
    now += 5000;
    vi.advanceTimersByTime(2000);
    drawer.open();
    expect(root.querySelector(".job-duration")!.textContent).toBe("20s");
  });

  it("keeps following the end of the log until the reader scrolls up", () => {
    mount([job({ id: "job-1" })]);
    drawer.open();
    rows()[0].click();
    const body = root.querySelector<HTMLElement>(".job-body")!;
    Object.defineProperty(body, "scrollHeight", {
      configurable: true,
      get: () => 900 + 100 * body.querySelectorAll(".job-line").length,
    });
    Object.defineProperty(body, "clientHeight", { configurable: true, value: 200 });
    body.scrollTop = 800;
    feed.push("job-1", line("one more"));
    expect(body.scrollTop).toBe(1100);

    body.scrollTop = 100;
    feed.push("job-1", line("and another"));
    expect(body.scrollTop).toBe(100);
  });

  it("stops a running job on request, and only offers it while the job runs", async () => {
    mount([job({ id: "job-1" })]);
    drawer.open();
    rows()[0].click();
    root.querySelector<HTMLButtonElement>("button.job-stop")!.click();
    await vi.waitFor(() => expect(feed.cancelled).toEqual(["job-1"]));
  });

  it("says why a stop failed", async () => {
    mount([job({ id: "job-1" })]);
    feed.cancel = async () => {
      throw new Error("job job-1 is not running");
    };
    drawer.open();
    rows()[0].click();
    root.querySelector<HTMLButtonElement>("button.job-stop")!.click();
    await vi.waitFor(() => expect(root.querySelector(".job-note")!.textContent).toContain("is not running"));
  });

  it("returns to the list when the job it shows has dropped out of the registry", () => {
    mount([job({ id: "job-1" })]);
    drawer.open();
    rows()[0].click();
    feed.remove("job-1");
    expect(root.querySelector("pre.job-command")).toBeNull();
    expect(panel().textContent).toContain("No agent jobs yet");
  });
});
