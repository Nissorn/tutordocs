import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { list, record, str } from "./api";
import { applyLine, applyStatus, parseJob, parseLine, type JobRecord } from "./jobs";

// The app's live copy of the job registry in Rust. It starts from
// `list_agent_jobs`, then follows `agent-job-line` and `agent-job-status`.
//
// A job it has no record of, and a job that just ended (the status event has no
// end time), are re-read with `agent_job`. Events that land while that read is
// in flight cannot be told apart from lines the snapshot already holds, so they
// only mark the read stale and it runs again.

export interface JobFeed {
  jobs(): JobRecord[];
  /** Calls `listener` after every change. Returns the way to stop. */
  subscribe(listener: () => void): () => void;
  /** Subscribes to the events, then reads the registry. Resolves once both are done. */
  start(): Promise<void>;
  stop(): void;
  cancel(jobId: string): Promise<void>;
}

export function createJobFeed(): JobFeed {
  let jobs: JobRecord[] = [];
  let stopped = false;
  const listeners = new Set<() => void>();
  const unlisten: UnlistenFn[] = [];
  /** Job ids being re-read, each with whether an event arrived meanwhile. */
  const reading = new Map<string, boolean>();

  function set(next: JobRecord[]): void {
    if (next === jobs) return;
    jobs = next;
    for (const listener of [...listeners]) listener();
  }

  async function reread(id: string): Promise<void> {
    if (reading.has(id)) {
      reading.set(id, true);
      return;
    }
    try {
      do {
        reading.set(id, false);
        const fresh = parseJob(await invoke<unknown>("agent_job", { id }));
        if (stopped) return;
        set(jobs.some((j) => j.id === id) ? jobs.map((j) => (j.id === id ? fresh : j)) : [...jobs, fresh]);
      } while (reading.get(id));
    } catch (error) {
      console.warn(`could not read ${id}: ${error instanceof Error ? error.message : String(error)}`);
    } finally {
      reading.delete(id);
    }
  }

  function onLine(payload: unknown): void {
    let jobId: string;
    let line;
    try {
      const r = record(payload, "agent-job-line");
      jobId = str(r, "jobId");
      line = parseLine(r.line);
    } catch (error) {
      console.warn(`ignored an agent-job-line event: ${error instanceof Error ? error.message : String(error)}`);
      return;
    }
    if (reading.has(jobId) || !jobs.some((j) => j.id === jobId)) void reread(jobId);
    else set(applyLine(jobs, jobId, line));
  }

  function onStatus(payload: unknown): void {
    let jobId: string;
    let status: string;
    try {
      const r = record(payload, "agent-job-status");
      jobId = str(r, "jobId");
      status = str(r, "status");
    } catch (error) {
      console.warn(`ignored an agent-job-status event: ${error instanceof Error ? error.message : String(error)}`);
      return;
    }
    const known = jobs.some((j) => j.id === jobId);
    if (known) set(applyStatus(jobs, jobId, status));
    if (!known || reading.has(jobId) || status !== "running") void reread(jobId);
  }

  return {
    jobs: () => jobs,
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    async start() {
      unlisten.push(await listen("agent-job-line", (event) => onLine(event.payload)));
      unlisten.push(await listen("agent-job-status", (event) => onStatus(event.payload)));
      const known = new Set(jobs.map((j) => j.id));
      const fresh = list(await invoke<unknown>("list_agent_jobs"), "list_agent_jobs")
        .map(parseJob)
        .filter((j) => !known.has(j.id));
      set([...fresh, ...jobs]);
    },
    stop() {
      stopped = true;
      for (const off of unlisten.splice(0)) off();
      listeners.clear();
    },
    async cancel(jobId) {
      await invoke("cancel_agent_job", { id: jobId });
    },
  };
}
