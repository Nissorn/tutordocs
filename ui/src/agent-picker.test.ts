// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import contract from "./fixtures/agents-contract.json";
import { mountAgentPicker } from "./agent-picker";

interface Driver {
  id: string;
  available: boolean;
  binary: string | null;
  version: string | null;
  capabilities: Record<string, unknown> | null;
  reason: string | null;
}
interface Choice {
  drivers: Driver[];
  selected: string | null;
}

/**
 * The default machine from the Rust contract: claude present, qoder absent, agy
 * broken. Its claude lists `--model` and `--effort` in `--help`, as the real
 * CLI does, which the recorded contract predates.
 */
const mixed: Choice = {
  ...contract.list_agent_drivers,
  drivers: contract.list_agent_drivers.drivers.map((d) =>
    d.id === "claude" ? { ...d, capabilities: { ...d.capabilities, model: true, effort: true } } : d,
  ),
};

function driver(id: string, patch: Partial<Driver> = {}): Driver {
  return {
    ...mixed.drivers[0],
    id,
    binary: `/opt/fake/bin/${id}`,
    ...patch,
  };
}

function withQoder(): Choice {
  const qoder = driver("qoder", {
    version: "1.1.85",
    capabilities: { ...mixed.drivers[0].capabilities, structured_output: false },
  });
  return { drivers: [mixed.drivers[0], qoder, mixed.drivers[2]], selected: "claude" };
}

function withAgy(): Choice {
  const agy = driver("agy", {
    version: "1.5.3",
    capabilities: { ...mixed.drivers[0].capabilities, structured_output: false },
  });
  return { drivers: [mixed.drivers[0], mixed.drivers[1], agy], selected: "claude" };
}

interface Tuning {
  model: string | null;
  effort: string | null;
}
interface ModelOption {
  id: string | null;
  label: string;
  efforts: string[];
}
interface Catalog {
  models: ModelOption[];
  efforts: string[];
  accepts_custom_model: boolean;
}

/** What the recorded Claude and agy listings report. A test fixture, not the app's list. */
const LEVELS = ["low", "medium", "high", "xhigh", "max"];
const option = (id: string | null, label: string, efforts: string[] = LEVELS): ModelOption => ({ id, label, efforts });
const CLI_DEFAULT: ModelOption = option(null, "CLI default");
const CLAUDE_MODELS: ModelOption[] = [
  CLI_DEFAULT,
  ...["fable", "opus", "sonnet"].map((id) => option(id, id)),
];
const AGY_MODELS: ModelOption[] = [
  CLI_DEFAULT,
  option("gemini-4-argon", "Gemini 4 Argon", ["low", "medium", "high"]),
  option("claude-opus-5-5", "Claude Opus 5.5", ["low", "medium", "high"]),
];

function wire(id: string, models: ModelOption[]): Catalog {
  const efforts: string[] = [];
  for (const model of models) {
    for (const level of model.efforts) if (!efforts.includes(level)) efforts.push(level);
  }
  return { models, efforts, accepts_custom_model: id === "claude" };
}
const NO_TUNING: Tuning = { model: null, effort: null };

interface Calls {
  list: number;
  refresh: number;
  set: string[];
  models: string[];
  tuning: string[];
  setModel: { id: string; model: string | null }[];
  setEffort: { id: string; effort: string | null }[];
}

interface Options {
  tuning?: Record<string, Tuning>;
  models?: Record<string, ModelOption[]>;
  overrides?: Record<string, (args: Record<string, unknown>) => unknown>;
}

