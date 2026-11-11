// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import contract from "./fixtures/reader-contract.json";
import wizardContract from "./fixtures/wizard-contract.json";
import annotationsContract from "./fixtures/annotations-contract.json";
import { mountReader } from "./reader";
import { stubWideMatchMedia } from "./test-media";

type Stored = Record<string, unknown> & { annotationId: string };

/** An in-memory stand-in for the Rust annotation commands over one chapter. */
function backend(markdown: string, stored: Stored[] = []) {
  const db = new Map(stored.map((m) => [m.annotationId, structuredClone(m)]));
  const saves: Stored[] = [];
  const deletes: string[] = [];
  mockIPC((cmd, raw) => {
    const args = (raw ?? {}) as Record<string, unknown>;
    switch (cmd) {
      case "list_courses":
        return contract.list_courses;
      case "get_outline":
        return wizardContract.outline_approved;
      case "list_chapters":
        return contract.list_chapters;
      case "read_chapter":
        return { ...contract.read_chapter_ready, markdown };
      case "list_annotations":
        return [...db.values()];
      case "list_course_sources":
        return [];
      case "save_annotation": {
        const mark = { ...(args.mark as Stored), createdAt: 1_700_000_000, updatedAt: 1_700_000_001 };
        db.set(mark.annotationId, mark);
        saves.push(structuredClone(mark));
        return mark;
      }
      case "delete_annotation":
        deletes.push(args.annotationId as string);
        return db.delete(args.annotationId as string);
      default:
        throw new Error(`unexpected command ${cmd}`);
    }
  });
  return { db, saves, deletes };
}

let root: HTMLElement;
let restoreMedia: (() => void) | undefined;

beforeEach(() => {
  restoreMedia = stubWideMatchMedia();
  document.body.innerHTML = '<div id="app"></div>';
  root = document.querySelector<HTMLElement>("#app")!;
});

afterEach(() => {
  restoreMedia?.();
  clearMocks();
  document.getSelection()?.removeAllRanges();
});

function prose(): HTMLElement {
  return root.querySelector<HTMLElement>("main.document .prose")!;
}

function margin(): HTMLElement {
  return root.querySelector<HTMLElement>("aside.margin")!;
}

function textNode(container: Element, contains: string): Text {
  const walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if ((node as Text).data.includes(contains)) return node as Text;
  }
  throw new Error(`no text node containing "${contains}"`);
}

/** Selects like a user dragging from one point to another, then releases. */
function drag(start: [Node, number], end: [Node, number]): void {
  const range = document.createRange();
  range.setStart(...start);
  range.setEnd(...end);
  const selection = document.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
  prose().dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
}

describe("making a highlight", () => {
  it("highlights a selection across formatting in the chosen color and saves its W3C anchor", async () => {
    const { saves } = backend("# Cells\n\nThe **cell membrane** is thin and flexible.\n");
    await mountReader(root, contract.list_courses[0]);
    const bold = textNode(prose(), "cell membrane");
    const after = textNode(prose(), " is thin");
    drag([bold, 5], [after, 3]);

    const green = root.querySelector<HTMLButtonElement>('[role="toolbar"] button[data-color="green"]');
    expect(green).not.toBeNull();
    green!.click();

    await vi.waitFor(() => expect(saves).toHaveLength(1));
    const mark = saves[0];
    expect(mark.quote).toBe("membrane is");
    expect(mark.color).toBe("green");
    expect(mark.kind).toBe("highlight");
    expect(mark.resolution).toBe("resolved");
    expect(mark.selectors).toEqual([
      {
        type: "TextQuoteSelector",
        exact: "membrane is",
        prefix: "Cells\nThe cell ",
        suffix: " thin and flexible.\n",
      },
      { type: "TextPositionSelector", start: 15, end: 26 },
    ]);
    const painted = Array.from(prose().querySelectorAll(`mark[data-annotation-id="${mark.annotationId}"]`));
    expect(painted.map((m) => m.textContent).join("")).toBe("membrane is");
    expect(painted.every((m) => (m as HTMLElement).dataset.color === "green")).toBe(true);
    expect(root.querySelector('[role="toolbar"]')).toBeNull();
  });
});

const STORED = annotationsContract.list_annotations[0] as Stored;
const CELLS = contract.read_chapter_ready.markdown;

function noteCard(annotationId: string): HTMLElement {
  const card = Array.from(margin().querySelectorAll<HTMLElement>(".note-card")).find(
    (c) => c.dataset.annotationId === annotationId || c.dataset.conflictId === annotationId,
  );
  if (!card) throw new Error(`no margin card for ${annotationId}`);
  return card;
}

