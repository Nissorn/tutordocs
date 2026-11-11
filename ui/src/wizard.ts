import "./wizard.css";
import { parseTagInput, type LibraryCourse } from "./courses";
import { createCourse, markCourseOpened, renameCourse, setCourseTags } from "./courses-api";
import { el, message } from "./dom";
import { listenForDrops, pickPaths } from "./file-input";
import type { JobFeed } from "./job-feed";
import { mountJobStatus, type JobStatusHandle } from "./job-status";
import { expectJobLabel } from "./job-label";
import { planOutline } from "./outline-api";
import {
  getCourseSetup,
  importFiles,
  listCourseSources,
  listCourseTemplates,
  setCourseSetup,
  LANGUAGES,
  LEVELS,
  type Language,
  type Level,
  type Template,
} from "./setup-api";
import { ingestUrl } from "./sources-api";
import {
  back,
  beginImport,
  beginUrl,
  blocker,
  describeSkip,
  failImport,
  failUrl,
  forward,
  importedSourceNames,
  parseHours,
  resumeWizard,
  settleImport,
  settleUrl,
  startWizard,
  STEPS,
  tally,
  toSetup,
  waiting,
  type CourseRef,
  type Material,
  type Step,
  type Wizard,
} from "./wizard-model";

// The New course screen: goal, materials, guide, plan. The state is one
// `Wizard` value (wizard-model.ts). This file draws it, takes the user's
// input into it, and runs the backend calls each step needs. Text the user or
// the agent wrote is only ever set as text.

export interface WizardOptions {
  /** A course that already exists, opened to finish its setup. Its title is fixed. */
  existing?: CourseRef & { title: string; tags: string[] };
  /** The app's job feed, for the planning job's progress. */
  jobs: JobFeed | null;
  /** The course was made, or its tags changed, so the page's list is out of date. */
  onChange(course: LibraryCourse): void;
  /** The draft outline is written. The caller decides whether to open the review. */
  onPlanned(course: CourseRef): void;
  /** An outline plan run has begun. */
  onPlanStart?(course: CourseRef): void;
  /** Leave without planning. A screen that has nowhere to leave to omits it, and the first step then has no Cancel. */
  onCancel?(): void;
  /** Aborting retires the screen. A running plan keeps going in Rust. */
  signal?: AbortSignal;
}

const STEP_WORDS: Record<Step, string> = { goal: "Goal", materials: "Materials", guide: "Guide", plan: "Plan" };
const LEVEL_WORDS: Record<Level, string> = { new: "New to this", some: "Some background", strong: "Strong background" };
const LANGUAGE_WORDS: Record<Language, string> = { en: "English", th: "Thai" };

function plural(n: number, noun: string): string {
  return `${n} ${noun}${n === 1 ? "" : "s"}`;
}

/** A radio in a group, wrapped in the label that is its text. */
function choice(name: string, value: string, text: string, checked: boolean, onPick: () => void): HTMLLabelElement {
  const label = el("label", "choice");
  const input = el("input");
  input.type = "radio";
  input.name = name;
  input.value = value;
  input.checked = checked;
  input.addEventListener("change", () => input.checked && onPick());
  label.append(input, el("span", undefined, text));
  return label;
}

function group(legend: string, ...children: Node[]): HTMLFieldSetElement {
  const set = el("fieldset", "wizard-group");
  set.append(el("legend", undefined, legend), ...children);
  return set;
}

/**
 * Mounts the wizard into `root`. Resolves once the templates (and, for an
 * existing course, its setup and sources) have loaded and the first step is on
 * screen. It never rejects: a load that fails is shown with a way to retry.
 */
