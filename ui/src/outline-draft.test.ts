import { describe, expect, it } from "vitest";
import contract from "./fixtures/wizard-contract.json";
import {
  approvalBlock,
  draftFromOutline,
  edit,
  hasComments,
  isDirty,
  removalBlock,
  saveBlock,
  toOutline,
  type OutlineDraft,
} from "./outline-draft";
import type { Outline } from "./outline-api";

const draftOutline = contract.outline_draft as Outline;
const withWritten = contract.outline_approved_with_written as Outline;

const titles = (d: OutlineDraft): Array<string | null> => d.chapters.map((c) => c.id);

describe("a draft from an outline", () => {
  it("keeps every field and the order, and sending it back is the same outline", () => {
    const d = draftFromOutline(draftOutline);
    expect(d.status).toBe("draft");
    expect(titles(d)).toEqual(["cells", "membranes", "nucleus"]);
    expect(toOutline(d)).toEqual(draftOutline);
  });
});

describe("editing text", () => {
  it("renames a chapter and rewrites its summary", () => {
    let d = draftFromOutline(draftOutline);
    d = edit(d, { kind: "title", key: "membranes", text: "Crossing the membrane" }).draft;
    d = edit(d, { kind: "summary", key: "membranes", text: "Diffusion, channels, pumps." }).draft;
    const out = toOutline(d).chapters[1];
    expect(out.title).toBe("Crossing the membrane");
    expect(out.summary).toBe("Diffusion, channels, pumps.");
    expect(out.id).toBe("membranes");
  });

  it("leaves the original draft untouched", () => {
    const d = draftFromOutline(draftOutline);
    edit(d, { kind: "title", key: "cells", text: "Changed" });
    expect(d.chapters[0].title).toBe("Cells as the unit of life");
  });

  it("keeps a comment on its chapter, and the comment list tells whether any exist", () => {
    let d = draftFromOutline(draftOutline);
    expect(hasComments(d)).toBe(false);
    d = edit(d, { kind: "comment", key: "nucleus", text: "Too short. Add the nucleolus." }).draft;
    expect(hasComments(d)).toBe(true);
    expect(toOutline(d).chapters[2].comment).toBe("Too short. Add the nucleolus.");
    d = edit(d, { kind: "comment", key: "nucleus", text: "   " }).draft;
    expect(hasComments(d)).toBe(false);
  });

  it("edits, adds and removes objectives, and drops blank ones when sending", () => {
    let d = draftFromOutline(draftOutline);
    d = edit(d, { kind: "objective", index: 0, text: "Describe the bilayer" }).draft;
    d = edit(d, { kind: "add-objective" }).draft;
    expect(d.objectives).toHaveLength(3);
    expect(toOutline(d).objectives).toEqual(["Describe the bilayer", "Compare diffusion and active transport"]);
    d = edit(d, { kind: "remove-objective", index: 1 }).draft;
    expect(toOutline(d).objectives).toEqual(["Describe the bilayer"]);
  });
});

describe("reordering", () => {
  it("moves a chapter to a new position and shifts the others", () => {
    const d = edit(draftFromOutline(draftOutline), { kind: "move", key: "nucleus", to: 0 }).draft;
    expect(titles(d)).toEqual(["nucleus", "cells", "membranes"]);
  });

  it("moves a chapter down", () => {
    const d = edit(draftFromOutline(draftOutline), { kind: "move", key: "cells", to: 2 }).draft;
    expect(titles(d)).toEqual(["membranes", "nucleus", "cells"]);
  });

  it("clamps a position past either end", () => {
    const d = edit(draftFromOutline(draftOutline), { kind: "move", key: "cells", to: 99 }).draft;
    expect(titles(d)).toEqual(["membranes", "nucleus", "cells"]);
    expect(titles(edit(d, { kind: "move", key: "cells", to: -5 }).draft)).toEqual(["cells", "membranes", "nucleus"]);
  });

  it("lets a written chapter move", () => {
    const d = edit(draftFromOutline(withWritten), { kind: "move", key: "cells", to: 1 }).draft;
    expect(titles(d)).toEqual(["membranes", "cells"]);
  });
});