describe("selection edges", () => {
  it("trims the whitespace a word or paragraph selection drags along", async () => {
    const { saves } = backend("# Cells\n\nThe membrane is thin.\n");
    await mountReader(root, contract.list_courses[0]);
    const line = textNode(prose(), "membrane");
    drag([line, 3], [line, 13]);
    root.querySelector<HTMLButtonElement>('[role="toolbar"] button[data-color="yellow"]')!.click();
    await vi.waitFor(() => expect(saves).toHaveLength(1));
    expect(saves[0].quote).toBe("membrane");
  });
});

describe("reading marks back", () => {
  it("paints a stored mark over its quote and shows its note in the right margin", async () => {
    const { saves } = backend(CELLS, [STORED]);
    await mountReader(root, contract.list_courses[0]);
    const painted = prose().querySelectorAll('mark[data-annotation-id="mark-1"]');
    expect(Array.from(painted).map((m) => m.textContent).join("")).toBe("basic unit");
    expect(noteCard("mark-1").querySelector("textarea")!.value).toBe(STORED.note);
    expect(noteCard("mark-1").closest("aside.margin")).not.toBeNull();
    expect(saves).toEqual([]);
  });

  it("saves a typed note against the mark", async () => {
    const { db } = backend(CELLS, [STORED]);
    await mountReader(root, contract.list_courses[0]);
    const field = noteCard("mark-1").querySelector("textarea")!;
    field.value = "Smallest thing that is still alive.";
    field.dispatchEvent(new Event("input"));
    field.dispatchEvent(new Event("blur"));
    await vi.waitFor(() => expect(db.get("mark-1")!.note).toBe("Smallest thing that is still alive."), {
      timeout: 200,
    });
    expect(db.get("mark-1")!.quote).toBe("basic unit");
  });

  it("offers the five swatches on a note card and recolors the mark on click", async () => {
    const { saves } = backend(CELLS, [STORED]);
    await mountReader(root, contract.list_courses[0]);
    const card = noteCard("mark-1");
    const swatches = Array.from(card.querySelectorAll<HTMLButtonElement>("button.swatch"));
    expect(swatches.map((s) => s.dataset.color)).toEqual(["yellow", "green", "blue", "pink", "purple"]);
    expect(swatches[1].getAttribute("aria-current")).toBe("true");

    swatches[2].click();
    await vi.waitFor(() => expect(saves).toHaveLength(1));
    expect(saves[0].color).toBe("blue");
    expect(card.dataset.color).toBe("blue");
    expect(swatches[2].getAttribute("aria-current")).toBe("true");
    expect(swatches[1].getAttribute("aria-current")).toBe("false");

    const painted = Array.from(prose().querySelectorAll<HTMLElement>('mark[data-annotation-id="mark-1"]'));
    expect(painted.every((m) => m.dataset.color === "blue")).toBe(true);
  });

  it("keeps swatches hidden until the color button is clicked", async () => {
    backend(CELLS, [STORED]);
    await mountReader(root, contract.list_courses[0]);
    const card = noteCard("mark-1");
    const toggle = card.querySelector<HTMLButtonElement>("button.card-color-toggle")!;
    expect(toggle).not.toBeNull();
    const swatchesBar = card.querySelector<HTMLElement>(".card-swatches")!;
    expect(swatchesBar.hidden).toBe(true);

    toggle.click();
    expect(swatchesBar.hidden).toBe(false);

    toggle.click();
    expect(swatchesBar.hidden).toBe(true);
  });

  it("closes swatches on Escape and returns focus to the button", async () => {
    backend(CELLS, [STORED]);
    await mountReader(root, contract.list_courses[0]);
    const card = noteCard("mark-1");
    const toggle = card.querySelector<HTMLButtonElement>("button.card-color-toggle")!;
    const swatchesBar = card.querySelector<HTMLElement>(".card-swatches")!;
    toggle.click();
    expect(swatchesBar.hidden).toBe(false);

    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(swatchesBar.hidden).toBe(true);
    expect(document.activeElement).toBe(toggle);
  });

  it("closes swatches on click outside and returns focus to the button", async () => {
    backend(CELLS, [STORED]);
    await mountReader(root, contract.list_courses[0]);
    const card = noteCard("mark-1");
    const toggle = card.querySelector<HTMLButtonElement>("button.card-color-toggle")!;
    const swatchesBar = card.querySelector<HTMLElement>(".card-swatches")!;
    toggle.click();
    expect(swatchesBar.hidden).toBe(false);

    document.body.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true }));
    expect(swatchesBar.hidden).toBe(true);
    expect(document.activeElement).toBe(toggle);
  });

  it("preserves in-progress note typing when another mark changes", async () => {
    const mark2: Stored = {
      ...STORED,
      annotationId: "mark-2",
      color: "yellow",
      quote: "Nucleus",
      selectors: [
        { type: "TextQuoteSelector", exact: "Nucleus", prefix: "of life.\n\n", suffix: "\nMembrane\n\n" },
        { type: "TextPositionSelector", start: 34, end: 41 },
      ],
    };
    backend(CELLS, [STORED, mark2]);
    await mountReader(root, contract.list_courses[0]);

    const card1 = noteCard("mark-1");
    const textarea1 = card1.querySelector("textarea")!;
    textarea1.value = "Drafting notes here...";
    textarea1.focus();

    // Create a new mark, which triggers render()
    const line = textNode(prose(), "Membrane");
    drag([line, 0], [line, 8]);
    root.querySelector<HTMLButtonElement>('[role="toolbar"] button[data-color="pink"]')!.click();

    const card1After = noteCard("mark-1");
    expect(card1After).toBe(card1);
    expect(card1After.querySelector("textarea")).toBe(textarea1);
    expect(textarea1.value).toBe("Drafting notes here...");
  });

  it("reveals a collapsed panel before focusing a new note", async () => {
    backend(CELLS, []);
    await mountReader(root, contract.list_courses[0]);

    const toggle = root.querySelector<HTMLButtonElement>("button.panel-toggle")!;
    toggle.click();
    expect(root.querySelector<HTMLElement>(".reader")!.dataset.panel).toBe("collapsed");
    expect(toggle.getAttribute("aria-expanded")).toBe("false");

    const line = textNode(prose(), "Membrane");
    drag([line, 0], [line, 8]);
    root.querySelector<HTMLButtonElement>('[role="toolbar"] button[data-color="yellow"]')!.click();

    await vi.waitFor(() => expect(root.querySelector<HTMLElement>(".reader")!.dataset.panel).toBe("open"));
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    const newCard = root.querySelector(".note-card")!;
    expect(document.activeElement).toBe(newCard.querySelector("textarea"));
  });
});