/** The Rust side of the contract, with the commands the picker uses. */
function backend(initial: Choice, { tuning = {}, models = {}, overrides = {} }: Options = {}) {
  const calls: Calls = { list: 0, refresh: 0, set: [], models: [], tuning: [], setModel: [], setEffort: [] };
  const lists: Record<string, ModelOption[]> = { claude: CLAUDE_MODELS, agy: AGY_MODELS, ...models };
  const saved: Record<string, Tuning> = { ...tuning };
  const tuningOf = (id: string): Tuning => saved[id] ?? NO_TUNING;
  let current = initial;
  mockIPC((cmd, raw) => {
    const args = (raw ?? {}) as Record<string, unknown>;
    if (overrides[cmd]) return overrides[cmd](args);
    switch (cmd) {
      case "list_agent_drivers":
        calls.list += 1;
        return current;
      case "refresh_agent_drivers":
        calls.refresh += 1;
        return current;
      case "set_selected_driver":
        calls.set.push(args.id as string);
        current = { ...current, selected: args.id as string };
        return current;
      case "list_driver_models":
        calls.models.push(args.id as string);
        return wire(args.id as string, lists[args.id as string] ?? [option(null, "CLI default", [])]);
      case "driver_tuning":
        calls.tuning.push(args.id as string);
        return tuningOf(args.id as string);
      case "set_driver_model":
        calls.setModel.push({ id: args.id as string, model: args.model as string | null });
        saved[args.id as string] = { ...tuningOf(args.id as string), model: args.model as string | null };
        return saved[args.id as string];
      case "set_driver_effort":
        calls.setEffort.push({ id: args.id as string, effort: args.effort as string | null });
        saved[args.id as string] = { ...tuningOf(args.id as string), effort: args.effort as string | null };
        return saved[args.id as string];
      default:
        throw new Error(`unexpected command ${cmd}`);
    }
  });
  return {
    calls,
    set(next: Choice) {
      current = next;
    },
  };
}

let host: HTMLElement;

beforeEach(() => {
  document.body.innerHTML = '<div id="host"></div>';
  host = document.querySelector<HTMLElement>("#host")!;
});

afterEach(() => {
  clearMocks();
  vi.useRealTimers();
});

function select(): HTMLSelectElement {
  return host.querySelector<HTMLSelectElement>(".agent-select")!;
}

function note(): HTMLElement {
  return host.querySelector<HTMLElement>(".tool-note:not(.error)")!;
}

function status(): HTMLElement {
  return host.querySelector<HTMLElement>(".tool-note.error")!;
}

function modelSelect(): HTMLSelectElement {
  return host.querySelector<HTMLSelectElement>(".model-select")!;
}

function customInput(): HTMLInputElement {
  return host.querySelector<HTMLInputElement>(".custom-model")!;
}

function effortSelect(): HTMLSelectElement {
  return host.querySelector<HTMLSelectElement>(".effort-select")!;
}

function options(of: HTMLSelectElement = select()) {
  return Array.from(of.options).map((o) => ({ value: o.value, text: o.textContent, disabled: o.disabled }));
}

function choose(id: string): void {
  pick(select(), id);
}

function pick(of: HTMLSelectElement, value: string): void {
  of.value = value;
  of.dispatchEvent(new Event("change", { bubbles: true }));
}

/** Mounts the picker and opens its panel, the way a user reaches every select. */
async function mountOpen(): Promise<void> {
  await mountAgentPicker(host);
  button().click();
}

function button(): HTMLButtonElement {
  return host.querySelector<HTMLButtonElement>("button.agent-button")!;
}

describe("the picker button", () => {
  it("names the agent and says CLI default when no model or effort is set", async () => {
    backend(mixed);
    await mountAgentPicker(host);
    expect(button().textContent).toBe("Claude \u00b7 CLI default");
  });

  it("names the model and the effort when both are set", async () => {
    backend(mixed, { tuning: { claude: { model: "opus", effort: "high" } } });
    await mountAgentPicker(host);
    expect(button().textContent).toBe("Claude \u00b7 opus \u00b7 high");
  });

  it("names a model alone, and an effort alone with the default model", async () => {
    backend(mixed, { tuning: { claude: { model: "sonnet", effort: null } } });
    await mountAgentPicker(host);
    expect(button().textContent).toBe("Claude \u00b7 sonnet");

    document.body.innerHTML = '<div id="host"></div>';
    host = document.querySelector<HTMLElement>("#host")!;
    backend(mixed, { tuning: { claude: { model: null, effort: "max" } } });
    await mountAgentPicker(host);
    expect(button().textContent).toBe("Claude \u00b7 default model \u00b7 max");
  });
});

function panel(): HTMLElement | null {
  return host.querySelector<HTMLElement>('[role="dialog"]');
}

function press(key: string): void {
  document.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
}

