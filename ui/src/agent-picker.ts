import {
  driverTuning,
  listAgentDrivers,
  listDriverModels,
  refreshAgentDrivers,
  setDriverEffort,
  setDriverModel,
  setSelectedDriver,
  type AgentChoice,
  type AgentDriver,
  type DriverCatalog,
  type DriverTuning,
} from "./agents-api";
import { el, message } from "./dom";

// The one place the user chooses which coding agent writes their course, and
// which model and effort it runs with. At rest it is a single button naming
// the choice. The panel with the three selects exists only while the button has
// it open, so the reader's standing chrome stays one control.
//
// The agent list shows all three agents; a CLI that is missing or broken stays
// in it, disabled and labelled, so absence is reported and never assumed.

const NAMES: Record<string, string> = { claude: "Claude Code", qoder: "Qoder", agy: "Antigravity" };
/** The button is narrow, so Claude Code is just Claude there. */
const SHORT_NAMES: Record<string, string> = { claude: "Claude" };

/** Regaining focus rechecks, but not more often than this. */
const RECHECK_MIN_MS = 10_000;

const NONE_AVAILABLE =
  "No agent CLI found. Install Claude Code, Qoder or Antigravity, then return to this window.";

function nameOf(id: string): string {
  return NAMES[id] ?? id;
}

/** CLIs print `2.1.319 (Claude Code)`; the name is already in the label. */
function versionNumber(version: string): string {
  return version.replace(/\s*\(.*\)\s*$/, "");
}

/** What the button says: the agent, then the model and effort it will run with. */
function buttonText(selected: string, tuning: DriverTuning | null): string {
  const name = SHORT_NAMES[selected] ?? nameOf(selected);
  if (tuning === null) return name;
  if (tuning.model === null && tuning.effort === null) return `${name} \u00b7 CLI default`;
  const parts = [name, tuning.model ?? "default model"];
  if (tuning.effort !== null) parts.push(tuning.effort);
  return parts.join(" \u00b7 ");
}

function optionText(driver: AgentDriver): string {
  const name = nameOf(driver.id);
  if (driver.available) return driver.version ? `${name} ${versionNumber(driver.version)}` : name;
  return driver.binary === null ? `${name} (not found)` : `${name} (found, not usable)`;
}

/** A token that is a filesystem path rather than a reason the user can act on. */
function looksLikePath(token: string): boolean {
  return token.includes("/") || token.includes("\\") || token.startsWith("~");
}

/**
 * The part of Rust's unusable reason worth showing: the failure, without the
 * "found but unusable" prefix and without any filesystem path.
 */
function plainDetail(id: string, reason: string): string {
  const prefix = `${id} CLI found but unusable:`;
  const body = reason.startsWith(prefix) ? reason.slice(prefix.length) : reason;
  return body
    .split(/\s+/)
    .filter((token) => token !== "" && !looksLikePath(token))
    .join(" ")
    .replace(/[.:]+$/, "");
}

/** One sentence for a CLI that exists but cannot run. */
function unusableSentence(driver: AgentDriver): string {
  const name = nameOf(driver.id);
  const detail = plainDetail(driver.id, driver.reason ?? "");
  if (detail === "") return `${name} was found but cannot be used.`;
  return `${name} was found but cannot be used, because ${detail}.`;
}

/** What is worth saying under the picker. Empty when everything is plain. */
function notes(choice: AgentChoice): string[] {
  if (choice.selected === null) return [NONE_AVAILABLE];
  const lines: string[] = [];
  const chosen = choice.drivers.find((d) => d.id === choice.selected);
  const name = nameOf(choice.selected);
  if (chosen?.emulates?.structuredOutput) lines.push(`${name}: structured output is emulated through the prompt.`);
  if (chosen?.emulates?.systemAppend) lines.push(`${name}: system instructions go through the prompt body.`);
  if (chosen?.accepts && !chosen.accepts.model) lines.push(`${name} does not take a model.`);
  if (chosen?.accepts && !chosen.accepts.effort) lines.push(`${name} does not take an effort.`);
  for (const driver of choice.drivers) {
    if (!driver.available && driver.binary !== null) lines.push(unusableSentence(driver));
  }
  return lines;
}

