// @vitest-environment happy-dom
import fs from "node:fs";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import contract from "./fixtures/reader-contract.json";
import wizardContract from "./fixtures/wizard-contract.json";
import pdfContract from "./fixtures/pdf-source-contract.json";
import { mountPdf, type PdfPosition } from "./pdf-reader";
import type { PdfSource } from "./api";
import { mountReader } from "./reader";

type Stored = Record<string, unknown> & { annotationId: string };

/** The Rust side: the reader contract plus an in-memory store of source marks. */
function backend(stored: Stored[] = []) {
  const db = new Map(stored.map((m) => [m.annotationId, structuredClone(m)]));
  const saves: Stored[] = [];
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
        return contract.read_chapter_ready;
      case "list_annotations":
        return [];
      case "list_course_sources":
        return pdfContract.list_pdf_sources.map((name) => ({
          name,
          path: `/library/sources/${name}`,
          kind: "pdf",
          url: null,
          fetchedAt: null,
          bytes: 1,
        }));
      case "read_pdf_source":
        return pdfContract.read_pdf_source;
      case "list_source_annotations":
        return [...db.values()];
      case "save_source_annotation": {
        expect(args.sourceName).toBe("lecture.pdf");
        const mark = { ...(args.mark as Stored), createdAt: 1_700_000_000, updatedAt: 1_700_000_001 };
        db.set(mark.annotationId, mark);
        saves.push(structuredClone(mark));
        return mark;
      }
      case "delete_annotation":
        return db.delete(args.annotationId as string);
      case "ask_about":
        return {
          id: 1,
          quote: args.quote,
          question: args.question,
          answer: "The bilayer acts as a selective barrier.",
          sources: ["lecture.pdf"],
          askedAt: "2026-11-09T00:00:00Z",
        };
      case "revise_section":
        return {
          ...contract.read_chapter_ready,
          markdown: "# Cell Membranes\n\nREVISED SECTION\n",
        };
      default:
        throw new Error(`unexpected command ${cmd}`);
    }
  });
  return { db, saves };
}

let root: HTMLElement;

beforeEach(() => {
  document.body.innerHTML = '<div id="app"></div>';
  root = document.querySelector<HTMLElement>("#app")!;
});

afterEach(() => {
  clearMocks();
  document.getSelection()?.removeAllRanges();
});

async function openLecture(): Promise<void> {
  await mountReader(root, contract.list_courses[0]);
  const button = root.querySelector<HTMLButtonElement>('nav button[data-source-name="lecture.pdf"]');
  expect(button, "the PDF is listed in the sidebar").not.toBeNull();
  button!.click();
  await vi.waitFor(() => expect(root.querySelector(".pdf-page")).not.toBeNull());
}

function page(n: number): HTMLElement {
  return root.querySelector<HTMLElement>(`.pdf-page[data-page="${n}"]`)!;
}

function word(n: number, quote: string): HTMLElement {
  const found = Array.from(page(n).querySelectorAll<HTMLElement>(".pdf-word")).find((w) => w.textContent === quote);
  if (!found) throw new Error(`no word ${quote} on page ${n}`);
  return found;
}

/** The CSS box of an element, in percent of its page. */
function percentBox(node: HTMLElement): number[] {
  return [node.style.left, node.style.top, node.style.width, node.style.height].map((v) => parseFloat(v));
}

function drag(start: [Node, number], end: [Node, number]): void {
  const range = document.createRange();
  range.setStart(...start);
  range.setEnd(...end);
  const selection = document.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
  page(1).dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
}

// "Hello world" on page 1, hand geometry from ticket 03's fixture:
// x 72..190.68 pt and y 715.2..744 pt of a 612 x 792 pt page.
const HELLO_WORLD_PERCENT = [(72 / 612) * 100, (48 / 792) * 100, (118.68 / 612) * 100, (28.8 / 792) * 100];