describe("opening and closing the panel", () => {
  it("keeps the panel and every select out of the page until the button opens it", async () => {
    backend(mixed);
    await mountAgentPicker(host);
    expect(panel()).toBeNull();
    expect(host.querySelector("select")).toBeNull();
    expect(button().getAttribute("aria-haspopup")).toBe("dialog");
    expect(button().getAttribute("aria-expanded")).toBe("false");
  });

  it("opens a labelled dialog with the agent, model and effort fields, and puts focus inside", async () => {
    backend(mixed);
    await mountAgentPicker(host);
    button().click();
    expect(panel()).not.toBeNull();
    expect(panel()!.getAttribute("aria-label")).toBe("Agent, model and effort");
    expect(button().getAttribute("aria-expanded")).toBe("true");
    expect(Array.from(panel()!.querySelectorAll("label")).map((l) => l.querySelector(".tool-label")!.textContent)).toEqual([
      "Agent",
      "Model",
      "Effort",
    ]);
    expect(panel()!.contains(document.activeElement)).toBe(true);
  });

  it("closes on Escape and gives focus back to the button", async () => {
    backend(mixed);
    await mountAgentPicker(host);
    button().click();
    press("Escape");
    expect(panel()).toBeNull();
    expect(button().getAttribute("aria-expanded")).toBe("false");
    expect(document.activeElement).toBe(button());
  });

  it("closes on a click outside and stays open on a click inside", async () => {
    backend(mixed);
    await mountAgentPicker(host);
    button().click();
    panel()!.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    expect(panel()).not.toBeNull();
    document.body.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    expect(panel()).toBeNull();
  });

  it("closes when keyboard focus moves to something outside the picker", async () => {
    backend(mixed);
    await mountAgentPicker(host);
    const elsewhere = document.createElement("input");
    document.body.append(elsewhere);
    button().click();
    expect(panel()).not.toBeNull();
    elsewhere.focus();
    expect(panel()).toBeNull();
  });

  it("closes when the button is pressed again, and stops listening once closed", async () => {
    backend(mixed);
    await mountAgentPicker(host);
    button().click();
    button().click();
    expect(panel()).toBeNull();
    const elsewhere = document.createElement("input");
    document.body.append(elsewhere);
    elsewhere.focus();
    press("Escape");
    expect(document.activeElement).toBe(elsewhere);
  });
});

