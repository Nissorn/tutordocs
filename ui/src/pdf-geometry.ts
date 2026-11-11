import type { Span } from "./anchor";

// Page geometry for PDF sources. A rect is ticket 03's normalized rect:
// fractions of the page's MediaBox, origin at the bottom left, y up. Words and
// highlights are both placed with `boxOf`, so a highlight covers exactly the
// words drawn under it at any page size.

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface PdfWord {
  quote: string;
  rect: Rect;
}

/** A word with its offsets in the page's extracted text. */
export interface PlacedWord extends PdfWord {
  start: number;
  end: number;
}

/** Fractions of the page box from its top-left corner, as CSS lays boxes out. */
export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

/**
 * Finds each word in the page text. The extractor writes the words in order
 * with only whitespace between them, so a forward search places every one.
 */
export function placeWords(text: string, words: PdfWord[]): PlacedWord[] {
  let cursor = 0;
  return words.map((word) => {
    const found = text.indexOf(word.quote, cursor);
    const start = found < 0 ? cursor : found;
    cursor = start + word.quote.length;
    return { ...word, start, end: cursor };
  });
}

/** Two rects share a line when their vertical extents mostly overlap. */
function sameLine(a: Rect, b: Rect): boolean {
  const overlap = Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y);
  return overlap > 0.5 * Math.min(a.h, b.h) && b.x >= a.x;
}

function union(a: Rect, b: Rect): Rect {
  const x = Math.min(a.x, b.x);
  const y = Math.min(a.y, b.y);
  return { x, y, w: Math.max(a.x + a.w, b.x + b.w) - x, h: Math.max(a.y + a.h, b.y + b.h) - y };
}

/** The rects a span of page text covers: one per run of words on a line. */
export function rectsForSpan(words: PlacedWord[], span: Span): Rect[] {
  const rects: Rect[] = [];
  for (const word of words) {
    if (word.end <= span.start || word.start >= span.end) continue;
    const last = rects[rects.length - 1];
    if (last && sameLine(last, word.rect)) rects[rects.length - 1] = union(last, word.rect);
    else rects.push({ ...word.rect });
  }
  return rects;
}

export function boxOf(rect: Rect): Box {
  return { left: rect.x, top: 1 - rect.y - rect.h, width: rect.w, height: rect.h };
}

/** A stored rect in pixels inside a page box of the given size. */
export function toScreen(rect: Rect, page: { width: number; height: number }): Box {
  const box = boxOf(rect);
  return {
    left: box.left * page.width,
    top: box.top * page.height,
    width: box.width * page.width,
    height: box.height * page.height,
  };
}
