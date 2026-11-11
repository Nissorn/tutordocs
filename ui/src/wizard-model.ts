import type { CourseSetup, CourseSource, ImportResult, Language, Level, Template } from "./setup-api";

// The New course wizard as data. Four steps, one value that holds everything
// the user has told it, and pure functions that move it. The screen in
// wizard.ts draws a `Wizard` and calls these; it keeps no flags of its own.

export const STEPS = ["goal", "materials", "guide", "plan"] as const;
export type Step = (typeof STEPS)[number];

/** The goal step's fields as typed. Hours stay text until `parseHours` reads them. */
export interface Goal {
  title: string;
  /** Comma separated, as typed. */
  tags: string;
  /** A template id. */
  template: string;
  level: Level;
  hours: string;
  language: Language;
}

/** What became of one file or page the user offered. */
export type MaterialState =
  | { phase: "importing" }
  | { phase: "imported"; sourceName: string }
  | { phase: "skipped"; reason: string | null }
  | { phase: "failed"; reason: string };

export interface Material {
  /** A dropped or picked path, or a page URL. The row's identity. */
  path: string;
  label: string;
  origin: "file" | "url";
  state: MaterialState;
}

export type PlanState = { phase: "idle" } | { phase: "running" } | { phase: "failed"; reason: string };

/** Both names of a course: the folder id every command takes, and the id inside course.json that job records carry. */
export interface CourseRef {
  id: string;
  storedId: string;
}

export interface Wizard {
  step: Step;
  goal: Goal;
  /** Set once the course exists, which is when the user leaves the goal step. */
  course: CourseRef | null;
  materials: Material[];
  guide: string;
  plan: PlanState;
  /** Set while the course is being made, so the step cannot be left twice. */
  busy: "creating" | null;
}

const DEFAULT_TEMPLATE = "overview";
const MAX_HOURS = 168;

function defaultGoal(templates: Template[]): Goal {
  const template = templates.some((t) => t.id === DEFAULT_TEMPLATE) ? DEFAULT_TEMPLATE : (templates[0]?.id ?? "");
  return { title: "", tags: "", template, level: "new", hours: "", language: "en" };
}

export function startWizard(templates: Template[]): Wizard {
  return { step: "goal", goal: defaultGoal(templates), course: null, materials: [], guide: "", plan: { phase: "idle" }, busy: null };
}

/** A course that already exists, opened to finish its setup. Its title cannot change. */
export function resumeWizard(
  templates: Template[],
  course: CourseRef & { title: string; tags: string[] },
  setup: CourseSetup | null,
  sources: CourseSource[],
): Wizard {
  const goal = defaultGoal(templates);
  return {
    step: "goal",
    goal: {
      ...goal,
      title: course.title,
      tags: course.tags.join(", "),
      ...(setup
        ? {
            template: setup.template,
            level: setup.level,
            hours: setup.hoursPerWeek === null ? "" : String(setup.hoursPerWeek),
            language: setup.language,
          }
        : {}),
    },
    course: { id: course.id, storedId: course.storedId },
    materials: sources.map((s) => ({
      path: s.path,
      label: s.name,
      origin: s.kind === "url" ? "url" : "file",
      state: { phase: "imported", sourceName: s.name },
    })),
    guide: setup?.guide ?? "",
    plan: { phase: "idle" },
    busy: null,
  };
}

export type Hours = { kind: "none" } | { kind: "some"; value: number } | { kind: "invalid" };

export function parseHours(text: string): Hours {
  const t = text.trim();
  if (t === "") return { kind: "none" };
  // The backend stores whole hours, 1 to 168, and refuses anything else.
  if (!/^\d+$/.test(t)) return { kind: "invalid" };
  const value = Number(t);
  return value >= 1 && value <= MAX_HOURS ? { kind: "some", value } : { kind: "invalid" };
}

const stepAfter = (step: Step): Step => STEPS[Math.min(STEPS.indexOf(step) + 1, STEPS.length - 1)];
const stepBefore = (step: Step): Step => STEPS[Math.max(STEPS.indexOf(step) - 1, 0)];