export async function mountWizard(root: HTMLElement, options: WizardOptions): Promise<void> {
  const { existing, signal } = options;
  const section = el("section", "wizard");
  section.setAttribute("aria-label", existing ? "Set up the course" : "New course");
  const heading = el("h1", undefined, existing ? "Set up this course" : "New course");
  const steps = el("ol", "wizard-steps");
  const body = el("div", "wizard-body");
  const note = el("p", "wizard-note");
  note.setAttribute("role", "alert");
  note.hidden = true;
  const nav = el("div", "wizard-nav");
  section.append(heading, steps, body, note, nav);
  root.replaceChildren(section);

  let w: Wizard | null = null;
  let templates: Template[] = [];
  /** The tags text the course already has, so only a change is sent. */
  let savedTags = "";
  /** The title text the course already has, so only a change is sent. */
  let savedTitle = "";
  let stopStatus: JobStatusHandle | null = null;
  let stopDrops: (() => void) | null = null;
  let materialsList: HTMLElement | null = null;

  const retired = (): boolean => signal?.aborted === true;

  function say(text: string | null): void {
    note.textContent = text ?? "";
    note.hidden = text === null;
  }

  function set(next: Wizard): void {
    w = next;
  }

  // Drops. The window-wide drag events belong to whichever step shows them.

  function onDrop(event: { type: "over" } | { type: "leave" } | { type: "drop"; paths: string[] }): void {
    if (retired() || !w || w.step !== "materials") return;
    if (event.type === "over") body.dataset.dragging = "true";
    else delete body.dataset.dragging;
    if (event.type === "drop") void importPaths(event.paths);
  }

  listenForDrops(onDrop).then(
    (stop) => {
      if (retired()) stop();
      else stopDrops = stop;
    },
    () => undefined,
  );
  signal?.addEventListener("abort", () => {
    stopDrops?.();
    stopStatus?.stop();
  });

  // Materials.

  async function importPaths(paths: string[]): Promise<void> {
    if (!w?.course || paths.length === 0) return;
    const course = w.course;
    set({ ...w, materials: beginImport(w.materials, paths) });
    drawMaterials();
    drawNav();
    try {
      const results = await importFiles(course.id, paths);
      if (w) set({ ...w, materials: settleImport(w.materials, paths, results) });
    } catch (error) {
      if (w) set({ ...w, materials: failImport(w.materials, paths, message(error)) });
    }
    drawMaterials();
    drawNav();
  }

  async function addUrl(url: string): Promise<void> {
    if (!w?.course) return;
    const course = w.course;
    set({ ...w, materials: beginUrl(w.materials, url) });
    drawMaterials();
    drawNav();
    try {
      await markCourseOpened(course.id);
      const source = await ingestUrl(url);
      if (w) set({ ...w, materials: settleUrl(w.materials, url, source.name) });
    } catch (error) {
      if (w) set({ ...w, materials: failUrl(w.materials, url, message(error)) });
    }
    drawMaterials();
    drawNav();
  }

  async function pick(directory: boolean): Promise<void> {
    try {
      await importPaths(await pickPaths(directory));
    } catch (error) {
      say(`The file picker could not open: ${message(error)}`);
    }
  }

  function materialRow(m: Material): HTMLElement {
    const row = el("li", "material");
    row.dataset.state = m.state.phase;
    const name = el("span", "material-name", m.label);
    name.title = m.path;
    const status = el(
      "span",
      "material-status",
      { importing: "Importing…", imported: "Imported", skipped: "Skipped", failed: "Failed" }[m.state.phase],
    );
    row.append(name, status);
    if (m.state.phase === "skipped") row.append(el("span", "material-reason", describeSkip(m.state.reason)));
    if (m.state.phase === "failed") row.append(el("span", "material-reason", m.state.reason));
    return row;
  }

  function drawMaterials(): void {
    if (!materialsList || !w) return;
    const counts = tally(w.materials);
    const empty = w.materials.length === 0;
    const summary = [
      counts.imported > 0 ? `${counts.imported} imported` : null,
      counts.skipped > 0 ? `${counts.skipped} skipped` : null,
      counts.failed > 0 ? `${counts.failed} failed` : null,
      counts.importing > 0 ? `${counts.importing} importing` : null,
    ].filter((part) => part !== null);
    const list = el("ul", "materials");
    list.append(...w.materials.map(materialRow));
    const status = el("p", "materials-summary", summary.join(", "));
    status.setAttribute("role", "status");
    const hint = el("p", "materials-empty", "No files yet. You can plan the course without them.");
    hint.hidden = !empty;
    status.hidden = empty;
    materialsList.replaceChildren(status, hint, list);
  }

  // Steps.

  function textField(label: string, value: string, placeholder: string, onInput: (value: string) => void): HTMLInputElement {
    const input = el("input", "wizard-input");
    input.type = "text";
    input.value = value;
    input.placeholder = placeholder;
    input.setAttribute("aria-label", label);
    input.addEventListener("input", () => onInput(input.value));
    return input;
  }

  function labelled(text: string, control: HTMLElement): HTMLElement {
    const wrap = el("div", "wizard-field");
    wrap.append(el("span", "wizard-label", text), control);
    return wrap;
  }

  function goalStep(state: Wizard): HTMLElement {
    const step = el("div", "step step-goal");
    const title = textField("Course title", state.goal.title, "Course title", (value) => {
      if (w) set({ ...w, goal: { ...w.goal, title: value } });
    });
    title.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        void next();
      }
    });
    const titleField = labelled("Course title", title);
    const tags = textField("Tags", state.goal.tags, "Optional, separated by commas", (value) => {
      if (w) set({ ...w, goal: { ...w.goal, tags: value } });
    });
    const hours = textField("Hours per week", state.goal.hours, "Optional", (value) => {
      if (w) set({ ...w, goal: { ...w.goal, hours: value } });
    });
    hours.inputMode = "decimal";

    const cards = el("div", "template-cards");
    for (const template of templates) {
      const card = choice("template", template.id, "", state.goal.template === template.id, () => {
        if (w) set({ ...w, goal: { ...w.goal, template: template.id } });
        for (const other of cards.children) other.classList.toggle("selected", other === card);
      });
      card.className = "template-card";
      card.classList.toggle("selected", state.goal.template === template.id);
      const text = card.querySelector("span")!;
      text.className = "template-text";
      text.append(el("span", "template-title", template.title), el("span", "template-desc", template.description));
      cards.append(card);
    }

    const levels = el("div", "choices");
    for (const level of LEVELS) {
      levels.append(
        choice("level", level, LEVEL_WORDS[level], state.goal.level === level, () => {
          if (w) set({ ...w, goal: { ...w.goal, level } });
        }),
      );
    }
    const languages = el("div", "choices");
    for (const language of LANGUAGES) {
      languages.append(
        choice("language", language, LANGUAGE_WORDS[language], state.goal.language === language, () => {
          if (w) set({ ...w, goal: { ...w.goal, language } });
        }),
      );
    }
    const row = el("div", "wizard-row");
    row.append(labelled("Hours per week", hours), group("Language", languages));
    step.append(
      titleField,
      labelled("Tags", tags),
      group("Template", cards),
      group("Your level", levels),
      row,
    );
    return step;
  }

  function materialsStep(): HTMLElement {
    const step = el("div", "step step-materials");
    const drop = el("div", "drop-zone");
    drop.append(
      el("p", "drop-title", "Drop files or a folder anywhere on this screen"),
      el("p", "wizard-hint", "PDF, Markdown and plain text files are imported. Other files are listed as skipped."),
    );
    const files = el("button", "quiet-button", "Choose files");
    files.type = "button";
    files.addEventListener("click", () => void pick(false));
    const folder = el("button", "quiet-button", "Choose a folder");
    folder.type = "button";
    folder.addEventListener("click", () => void pick(true));
    const picks = el("div", "wizard-actions");
    picks.append(files, folder);
    drop.append(picks);

    const urlForm = el("form", "url-row");
    urlForm.noValidate = true;
    const url = el("input", "wizard-input");
    url.type = "text";
    url.placeholder = "Or paste a web page URL";
    url.setAttribute("aria-label", "Web page URL");
    url.inputMode = "url";
    url.autocomplete = "off";
    url.spellcheck = false;
    const add = el("button", "quiet-button", "Add page");
    add.type = "submit";
    urlForm.append(url, add);
    urlForm.addEventListener("submit", (event) => {
      event.preventDefault();
      const value = url.value.trim();
      if (value === "") return;
      url.value = "";
      void addUrl(value);
    });

    materialsList = el("div", "materials-box");
    step.append(drop, urlForm, el("p", "wizard-hint", "A page is fetched by the agent and can take a minute."), materialsList);
    return step;
  }

  function guideStep(state: Wizard): HTMLElement {
    const step = el("div", "step step-guide");
    const guide = el("textarea", "wizard-textarea");
    guide.rows = 8;
    guide.value = state.guide;
    guide.placeholder = "What to focus on, what to skip, how you will use the course. Optional.";
    guide.setAttribute("aria-label", "Guide");
    guide.addEventListener("input", () => {
      if (w) set({ ...w, guide: guide.value });
    });
    const chosen = templates.find((t) => t.id === state.goal.template);
    const aside = el("aside", "template-note");
    aside.setAttribute("aria-label", "Chosen template");
    if (chosen) aside.append(el("span", "template-title", chosen.title), el("span", "template-desc", chosen.description));
    step.append(labelled("Your guide for the planner", guide), aside);
    return step;
  }

  function planStep(state: Wizard): HTMLElement {
    const step = el("div", "step step-plan");
    const hours = parseHours(state.goal.hours);
    const imported = importedSourceNames(state).length;
    const rows: Array<[string, string]> = [
      ["Course", state.goal.title.trim()],
      ["Template", templates.find((t) => t.id === state.goal.template)?.title ?? state.goal.template],
      ["Level", LEVEL_WORDS[state.goal.level]],
      ["Time", hours.kind === "some" ? `${plural(hours.value, "hour")} a week` : "Not set"],
      ["Language", LANGUAGE_WORDS[state.goal.language]],
      ["Materials", imported === 0 ? "None" : plural(imported, "source")],
    ];
    const list = el("dl", "plan-summary");
    for (const [term, value] of rows) list.append(el("dt", undefined, term), el("dd", undefined, value));
    step.append(list);
    if (state.plan.phase === "running" && state.course) {
      step.append(
        el("p", "wizard-hint", "The agent is planning the course. This can take a few minutes, and it keeps going if you switch tabs."),
      );
      const host = el("div", "job-host");
      step.append(host);
      stopStatus = mountJobStatus(host, options.jobs, {
        label: "Planning the course",
        kind: "outline",
        courseIds: [state.course.id, state.course.storedId],
      });
    }
    return step;
  }

  // The frame around a step.

  function drawSteps(state: Wizard): void {
    steps.replaceChildren(
      ...STEPS.map((step, i) => {
        const item = el("li", undefined, `${i + 1} ${STEP_WORDS[step]}`);
        if (step === state.step) item.setAttribute("aria-current", "step");
        return item;
      }),
    );
  }

  function drawNav(): void {
    if (!w) return;
    const state = w;
    nav.replaceChildren();
    const make = (text: string, onClick: () => void, className = "quiet-button"): HTMLButtonElement => {
      const b = el("button", className, text);
      b.type = "button";
      b.addEventListener("click", onClick);
      return b;
    };
    if (state.step === "plan") {
      if (state.plan.phase === "failed") {
        nav.append(make("Back", goBack), make("Try again", () => void runPlan(), "outline-submit"));
      } else if (state.plan.phase === "running") {
        nav.append(make("Back to Home, plan keeps running", () => options.onCancel?.()));
      }
      return;
    }
    if (state.step !== "goal") nav.append(make("Back", goBack));
    else if (options.onCancel) nav.append(make("Cancel", () => options.onCancel?.()));
    const label = state.step === "guide" ? "Plan the course" : "Next";
    const primary = make(label, () => void next(), "outline-submit");
    primary.disabled = waiting(state);
    nav.append(primary);
  }

  function draw(): void {
    if (!w) return;
    const state = w;
    stopStatus?.stop();
    stopStatus = null;
    materialsList = null;
    delete body.dataset.dragging;
    drawSteps(state);
    body.dataset.step = state.step;
    const content =
      state.step === "goal"
        ? goalStep(state)
        : state.step === "materials"
          ? materialsStep()
          : state.step === "guide"
            ? guideStep(state)
            : planStep(state);
    body.replaceChildren(content);
    drawMaterials();
    drawNav();
    if (state.step !== "plan") content.querySelector<HTMLElement>("input:not([disabled]):not([type='radio']), textarea, button")?.focus();
  }

  // Moving.

  function goBack(): void {
    if (!w) return;
    say(null);
    set(back(w));
    draw();
  }

  /** Makes the course the first time the goal step is left, and keeps its title and tags current after. */
  async function ensureCourse(): Promise<boolean> {
    if (!w) return false;
    const tags = w.goal.tags;
    const title = w.goal.title.trim();
    try {
      if (!w.course) {
        set({ ...w, busy: "creating" });
        drawNav();
        const made = await createCourse(title, parseTagInput(tags));
        if (w) set({ ...w, busy: null, course: { id: made.id, storedId: made.storedId } });
        savedTags = tags;
        savedTitle = made.title;
        options.onChange(made);
      } else {
        if (title !== "" && title !== savedTitle) {
          const renamed = await renameCourse(w.course.id, title);
          savedTitle = renamed.title;
          options.onChange(renamed);
        }
        if (tags !== savedTags) {
          const saved = await setCourseTags(w.course.id, parseTagInput(tags));
          savedTags = tags;
          options.onChange(saved);
        }
      }
      return true;
    } catch (error) {
      if (w) set({ ...w, busy: null });
      say(w?.course ? `The course could not be updated: ${message(error)}` : `The course could not be created: ${message(error)}`);
      drawNav();
      return false;
    }
  }

  async function next(): Promise<void> {
    if (!w) return;
    say(null);
    const reason = blocker(w);
    if (reason !== null) {
      say(reason);
      return;
    }
    if (w.step === "goal" && !(await ensureCourse())) return;
    if (!w) return;
    if (w.step === "guide") {
      void runPlan();
      return;
    }
    set(forward(w));
    draw();
  }

  async function runPlan(): Promise<void> {
    if (!w?.course) return;
    const course = w.course;
    set({ ...w, step: "plan", plan: { phase: "running" } });
    options.onPlanStart?.(course);
    say(null);
    draw();
    const snapshot = w;
    let claim: ReturnType<typeof expectJobLabel> | null = null;
    try {
      await setCourseSetup(course.id, toSetup(snapshot));
      claim = expectJobLabel(options.jobs, [course.id, course.storedId], "outline", "Outline");
      await planOutline(course.id, snapshot.goal.title.trim(), importedSourceNames(snapshot));
    } catch (error) {
      claim?.release(options.jobs?.jobs() ?? []);
      const reason = message(error);
      if (w) set({ ...w, plan: { phase: "failed", reason } });
      if (!retired()) {
        draw();
        say(`The course could not be planned: ${reason}`);
      }
      return;
    }
    stopStatus?.stop();
    stopDrops?.();
    options.onPlanned(course);
  }

  // Loading.

  async function load(): Promise<void> {
    say(null);
    try {
      templates = await listCourseTemplates();
      if (existing) {
        const [setup, sources] = await Promise.all([getCourseSetup(existing.id), listCourseSources(existing.id)]);
        set(resumeWizard(templates, existing, setup, sources));
        savedTags = existing.tags.join(", ");
        savedTitle = existing.title;
      } else {
        set(startWizard(templates));
      }
    } catch (error) {
      body.replaceChildren();
      steps.replaceChildren();
      nav.replaceChildren();
      const retry = el("button", "outline-submit", "Try again");
      retry.type = "button";
      retry.addEventListener("click", () => void load());
      nav.append(retry);
      say(`The setup could not be loaded: ${message(error)}`);
      return;
    }
    draw();
  }

  await load();
}