describe("listing the agents on this machine", () => {
  it("shows all three, selects the available one, and reports the others as absent", async () => {
    backend(mixed);
    await mountOpen();
    expect(options()).toEqual([
      { value: "claude", text: "Claude Code 2.1.319", disabled: false },
      { value: "qoder", text: "Qoder (not found)", disabled: true },
      { value: "agy", text: "Antigravity (found, not usable)", disabled: true },
    ]);
    expect(select().value).toBe("claude");
  });

  it("labels the version by its number alone when the CLI prints a suffix", async () => {
    const real: Choice = {
      ...mixed,
      drivers: [{ ...mixed.drivers[0], version: "2.1.319 (Claude Code)" }, ...mixed.drivers.slice(1)],
    };
    backend(real);
    await mountOpen();
    expect(options()[0].text).toBe("Claude Code 2.1.319");
  });

  it("explains an unusable CLI in one sentence and leaves a missing CLI to its row", async () => {
    backend(mixed);
    await mountOpen();
    const sentence = "Antigravity was found but cannot be used, because --version exited with status 1.";
    expect(note().textContent).toBe(sentence);
    const lines = Array.from(note().querySelectorAll(".tool-line"));
    expect(lines).toHaveLength(1);
    expect(lines[0].getAttribute("title")).toBe(sentence);
    expect(note().textContent).not.toContain("qoder CLI not found");
  });

  it("does not show a filesystem path when the unusable reason quotes one", async () => {
    const path = "/Users/secret/.local/bin/agy";
    const broken: Choice = {
      ...mixed,
      drivers: mixed.drivers.map((d) =>
        d.id === "agy"
          ? {
              ...d,
              reason: `agy CLI found but unusable: ${path} --version exited unsuccessfully: status 1`,
            }
          : d,
      ),
    };
    backend(broken);
    await mountOpen();
    const sentence =
      "Antigravity was found but cannot be used, because --version exited unsuccessfully: status 1.";
    expect(note().textContent).toBe(sentence);
    expect(note().textContent).not.toContain(path);
    expect(note().querySelector(".tool-line")?.getAttribute("title")).toBe(sentence);
  });

  it("says no agent is available and what to install, with nothing to choose", async () => {
    const none: Choice = {
      drivers: mixed.drivers.map((d) => ({ ...d, available: false, version: null, capabilities: null })),
      selected: null,
    };
    backend(none);
    await mountAgentPicker(host);
    expect(button().textContent).toBe("No agent available");
    button().click();
    expect(select().disabled).toBe(true);
    expect(options().every((o) => o.disabled)).toBe(true);
    expect(note().textContent).toContain("Install Claude Code, Qoder or Antigravity");
    expect(panel()!.querySelector(".model-select")!.closest("label")!.hidden).toBe(true);
    expect(panel()!.querySelector(".effort-select")!.closest("label")!.hidden).toBe(true);
  });

  it("is busy and cannot be opened while the first check runs", async () => {
    let release!: (value: Choice) => void;
    const pending = new Promise<Choice>((resolve) => (release = resolve));
    backend(mixed, { overrides: { list_agent_drivers: () => pending } });
    const mounted = mountAgentPicker(host);
    expect(button().disabled).toBe(true);
    expect(button().textContent).toBe("Checking agents\u2026");
    expect(host.querySelector(".agent-picker")!.getAttribute("aria-busy")).toBe("true");
    button().click();
    expect(panel()).toBeNull();
    release(mixed);
    await mounted;
    expect(button().disabled).toBe(false);
    expect(button().textContent).toBe("Claude \u00b7 CLI default");
    expect(host.querySelector(".agent-picker")!.hasAttribute("aria-busy")).toBe(false);
  });

  it("says the check failed instead of showing an empty picker", async () => {
    backend(mixed, {
      overrides: {
        list_agent_drivers: () => {
          throw new Error("the agent check did not finish: panicked");
        },
      },
    });
    await mountAgentPicker(host);
    expect(button().textContent).toBe("Agents unavailable");
    button().click();
    expect(select().disabled).toBe(true);
    expect(status().getAttribute("role")).toBe("alert");
    expect(status().textContent).toContain("Could not check agents: the agent check did not finish");
  });

  it("rejects a response with an unknown shape rather than showing it", async () => {
    backend(mixed, { overrides: { list_agent_drivers: () => ({ drivers: "claude" }) } });
    await mountOpen();
    expect(status().textContent).toContain("Could not check agents");
  });
});

describe("choosing an agent", () => {
  it("tells Rust and keeps the new choice, with a note when the agent emulates a feature", async () => {
    const rust = backend(withQoder());
    await mountOpen();
    choose("qoder");
    await vi.waitFor(() => expect(note().textContent).toContain("structured output is emulated"));
    expect(rust.calls.set).toEqual(["qoder"]);
    expect(select().value).toBe("qoder");
    expect(status().hidden).toBe(true);
    expect(button().textContent).toBe("Qoder \u00b7 CLI default");
  });

  it("puts the old choice back and says why when Rust refuses the switch", async () => {
    backend(withQoder(), {
      overrides: {
        set_selected_driver: () => {
          throw new Error("qoder is not available: qoder CLI not found on PATH");
        },
      },
    });
    await mountOpen();
    choose("qoder");
    await vi.waitFor(() => expect(status().hidden).toBe(false));
    expect(status().textContent).toContain("Could not switch agent: qoder is not available");
    expect(select().value).toBe("claude");
    expect(button().textContent).toBe("Claude \u00b7 CLI default");
  });
});

