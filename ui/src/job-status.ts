import "./job-status.css";
import { el } from "./dom";
import type { JobFeed } from "./job-feed";
import { describeDuration, newestFirst, type JobKind, type JobLine } from "./jobs";

// The running agent job a screen is waiting on: what it is doing, how long it
// has run, and its last few lines. It reads the app's job feed, so it shows the
// same job the log drawer lists. Without a feed it shows the label alone.

export interface JobStatusOptions {
  /** What the job is for, such as "Planning the course". */
  label: string;
  kind: JobKind;
  /** The course's folder id and the id inside course.json: a job record carries the second. */
  courseIds: string[];
  /** Milliseconds, so a duration can be tested. */
  now?: () => number;
}

export interface JobStatusHandle {
  stop(): void;
}

const SHOWN_LINES = 4;
const TICK_MS = 1000;

const isWorkLine = (line: JobLine): boolean => line.kind !== "status";

export function mountJobStatus(host: HTMLElement, feed: JobFeed | null, options: JobStatusOptions): JobStatusHandle {
  const now = (): number => options.now?.() ?? Date.now();
  const box = el("div", "job-status");
  box.setAttribute("role", "status");
  const head = el("p", "job-status-head");
  const title = el("span", "job-status-title", options.label);
  const time = el("span", "job-status-time");
  head.append(title, time);
  const note = el("p", "job-status-note", "Starting the agent…");
  const lines = el("ol", "job-status-lines");
  box.append(head, note, lines);
  host.append(box);

  function draw(): void {
    const mine = feed
      ? newestFirst(feed.jobs()).find(
          (j) => j.status === "running" && j.kind === options.kind && options.courseIds.includes(j.courseId),
        )
      : undefined;
    note.hidden = mine !== undefined;
    time.textContent = mine ? describeDuration(mine, now()) : "";
    lines.replaceChildren(
      ...(mine?.lines.filter(isWorkLine).slice(-SHOWN_LINES) ?? []).map((line) => {
        const item = el("li", undefined, line.text);
        item.title = line.text;
        if (line.kind === "error") item.className = "job-line-error";
        return item;
      }),
    );
  }

  draw();
  const unsubscribe = feed?.subscribe(draw);
  const timer = feed ? setInterval(draw, TICK_MS) : null;

  return {
    stop() {
      unsubscribe?.();
      if (timer !== null) clearInterval(timer);
    },
  };
}