describe("reading a PDF source", () => {
  it("opens from the sidebar as selectable words placed at their extracted rects, and says so", async () => {
    backend();
    await openLecture();
    const hello = word(1, "Hello");
    const [left, top, width, height] = percentBox(hello);
    expect(left).toBeCloseTo((72 / 612) * 100, 6);
    expect(top).toBeCloseTo((48 / 792) * 100, 6);
    expect(width).toBeCloseTo((54.672 / 612) * 100, 6);
    expect(height).toBeCloseTo((28.8 / 792) * 100, 6);
    expect(root.querySelector(".pdf-notice")?.textContent).toMatch(/extracted text/);
  });

  it("warns on a page whose text came through a transformed form, and names the pdfium follow-up", async () => {
    backend();
    await openLecture();
    expect(page(1).closest(".pdf-sheet")?.querySelector(".pdf-caveat")).toBeNull();
    const caveat = page(2).closest(".pdf-sheet")?.querySelector('.pdf-caveat[role="note"]');
    expect(caveat?.textContent).toMatch(/may not match/);
    expect(caveat?.textContent).toMatch(/pdfium/);
  });

  it("uses theme tokens for caveat wash and page shadow instead of literal colors", () => {
    const css = fs.readFileSync(path.resolve(__dirname, "pdf-reader.css"), "utf-8");
    expect(css).not.toContain("rgba(196, 66, 59, 0.06)");
    expect(css).not.toContain("rgba(15, 15, 15, 0.06)");
    expect(css).toContain("var(--pdf-caveat-bg)");
    expect(css).toContain("var(--shadow-card)");
  });

  it("shows an empty-state line in the margin while the PDF has no marks, and drops it when the first mark lands", async () => {
    const { saves } = backend();
    await openLecture();
    const margin = root.querySelector<HTMLElement>("aside.margin")!;
    await vi.waitFor(() =>
      expect(margin.querySelector(".pdf-empty-note")?.textContent).toBe("Highlight a passage in this PDF to keep a note here."),
    );
    const note = margin.querySelector<HTMLElement>(".pdf-empty-note")!;
    expect(note.closest("button, a, input, select, textarea, summary, [tabindex]")).toBeNull();

    drag([word(1, "Hello").firstChild!, 0], [word(1, "world").firstChild!, 5]);
    root.querySelector<HTMLButtonElement>('[role="toolbar"] button[data-color="yellow"]')!.click();
    await vi.waitFor(() => expect(saves).toHaveLength(1));
    expect(margin.querySelector(".pdf-empty-note")).toBeNull();
    expect(margin.querySelector(".note-card")).not.toBeNull();
  });

  it("shows current page and total pages in the toolbar, and jumping to a page scrolls to it", async () => {
    backend();
    await openLecture();
    const pageInput = root.querySelector<HTMLInputElement>("input.pdf-page-input");
    expect(pageInput).not.toBeNull();
    expect(pageInput?.value).toBe("1");
    expect(pageInput?.max).toBe("2");
    const total = root.querySelector<HTMLElement>(".pdf-page-total");
    expect(total?.textContent).toMatch(/2/);

    const scroller = root.querySelector<HTMLElement>(".document")!;
    scroller.getBoundingClientRect = () => ({ top: 0, bottom: 800, left: 0, right: 800, width: 800, height: 800, x: 0, y: 0, toJSON: () => ({}) });

    const sheet1 = page(1).closest<HTMLElement>(".pdf-sheet")!;
    const sheet2 = page(2).closest<HTMLElement>(".pdf-sheet")!;
    sheet1.getBoundingClientRect = () => ({ top: 0, bottom: 800, left: 0, right: 800, width: 800, height: 800, x: 0, y: 0, toJSON: () => ({}) });
    sheet2.getBoundingClientRect = () => ({ top: 800, bottom: 1600, left: 0, right: 800, width: 800, height: 800, x: 0, y: 800, toJSON: () => ({}) });

    pageInput!.value = "2";
    pageInput!.dispatchEvent(new Event("change"));
    expect(scroller.scrollTop).toBe(800);
    expect(pageInput?.value).toBe("2");

    // After scrolling to page 2, sheet2 is at the top and sheet1 is scrolled above
    sheet1.getBoundingClientRect = () => ({ top: -800, bottom: 0, left: 0, right: 800, width: 800, height: 800, x: 0, y: -800, toJSON: () => ({}) });
    sheet2.getBoundingClientRect = () => ({ top: 0, bottom: 800, left: 0, right: 800, width: 800, height: 800, x: 0, y: 0, toJSON: () => ({}) });

    // Clamps jump to total page count
    pageInput!.value = "99";
    pageInput!.dispatchEvent(new Event("change"));
    expect(scroller.scrollTop).toBe(800);
    expect(pageInput?.value).toBe("2");

    pageInput!.value = "0";
    pageInput!.dispatchEvent(new Event("change"));
    expect(scroller.scrollTop).toBe(0);
    expect(pageInput?.value).toBe("1");
  });

  it("updates page readout when scrolling between pages", async () => {
    backend();
    await openLecture();
    const pageInput = root.querySelector<HTMLInputElement>("input.pdf-page-input")!;
    const scroller = root.querySelector<HTMLElement>(".document")!;

    const sheet1 = page(1).closest<HTMLElement>(".pdf-sheet")!;
    const sheet2 = page(2).closest<HTMLElement>(".pdf-sheet")!;
    scroller.getBoundingClientRect = () => ({ top: 0, bottom: 800, left: 0, right: 800, width: 800, height: 800, x: 0, y: 0, toJSON: () => ({}) });

    // Scrolling down so sheet2 reaches anchor line
    sheet1.getBoundingClientRect = () => ({ top: -800, bottom: 0, left: 0, right: 800, width: 800, height: 800, x: 0, y: -800, toJSON: () => ({}) });
    sheet2.getBoundingClientRect = () => ({ top: 40, bottom: 840, left: 0, right: 800, width: 800, height: 800, x: 0, y: 40, toJSON: () => ({}) });

    scroller.scrollTop = 800;
    scroller.dispatchEvent(new Event("scroll"));

    expect(pageInput.value).toBe("2");
  });

  it("calls onPosition when page or zoom changes and respects initialPage and initialZoom", async () => {
    const onPosition = vi.fn();
    const host = document.createElement("div");
    const margin = document.createElement("div");
    root.append(host, margin);

    await mountPdf(pdfContract.read_pdf_source as unknown as PdfSource, host, margin, { initialPage: 2, initialZoom: 1.5, onPosition });
    expect(onPosition).toHaveBeenCalledWith({ page: 2, zoom: 1.5 });
    const zoomLabel = host.querySelector<HTMLElement>(".pdf-zoom-label");
    expect(zoomLabel?.textContent).toBe("150%");
    const pageInput = host.querySelector<HTMLInputElement>("input.pdf-page-input");
    expect(pageInput?.value).toBe("2");

    const zoomOut = host.querySelector<HTMLButtonElement>('button[aria-label="Zoom out"]')!;
    zoomOut.click();
    expect(onPosition).toHaveBeenLastCalledWith({ page: 2, zoom: 1.25 });

    pageInput!.value = "1";
    pageInput!.dispatchEvent(new Event("change"));
    expect(onPosition).toHaveBeenLastCalledWith({ page: 1, zoom: 1.25 });
  });
});

