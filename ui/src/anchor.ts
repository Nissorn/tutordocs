export type Selector =
  | { type: "TextQuoteSelector"; exact: string; prefix: string; suffix: string }
  | { type: "TextPositionSelector"; start: number; end: number };

export interface Span {
  start: number;
  end: number;
}

export interface Anchorable {
  quote: string;
  selectors: Selector[];
}

export interface Anchored<M> {
  mark: M;
  span: Span;
  selectors: Selector[];
}

export interface Conflicted<M> {
  mark: M;
  resolution: "unresolved" | "ambiguous";
}

const CONTEXT = 64;

export function selectorsAt(text: string, span: Span): Selector[] {
  return [
    {
      type: "TextQuoteSelector",
      exact: text.slice(span.start, span.end),
      prefix: text.slice(Math.max(0, span.start - CONTEXT), span.start),
      suffix: text.slice(span.end, span.end + CONTEXT),
    },
    { type: "TextPositionSelector", start: span.start, end: span.end },
  ];
}

/**
 * Re-anchors stored marks against a chapter's current canonical text. A mark
 * matches by its exact quote; several copies are told apart by how much stored
 * context still touches each, then by the stored position. Whatever cannot be
 * placed comes back conflicted with the mark untouched, never dropped.
 */
export function reanchor<M extends Anchorable>(
  marks: M[],
  text: string,
): { anchored: Anchored<M>[]; conflicted: Conflicted<M>[] } {
  const anchored: Anchored<M>[] = [];
  const conflicted: Conflicted<M>[] = [];
  for (const mark of marks) {
    const placed = place(mark, text);
    if (typeof placed === "string") conflicted.push({ mark, resolution: placed });
    else anchored.push({ mark, span: placed, selectors: selectorsAt(text, placed) });
  }
  return { anchored, conflicted };
}

function place(mark: Anchorable, text: string): Span | Conflicted<never>["resolution"] {
  let quote: Extract<Selector, { type: "TextQuoteSelector" }> | undefined;
  let position: Extract<Selector, { type: "TextPositionSelector" }> | undefined;
  for (const selector of mark.selectors) {
    if (selector.type === "TextQuoteSelector") quote ??= selector;
    else position ??= selector;
  }
  const exact = quote?.exact || mark.quote;
  if (exact === "") {
    return position && position.end <= text.length ? { start: position.start, end: position.end } : "unresolved";
  }
  const starts = occurrences(text, exact);
  if (starts.length === 0) return "unresolved";
  const scored = starts.map((start) => ({
    start,
    score: quote ? contextScore(text, start, start + exact.length, quote.prefix, quote.suffix) : 0,
  }));
  const best = Math.max(...scored.map((c) => c.score));
  const finalists = scored.filter((c) => c.score === best).map((c) => c.start);
  if (finalists.length === 1) return { start: finalists[0], end: finalists[0] + exact.length };
  if (!position) return "ambiguous";
  const distance = (start: number) => Math.abs(start - position.start);
  const nearest = Math.min(...finalists.map(distance));
  const closest = finalists.filter((start) => distance(start) === nearest);
  return closest.length === 1 ? { start: closest[0], end: closest[0] + exact.length } : "ambiguous";
}

function occurrences(text: string, exact: string): number[] {
  const out: number[] = [];
  for (let at = text.indexOf(exact); at >= 0; at = text.indexOf(exact, at + 1)) out.push(at);
  return out;
}

/** Characters of stored prefix and suffix that still touch this occurrence. */
function contextScore(text: string, start: number, end: number, prefix: string, suffix: string): number {
  let before = 0;
  while (before < prefix.length && before < start && text[start - 1 - before] === prefix[prefix.length - 1 - before]) {
    before++;
  }
  let after = 0;
  while (after < suffix.length && end + after < text.length && text[end + after] === suffix[after]) after++;
  return before + after;
}