describe("choosing a model", () => {
  it("lists the models Rust offers for the agent in use, with the CLI default first", async () => {
    const rust = backend(mixed);
    await mountOpen();
    await vi.waitFor(() => expect(options(modelSelect()).map((o) => o.text)).toEqual([
      "CLI default",
      "fable",
      "opus",
      "sonnet",
      "Custom model",
    ]));
    expect(rust.calls.models).toEqual(["claude"]);
    expect(modelSelect().value).toBe("");
    expect(modelSelect().disabled).toBe(false);
  });

  it("offers nothing to pick while the list is still loading", async () => {
    let release!: (value: Catalog) => void;
    const pending = new Promise<Catalog>((resolve) => (release = resolve));
    backend(mixed, { overrides: { list_driver_models: () => pending } });
    await mountOpen();
    expect(options(modelSelect()).map((o) => o.text)).toEqual(["Loading models\u2026"]);
    expect(modelSelect().disabled).toBe(true);
    release(wire("claude", CLAUDE_MODELS));
    await vi.waitFor(() => expect(modelSelect().disabled).toBe(false));
  });

  it("saves a picked model at once and names it on the button", async () => {
    const rust = backend(mixed);
    await mountOpen();
    await vi.waitFor(() => expect(modelSelect().disabled).toBe(false));
    pick(modelSelect(), "opus");
    await vi.waitFor(() => expect(button().textContent).toBe("Claude \u00b7 opus"));
    expect(rust.calls.setModel).toEqual([{ id: "claude", model: "opus" }]);
    expect(modelSelect().value).toBe("opus");
    expect(status().hidden).toBe(true);
  });

  it("sends null when the user goes back to the CLI default", async () => {
    const rust = backend(mixed, { tuning: { claude: { model: "opus", effort: "high" } } });
    await mountOpen();
    await vi.waitFor(() => expect(modelSelect().value).toBe("opus"));
    pick(modelSelect(), "");
    await vi.waitFor(() => expect(button().textContent).toBe("Claude \u00b7 default model \u00b7 high"));
    expect(rust.calls.setModel).toEqual([{ id: "claude", model: null }]);
  });

  it("keeps a saved model visible when the CLI no longer lists it", async () => {
    backend({ ...withAgy(), selected: "agy" }, { tuning: { agy: { model: "retired", effort: null } } });
    await mountOpen();
    await vi.waitFor(() => expect(modelSelect().disabled).toBe(false));
    expect(modelSelect().value).toBe("retired");
    expect(options(modelSelect()).at(-1)).toEqual({ value: "retired", text: "retired (not listed)", disabled: false });
  });

  it("saves a typed Claude model the list does not name", async () => {
    const rust = backend(mixed);
    await mountOpen();
    await vi.waitFor(() => expect(modelSelect().disabled).toBe(false));
    pick(modelSelect(), "*");
    const input = customInput();
    expect(input.hidden).toBe(false);
    input.value = "claude-opus-4-6";
    input.dispatchEvent(new Event("change", { bubbles: true }));
    await vi.waitFor(() => expect(button().textContent).toBe("Claude \u00b7 claude-opus-4-6"));
    expect(rust.calls.setModel).toEqual([{ id: "claude", model: "claude-opus-4-6" }]);
    expect(modelSelect().value).toBe("*");
    expect(input.value).toBe("claude-opus-4-6");
  });

  it("does not save a custom model that would be a flag", async () => {
    const rust = backend(mixed);
    await mountOpen();
    await vi.waitFor(() => expect(modelSelect().disabled).toBe(false));
    pick(modelSelect(), "*");
    customInput().value = "--opus";
    customInput().dispatchEvent(new Event("change", { bubbles: true }));
    await vi.waitFor(() => expect(status().textContent).toContain("must not start with '-'"));
    expect(rust.calls.setModel).toEqual([]);
  });

  it("says so when the list cannot be loaded, and leaves the model alone", async () => {
    backend(mixed, {
      tuning: { claude: { model: "opus", effort: null } },
      overrides: {
        list_driver_models: () => {
          throw new Error("agy models did not answer");
        },
      },
    });
    await mountOpen();
    await vi.waitFor(() => expect(status().hidden).toBe(false));
    expect(status().textContent).toBe("Could not load the model list: agy models did not answer");
    expect(modelSelect().disabled).toBe(true);
    expect(modelSelect().value).toBe("opus");
  });
});