interface Row {
  value: string;
  text: string;
  disabled?: boolean;
}

/** Where the agent check stands. The panel can only be open once it is ready or failed. */
type Phase =
  | { kind: "checking" }
  | { kind: "failed"; error: string }
  | { kind: "ready"; choice: AgentChoice };

/** The catalog of the selected agent. Not yet requested counts as loading. */
type Models = { kind: "loading" } | { kind: "ready"; catalog: DriverCatalog } | { kind: "failed" };

const CLI_DEFAULT = "CLI default";
/** Not a model id: `*` cannot pass the model check, so it cannot collide with one. */
const CUSTOM = "*";

function agentRows(phase: Phase): Row[] {
  if (phase.kind !== "ready") return [{ value: "", text: "Agents unavailable", disabled: true }];
  const rows = phase.choice.drivers.map((d) => ({ value: d.id, text: optionText(d), disabled: !d.available }));
  if (phase.choice.selected === null) rows.unshift({ value: "", text: "No agent available", disabled: true });
  return rows;
}

function modelRows(models: Models, current: string | null): Row[] {
  if (models.kind === "ready") {
    const rows = models.catalog.models.map((m) => ({ value: m.id ?? "", text: m.label }));
    if (models.catalog.acceptsCustomModel) rows.push({ value: CUSTOM, text: "Custom model" });
    // A saved model the CLI no longer lists stays visible rather than silently becoming the default.
    // Claude keeps that name in the custom field instead of a second row.
    if (current !== null && !rows.some((r) => r.value === current) && !models.catalog.acceptsCustomModel) {
      rows.push({ value: current, text: `${current} (not listed)` });
    }
    return rows;
  }
  const rows = [{ value: "", text: models.kind === "loading" ? "Loading models\u2026" : CLI_DEFAULT }];
  if (current !== null) rows.push({ value: current, text: current });
  return rows;
}

/** The select value for a saved model. An unlisted Claude model is the custom entry. */
function modelValue(models: Models, current: string | null): string {
  if (current === null) return "";
  if (models.kind !== "ready") return current;
  if (models.catalog.models.some((model) => model.id === current)) return current;
  if (models.catalog.acceptsCustomModel) return CUSTOM;
  return current;
}

/** Levels the selected model reported. Empty means the menu is only "CLI default". */
function effortsFor(models: Models, modelId: string | null): string[] {
  if (models.kind !== "ready") return [];
  const match = models.catalog.models.find((model) => (model.id ?? "") === (modelId ?? ""));
  if (match) return match.efforts;
  if (models.catalog.acceptsCustomModel) return models.catalog.efforts;
  return [];
}

function effortRows(models: Models, modelId: string | null): Row[] {
  return [{ value: "", text: CLI_DEFAULT }, ...effortsFor(models, modelId).map((level) => ({ value: level, text: level }))];
}

function asEffort(value: string): string | null {
  return /^[a-z]+$/.test(value) ? value : null;
}

/** The same boundary as `ModelId::parse`, so a typed name is refused before it is saved. */
function modelIdError(raw: string): string | null {
  if (raw === "") return "model is empty";
  if (raw.startsWith("-")) return "model must not start with '-'";
  if (!/^[A-Za-z0-9._:-]+$/.test(raw)) return "model may contain only A-Za-z0-9._:-";
  return null;
}

function option(row: Row): HTMLOptionElement {
  const node = el("option", undefined, row.text);
  node.value = row.value;
  node.disabled = row.disabled ?? false;
  return node;
}

/**
 * Sets a select to `rows` and `value`. The options are replaced only when the
 * rows changed, so an open menu is not rebuilt under the user's pointer.
 */