/** Why the user cannot leave the current step yet, or null. */
export function blocker(w: Wizard): string | null {
  if (w.busy === "creating") return "Creating the course…";
  switch (w.step) {
    case "goal":
      if (w.goal.title.trim() === "") return "Name the course first.";
      if (parseHours(w.goal.hours).kind === "invalid") {
        return `Hours per week must be a whole number from 1 to ${MAX_HOURS}, or left empty.`;
      }
      return null;
    case "materials":
      return w.materials.some((m) => m.state.phase === "importing") ? "Wait for the files to finish importing." : null;
    case "guide":
    case "plan":
      return null;
  }
}

/** True while something the user started is still running, so Next should wait rather than explain. */
export function waiting(w: Wizard): boolean {
  return w.busy !== null || w.materials.some((m) => m.state.phase === "importing");
}

export function forward(w: Wizard): Wizard {
  return blocker(w) === null && w.step !== "plan" ? { ...w, step: stepAfter(w.step) } : w;
}

export function back(w: Wizard): Wizard {
  return w.step === "plan" && w.plan.phase === "running" ? w : { ...w, step: stepBefore(w.step) };
}

export function toSetup(w: Wizard): CourseSetup {
  const hours = parseHours(w.goal.hours);
  return {
    template: w.goal.template,
    level: w.goal.level,
    hoursPerWeek: hours.kind === "some" ? hours.value : null,
    language: w.goal.language,
    guide: w.guide,
  };
}

/** The last part of a path, with either slash. */
export function basename(path: string): string {
  const parts = path.split(/[\\/]/).filter((part) => part !== "");
  return parts[parts.length - 1] ?? path;
}

function without(materials: Material[], paths: string[]): Material[] {
  return materials.filter((m) => !paths.includes(m.path));
}

const isImported = (m: Material): boolean => m.state.phase === "imported";

/** Rows for the paths just offered. A path already imported keeps its row; any other path starts over. */
export function beginImport(materials: Material[], paths: string[]): Material[] {
  const fresh = paths.filter((path) => !materials.some((m) => m.path === path && isImported(m)));
  const rows: Material[] = fresh.map((path) => ({
    path,
    label: basename(path),
    origin: "file",
    state: { phase: "importing" },
  }));
  return [...without(materials, fresh), ...rows];
}

/**
 * Swaps the importing rows for the backend's results, one row per file. A path
 * that is already imported keeps its row when it comes back as a duplicate.
 */
export function settleImport(materials: Material[], offered: string[], results: ImportResult[]): Material[] {
  let next = materials.filter((m) => !(offered.includes(m.path) && m.state.phase === "importing"));
  for (const result of results) {
    const known = next.find((m) => m.path === result.path);
    if (known && isImported(known)) continue;
    const state: MaterialState =
      result.status === "imported"
        ? { phase: "imported", sourceName: result.source.name }
        : { phase: "skipped", reason: result.reason };
    next = [...without(next, [result.path]), { path: result.path, label: basename(result.path), origin: "file", state }];
  }
  return next;
}

export function failImport(materials: Material[], offered: string[], reason: string): Material[] {
  return materials.map((m) =>
    offered.includes(m.path) && m.state.phase === "importing" ? { ...m, state: { phase: "failed", reason } } : m,
  );
}

export function beginUrl(materials: Material[], url: string): Material[] {
  return [...without(materials, [url]), { path: url, label: url, origin: "url", state: { phase: "importing" } }];
}

export function settleUrl(materials: Material[], url: string, sourceName: string): Material[] {
  return materials.map((m) => (m.path === url ? { ...m, state: { phase: "imported", sourceName } } : m));
}

export function failUrl(materials: Material[], url: string, reason: string): Material[] {
  return failImport(materials, [url], reason);
}

export function tally(materials: Material[]): Record<MaterialState["phase"], number> {
  const counts = { importing: 0, imported: 0, skipped: 0, failed: 0 };
  for (const m of materials) counts[m.state.phase]++;
  return counts;
}

/** The source names the backend made, for the planner. */
export function importedSourceNames(w: Wizard): string[] {
  return w.materials.flatMap((m) => (m.state.phase === "imported" ? [m.state.sourceName] : []));
}

/** A skip reason as the reader sees it. "duplicate" is the contract's word, so it gets a sentence. */
export function describeSkip(reason: string | null): string {
  if (reason === null) return "Skipped.";
  return reason === "duplicate" ? "Already in this course, same contents." : reason;
}