describe("choosing an effort", () => {
  const flash = option("gemini-3.8-flash", "Gemini 3.8 Flash", ["low", "medium", "high"]);
  const oss = option("gpt-oss-120b", "GPT-OSS 120B", ["medium"]);
  const varied = [option(null, "CLI default", LEVELS), flash, oss];

  it("offers the CLI default and the selected model's efforts, and shows the saved one", async () => {
    backend(mixed, { tuning: { claude: { model: null, effort: "medium" } } });
    await mountOpen();
    await vi.waitFor(() => expect(effortSelect().disabled).toBe(false));
    expect(options(effortSelect()).map((o) => o.text)).toEqual([
      "CLI default",
      "low",
      "medium",
      "high",
      "xhigh",
      "max",
    ]);
    expect(effortSelect().value).toBe("medium");
  });

  it("saves a picked effort at once and names it on the button", async () => {
    const rust = backend(mixed);
    await mountOpen();
    await vi.waitFor(() => expect(effortSelect().disabled).toBe(false));
    pick(effortSelect(), "xhigh");
    await vi.waitFor(() => expect(button().textContent).toBe("Claude \u00b7 default model \u00b7 xhigh"));
    expect(rust.calls.setEffort).toEqual([{ id: "claude", effort: "xhigh" }]);
    expect(effortSelect().value).toBe("xhigh");
  });

  it("sends null when the user goes back to the CLI default", async () => {
    const rust = backend(mixed, { tuning: { claude: { model: "opus", effort: "high" } } });
    await mountOpen();
    await vi.waitFor(() => expect(effortSelect().value).toBe("high"));
    pick(effortSelect(), "");
    await vi.waitFor(() => expect(button().textContent).toBe("Claude \u00b7 opus"));
    expect(rust.calls.setEffort).toEqual([{ id: "claude", effort: null }]);
  });

  it("keeps the model when the effort changes, as the saved choice Rust returns", async () => {
    backend(mixed, { tuning: { claude: { model: "sonnet", effort: null } } });
    await mountOpen();
    await vi.waitFor(() => expect(effortSelect().disabled).toBe(false));
    pick(effortSelect(), "low");
    await vi.waitFor(() => expect(button().textContent).toBe("Claude \u00b7 sonnet \u00b7 low"));
    expect(modelSelect().value).toBe("sonnet");
  });

  it("shows only the selected model's efforts, and drops one the new model lacks", async () => {
    const rust = backend(
      { ...withAgy(), selected: "agy" },
      { tuning: { agy: { model: "gemini-3.8-flash", effort: "high" } }, models: { agy: varied } },
    );
    await mountOpen();
    await vi.waitFor(() => expect(options(effortSelect()).map((o) => o.text)).toEqual([
      "CLI default",
      "low",
      "medium",
      "high",
    ]));
    expect(effortSelect().value).toBe("high");
    pick(modelSelect(), "gpt-oss-120b");
    await vi.waitFor(() => expect(options(effortSelect()).map((o) => o.text)).toEqual(["CLI default", "medium"]));
    expect(effortSelect().value).toBe("");
    expect(rust.calls.setModel).toEqual([{ id: "agy", model: "gpt-oss-120b" }]);
    expect(rust.calls.setEffort).toEqual([{ id: "agy", effort: null }]);
    expect(button().textContent).toBe("Antigravity \u00b7 gpt-oss-120b");
  });

  it("clears a saved effort the loaded model does not offer", async () => {
    const rust = backend(
      { ...withAgy(), selected: "agy" },
      { tuning: { agy: { model: "gpt-oss-120b", effort: "high" } }, models: { agy: varied } },
    );
    await mountOpen();
    await vi.waitFor(() => expect(rust.calls.setEffort).toEqual([{ id: "agy", effort: null }]));
    expect(effortSelect().value).toBe("");
    expect(options(effortSelect()).map((o) => o.text)).toEqual(["CLI default", "medium"]);
    expect(button().textContent).toBe("Antigravity \u00b7 gpt-oss-120b");
  });
});

describe("a CLI that does not take a model or an effort", () => {
  it("disables that field and says why, without touching the other", async () => {
    const qoder = driver("qoder", {
      version: "1.1.85",
      capabilities: { ...mixed.drivers[0].capabilities, effort: false },
    });
    const noEffort: Choice = { drivers: [mixed.drivers[0], qoder, mixed.drivers[2]], selected: "qoder" };
    backend(noEffort);
    await mountOpen();
    await vi.waitFor(() => expect(modelSelect().disabled).toBe(false));
    expect(effortSelect().disabled).toBe(true);
    expect(note().textContent).toContain("Qoder does not take an effort.");
    expect(note().textContent).not.toContain("does not take a model");
    expect(options(modelSelect()).map((o) => o.text)).toEqual(["CLI default"]);
  });
});