describe("after the chapter is rewritten", () => {
  const vanished: Stored = {
    ...STORED,
    annotationId: "mark-2",
    color: "pink",
    quote: "Nucleus",
    note: "Only in eukaryotes.",
    selectors: [
      { type: "TextQuoteSelector", exact: "Nucleus", prefix: "of life.\n\n", suffix: "\nMembrane\n\n" },
      { type: "TextPositionSelector", start: 43, end: 50 },
    ],
  };
  const REWRITTEN = "# Cells\n\nEvery organism is made of cells. The cell is the basic unit of life.\n\n- Membrane\n";

  it("re-anchors the moved mark, conflicts the vanished one with its quote and note, and deletes nothing", async () => {
    const { db, saves, deletes } = backend(REWRITTEN, [STORED, vanished]);
    await mountReader(root, contract.list_courses[0]);

    const moved = Array.from(prose().querySelectorAll('mark[data-annotation-id="mark-1"]'));
    expect(moved.map((m) => m.textContent).join("")).toBe("basic unit");
    const at = "Cells\nEvery organism is made of cells. The cell is the ".length;
    expect(db.get("mark-1")!.selectors).toContainEqual({ type: "TextPositionSelector", start: at, end: at + 10 });
    expect(db.get("mark-1")!.resolution).toBe("resolved");

    const group = margin().querySelector(".margin-conflicts")!;
    expect(group.textContent).toContain("Needs attention");
    const card = noteCard("mark-2");
    expect(group.contains(card)).toBe(true);
    expect(card.querySelector("blockquote")!.textContent).toBe("Nucleus");
    expect(card.querySelector("textarea")!.value).toBe("Only in eukaryotes.");
    expect(prose().querySelector('mark[data-annotation-id="mark-2"]')).toBeNull();

    expect(db.get("mark-2")).toMatchObject({ resolution: "unresolved", note: "Only in eukaryotes.", quote: "Nucleus" });
    expect(saves.map((m) => m.annotationId).sort()).toEqual(["mark-1", "mark-2"]);
    expect(deletes).toEqual([]);
  });

  it("re-attaches a conflicted mark to a new selection, keeping its note", async () => {
    const { db } = backend(REWRITTEN, [STORED, vanished]);
    await mountReader(root, contract.list_courses[0]);
    const membrane = textNode(prose(), "Membrane");
    drag([membrane, 0], [membrane, 8]);
    noteCard("mark-2").querySelector<HTMLButtonElement>("button.card-action")!.click();

    await vi.waitFor(() => expect(db.get("mark-2")!.resolution).toBe("resolved"));
    expect(db.get("mark-2")).toMatchObject({ quote: "Membrane", note: "Only in eukaryotes.", color: "pink" });
    expect(prose().querySelector('mark[data-annotation-id="mark-2"]')!.textContent).toBe("Membrane");
    expect(margin().querySelector(".margin-conflicts")).toBeNull();
    expect(noteCard("mark-2").querySelector("textarea")!.value).toBe("Only in eukaryotes.");
  });

  it("deletes a conflicted mark only when the user asks", async () => {
    const { db, deletes } = backend(REWRITTEN, [STORED, vanished]);
    await mountReader(root, contract.list_courses[0]);
    const del = Array.from(noteCard("mark-2").querySelectorAll("button")).find((b) => b.textContent === "Delete")!;
    del.click();
    await vi.waitFor(() => expect(margin().querySelector(".margin-conflicts")).toBeNull());
    expect(deletes).toEqual(["mark-2"]);
    expect(db.has("mark-2")).toBe(false);
    expect(db.has("mark-1")).toBe(true);
  });
});

