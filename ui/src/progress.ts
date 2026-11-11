import { invoke } from "@tauri-apps/api/core";
import { record, str } from "./api";
import "./quiz.css";

// Progress per course objective, from the course_progress command. The Rust
// side projects it from the mastery log, so what this shows survives restarts.

export interface ObjectiveProgress {
  objective: string;
  mastery: number;
  attempts: number;
  nextReviewAt: number | null;
  due: boolean;
}

export function parseProgress(raw: unknown): ObjectiveProgress[] {
  if (!Array.isArray(raw)) throw new Error("course_progress: expected a list");
  return raw.map((item) => {
    const r = record(item, "objective progress");
    if (typeof r.mastery !== "number" || typeof r.attempts !== "number" || typeof r.due !== "boolean") {
      throw new Error("course_progress: malformed objective");
    }
    const next = r.nextReviewAt;
    if (next !== null && typeof next !== "number") throw new Error("course_progress: malformed nextReviewAt");
    return { objective: str(r, "objective"), mastery: r.mastery, attempts: r.attempts, nextReviewAt: next, due: r.due };
  });
}

export async function courseProgress(): Promise<ObjectiveProgress[]> {
  return parseProgress(await invoke<unknown>("course_progress"));
}

const DAY_SECONDS = 86_400;

/** When to come back, in words. */
export function reviewWords(p: ObjectiveProgress, nowSeconds: number): string {
  if (p.attempts === 0 || p.nextReviewAt === null) return "Not assessed yet";
  if (p.due) return "Review now";
  const days = Math.ceil((p.nextReviewAt - nowSeconds) / DAY_SECONDS);
  return days <= 1 ? "Review tomorrow" : `Review in ${days} days`;
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string, text?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

/** The sidebar's progress list. Call `refresh` after a grading is saved. */
export function progressPanel(): { node: HTMLElement; refresh: () => Promise<void> } {
  const node = el("section", "progress");
  node.setAttribute("aria-label", "Progress by objective");

  async function refresh(): Promise<void> {
    let items: ObjectiveProgress[];
    try {
      items = await courseProgress();
    } catch {
      node.replaceChildren();
      return;
    }
    const heading = el("h3", "progress-title", "Progress");
    const rows = el("ul", "progress-list");
    const now = Date.now() / 1000;
    for (const p of items) {
      const row = el("li", "progress-item");
      row.dataset.due = String(p.due);
      const percent = Math.round(p.mastery * 100);
      const bar = el("div", "progress-bar");
      bar.setAttribute("role", "meter");
      bar.setAttribute("aria-valuemin", "0");
      bar.setAttribute("aria-valuemax", "100");
      bar.setAttribute("aria-valuenow", String(percent));
      bar.setAttribute("aria-label", p.objective);
      const fill = el("span");
      fill.style.width = `${percent}%`;
      bar.append(fill);
      const answers = p.attempts === 1 ? "1 answer" : `${p.attempts} answers`;
      row.append(
        el("span", "progress-objective", p.objective),
        bar,
        el("span", "progress-meta", `${percent}% · ${answers} · ${reviewWords(p, now)}`),
      );
      rows.append(row);
    }
    node.replaceChildren(heading, rows);
  }

  return { node, refresh };
}