describe("switching agent in the panel", () => {
  it("loads the new agent's model list and saved choice", async () => {
    const rust = backend(withAgy(), {
      tuning: { agy: { model: "gemini-4-argon", effort: "high" } },
    });
    await mountOpen();
    await vi.waitFor(() => expect(modelSelect().disabled).toBe(false));
    choose("agy");
    await vi.waitFor(() => expect(button().textContent).toBe("Antigravity \u00b7 gemini-4-argon \u00b7 high"));
    expect(rust.calls.models).toEqual(["claude", "agy"]);
    expect(rust.calls.tuning).toEqual(["claude", "agy"]);
    expect(options(modelSelect()).map((o) => o.text)).toEqual(["CLI default", "Gemini 4 Argon", "Claude Opus 5.5"]);
    expect(modelSelect().value).toBe("gemini-4-argon");
    expect(effortSelect().value).toBe("high");
  });

  it("locks the model and effort while the new agent's choice is being read", async () => {
    let release!: (value: Tuning) => void;
    const pending = new Promise<Tuning>((resolve) => (release = resolve));
    backend(withAgy(), {
      tuning: { claude: { model: "opus", effort: "max" } },
      overrides: {
        driver_tuning: (args) => (args.id === "agy" ? pending : { model: "opus", effort: "max" }),
      },
    });
    await mountOpen();
    await vi.waitFor(() => expect(effortSelect().value).toBe("max"));
    choose("agy");
    await vi.waitFor(() => expect(button().textContent).toBe("Antigravity"));
    expect(modelSelect().disabled).toBe(true);
    expect(effortSelect().disabled).toBe(true);
    expect(effortSelect().value).toBe("");
    release({ model: null, effort: null });
    await vi.waitFor(() => expect(button().textContent).toBe("Antigravity \u00b7 CLI default"));
    expect(effortSelect().disabled).toBe(false);
  });

  it("ignores a slow answer for the agent the user already left", async () => {
    let release!: (value: Catalog) => void;
    const slow = new Promise<Catalog>((resolve) => (release = resolve));
    backend(withAgy(), {
      overrides: {
        list_driver_models: (args) => (args.id === "claude" ? slow : wire("agy", AGY_MODELS)),
        driver_tuning: () => NO_TUNING,
      },
    });
    await mountOpen();
    choose("agy");
    await vi.waitFor(() => expect(options(modelSelect()).map((o) => o.text)).toContain("Gemini 4 Argon"));
    release(wire("claude", CLAUDE_MODELS));
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(options(modelSelect()).map((o) => o.text)).toEqual(["CLI default", "Gemini 4 Argon", "Claude Opus 5.5"]);
  });

  it("asks each agent for its model list once, not on every visit", async () => {
    const rust = backend(withAgy());
    await mountOpen();
    await vi.waitFor(() => expect(modelSelect().disabled).toBe(false));
    choose("agy");
    await vi.waitFor(() => expect(button().textContent).toBe("Antigravity \u00b7 CLI default"));
    choose("claude");
    await vi.waitFor(() => expect(button().textContent).toBe("Claude \u00b7 CLI default"));
    choose("agy");
    await vi.waitFor(() => expect(button().textContent).toBe("Antigravity \u00b7 CLI default"));
    expect(rust.calls.models).toEqual(["claude", "agy"]);
  });

  it("does not let a save for the old agent change the new one", async () => {
    let release!: (value: Tuning) => void;
    const pending = new Promise<Tuning>((resolve) => (release = resolve));
    backend(withAgy(), { overrides: { set_driver_model: () => pending } });
    await mountOpen();
    await vi.waitFor(() => expect(modelSelect().disabled).toBe(false));
    pick(modelSelect(), "opus");
    choose("agy");
    await vi.waitFor(() => expect(button().textContent).toBe("Antigravity \u00b7 CLI default"));
    release({ model: "opus", effort: null });
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(button().textContent).toBe("Antigravity \u00b7 CLI default");
    expect(modelSelect().value).toBe("");
  });
});

