import { invoke } from "@tauri-apps/api/core";
import { bool, list, record, str, strings, strOrNull } from "./api";

// The typed bridge to the agent commands in src-tauri/src/agents.rs. Rust sends
// the detection record as the agent crate serializes it, so its keys are
// snake_case; this module is the only place that knows.

export interface AgentDriver {
  id: string;
  available: boolean;
  /** Where the CLI was found. Null when it was not found at all. */
  binary: string | null;
  version: string | null;
  /** Present only for an available driver. */
  emulates: { structuredOutput: boolean; systemAppend: boolean } | null;
  /**
   * Whether the CLI's `--help` lists `--model` and `--effort`. A choice for a
   * flag it does not list is never passed to a job. Present only for an
   * available driver.
   */
  accepts: { model: boolean; effort: boolean } | null;
  /** Why the driver is not available. */
  reason: string | null;
}

export interface AgentChoice {
  drivers: AgentDriver[];
  /** The driver jobs run on. Null when none is available. */
  selected: string | null;
}

export async function listAgentDrivers(): Promise<AgentChoice> {
  return parseChoice(await invoke<unknown>("list_agent_drivers"), "list_agent_drivers");
}

export async function refreshAgentDrivers(): Promise<AgentChoice> {
  return parseChoice(await invoke<unknown>("refresh_agent_drivers"), "refresh_agent_drivers");
}

export async function setSelectedDriver(id: string): Promise<AgentChoice> {
  return parseChoice(await invoke<unknown>("set_selected_driver", { id }), "set_selected_driver");
}

function parseChoice(value: unknown, what: string): AgentChoice {
  const r = record(value, what);
  return { drivers: list(r.drivers, `${what} drivers`).map(parseDriver), selected: strOrNull(r, "selected") };
}

/** A null id is the CLI's own default, which passes no `--model`. */
export interface ModelOption {
  id: string | null;
  label: string;
  /** Empty when the CLI did not say which levels this model accepts. */
  efforts: string[];
}

/**
 * One driver's reported choices. `efforts` is the driver-wide list from
 * `--help`. `acceptsCustomModel` is true when the user may type a full name.
 */
export interface DriverCatalog {
  models: ModelOption[];
  efforts: string[];
  acceptsCustomModel: boolean;
}

/** A lowercase word the CLI reported. Null means the CLI keeps its own default. */
export type Effort = string;

/** Null model or effort means the CLI keeps its own default. */
export interface DriverTuning {
  model: string | null;
  effort: Effort | null;
}

export async function listDriverModels(id: string): Promise<DriverCatalog> {
  const raw = await invoke<unknown>("list_driver_models", { id });
  const r = record(raw, "list_driver_models");
  return {
    models: list(r.models, "list_driver_models models").map(parseModelOption),
    efforts: strings(r, "efforts"),
    acceptsCustomModel: bool(r, "accepts_custom_model"),
  };
}

export async function driverTuning(id: string): Promise<DriverTuning> {
  return parseTuning(await invoke<unknown>("driver_tuning", { id }), "driver_tuning");
}

export async function setDriverModel(id: string, model: string | null): Promise<DriverTuning> {
  return parseTuning(await invoke<unknown>("set_driver_model", { id, model }), "set_driver_model");
}

export async function setDriverEffort(id: string, effort: Effort | null): Promise<DriverTuning> {
  return parseTuning(await invoke<unknown>("set_driver_effort", { id, effort }), "set_driver_effort");
}

function parseModelOption(value: unknown): ModelOption {
  const r = record(value, "model option");
  return { id: strOrNull(r, "id"), label: str(r, "label"), efforts: strings(r, "efforts") };
}

function parseTuning(value: unknown, what: string): DriverTuning {
  const r = record(value, what);
  return { model: strOrNull(r, "model"), effort: effortOrNull(r.effort) };
}

function effortOrNull(value: unknown): Effort | null {
  if (value === null) return null;
  if (typeof value === "string" && /^[a-z]+$/.test(value)) return value;
  throw new Error('expected "effort" to be lowercase letters or null');
}

function parseDriver(value: unknown): AgentDriver {
  const r = record(value, "agent driver");
  const caps = r.capabilities === null ? null : record(r.capabilities, "agent capabilities");
  return {
    id: str(r, "id"),
    available: bool(r, "available"),
    binary: strOrNull(r, "binary"),
    version: strOrNull(r, "version"),
    emulates: caps && {
      structuredOutput: !bool(caps, "structured_output"),
      systemAppend: !bool(caps, "system_append"),
    },
    accepts: caps && { model: caps.model === true, effort: caps.effort === true },
    reason: strOrNull(r, "reason"),
  };
}
