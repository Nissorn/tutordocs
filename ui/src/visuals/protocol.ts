/**
 * The host side of the visual postMessage protocol, as a pure state machine.
 * Every message carries `tutordoc: <version>`. The host sends `init`; the
 * artifact answers `ready`, then may send `resize`, `quiz.answer`, `progress`,
 * `select` and `error`. Messages are data, never instructions.
 */

export const PROTOCOL_VERSION = 1;
export const READY_TIMEOUT_MS = 3000;
const MIN_HEIGHT = 80;
const MAX_HEIGHT = 1600;

export type ArtifactMessage =
  | { type: "ready"; visualId: string; height?: number }
  | { type: "resize"; height: number }
  | { type: "quiz.answer"; itemId: string; choice: string | number; correct: boolean }
  | { type: "progress"; step: number; total: number; completed: boolean }
  | { type: "select"; part: string; text?: string }
  | { type: "error"; message: string };

/** What the learner did, forwarded to Rust. */
export type LearnerMessage = Extract<ArtifactMessage, { type: "quiz.answer" | "progress" | "select" | "error" }>;

export type HostEvent = { kind: "loaded" } | { kind: "message"; data: unknown } | { kind: "timeout" };

/** `fallback` is terminal: once the text version shows, the visual never returns. */
export type Phase = "starting" | "live" | "fallback";

export interface Session {
  readonly visualId: string;
  readonly estimatedHeight: number;
  readonly phase: Phase;
  readonly initSent: boolean;
}

export type Effect =
  | { kind: "send-init" }
  | { kind: "show"; height: number }
  | { kind: "resize"; height: number }
  | { kind: "forward"; message: LearnerMessage }
  | { kind: "fallback"; reason: string };

export function start(visualId: string, estimatedHeight: number): Session {
  return { visualId, estimatedHeight, phase: "starting", initSent: false };
}

type Parsed =
  | { kind: "ignore" }
  | { kind: "version"; version: number }
  | { kind: "ok"; message: ArtifactMessage };

const isNumber = (v: unknown): v is number => typeof v === "number" && Number.isFinite(v);
const isString = (v: unknown): v is string => typeof v === "string";

/** Reads one message. Anything that is not a well-formed protocol message is ignored. */
function parse(data: unknown): Parsed {
  if (typeof data !== "object" || data === null) return { kind: "ignore" };
  const m = data as Record<string, unknown>;
  if (!isNumber(m.tutordoc)) return { kind: "ignore" };
  if (m.tutordoc !== PROTOCOL_VERSION) return { kind: "version", version: m.tutordoc };
  const ok = (message: ArtifactMessage): Parsed => ({ kind: "ok", message });
  switch (m.type) {
    case "ready":
      if (!isString(m.visualId)) break;
      return ok(isNumber(m.height) ? { type: "ready", visualId: m.visualId, height: m.height } : { type: "ready", visualId: m.visualId });
    case "resize":
      if (isNumber(m.height)) return ok({ type: "resize", height: m.height });
      break;
    case "quiz.answer":
      if (isString(m.itemId) && (isString(m.choice) || isNumber(m.choice)) && typeof m.correct === "boolean") {
        return ok({ type: "quiz.answer", itemId: m.itemId, choice: m.choice, correct: m.correct });
      }
      break;
    case "progress":
      if (isNumber(m.step) && isNumber(m.total) && typeof m.completed === "boolean") {
        return ok({ type: "progress", step: m.step, total: m.total, completed: m.completed });
      }
      break;
    case "select":
      if (!isString(m.part)) break;
      return ok(isString(m.text) ? { type: "select", part: m.part, text: m.text } : { type: "select", part: m.part });
    case "error":
      return ok({ type: "error", message: isString(m.message) ? m.message : "an unknown error" });
  }
  return { kind: "ignore" };
}

const clamp = (height: number) => Math.round(Math.min(MAX_HEIGHT, Math.max(MIN_HEIGHT, height)));

export function step(session: Session, event: HostEvent): { session: Session; effects: Effect[] } {
  const same = { session, effects: [] };
  const fallback = (reason: string, effects: Effect[] = []) => ({
    session: { ...session, phase: "fallback" as const },
    effects: [...effects, { kind: "fallback" as const, reason }],
  });
  if (session.phase === "fallback") return same;

  if (event.kind === "loaded") {
    if (session.initSent || session.phase !== "starting") return same;
    return { session: { ...session, initSent: true }, effects: [{ kind: "send-init" }] };
  }
  if (event.kind === "timeout") {
    if (session.phase !== "starting") return same;
    return fallback(`it did not report ready within ${READY_TIMEOUT_MS / 1000} seconds`);
  }

  const parsed = parse(event.data);
  if (parsed.kind === "ignore") return same;
  if (parsed.kind === "version") {
    return fallback(`it speaks protocol version ${parsed.version}; the reader speaks ${PROTOCOL_VERSION}`);
  }
  const message = parsed.message;
  switch (message.type) {
    case "error":
      return fallback(`it reported an error: ${message.message}`, [{ kind: "forward", message }]);
    case "ready":
      if (session.phase !== "starting") return same;
      if (message.visualId !== session.visualId) return fallback(`it reported ready as "${message.visualId}"`);
      return {
        session: { ...session, phase: "live" },
        effects: [{ kind: "show", height: clamp(message.height ?? session.estimatedHeight) }],
      };
    case "resize":
      return session.phase === "live" ? { session, effects: [{ kind: "resize", height: clamp(message.height) }] } : same;
    default:
      return session.phase === "live" ? { session, effects: [{ kind: "forward", message }] } : same;
  }
}