describe("adding and removing chapters", () => {
  it("adds an empty chapter with no id at the end", () => {
    const d = edit(draftFromOutline(draftOutline), { kind: "add-chapter" }).draft;
    expect(d.chapters).toHaveLength(4);
    const added = toOutline(d).chapters[3];
    expect(added).toEqual({ id: null, title: "", summary: "", sources: [], comment: "", written: false });
  });

  it("gives each added chapter its own key so two can be edited apart", () => {
    let d = edit(draftFromOutline(draftOutline), { kind: "add-chapter" }).draft;
    d = edit(d, { kind: "add-chapter" }).draft;
    const [a, b] = d.chapters.slice(3);
    expect(a.key).not.toBe(b.key);
    d = edit(d, { kind: "title", key: b.key, text: "Second" }).draft;
    expect(d.chapters.slice(3).map((c) => c.title)).toEqual(["", "Second"]);
  });

  it("removes an unwritten chapter", () => {
    const { draft, refused } = edit(draftFromOutline(draftOutline), { kind: "remove-chapter", key: "membranes" });
    expect(refused).toBeNull();
    expect(titles(draft)).toEqual(["cells", "nucleus"]);
  });

  it("refuses to remove a written chapter and says why", () => {
    const before = draftFromOutline(withWritten);
    const { draft, refused } = edit(before, { kind: "remove-chapter", key: "cells" });
    expect(refused).toBe('"Cells as the unit of life" is already written, so it cannot be removed.');
    expect(draft).toBe(before);
    expect(removalBlock(before.chapters[0])).toBe(refused);
    expect(removalBlock(before.chapters[1])).toBeNull();
  });

  it("removes an added chapter that has no id yet", () => {
    let d = edit(draftFromOutline(draftOutline), { kind: "add-chapter" }).draft;
    d = edit(d, { kind: "remove-chapter", key: d.chapters[3].key }).draft;
    expect(d.chapters).toHaveLength(3);
  });
});

describe("what blocks approval", () => {
  it("asks for at least one chapter", () => {
    let d = draftFromOutline(draftOutline);
    expect(approvalBlock(d)).toBeNull();
    for (const key of ["cells", "membranes", "nucleus"]) d = edit(d, { kind: "remove-chapter", key }).draft;
    expect(approvalBlock(d)).toBe("Add at least one chapter first.");
  });

  it("asks for a title on every chapter, naming the first one without", () => {
    let d = edit(draftFromOutline(draftOutline), { kind: "add-chapter" }).draft;
    expect(approvalBlock(d)).toBe("Chapter 4 needs a title.");
    d = edit(d, { kind: "title", key: d.chapters[3].key, text: "Mitochondria" }).draft;
    expect(approvalBlock(d)).toBeNull();
  });

  it("lets an outline with no chapters be saved but asks for a title on one that has none", () => {
    let d = draftFromOutline(draftOutline);
    for (const key of ["cells", "membranes", "nucleus"]) d = edit(d, { kind: "remove-chapter", key }).draft;
    expect(saveBlock(d)).toBeNull();
    d = edit(d, { kind: "add-chapter" }).draft;
    expect(saveBlock(d)).toBe("Chapter 1 needs a title.");
  });
});

describe("unsaved changes", () => {
  it("is clean as loaded, dirty after an edit, and clean again when the edit is undone", () => {
    const d = draftFromOutline(draftOutline);
    expect(isDirty(d, draftOutline)).toBe(false);
    const edited = edit(d, { kind: "title", key: "cells", text: "Changed" }).draft;
    expect(isDirty(edited, draftOutline)).toBe(true);
    const undone = edit(edited, { kind: "title", key: "cells", text: "Cells as the unit of life" }).draft;
    expect(isDirty(undone, draftOutline)).toBe(false);
  });

  it("counts a reorder and a removal as changes", () => {
    const d = draftFromOutline(draftOutline);
    expect(isDirty(edit(d, { kind: "move", key: "cells", to: 1 }).draft, draftOutline)).toBe(true);
    expect(isDirty(edit(d, { kind: "remove-chapter", key: "cells" }).draft, draftOutline)).toBe(true);
  });
});
