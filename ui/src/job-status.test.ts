// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";
import type { JobFeed } from "./job-feed";
import type { JobRecord } from "./jobs";
import { mountJobStatus } from "./job-status";

function job(over: Partial<JobRecord> & { id: string }): JobRecord {
  return {
    courseId: "cb-1",
    kind: "outline",
    driver: "claude",
    model: null,
    effort: null,
    command: "claude -p",
    startedAt: "2026-11-09T10:00:00Z",
    endedAt: null,
    status: "running",
    lines: [],
    ...over,
  };
}

/** A feed the test moves by hand. Only the two methods the panel reads. */
function fakeFeed(initial: JobRecord[]) {
  let jobs = initial;
  const listeners = new Set<() => void>();
  const feed = {
    jobs: () => jobs,
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  } as unknown as JobFeed;
  return {
    feed,
    set(next: JobRecord[]) {
      jobs = next;
      for (const l of [...listeners]) l();
    },
    listeners: () => listeners.size,
  };
}

const NOW = Date.parse("2026-11-09T10:00:42Z");

function mount(feed: JobFeed | null) {
  const host = document.createElement("div");
  const handle = mountJobStatus(host, feed, { label: "Planning the course", kind: "outline", courseIds: ["cell-biology", "cb-1"], now: () => NOW });
  return { host, handle };
}

describe("the running job on screen", () => {
  it("says the agent is starting until the job shows up", () => {
    const { host } = mount(fakeFeed([]).feed);
    expect(host.textContent).toContain("Planning the course");
    expect(host.textContent).toContain("Starting the agent");
  });

  it("shows how long the matching job has run and its latest lines", () => {
    const { feed } = fakeFeed([
      job({
        id: "job-1",
        lines: [
          { at: "2026-11-09T10:00:01Z", kind: "status", text: "running" },
          { at: "2026-11-09T10:00:05Z", kind: "tool", text: "Read sources/membranes.pdf" },
          { at: "2026-11-09T10:00:30Z", kind: "text", text: "Planning six chapters." },
        ],
      }),
    ]);
    const { host } = mount(feed);
    expect(host.querySelector(".job-status-time")!.textContent).toBe("42s");
    expect(Array.from(host.querySelectorAll(".job-status-lines li"), (li) => li.textContent)).toEqual([
      "Read sources/membranes.pdf",
      "Planning six chapters.",
    ]);
  });

  it("follows the feed as lines arrive", () => {
    const fake = fakeFeed([job({ id: "job-1" })]);
    const { host } = mount(fake.feed);
    fake.set([job({ id: "job-1", lines: [{ at: "2026-11-09T10:00:10Z", kind: "text", text: "Drafting the outline." }] })]);
    expect(host.querySelector(".job-status-lines li")!.textContent).toBe("Drafting the outline.");
  });

  it("ignores another course's job, another kind, and a job that has ended", () => {
    const { host } = mount(
      fakeFeed([
        job({ id: "job-1", courseId: "other", lines: [{ at: "x", kind: "text", text: "not mine" }] }),
        job({ id: "job-2", kind: "chapter", lines: [{ at: "x", kind: "text", text: "chapter text" }] }),
        job({ id: "job-3", status: "succeeded", lines: [{ at: "x", kind: "text", text: "done long ago" }] }),
      ]).feed,
    );
    expect(host.querySelectorAll(".job-status-lines li")).toHaveLength(0);
    expect(host.textContent).toContain("Starting the agent");
  });

  it("writes job text as text, never as markup", () => {
    const { host } = mount(fakeFeed([job({ id: "job-1", lines: [{ at: "x", kind: "text", text: "<img src=x onerror=alert(1)>" }] })]).feed);
    expect(host.querySelector("img")).toBeNull();
    expect(host.querySelector(".job-status-lines li")!.textContent).toBe("<img src=x onerror=alert(1)>");
  });

  it("stops listening when stopped", () => {
    const fake = fakeFeed([]);
    const { handle } = mount(fake.feed);
    expect(fake.listeners()).toBe(1);
    handle.stop();
    expect(fake.listeners()).toBe(0);
  });

  it("works with no feed at all", () => {
    const { host } = mount(null);
    expect(host.textContent).toContain("Planning the course");
  });

  it("is announced politely to a screen reader", () => {
    const { host } = mount(null);
    expect(host.querySelector('[role="status"]')).not.toBeNull();
  });
});
