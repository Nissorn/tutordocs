import { describe, expect, it } from "vitest";
import { BOUNDS, MIN_DOCUMENT, clampWidth, defaultWidth, parseTracks, widthRange } from "./panel-width";

describe("panel widths", () => {
  it("defaults the sidebar to 15rem and grows the study panel with the window between 16rem and 22rem", () => {
    expect(defaultWidth("left", 1400)).toBe(240);
    expect(defaultWidth("right", 1000)).toBe(256);
    expect(defaultWidth("right", 1200)).toBe(264);
    expect(defaultWidth("right", 2400)).toBe(352);
  });

  it("clamps a requested width to the side's own bounds when the window is wide", () => {
    const fit = { container: 2000, other: 300 };
    expect(clampWidth("left", 10, fit)).toBe(BOUNDS.left.min);
    expect(clampWidth("left", 9999, fit)).toBe(BOUNDS.left.max);
    expect(clampWidth("right", 10, fit)).toBe(BOUNDS.right.min);
    expect(clampWidth("right", 9999, fit)).toBe(BOUNDS.right.max);
    expect(clampWidth("left", 301.6, fit)).toBe(302);
  });

  it("never lets the two panels squeeze the reading column below its floor", () => {
    const fit = { container: 1200, other: 300 };
    const widest = clampWidth("left", 9999, fit);
    expect(widest).toBe(1200 - 300 - MIN_DOCUMENT);
    expect(1200 - widest - 300).toBe(MIN_DOCUMENT);
  });

  it("falls back to the side's minimum when the window leaves no room", () => {
    expect(clampWidth("left", 400, { container: 700, other: 400 })).toBe(BOUNDS.left.min);
    expect(widthRange("right", { container: 700, other: 400 })).toEqual({ min: BOUNDS.right.min, max: BOUNDS.right.min });
  });

  it("reads the rendered grid tracks, whether the study panel is a column or not", () => {
    expect(parseTracks("240px 836px 304px")).toEqual([240, 836, 304]);
    expect(parseTracks("240px 1040px")).toEqual([240, 1040]);
    expect(parseTracks("none")).toEqual([]);
    expect(parseTracks("")).toEqual([]);
  });
});
