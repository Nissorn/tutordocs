import { describe, expect, it } from "vitest";
import { reanchor, selectorsAt, type Selector } from "./anchor";

interface TestMark {
  id: string;
  quote: string;
  note: string;
  selectors: Selector[];
}

function markOn(text: string, quote: string, note = "", occurrence = 0): TestMark {
  let start = -1;
  for (let i = 0; i <= occurrence; i++) start = text.indexOf(quote, start + 1);
  if (start < 0) throw new Error(`fixture: "${quote}" not in text`);
  return { id: quote, quote, note, selectors: selectorsAt(text, { start, end: start + quote.length }) };
}

const BEFORE = "Cells are the unit of life. The membrane separates the cell from its surroundings.";

describe("selectorsAt", () => {
  it("captures the exact quote with its surrounding context and its position", () => {
    expect(selectorsAt(BEFORE, { start: 32, end: 40 })).toEqual([
      {
        type: "TextQuoteSelector",
        exact: "membrane",
        prefix: "Cells are the unit of life. The ",
        suffix: " separates the cell from its surroundings.",
      },
      { type: "TextPositionSelector", start: 32, end: 40 },
    ]);
  });
});

describe("reanchor", () => {
  it("keeps a mark where it was when the exact quote is still present", () => {
    const mark = markOn(BEFORE, "membrane");
    const { anchored, conflicted } = reanchor([mark], BEFORE);
    expect(conflicted).toEqual([]);
    expect(anchored).toHaveLength(1);
    expect(anchored[0].mark).toBe(mark);
    expect(anchored[0].span).toEqual({ start: 32, end: 40 });
    expect(anchored[0].selectors).toEqual(mark.selectors);
  });

  it("follows a quote that moved when the chapter was rewritten around it", () => {
    const mark = markOn(BEFORE, "membrane");
    const after = "A new opening paragraph.\n\n" + BEFORE.replace("unit of life", "smallest unit of life");
    const { anchored, conflicted } = reanchor([mark], after);
    expect(conflicted).toEqual([]);
    const at = after.indexOf("membrane");
    expect(at).toBe(32 + 26 + 9);
    expect(anchored[0].span).toEqual({ start: at, end: at + 8 });
    expect(anchored[0].selectors).toEqual(selectorsAt(after, { start: at, end: at + 8 }));
  });

  it("uses the surrounding context to pick the right copy of a duplicated quote", () => {
    const before = "Osmosis moves water. Diffusion moves solutes down a gradient.";
    const mark = markOn(before, "moves");
    const after = "Diffusion moves solutes down a gradient. Osmosis moves water across a membrane.";
    const wanted = after.indexOf("Osmosis moves") + "Osmosis ".length;
    const { anchored, conflicted } = reanchor([mark], after);
    expect(conflicted).toEqual([]);
    expect(anchored[0].span).toEqual({ start: wanted, end: wanted + 5 });
  });

  it("prefers context over a stale position that now points at the other copy", () => {
    const before = "Osmosis moves water. Diffusion moves solutes down a gradient.";
    const mark = markOn(before, "moves");
    expect(mark.selectors[1]).toEqual({ type: "TextPositionSelector", start: 8, end: 13 });
    const after = "Diffusion moves solutes down a gradient. Then, for contrast: Osmosis moves water.";
    const wanted = after.indexOf("Osmosis moves") + "Osmosis ".length;
    const { anchored } = reanchor([mark], after);
    expect(anchored[0].span).toEqual({ start: wanted, end: wanted + 5 });
  });

  it("conflicts a mark whose quote is gone instead of dropping it", () => {
    const mark = markOn(BEFORE, "membrane");
    const after = "Cells are the unit of life. A lipid bilayer separates the cell from its surroundings.";
    const { anchored, conflicted } = reanchor([mark], after);
    expect(anchored).toEqual([]);
    expect(conflicted).toEqual([{ mark, resolution: "unresolved" }]);
  });

  it("carries the note and the original quote through a conflict untouched", () => {
    const mark = markOn(BEFORE, "membrane", "Ask: is this the plasma membrane or all membranes?");
    const snapshot = structuredClone(mark);
    const { conflicted } = reanchor([mark], "The chapter was rewritten from scratch.");
    expect(conflicted).toHaveLength(1);
    expect(conflicted[0].mark).toEqual(snapshot);
    expect(conflicted[0].mark.note).toBe("Ask: is this the plasma membrane or all membranes?");
    expect(conflicted[0].mark.quote).toBe("membrane");
  });

  it("falls back to the stored position only when context cannot tell the copies apart", () => {
    const text = "Note. Note. Note.";
    const mark: TestMark = {
      id: "n",
      quote: "Note",
      note: "",
      selectors: [
        { type: "TextQuoteSelector", exact: "Note", prefix: "", suffix: "" },
        { type: "TextPositionSelector", start: 7, end: 11 },
      ],
    };
    const { anchored, conflicted } = reanchor([mark], text);
    expect(conflicted).toEqual([]);
    expect(anchored[0].span).toEqual({ start: 6, end: 10 });
  });

  it("conflicts as ambiguous when neither context nor a position settles the copy", () => {
    const mark: TestMark = {
      id: "n",
      quote: "Note",
      note: "keep me",
      selectors: [{ type: "TextQuoteSelector", exact: "Note", prefix: "", suffix: "" }],
    };
    const { anchored, conflicted } = reanchor([mark], "Note. Note.");
    expect(anchored).toEqual([]);
    expect(conflicted).toEqual([{ mark, resolution: "ambiguous" }]);
  });

  it("places a mark with no quote by its position, and conflicts it when the text is now too short", () => {
    const mark: TestMark = {
      id: "p",
      quote: "",
      note: "",
      selectors: [{ type: "TextPositionSelector", start: 6, end: 10 }],
    };
    expect(reanchor([mark], "Cells are small.").anchored[0].span).toEqual({ start: 6, end: 10 });
    expect(reanchor([mark], "Cells").conflicted).toEqual([{ mark, resolution: "unresolved" }]);
  });
});
