// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { createJobFeed } from "./job-feed";

const started = "2026-11-09T12:00:00Z";

function wire(id: string, over: Record<string, unknown> = {}) {
  return {
    id,
    courseId: "bio101",
    kind: "chapter",
    driver: "claude",
    model: "sonnet",
    effort: null,
    command: "claude -p <prompt, 10 chars> --model sonnet",
    startedAt: started,
    endedAt: null,
    status: "running",
    lines: [{ at: started, kind: "status", text: "running" }],
    ...over,
  };
}

/** The registry as Rust holds it. A test changes it, then emits the event Rust would. */
let registry: Record<string, ReturnType<typeof wire>>;
let calls: string[];

beforeEach(() => {
  registry = {};
  calls = [];
  mockIPC(
    (cmd, raw) => {
      const args = (raw ?? {}) as Record<string, unknown>;
      calls.push(cmd);
      if (cmd === "list_agent_jobs") return Object.values(registry);
      if (cmd === "agent_job") return registry[String(args.id)];
      if (cmd === "cancel_agent_job") return null;
      throw new Error(`unexpected command ${cmd}`);
    },
    { shouldMockEvents: true },
  );
});

afterEach(() => {
  clearMocks();
});

describe("the live job list", () => {
  it("starts from the registry Rust already holds", async () => {
    registry["job-1"] = wire("job-1", { status: "succeeded", endedAt: "2026-11-09T12:00:30Z" });
    const feed = createJobFeed();
    await feed.start();
    expect(feed.jobs().map((j) => j.id)).toEqual(["job-1"]);
    feed.stop();
  });

  it("appends a line event to its job and tells subscribers", async () => {
    registry["job-1"] = wire("job-1");
    const feed = createJobFeed();
    await feed.start();
    const seen = vi.fn();
    feed.subscribe(seen);
    await emit("agent-job-line", { jobId: "job-1", line: { at: started, kind: "text", text: "Drafting the chapter" } });
    expect(feed.jobs()[0].lines.map((l) => l.text)).toEqual(["running", "Drafting the chapter"]);
    expect(seen).toHaveBeenCalled();
    feed.stop();
  });

  it("learns about a job that started after launch from the first event it sees", async () => {
    const feed = createJobFeed();
    await feed.start();
    registry["job-7"] = wire("job-7");
    await emit("agent-job-status", { jobId: "job-7", status: "running" });
    await vi.waitFor(() => expect(feed.jobs().map((j) => j.id)).toEqual(["job-7"]));
    feed.stop();
  });

  it("re-reads a job when it ends, to get its end time", async () => {
    registry["job-1"] = wire("job-1");
    const feed = createJobFeed();
    await feed.start();
    registry["job-1"] = wire("job-1", {
      status: "succeeded",
      endedAt: "2026-11-09T12:00:42Z",
      lines: [
        { at: started, kind: "status", text: "running" },
        { at: "2026-11-09T12:00:42Z", kind: "status", text: "succeeded" },
      ],
    });
    await emit("agent-job-status", { jobId: "job-1", status: "succeeded" });
    await vi.waitFor(() => expect(feed.jobs()[0].endedAt).toBe("2026-11-09T12:00:42Z"));
    expect(feed.jobs()[0].status).toBe("succeeded");
    feed.stop();
  });

  it("keeps every line when events arrive while the job is being read", async () => {
    const feed = createJobFeed();
    await feed.start();
    const running = { at: started, kind: "status", text: "running" };
    const said = (text: string) => ({ at: started, kind: "text", text });
    registry["job-3"] = wire("job-3", { lines: [running, said("first")] });
    await emit("agent-job-line", { jobId: "job-3", line: said("first") });
    registry["job-3"] = wire("job-3", { lines: [running, said("first"), said("second")] });
    await emit("agent-job-line", { jobId: "job-3", line: said("second") });
    await vi.waitFor(() => expect(feed.jobs()[0]?.lines.map((l) => l.text)).toEqual(["running", "first", "second"]));
    feed.stop();
  });

  it("ignores an event with a shape it does not know", async () => {
    registry["job-1"] = wire("job-1");
    const feed = createJobFeed();
    await feed.start();
    await emit("agent-job-line", { nope: true });
    await emit("agent-job-status", { jobId: "job-1", status: 5 });
    expect(feed.jobs()[0].lines).toHaveLength(1);
    feed.stop();
  });

  it("stops listening after stop", async () => {
    registry["job-1"] = wire("job-1");
    const feed = createJobFeed();
    await feed.start();
    feed.stop();
    await emit("agent-job-line", { jobId: "job-1", line: { at: started, kind: "text", text: "late" } });
    expect(feed.jobs()[0].lines).toHaveLength(1);
  });

  it("asks Rust to cancel a job", async () => {
    const feed = createJobFeed();
    await feed.cancel("job-1");
    expect(calls).toContain("cancel_agent_job");
  });
});