function fill(select: HTMLSelectElement, rows: Row[], value: string, disabled: boolean): void {
  const key = JSON.stringify(rows);
  if (select.dataset.rows !== key) {
    select.replaceChildren(...rows.map(option));
    select.dataset.rows = key;
  }
  select.value = value;
  select.disabled = disabled;
}

/**
 * Shows `lines` in a note, one sentence per line, each its own tooltip. A note
 * that already says this is left alone, so a live region does not announce it twice.
 */
function say(note: HTMLElement, lines: string[]): void {
  const key = JSON.stringify(lines);
  if (note.dataset.lines === key) return;
  note.dataset.lines = key;
  note.replaceChildren(
    ...lines.map((text) => {
      const line = el("div", "tool-line", text);
      line.title = text;
      return line;
    }),
  );
  note.hidden = lines.length === 0;
}

interface Panel {
  root: HTMLElement;
  agent: HTMLSelectElement;
  note: HTMLElement;
  modelField: HTMLElement;
  model: HTMLSelectElement;
  custom: HTMLInputElement;
  effortField: HTMLElement;
  effort: HTMLSelectElement;
  status: HTMLElement;
}

function selectField(caption: string, className: string): { field: HTMLElement; select: HTMLSelectElement } {
  const field = el("label", "tool-field");
  const select = el("select", className);
  field.append(el("span", "tool-label", caption), select);
  return { field, select };
}

function buildPanel(): Panel {
  const agent = selectField("Agent", "agent-select");
  const model = selectField("Model", "model-select");
  const custom = el("input", "custom-model");
  custom.type = "text";
  custom.hidden = true;
  custom.spellcheck = false;
  custom.setAttribute("aria-label", "Custom model");
  const effort = selectField("Effort", "effort-select");
  const note = el("div", "tool-note");
  note.setAttribute("aria-live", "polite");
  const status = el("div", "tool-note error");
  status.setAttribute("role", "alert");
  const root = el("div", "agent-panel");
  root.setAttribute("role", "dialog");
  root.setAttribute("aria-label", "Agent, model and effort");
  root.append(agent.field, note, model.field, effort.field, status);
  return {
    root,
    agent: agent.select,
    note,
    modelField: model.field,
    model: model.select,
    custom,
    effortField: effort.field,
    effort: effort.select,
    status,
  };
}

/**
 * Mounts the picker into `host` and resolves once the first check finished.
 * It never rejects: a failed check is shown in place.
 */