describe("PDF selection actions", () => {
  it("offers Ask and never Fix alongside highlight swatches on a PDF selection, but keeps Fix on a chapter selection", async () => {
    backend();
    await mountReader(root, contract.list_courses[0]);
    // Chapter selection has Fix this section and Ask
    const walker = document.createTreeWalker(root.querySelector(".prose")!, NodeFilter.SHOW_TEXT);
    let textNode: Text | null = null;
    for (let node = walker.nextNode(); node && !textNode; node = walker.nextNode()) {
      if ((node as Text).data.includes("basic unit")) textNode = node as Text;
    }
    const range = document.createRange();
    range.setStart(textNode!, textNode!.data.indexOf("basic unit"));
    range.setEnd(textNode!, textNode!.data.indexOf("basic unit") + "basic unit".length);
    const sel = document.getSelection()!;
    sel.removeAllRanges();
    sel.addRange(range);
    root.querySelector(".prose")!.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));

    const chapterPopup = root.querySelector<HTMLElement>(".mark-popover");
    expect(chapterPopup).not.toBeNull();
    expect(chapterPopup?.querySelector('button[data-action="fix"]')?.textContent).toBe("Fix this section");
    expect(chapterPopup?.querySelector('button[data-action="ask"]')?.textContent).toBe("Ask");

    // Open PDF and select text
    const button = root.querySelector<HTMLButtonElement>('nav button[data-source-name="lecture.pdf"]')!;
    button.click();
    await vi.waitFor(() => expect(root.querySelector(".pdf-page")).not.toBeNull());

    drag([word(1, "Hello").firstChild!, 0], [word(1, "world").firstChild!, 5]);
    const pdfPopup = root.querySelector<HTMLElement>(".mark-popover");
    expect(pdfPopup).not.toBeNull();
    expect(pdfPopup?.querySelector('button[data-action="fix"]')).toBeNull();
    const ask = pdfPopup?.querySelector<HTMLButtonElement>('button[data-action="ask"]');
    expect(ask?.textContent).toBe("Ask");
  });

  it("asking about a selected PDF passage notes the saved chapter and renders the answer", async () => {
    backend();
    await openLecture();
    drag([word(1, "Hello").firstChild!, 0], [word(1, "world").firstChild!, 5]);
    const askBtn = root.querySelector<HTMLButtonElement>('.mark-popover button[data-action="ask"]')!;
    expect(askBtn.title).toContain("Saved with chapter Cells");
    askBtn.click();

    const popup = root.querySelector<HTMLElement>(".mark-popover")!;
    expect(popup.textContent).toContain("Saved with chapter Cells");

    const field = root.querySelector<HTMLTextAreaElement>(".mark-popover textarea.popup-field")!;
    expect(field).not.toBeNull();
    field.value = "What is the meaning of hello?";
    root.querySelector<HTMLFormElement>(".mark-popover form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));

    await vi.waitFor(() => expect(root.querySelector<HTMLElement>(".mark-popover")?.dataset.phase).toBe("done"));
    expect(root.querySelector(".mark-popover .popup-answer")?.textContent).toMatch(/selective barrier/);
    expect(popup.textContent).toContain("Saved with chapter Cells");
  });

  it("disables Ask with a short reason when no chapter is written yet", async () => {
    const unwrittenChapters = contract.list_chapters.map((c) => ({ ...c, placeholder: true }));
    mockIPC((cmd) => {
      switch (cmd) {
        case "list_courses":
          return contract.list_courses;
        case "get_outline":
          return wizardContract.outline_approved;
        case "list_chapters":
          return unwrittenChapters;
        case "list_annotations":
          return [];
        case "list_course_sources":
          return pdfContract.list_pdf_sources.map((name) => ({
            name,
            path: `/library/sources/${name}`,
            kind: "pdf",
            url: null,
            fetchedAt: null,
            bytes: 1,
          }));
        case "read_pdf_source":
          return pdfContract.read_pdf_source;
        case "list_source_annotations":
          return [];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    await mountReader(root, contract.list_courses[0]);
    const button = root.querySelector<HTMLButtonElement>('nav button[data-source-name="lecture.pdf"]')!;
    button.click();
    await vi.waitFor(() => expect(root.querySelector(".pdf-page")).not.toBeNull());

    drag([word(1, "Hello").firstChild!, 0], [word(1, "world").firstChild!, 5]);
    const pdfPopup = root.querySelector<HTMLElement>(".mark-popover");
    expect(pdfPopup).not.toBeNull();
    expect(pdfPopup?.querySelector('button[data-action="fix"]')).toBeNull();
    const askBtn = pdfPopup?.querySelector<HTMLButtonElement>('button[data-action="ask"]')!;
    expect(askBtn.disabled).toBe(true);
    expect(askBtn.title).toMatch(/No chapter is written yet/i);
    const reason = pdfPopup?.querySelector<HTMLElement>(".popover-disabled-reason")!;
    expect(reason).not.toBeNull();
    expect(reason.textContent).toMatch(/No chapter is written yet/i);
  });

  it("shows no Ask button when mounted without actions.ask and without askDisabledReason", async () => {
    const host = document.createElement("div");
    const margin = document.createElement("div");
    root.append(host, margin);
    await mountPdf(pdfContract.read_pdf_source as unknown as PdfSource, host, margin, {});

    // Select text
    const firstWord = host.querySelector<HTMLElement>('.pdf-word[data-index="0"]')!;
    const lastWord = host.querySelector<HTMLElement>('.pdf-word[data-index="1"]')!;
    const range = document.createRange();
    range.setStart(firstWord.firstChild!, 0);
    range.setEnd(lastWord.firstChild!, lastWord.textContent!.length);
    const sel = document.getSelection()!;
    sel.removeAllRanges();
    sel.addRange(range);
    host.querySelector(".pdf-pages")!.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));

    const popup = host.querySelector<HTMLElement>(".mark-popover");
    expect(popup).not.toBeNull();
    expect(popup?.querySelector('button[data-action="ask"]')).toBeNull();
    expect(popup?.querySelector('button[data-action="fix"]')).toBeNull();
    expect(popup?.querySelectorAll("button.swatch")).toHaveLength(5);
  });
});

