import { describe, expect, it } from "vitest";
import { start, step, type Effect, type HostEvent, type Session } from "./protocol";

/** Replays a recorded sequence and returns the final session and every effect. */
function replay(events: HostEvent[], session: Session = start("cube", 360)) {
  const effects: Effect[] = [];
  for (const event of events) {
    const next = step(session, event);
    session = next.session;
    effects.push(...next.effects);
  }
  return { session, effects };
}

const msg = (data: unknown): HostEvent => ({ kind: "message", data });

describe("visual protocol", () => {
  it("sends init on load, goes live on ready and forwards learner events", () => {
    const { session, effects } = replay([
      { kind: "loaded" },
      msg({ tutordoc: 1, type: "ready", visualId: "cube", height: 300 }),
      msg({ tutordoc: 1, type: "quiz.answer", itemId: "q1", choice: 2, correct: true }),
      msg({ tutordoc: 1, type: "progress", step: 1, total: 3, completed: false }),
      msg({ tutordoc: 1, type: "select", part: "edge", text: "an edge" }),
      msg({ tutordoc: 1, type: "resize", height: 500 }),
    ]);
    expect(session.phase).toBe("live");
    expect(effects).toEqual([
      { kind: "send-init" },
      { kind: "show", height: 300 },
      { kind: "forward", message: { type: "quiz.answer", itemId: "q1", choice: 2, correct: true } },
      { kind: "forward", message: { type: "progress", step: 1, total: 3, completed: false } },
      { kind: "forward", message: { type: "select", part: "edge", text: "an edge" } },
      { kind: "resize", height: 500 },
    ]);
  });

  it("falls back when ready never arrives, and a late ready does not undo it", () => {
    const { session, effects } = replay([
      { kind: "loaded" },
      msg({ tutordoc: 1, type: "progress", step: 1, total: 3, completed: false }),
      { kind: "timeout" },
      msg({ tutordoc: 1, type: "ready", visualId: "cube" }),
    ]);
    expect(session.phase).toBe("fallback");
    expect(effects).toEqual([
      { kind: "send-init" },
      { kind: "fallback", reason: "it did not report ready within 3 seconds" },
    ]);
  });

  it("uses the manifest height when ready gives none, clamped to a sane range", () => {
    expect(replay([msg({ tutordoc: 1, type: "ready", visualId: "cube" })]).effects).toEqual([
      { kind: "show", height: 360 },
    ]);
    const tall = replay([
      msg({ tutordoc: 1, type: "ready", visualId: "cube", height: 90000 }),
      msg({ tutordoc: 1, type: "resize", height: 1 }),
    ]);
    expect(tall.effects).toEqual([
      { kind: "show", height: 1600 },
      { kind: "resize", height: 80 },
    ]);
  });

  it("falls back on an error before ready and forwards the error", () => {
    const { session, effects } = replay([
      { kind: "loaded" },
      msg({ tutordoc: 1, type: "error", visualId: "cube", message: "TypeError: x is null" }),
    ]);
    expect(session.phase).toBe("fallback");
    expect(effects).toEqual([
      { kind: "send-init" },
      { kind: "forward", message: { type: "error", message: "TypeError: x is null" } },
      { kind: "fallback", reason: "it reported an error: TypeError: x is null" },
    ]);
  });

  it("falls back on an error after it went live", () => {
    const { session, effects } = replay([
      msg({ tutordoc: 1, type: "ready", visualId: "cube" }),
      msg({ tutordoc: 1, type: "error", message: "lost the WebGL context" }),
      msg({ tutordoc: 1, type: "select", part: "edge" }),
    ]);
    expect(session.phase).toBe("fallback");
    expect(effects.at(-1)).toEqual({ kind: "fallback", reason: "it reported an error: lost the WebGL context" });
  });

  it("falls back when ready names another visual or another protocol version", () => {
    expect(replay([msg({ tutordoc: 1, type: "ready", visualId: "sphere" })]).effects).toEqual([
      { kind: "fallback", reason: 'it reported ready as "sphere"' },
    ]);
    expect(replay([msg({ tutordoc: 2, type: "ready", visualId: "cube" })]).effects).toEqual([
      { kind: "fallback", reason: "it speaks protocol version 2; the reader speaks 1" },
    ]);
  });

  it("ignores messages that are not protocol messages or are malformed", () => {
    const { session, effects } = replay([
      msg("hello"),
      msg({ type: "ready", visualId: "cube" }),
      msg({ tutordoc: 1, type: "ready", visualId: "cube" }),
      msg({ tutordoc: 1, type: "quiz.answer", itemId: 3 }),
      msg({ tutordoc: 1, type: "launch-missiles" }),
    ]);
    expect(session.phase).toBe("live");
    expect(effects).toEqual([{ kind: "show", height: 360 }]);
  });
});
