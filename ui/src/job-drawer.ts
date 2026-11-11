import "./job-drawer.css";
import type { LibraryCourse } from "./courses";
import { el, message } from "./dom";
import type { JobFeed } from "./job-feed";
import { assignJobLabels, jobLabel } from "./job-label";
import { describeDuration, newestFirst, type JobLine, type JobRecord } from "./jobs";
import { JOB_DRAWER_ID } from "./tabbar";

// The job log drawer. Closed, it is a hidden panel over the right edge of the
// window and takes no room from the reading area. Open, it lists the recent
// jobs, and selecting one shows the command line it ran and its log lines,
// following the job live.

export interface DrawerOptions {
  courses(): LibraryCourse[];
  /** Milliseconds, so a running job's duration can be tested. */
  now?: () => number;
  /** The drawer closed itself (Escape or its close button), so the page can redraw the control. */
  onClose(): void;
}

export interface DrawerHandle {
  open(): void;
  close(): void;
  isOpen(): boolean;
}

/** How close to the end the reader must be for new lines to keep the end in view. */
const FOLLOW_SLACK = 40;

/** The command with each flag and its value in one span, so a narrow drawer wraps between flags and never inside one. */
function commandNode(command: string): HTMLElement {
  const pre = el("pre", "job-command");
  command.split(/ (?=--?[A-Za-z])/).forEach((part, i) => {
    if (i > 0) pre.append(" ");
    pre.append(el("span", "job-arg", part));
  });
  return pre;
}

function agentText(job: JobRecord): string {
  return [job.driver, job.model ?? "default model", job.effort].filter((part) => part !== null).join(" · ");
}