export async function mountAgentPicker(host: HTMLElement): Promise<void> {
  const field = el("div", "agent-picker");
  const button = el("button", "agent-button");
  button.type = "button";
  button.setAttribute("aria-haspopup", "dialog");
  button.setAttribute("aria-expanded", "false");
  field.append(el("span", "tool-label", "Agent"), button);
  host.append(field);

  let phase: Phase = { kind: "checking" };
  /** The saved choice of the selected agent. Null until read, which is not the same as unset. */
  let tuning: DriverTuning | null = null;
  let models: Models = { kind: "loading" };
  /** Catalogs come from the CLIs, which can take seconds, so each agent is asked once per change of installed CLIs. */
  const lists = new Map<string, DriverCatalog>();
  /** The user opened the custom-model field and has not picked a listed model since. */
  let customOpen = false;
  /** A clear of an effort the model lacks is in flight, so a second one is not sent. */
  let dropping = false;
  /** The last action that failed, as a plain sentence. Cleared when the panel opens. */
  let error: string | null = null;
  let panel: Panel | null = null;

  const choice = (): AgentChoice | null => (phase.kind === "ready" ? phase.choice : null);
  const selected = (): string | null => choice()?.selected ?? null;
  const chosenDriver = (): AgentDriver | undefined => choice()?.drivers.find((d) => d.id === selected());

  function buttonLabel(): string {
    if (phase.kind === "checking") return "Checking agents\u2026";
    if (phase.kind === "failed") return "Agents unavailable";
    const id = selected();
    return id === null ? "No agent available" : buttonText(id, tuning);
  }

  function paint(): void {
    const text = buttonLabel();
    button.textContent = text;
    button.title = text;
    button.setAttribute("aria-label", `Agent, model and effort: ${text}`);
    button.disabled = phase.kind === "checking";
    if (phase.kind === "checking") field.setAttribute("aria-busy", "true");
    else field.removeAttribute("aria-busy");
    if (panel) paintPanel(panel);
  }

  function paintPanel(p: Panel): void {
    const id = selected();
    const accepts = chosenDriver()?.accepts;
    fill(p.agent, agentRows(phase), id ?? "", id === null);
    say(p.note, phase.kind === "ready" ? notes(phase.choice) : []);
    p.modelField.hidden = p.effortField.hidden = id === null;
    const modelLocked = models.kind !== "ready" || tuning === null || !accepts?.model;
    const current = tuning?.model ?? null;
    const picked = modelValue(models, current);
    if (picked === CUSTOM) customOpen = true;
    const shown = customOpen && models.kind === "ready" && models.catalog.acceptsCustomModel ? CUSTOM : picked;
    fill(p.model, modelRows(models, current), shown, modelLocked);
    const typed =
      current !== null && models.kind === "ready" && !models.catalog.models.some((model) => model.id === current)
        ? current
        : "";
    // The field stays out of the panel until a custom name is the selection, so
    // opening the panel does not add a standing control.
    if (shown === CUSTOM) {
      if (!p.custom.isConnected) p.modelField.append(p.custom);
      p.custom.hidden = false;
      if (document.activeElement !== p.custom) p.custom.value = typed;
    } else if (p.custom.isConnected) {
      p.custom.remove();
    }
    const levels = effortsFor(models, current);
    const effortValue = tuning?.effort && levels.includes(tuning.effort) ? tuning.effort : "";
    const effortLocked = tuning === null || !accepts?.effort || models.kind !== "ready";
    fill(p.effort, effortRows(models, current), effortValue, effortLocked);
    say(p.status, phase.kind === "failed" ? [phase.error] : error === null ? [] : [error]);
  }

  /**
   * Takes a listing as the truth. When the selected agent changed, what was
   * known about the old one is dropped and the new one's is read.
   */
  async function apply(next: AgentChoice): Promise<void> {
    if (JSON.stringify(next.drivers) !== JSON.stringify(choice()?.drivers)) lists.clear();
    const changed = next.selected !== selected();
    phase = { kind: "ready", choice: next };
    if (changed) {
      tuning = null;
      models = { kind: "loading" };
      customOpen = false;
    }
    paint();
    const id = next.selected;
    if (changed && id !== null) await Promise.all([loadTuning(id), panel ? loadModels(id) : undefined]);
  }

  async function loadTuning(id: string): Promise<void> {
    try {
      const read = await driverTuning(id);
      if (selected() !== id) return;
      tuning = read;
    } catch (failure) {
      if (selected() !== id) return;
      error = `Could not read the saved model and effort: ${message(failure)}`;
    }
    paint();
    reconcileEffort(id);
  }

  async function loadModels(id: string): Promise<void> {
    const known = lists.get(id);
    if (known) {
      models = { kind: "ready", catalog: known };
      paint();
      reconcileEffort(id);
      return;
    }
    try {
      const catalog = await listDriverModels(id);
      lists.set(id, catalog);
      if (selected() !== id) return;
      models = { kind: "ready", catalog };
    } catch (failure) {
      if (selected() !== id) return;
      models = { kind: "failed" };
      error = `Could not load the model list: ${message(failure)}`;
    }
    paint();
    reconcileEffort(id);
  }

  /** Clears a saved effort the selected model does not offer. */
  function reconcileEffort(id: string): void {
    if (dropping || selected() !== id || models.kind !== "ready" || tuning?.effort == null) return;
    if (effortsFor(models, tuning.model).includes(tuning.effort)) return;
    dropping = true;
    void save(id, "effort", () => setDriverEffort(id, null)).finally(() => {
      dropping = false;
    });
  }

  /** Writes one setting. The selects fall back to the last saved value when it fails. */
  async function save(id: string, what: string, write: () => Promise<DriverTuning>): Promise<void> {
    try {
      const saved = await write();
      if (selected() !== id) return;
      tuning = saved;
      error = null;
    } catch (failure) {
      if (selected() !== id) return;
      error = `Could not save the ${what}: ${message(failure)}`;
    }
    paint();
  }

  async function switchTo(id: string): Promise<void> {
    try {
      error = null;
      await apply(await setSelectedDriver(id));
    } catch (failure) {
      error = `Could not switch agent: ${message(failure)}`;
      paint();
    }
  }

  const onOutside = (event: Event): void => {
    if (!field.contains(event.target as Node)) closePanel(false);
  };
  const onKeyDown = (event: KeyboardEvent): void => {
    if (event.key === "Escape") closePanel(true);
  };

  function openPanel(): void {
    error = null;
    panel = buildPanel();
    const p = panel;
    p.agent.addEventListener("change", () => void switchTo(p.agent.value));
    p.model.addEventListener("change", () => {
      const id = selected();
      if (id === null) return;
      if (p.model.value === CUSTOM) {
        customOpen = true;
        paint();
        p.custom.focus();
        return;
      }
      customOpen = false;
      const model = p.model.value === "" ? null : p.model.value;
      void save(id, "model", () => setDriverModel(id, model)).then(() => {
        if (selected() === id) reconcileEffort(id);
      });
    });
    p.custom.addEventListener("change", () => {
      const id = selected();
      if (id === null) return;
      const raw = p.custom.value.trim();
      const problem = modelIdError(raw);
      if (problem) {
        error = problem;
        paint();
        return;
      }
      void save(id, "model", () => setDriverModel(id, raw)).then(() => {
        if (selected() === id) reconcileEffort(id);
      });
    });
    p.effort.addEventListener("change", () => {
      const id = selected();
      if (id !== null) void save(id, "effort", () => setDriverEffort(id, asEffort(p.effort.value)));
    });
    field.append(p.root);
    button.setAttribute("aria-expanded", "true");
    document.addEventListener("pointerdown", onOutside, true);
    document.addEventListener("focusin", onOutside);
    document.addEventListener("keydown", onKeyDown);
    paint();
    const first = [p.agent, p.model, p.effort].find((select) => !select.disabled);
    if (first) first.focus();
    else {
      p.root.tabIndex = -1;
      p.root.focus();
    }
    const id = selected();
    if (id !== null) {
      if (tuning === null) void loadTuning(id);
      if (models.kind !== "ready") void loadModels(id);
    }
  }

  function closePanel(restoreFocus: boolean): void {
    if (!panel) return;
    panel.root.remove();
    panel = null;
    button.setAttribute("aria-expanded", "false");
    document.removeEventListener("pointerdown", onOutside, true);
    document.removeEventListener("focusin", onOutside);
    document.removeEventListener("keydown", onKeyDown);
    if (restoreFocus) button.focus();
  }

  button.addEventListener("click", () => (panel ? closePanel(false) : openPanel()));

  paint();
  let lastCheck = Date.now();
  try {
    await apply(await listAgentDrivers());
  } catch (failure) {
    phase = { kind: "failed", error: `Could not check agents: ${message(failure)}` };
    paint();
  }
  lastCheck = Date.now();

  async function recheck(): Promise<void> {
    if (!host.isConnected) {
      window.removeEventListener("focus", recheck);
      return;
    }
    if (Date.now() - lastCheck < RECHECK_MIN_MS) return;
    lastCheck = Date.now();
    try {
      await apply(await refreshAgentDrivers());
    } catch (failure) {
      console.warn(`agent recheck failed: ${message(failure)}`);
    }
  }
  window.addEventListener("focus", recheck);
}
