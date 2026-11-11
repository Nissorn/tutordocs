import { message } from "./dom";
import type { Answer, ChapterContext } from "./study-api";

// What the study panel knows, as data. The DOM in study-panel.ts and
// selection-popup.ts only draws these values, so no state lives in a flag.

/** One part of the panel. A list section is never `ready` with nothing in it: that is `empty`. */
export type Section<T> =
  | { state: "loading" }
  | { state: "ready"; value: T }
  | { state: "empty" }
  | { state: "error"; message: string };

export interface Goals {
  summary: string;
  objectives: string[];
}

/** A source of the chapter. `opens` is true for a PDF in the library, the one kind the reader can show. */
export interface SourceChip {
  name: string;
  opens: boolean;
}

export interface StudyModel {
  goals: Section<Goals>;
  sources: Section<SourceChip[]>;
  /** How many marks the chapter holds. The cards themselves belong to annotations.ts. */
  notes: Section<number>;
  /** Newest first. */
  answers: Section<Answer[]>;
}

export type StudyEvent =
  | { type: "context"; context: ChapterContext; readable: readonly string[] }
  | { type: "context-loading" }
  | { type: "context-failed"; error: unknown }
  | { type: "answers-loading" }
  | { type: "answers"; answers: Answer[] }
  | { type: "answers-failed"; error: unknown }
  | { type: "answer"; answer: Answer }
  | { type: "notes"; count: number }
  | { type: "notes-failed"; error: unknown };

export function emptyStudy(): StudyModel {
  return {
    goals: { state: "loading" },
    sources: { state: "loading" },
    notes: { state: "loading" },
    answers: { state: "loading" },
  };
}

function listed<T>(items: T[]): Section<T[]> {
  return items.length === 0 ? { state: "empty" } : { state: "ready", value: items };
}

export function studyReduce(model: StudyModel, event: StudyEvent): StudyModel {
  switch (event.type) {
    case "context": {
      const { summary, objectives, sources } = event.context;
      const goals: Section<Goals> =
        summary.trim() === "" && objectives.length === 0
          ? { state: "empty" }
          : { state: "ready", value: { summary, objectives } };
      return {
        ...model,
        goals,
        sources: listed(sources.map((name) => ({ name, opens: event.readable.includes(name) }))),
      };
    }
    case "context-loading":
      return { ...model, goals: { state: "loading" }, sources: { state: "loading" } };
    case "answers-loading":
      return { ...model, answers: { state: "loading" } };
    case "context-failed": {
      const failed = { state: "error", message: message(event.error) } as const;
      return { ...model, goals: failed, sources: failed };
    }
    case "answers":
      return { ...model, answers: listed(event.answers) };
    case "answers-failed":
      return { ...model, answers: { state: "error", message: message(event.error) } };
    case "answer": {
      const before = model.answers.state === "ready" ? model.answers.value : [];
      return { ...model, answers: { state: "ready", value: [event.answer, ...before.filter((a) => a.id !== event.answer.id)] } };
    }
    case "notes":
      return { ...model, notes: event.count === 0 ? { state: "empty" } : { state: "ready", value: event.count } };
    case "notes-failed":
      return { ...model, notes: { state: "error", message: message(event.error) } };
  }
}

/**
 * One fix or one ask. `editing` holds the draft, `running` the text that was
 * sent, `failed` the same text with why it failed, and `done` what came back.
 * A running job cannot be closed from here: it keeps running in Rust.
 */
export type Flow<T> =
  | { phase: "idle" }
  | { phase: "editing"; text: string }
  | { phase: "running"; text: string }
  | { phase: "done"; result: T }
  | { phase: "failed"; text: string; error: string };

export type FlowEvent<T> =
  | { type: "open" }
  | { type: "edit"; text: string }
  | { type: "submit" }
  | { type: "succeed"; result: T }
  | { type: "fail"; error: string }
  | { type: "close" };

/** Returns `from` itself when the event does nothing in that phase. */
export function flow<T>(from: Flow<T>, event: FlowEvent<T>): Flow<T> {
  switch (from.phase) {
    case "idle":
      if (event.type === "open") return { phase: "editing", text: "" };
      if (event.type === "edit") return { phase: "editing", text: event.text };
      return from;
    case "editing":
      if (event.type === "edit") return { phase: "editing", text: event.text };
      if (event.type === "submit") return from.text.trim() === "" ? from : { phase: "running", text: from.text.trim() };
      if (event.type === "close") return { phase: "idle" };
      return from;
    case "running":
      if (event.type === "succeed") return { phase: "done", result: event.result };
      if (event.type === "fail") return { phase: "failed", text: from.text, error: event.error };
      return from;
    case "failed":
      if (event.type === "edit") return { phase: "editing", text: event.text };
      if (event.type === "submit") return { phase: "running", text: from.text };
      if (event.type === "close") return { phase: "idle" };
      return from;
    case "done":
      if (event.type === "open") return { phase: "editing", text: "" };
      if (event.type === "edit") return { phase: "editing", text: event.text };
      if (event.type === "close") return { phase: "idle" };
      return from;
  }
}

export interface FlowController<T> {
  readonly state: Flow<T>;
  /** The user's events: open, edit, submit and close. A submit that starts a run also calls the job. */
  send(event: FlowEvent<T>): void;
}

/**
 * Drives one flow and the job behind it, so the panel's ask box and the
 * selection popup share the same rules. `onChange` runs after every change of state.
 */
export function flowController<T>(
  run: (text: string) => Promise<T>,
  onChange: (next: Flow<T>, previous: Flow<T>) => void,
): FlowController<T> {
  let state: Flow<T> = { phase: "idle" };

  function apply(event: FlowEvent<T>): void {
    const previous = state;
    state = flow(state, event);
    if (state !== previous) onChange(state, previous);
  }

  return {
    get state() {
      return state;
    },
    send(event) {
      const previous = state;
      apply(event);
      if (event.type === "submit" && state !== previous && state.phase === "running") {
        run(state.text).then(
          (result) => apply({ type: "succeed", result }),
          (error: unknown) => apply({ type: "fail", error: message(error) }),
        );
      }
    },
  };
}