describe("when saving fails", () => {
  it("shows the reason as a plain sentence and puts the model back", async () => {
    const rust = backend(mixed, {
      tuning: { claude: { model: "opus", effort: "high" } },
      overrides: {
        set_driver_model: () => {
          throw new Error("model must not start with '-'");
        },
      },
    });
    await mountOpen();
    await vi.waitFor(() => expect(modelSelect().value).toBe("opus"));
    pick(modelSelect(), "sonnet");
    await vi.waitFor(() => expect(status().hidden).toBe(false));
    expect(status().getAttribute("role")).toBe("alert");
    expect(status().textContent).toBe("Could not save the model: model must not start with '-'");
    expect(modelSelect().value).toBe("opus");
    expect(button().textContent).toBe("Claude \u00b7 opus \u00b7 high");
    expect(rust.calls.setModel).toEqual([]);
  });

  it("shows the reason for an effort that is refused, and clears it after the next save works", async () => {
    let refuse = true;
    backend(mixed, {
      overrides: {
        set_driver_effort: (args) => {
          if (refuse) throw new Error("effort must be low, medium, high, xhigh or max");
          return { model: null, effort: args.effort };
        },
      },
    });
    await mountOpen();
    await vi.waitFor(() => expect(effortSelect().disabled).toBe(false));
    pick(effortSelect(), "max");
    await vi.waitFor(() => expect(status().hidden).toBe(false));
    expect(status().textContent).toBe("Could not save the effort: effort must be low, medium, high, xhigh or max");
    expect(effortSelect().value).toBe("");
    refuse = false;
    pick(effortSelect(), "max");
    await vi.waitFor(() => expect(status().hidden).toBe(true));
    expect(button().textContent).toBe("Claude \u00b7 default model \u00b7 max");
  });

  it("forgets the sentence when the panel is closed and opened again", async () => {
    backend(mixed, {
      overrides: {
        set_driver_model: () => {
          throw new Error("disk full");
        },
      },
    });
    await mountOpen();
    await vi.waitFor(() => expect(modelSelect().disabled).toBe(false));
    pick(modelSelect(), "opus");
    await vi.waitFor(() => expect(status().hidden).toBe(false));
    press("Escape");
    button().click();
    expect(status().hidden).toBe(true);
  });

  it("names only the agent on the button, and says why in the panel, when the saved choice cannot be read", async () => {
    backend(mixed, {
      overrides: {
        driver_tuning: () => {
          throw new Error("agent.json is unreadable");
        },
      },
    });
    await mountAgentPicker(host);
    expect(button().textContent).toBe("Claude");
    button().click();
    await vi.waitFor(() => expect(status().hidden).toBe(false));
    expect(status().textContent).toBe("Could not read the saved model and effort: agent.json is unreadable");
    expect(modelSelect().disabled).toBe(true);
    expect(effortSelect().disabled).toBe(true);
  });
});

describe("noticing a CLI installed or updated while the app is open", () => {
  it("rechecks when the window regains focus and lists the newly installed agent", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    const rust = backend(mixed);
    await mountOpen();
    rust.set(withQoder());
    vi.setSystemTime(Date.now() + 60_000);
    window.dispatchEvent(new Event("focus"));
    await vi.waitFor(() => expect(options()[1]).toEqual({ value: "qoder", text: "Qoder 1.1.85", disabled: false }));
    expect(rust.calls.refresh).toBe(1);
  });

  it("does not recheck again within a few seconds of the last check", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    const rust = backend(mixed);
    await mountAgentPicker(host);
    vi.setSystemTime(Date.now() + 2_000);
    window.dispatchEvent(new Event("focus"));
    await Promise.resolve();
    expect(rust.calls.refresh).toBe(0);
  });

  it("leaves the open menu alone when nothing changed", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    backend(mixed);
    await mountOpen();
    const before = select().options[0];
    vi.setSystemTime(Date.now() + 60_000);
    window.dispatchEvent(new Event("focus"));
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(select().options[0]).toBe(before);
  });
});
