// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";
import { readCollapsed, writeCollapsed, type PrefStorage } from "./study-prefs";

function memory(): PrefStorage & { held: Map<string, string> } {
  const held = new Map<string, string>();
  return { held, getItem: (k) => held.get(k) ?? null, setItem: (k, v) => void held.set(k, v) };
}

describe("remembering whether the panel is collapsed", () => {
  it("reads back what was written", () => {
    const store = memory();
    expect(readCollapsed(store)).toBe(false);
    writeCollapsed(store, true);
    expect(readCollapsed(store)).toBe(true);
    writeCollapsed(store, false);
    expect(readCollapsed(store)).toBe(false);
  });

  it("treats an unknown stored value as open", () => {
    const store = memory();
    store.setItem("tutordocs.studyPanel.v1", "sideways");
    expect(readCollapsed(store)).toBe(false);
  });

  it("works with no storage at all", () => {
    expect(readCollapsed(null)).toBe(false);
    expect(() => writeCollapsed(null, true)).not.toThrow();
  });

  it("survives a store whose reads and writes both throw", () => {
    const blocked: PrefStorage = {
      getItem: () => {
        throw new Error("SecurityError");
      },
      setItem: () => {
        throw new Error("QuotaExceededError");
      },
    };
    expect(readCollapsed(blocked)).toBe(false);
    expect(() => writeCollapsed(blocked, true)).not.toThrow();
  });
});