describe("highlighting a PDF", () => {
  it("stores page, quote and one line rect, and paints the highlight at that rect", async () => {
    const { saves } = backend();
    await openLecture();
    drag([word(1, "Hello").firstChild!, 0], [word(1, "world").firstChild!, 5]);
    root.querySelector<HTMLButtonElement>('[role="toolbar"] button[data-color="yellow"]')!.click();

    await vi.waitFor(() => expect(saves).toHaveLength(1));
    const mark = saves[0] as Record<string, unknown>;
    expect(mark.locator).toBe(1);
    expect(mark.quote).toBe("Hello world");
    expect(mark.selectors).toEqual(pdfContract.save_source_annotation.mark.selectors);
    const [rect] = mark.rects as Record<string, number>[];
    const want = pdfContract.save_source_annotation.mark.rects[0];
    for (const key of ["x", "y", "w", "h"] as const) expect(rect[key], key).toBeCloseTo(want[key], 9);

    const painted = page(1).querySelector<HTMLElement>(`.pdf-highlight[data-annotation-id="${mark.annotationId}"]`);
    expect(painted?.dataset.color).toBe("yellow");
    percentBox(painted!).forEach((v, i) => expect(v).toBeCloseTo(HELLO_WORLD_PERCENT[i], 6));
  });

  it("zooming changes the page size and leaves highlights and words in the same page fractions", async () => {
    backend([pdfContract.list_source_annotations[0] as Stored]);
    await openLecture();
    const pages = root.querySelector<HTMLElement>(".pdf-pages")!;
    const before = pages.style.getPropertyValue("--zoom");
    root.querySelector<HTMLButtonElement>('button[aria-label="Zoom in"]')!.click();
    expect(pages.style.getPropertyValue("--zoom")).not.toBe(before);
    const painted = page(1).querySelector<HTMLElement>(".pdf-highlight")!;
    percentBox(painted).forEach((v, i) => expect(v).toBeCloseTo(HELLO_WORLD_PERCENT[i], 6));
  });
});