describe("in a chapter with math", () => {
  // KaTeX writes each formula three times: MathML for screen readers, the TeX
  // source in an annotation, and the glyphs on screen. Marks anchor to the
  // glyphs, the text a reader sees.
  const PHOTO =
    "# Energy\n\nPlants turn $6\\,CO_2$ and water into sugar, so the cell can grow.\n\n" +
    "$$6\\,CO_2 + 6\\,H_2O \\rightarrow C_6H_{12}O_6 + 6\\,O_2$$\n\nThe sugar then fuels respiration.\n";

  async function select(from: [string, number], to: [string, number]): Promise<Stored> {
    const { saves } = currentBackend;
    drag([textNode(prose(), from[0]), from[1]], [textNode(prose(), to[0]), to[1]]);
    root.querySelector<HTMLButtonElement>('[role="toolbar"] button[data-color="yellow"]')!.click();
    await vi.waitFor(() => expect(saves).toHaveLength(1));
    return saves[0];
  }

  let currentBackend: ReturnType<typeof backend>;

  const selectorsOf = (mark: Stored) => mark.selectors as Record<string, unknown>[];

  it("quotes a selection across an inline formula by the glyphs on screen", async () => {
    currentBackend = backend(PHOTO);
    await mountReader(root, contract.list_courses[0]);
    const mark = await select(["Plants turn ", 7], [" and water into sugar", 21]);

    expect(mark.quote).toBe("turn 6CO2\u200b and water into sugar");
    const painted = Array.from(prose().querySelectorAll(`mark[data-annotation-id="${mark.annotationId}"]`));
    expect(painted.map((m) => m.textContent).join("")).toBe("turn 6CO2\u200b and water into sugar");
    expect(prose().querySelector(".katex-mathml mark")).toBeNull();
  });

  it("anchors a mark next to a formula on the text around it, not the formula's hidden copies", async () => {
    currentBackend = backend(PHOTO);
    await mountReader(root, contract.list_courses[0]);
    const mark = await select([", so the cell", 23], [", so the cell", 34]);

    expect(mark.quote).toBe("so the cell");
    expect(selectorsOf(mark)[0]).toEqual({
      type: "TextQuoteSelector",
      exact: "so the cell",
      prefix: "Energy\nPlants turn 6CO2\u200b and water into sugar, ",
      suffix: " can grow.\n6CO2\u200b+6H2\u200bO\u2192C6\u200bH12\u200bO6\u200b+6O2\u200b\nThe sugar then fuels resp",
    });
    expect(selectorsOf(mark)[1]).toEqual({ type: "TextPositionSelector", start: 47, end: 58 });
  });

  it("anchors a mark that sits after a display formula", async () => {
    currentBackend = backend(PHOTO);
    await mountReader(root, contract.list_courses[0]);
    const mark = await select(["The sugar then fuels respiration.", 4], ["The sugar then fuels respiration.", 9]);

    expect(mark.quote).toBe("sugar");
    expect((selectorsOf(mark)[0] as { prefix: string }).prefix).toBe(
      "to sugar, so the cell can grow.\n6CO2\u200b+6H2\u200bO\u2192C6\u200bH12\u200bO6\u200b+6O2\u200b\nThe ",
    );
  });

  it("re-anchors those marks when the chapter is opened again, without rewriting them", async () => {
    currentBackend = backend(PHOTO);
    await mountReader(root, contract.list_courses[0]);
    const across = await select(["Plants turn ", 7], [" and water into sugar", 21]);
    clearMocks();
    document.body.innerHTML = '<div id="app"></div>';
    root = document.querySelector<HTMLElement>("#app")!;

    const again = backend(PHOTO, [across]);
    await mountReader(root, contract.list_courses[0]);
    const painted = Array.from(prose().querySelectorAll(`mark[data-annotation-id="${across.annotationId}"]`));
    expect(painted.map((m) => m.textContent).join("")).toBe("turn 6CO2\u200b and water into sugar");
    expect(margin().querySelector(".margin-conflicts")).toBeNull();
    expect(again.saves).toEqual([]);
  });
});
