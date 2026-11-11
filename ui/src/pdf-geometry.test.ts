import { describe, expect, it } from "vitest";
import { placeWords, rectsForSpan, toScreen, type PdfWord } from "./pdf-geometry";

// US Letter, 612 x 792 pt. Hand geometry: 24 pt Helvetica at baseline 720,
// glyph box y 715.2..744 (ticket 03's 1.2 em box). "Hello" spans x 72..126.672,
// "world" x 133.344..190.68. A second line at baseline 672, box y 667.2..696,
// holds "Second" x 72..153.384.
const W = 612;
const H = 792;
const box = (x0: number, y0: number, x1: number, y1: number) => ({
  x: x0 / W,
  y: y0 / H,
  w: (x1 - x0) / W,
  h: (y1 - y0) / H,
});
const TEXT = "Hello world\nSecond";
const WORDS: PdfWord[] = [
  { quote: "Hello", rect: box(72, 715.2, 126.672, 744) },
  { quote: "world", rect: box(133.344, 715.2, 190.68, 744) },
  { quote: "Second", rect: box(72, 667.2, 153.384, 696) },
];

function close(got: object, want: object) {
  const g = got as Record<string, number>;
  for (const [key, value] of Object.entries(want)) expect(g[key], key).toBeCloseTo(value, 6);
}

describe("selection to stored rect and back to the screen", () => {
  const placed = placeWords(TEXT, WORDS);

  it("places each word at its offset in the page text", () => {
    expect(placed.map((w) => [w.start, w.end])).toEqual([
      [0, 5],
      [6, 11],
      [12, 18],
    ]);
  });

  it("stores one rect for a run of words on one line", () => {
    const rects = rectsForSpan(placed, { start: 0, end: 11 });
    expect(rects).toHaveLength(1);
    close(rects[0], box(72, 715.2, 190.68, 744));
  });

  it("stores a rect per line when the selection wraps", () => {
    const rects = rectsForSpan(placed, { start: 6, end: 18 });
    expect(rects).toHaveLength(2);
    close(rects[0], box(133.344, 715.2, 190.68, 744));
    close(rects[1], box(72, 667.2, 153.384, 696));
  });

  it("maps a stored rect to the same words at 100%, at 200% and after a resize", () => {
    const [stored] = rectsForSpan(placed, { start: 0, end: 11 });
    // At 100% one CSS pixel per point: "Hello world" sits 48 px below the top.
    close(toScreen(stored, { width: 612, height: 792 }), { left: 72, top: 48, width: 118.68, height: 28.8 });
    // At 200% every distance doubles.
    close(toScreen(stored, { width: 1224, height: 1584 }), { left: 144, top: 96, width: 237.36, height: 57.6 });
    // A narrower window halves the page.
    close(toScreen(stored, { width: 306, height: 396 }), { left: 36, top: 24, width: 59.34, height: 14.4 });
  });

  it("covers the selected word and not its neighbour at any zoom", () => {
    const [stored] = rectsForSpan(placed, { start: 6, end: 11 });
    for (const width of [612, 1224, 306]) {
      const page = { width, height: (width * H) / W };
      const mark = toScreen(stored, page);
      const world = toScreen(WORDS[1].rect, page);
      const hello = toScreen(WORDS[0].rect, page);
      close(mark, world);
      expect(mark.left).toBeGreaterThan(hello.left + hello.width);
    }
  });
});