describe("PDF marks across reopening", () => {
  it("paints a stored mark with its note, and moves a mark whose quote is gone to Needs attention untouched", async () => {
    const vanished: Stored = {
      ...(pdfContract.list_source_annotations[0] as Stored),
      annotationId: "pdf-mark-2",
      quote: "Goodbye",
      note: "Keep me.",
      selectors: [{ type: "TextQuoteSelector", exact: "Goodbye", prefix: "", suffix: "" }],
    };
    const { db } = backend([pdfContract.list_source_annotations[0] as Stored, vanished]);
    await openLecture();

    const painted = page(1).querySelector<HTMLElement>('.pdf-highlight[data-annotation-id="pdf-mark-1"]');
    expect(painted).not.toBeNull();
    const margin = root.querySelector<HTMLElement>("aside.margin")!;
    await vi.waitFor(() => expect(margin.querySelector(".margin-conflicts")).not.toBeNull());
    const fields = Array.from(margin.querySelectorAll<HTMLTextAreaElement>(".note-field")).map((f) => f.value);
    expect(fields).toContain("The first words on the page.");
    const lost = margin.querySelector<HTMLElement>('.margin-conflicts [data-conflict-id="pdf-mark-2"]')!;
    expect(lost.querySelector(".lost-quote")?.textContent).toBe("Goodbye");
    expect(lost.querySelector<HTMLTextAreaElement>(".note-field")?.value).toBe("Keep me.");
    await vi.waitFor(() => expect(db.get("pdf-mark-2")?.resolution).toBe("unresolved"));
    expect(db.get("pdf-mark-2")?.note).toBe("Keep me.");
  });
});