export function mountJobDrawer(root: HTMLElement, feed: JobFeed, options: DrawerOptions): DrawerHandle {
  const now = (): number => options.now?.() ?? Date.now();
  const panel = el("aside", "job-drawer");
  panel.id = JOB_DRAWER_ID;
  panel.setAttribute("aria-label", "Agent jobs");
  panel.hidden = true;
  const head = el("header", "job-head");
  const heading = el("h2", undefined, "Agent jobs");
  const close = el("button", "job-close", "×");
  close.type = "button";
  close.setAttribute("aria-label", "Close the job log");
  head.append(heading, close);
  const body = el("div", "job-body");
  panel.append(head, body);
  root.append(panel);

  let opened = false;
  let selected: string | null = null;
  let timer: ReturnType<typeof setInterval> | null = null;
  let unsubscribe: (() => void) | null = null;
  /** The detail view's parts, present while one job is on screen. */
  let detail: {
    id: string;
    status: HTMLElement;
    duration: HTMLElement;
    stop: HTMLButtonElement | null;
    lines: HTMLElement;
    note: HTMLElement;
  } | null = null;

  const record = (id: string): JobRecord | undefined => feed.jobs().find((j) => j.id === id);

  function courseName(job: JobRecord): string {
    const found = options.courses().find((c) => c.id === job.courseId || c.storedId === job.courseId);
    return found ? (found.title.trim() === "" ? found.id : found.title) : job.courseId;
  }

  function statusChip(job: JobRecord): HTMLElement {
    const chip = el("span", "job-chip", job.status);
    chip.dataset.status = job.status;
    return chip;
  }

  function lineItem(line: JobLine): HTMLElement {
    const item = el("li", "job-line", line.text);
    item.dataset.kind = line.kind;
    item.title = line.at;
    return item;
  }

  function renderList(): void {
    detail = null;
    heading.textContent = "Agent jobs";
    const jobs = newestFirst(feed.jobs());
    if (jobs.length === 0) {
      body.replaceChildren(el("p", "job-empty", "No agent jobs yet. A job shows up here the moment an agent starts."));
      return;
    }
    const list = el("ul", "job-list");
    for (const job of jobs) {
      const row = el("button", "job-row");
      row.type = "button";
      row.dataset.jobId = job.id;
      const top = el("span", "job-row-top");
      top.append(el("span", "job-kind", jobLabel(job)), statusChip(job));
      const course = el("span", "job-course", courseName(job));
      const meta = el("span", "job-meta");
      const duration = el("span", "job-duration", describeDuration(job, now()));
      duration.dataset.jobId = job.id;
      meta.append(el("span", "job-agent", agentText(job)), duration);
      row.append(top, course, meta);
      row.addEventListener("click", () => select(job.id));
      const item = el("li");
      item.append(row);
      list.append(item);
    }
    body.replaceChildren(list);
  }

  function select(id: string | null): void {
    selected = id;
    render();
  }

  function renderDetail(job: JobRecord): void {
    heading.textContent = `${jobLabel(job)}: ${courseName(job)}`;
    const back = el("button", "job-back quiet-button", "← All jobs");
    back.type = "button";
    back.addEventListener("click", () => select(null));

    const status = statusChip(job);
    const duration = el("span", "job-duration", describeDuration(job, now()));
    const summary = el("div", "job-summary");
    summary.append(status, duration, el("span", "job-agent", agentText(job)));
    const note = el("p", "job-note");
    note.setAttribute("role", "alert");
    note.hidden = true;

    let stop: HTMLButtonElement | null = null;
    const bar = el("div", "job-bar");
    bar.append(back);
    if (job.status === "running") {
      stop = el("button", "job-stop quiet-button", "Stop job");
      stop.type = "button";
      const stopping = stop;
      stop.addEventListener("click", () => {
        stopping.disabled = true;
        feed.cancel(job.id).catch((error: unknown) => {
          stopping.disabled = false;
          note.textContent = `Could not stop the job: ${message(error)}`;
          note.hidden = false;
        });
      });
      bar.append(stop);
    }

    const command = commandNode(job.command);
    const lines = el("ol", "job-lines");
    lines.append(...job.lines.map(lineItem));
    body.replaceChildren(bar, summary, note, command, lines);
    detail = { id: job.id, status, duration, stop, lines, note };
    body.scrollTop = body.scrollHeight;
  }

  /** Brings the open detail view up to the record without rebuilding the lines it already shows. */
  function syncDetail(current: NonNullable<typeof detail>, job: JobRecord): void {
    current.status.textContent = job.status;
    current.status.dataset.status = job.status;
    current.duration.textContent = describeDuration(job, now());
    if (job.status !== "running" && current.stop) {
      current.stop.remove();
      current.stop = null;
    }
    const following = body.scrollHeight - body.scrollTop - body.clientHeight <= FOLLOW_SLACK;
    const shown = current.lines.children;
    // The last shown line can be replaced by a truncation marker, so compare it too.
    const keep = Math.max(0, Math.min(shown.length, job.lines.length) - 1);
    while (shown.length > keep) shown[shown.length - 1].remove();
    current.lines.append(...job.lines.slice(keep).map(lineItem));
    if (following) body.scrollTop = body.scrollHeight;
  }

  function render(): void {
    if (!opened) return;
    assignJobLabels(feed.jobs());
    const job = selected === null ? undefined : record(selected);
    if (selected !== null && !job) selected = null;
    if (!job) renderList();
    else if (detail?.id === job.id) syncDetail(detail, job);
    else renderDetail(job);
  }

  function tick(): void {
    if (!opened) return;
    for (const job of feed.jobs()) {
      if (job.status !== "running") continue;
      const text = describeDuration(job, now());
      if (detail?.id === job.id) detail.duration.textContent = text;
      for (const node of body.querySelectorAll<HTMLElement>(`.job-duration[data-job-id="${CSS.escape(job.id)}"]`)) {
        node.textContent = text;
      }
    }
  }

  function open(): void {
    if (opened) return;
    opened = true;
    panel.hidden = false;
    selected = null;
    detail = null;
    render();
    unsubscribe = feed.subscribe(render);
    timer = setInterval(tick, 1000);
    document.addEventListener("keydown", onKey);
  }

  function shut(): void {
    if (!opened) return;
    opened = false;
    panel.hidden = true;
    unsubscribe?.();
    unsubscribe = null;
    if (timer !== null) clearInterval(timer);
    timer = null;
    document.removeEventListener("keydown", onKey);
  }

  function onKey(event: KeyboardEvent): void {
    if (event.key !== "Escape") return;
    shut();
    options.onClose();
  }

  close.addEventListener("click", () => {
    shut();
    options.onClose();
  });

  return { open, close: shut, isOpen: () => opened };
}
