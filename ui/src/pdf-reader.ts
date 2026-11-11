import "./pdf-reader.css";
import "./annotations.css";
import { reanchor, selectorsAt, type Selector, type Span } from "./anchor";
import {
  COLORS,
  deleteAnnotation,
  listSourceAnnotations,
  saveSourceAnnotation,
  type Color,
  type Mark,
  type MarkInput,
} from "./annotation-api";
import { claimMargin, stackTops } from "./annotations";
import type { PdfPage, PdfSource } from "./api";
import { boxOf, placeWords, rectsForSpan, type PlacedWord, type Rect } from "./pdf-geometry";
import { openSelectionPopup, type SelectionPopup } from "./selection-popup";
import type { Answer } from "./study-api";

// A PDF source read as its extracted text, each word placed at the rect ticket
// 03 extracted for it. Highlights are boxes in the same page fractions, so a
// highlight covers the words drawn under it at every zoom and window size.
// Marks are ticket 06's: the quote and position selectors index the page's
// extracted text, `locator` is the page and `rects` the stored page rects.

export interface PdfPosition {
  page: number;
  zoom: number;
}

export interface PdfSelectionActions {
  ask(quote: string, question: string): Promise<Answer>;
}

export interface PdfOptions {
  initialPage?: number;
  initialZoom?: number;
  onPosition?(position: PdfPosition): void;
  actions?: PdfSelectionActions;
  targetChapterTitle?: string | null;
  askDisabledReason?: string | null;
}

const NOTICE =
  "This PDF is shown as its extracted text, each word placed where it sits on the page. " +
  "Images, drawings and the original fonts are not shown.";
const NO_TEXT = "This PDF has no extractable text, so it cannot be read or highlighted here. It may be a scan.";
const EMPTY_MARGIN = "Highlight a passage in this PDF to keep a note here.";
const CAVEAT =
  "Some text on this page is drawn through a transformed form, which the extractor places wrong. " +
  "Word positions and sizes here may not match the original page, and neither may highlights on them. " +
  "Placing it correctly needs the pdfium fallback, a planned follow-up.";
const ZOOMS = [0.5, 0.75, 1, 1.25, 1.5, 2, 3];
const MAX_QUOTE = 2000;
const NOTE_SAVE_DELAY_MS = 600;
const CARD_GAP = 8;

interface Sheet {
  page: PdfPage;
  words: PlacedWord[];
  box: HTMLElement;
  layer: HTMLElement;
}

interface Anchor {
  locator: number;
  span: Span;
  selectors: Selector[];
  rects: Rect[];
}

interface Entry {
  mark: MarkInput;
  placed: boolean;
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string, text?: string) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function placeBox(node: HTMLElement, rect: Rect): void {
  const box = boxOf(rect);
  node.style.left = `${box.left * 100}%`;
  node.style.top = `${box.top * 100}%`;
  node.style.width = `${box.width * 100}%`;
  node.style.height = `${box.height * 100}%`;
}

function quoteOf(selectors: Selector[]): string {
  const quote = selectors.find((s) => s.type === "TextQuoteSelector");
  return quote?.type === "TextQuoteSelector" ? quote.exact : "";
}

function toInput({ createdAt: _c, updatedAt: _u, ...mark }: Mark): MarkInput {
  return mark;
}

/** The word span at one end of a selection, or null outside any word. */
function wordAt(node: Node, offset: number, edge: "start" | "end"): HTMLElement | null {
  let target: Node | null | undefined = node;
  if (node instanceof Element && !node.closest(".pdf-word")) {
    target = edge === "start" ? node.childNodes[offset] : node.childNodes[offset - 1];
  }
  const element = target instanceof Element ? target : target?.parentElement;
  return element?.closest<HTMLElement>(".pdf-word") ?? null;
}

/** Lays the source out into `host` and fills `margin` with its marks. */
export async function mountPdf(
  source: PdfSource,
  host: HTMLElement,
  margin: HTMLElement,
  options: PdfOptions = {},
): Promise<void> {
  const signal = claimMargin(margin);
  const scroller = host.closest<HTMLElement>(".document") ?? host;
  const name = source.name;
  const entries = new Map<string, Entry>();
  let popover: SelectionPopup | null = null;

  let currentPage = 1;
  let zoom = 1;
  if (options.initialZoom !== undefined && ZOOMS.includes(options.initialZoom)) {
    zoom = options.initialZoom;
  }

  let lastReportedPage = -1;
  let lastReportedZoom = -1;
  function reportPosition(): void {
    if (currentPage === lastReportedPage && zoom === lastReportedZoom) return;
    lastReportedPage = currentPage;
    lastReportedZoom = zoom;
    options.onPosition?.({ page: currentPage, zoom });
  }

  const totalPages = Math.max(1, source.pages.length);
  const toolbar = el("div", "pdf-toolbar");
  const notice = el("p", "pdf-notice", NOTICE);
  const zoomOut = el("button", "pdf-zoom", "−");
  const zoomLabel = el("span", "pdf-zoom-label");
  const zoomIn = el("button", "pdf-zoom", "+");
  zoomOut.type = zoomIn.type = "button";
  zoomOut.setAttribute("aria-label", "Zoom out");
  zoomIn.setAttribute("aria-label", "Zoom in");

  const pageNav = el("div", "pdf-page-nav");
  const pageInput = el("input", "pdf-page-input");
  pageInput.type = "number";
  pageInput.min = "1";
  pageInput.max = String(totalPages);
  pageInput.value = "1";
  pageInput.setAttribute("aria-label", "Page number");
  const pageTotal = el("span", "pdf-page-total", `/ ${totalPages}`);
  pageNav.append(pageInput, pageTotal);

  toolbar.append(zoomOut, zoomLabel, zoomIn, pageNav);
  const pages = el("div", "pdf-pages");
  host.replaceChildren(notice, toolbar, pages);
  if (source.pages.every((p) => p.words.length === 0)) notice.textContent = NO_TEXT;

  const sheets = source.pages.map((page) => renderPage(page, source.glyphHeightFactor));
  pages.append(...sheets.map((s) => s.box.closest(".pdf-sheet")!));
  applyZoom();

  function renderPage(page: PdfPage, glyphHeightFactor: number): Sheet {
    const sheet = el("section", "pdf-sheet");
    sheet.setAttribute("aria-label", `Page ${page.page}`);
    if (page.caveat === "transformedFormText") {
      const caveat = el("p", "pdf-caveat", CAVEAT);
      caveat.setAttribute("role", "note");
      sheet.append(caveat);
    }
    const box = el("div", "pdf-page");
    box.dataset.page = String(page.page);
    box.style.aspectRatio = `${page.width} / ${page.height}`;
    const layer = el("div", "pdf-highlights");
    box.append(layer);
    const words = placeWords(page.text, page.words);
    words.forEach((word, i) => {
      const span = el("span", "pdf-word", word.quote);
      span.dataset.index = String(i);
      placeBox(span, word.rect);
      // The rect is ticket 03's glyph box, glyphHeightFactor em tall, so the
      // em is its height over that factor, here in page widths for cqw.
      const em = (word.rect.h / glyphHeightFactor) * (page.height / page.width);
      span.style.fontSize = `${em * 100}cqw`;
      span.style.lineHeight = String(glyphHeightFactor);
      box.append(span);
    });
    box.append(el("div", "pdf-end"));
    sheet.append(box);
    return { page, words, box, layer };
  }

  /** Stretches each word's browser glyphs to its extracted width. */
  function fitWords(): void {
    for (const sheet of sheets) {
      const spans = Array.from(sheet.box.querySelectorAll<HTMLElement>(".pdf-word"));
      const pageWidth = sheet.box.getBoundingClientRect().width;
      spans.forEach((s) => (s.style.transform = ""));
      const natural = spans.map((s) => s.scrollWidth);
      spans.forEach((s, i) => {
        const target = sheet.words[i].rect.w * pageWidth;
        if (natural[i] > 0 && target > 0) s.style.transform = `scaleX(${target / natural[i]})`;
      });
    }
  }

  function applyZoom(): void {
    pages.style.setProperty("--zoom", String(zoom));
    zoomLabel.textContent = `${Math.round(zoom * 100)}%`;
    zoomOut.disabled = zoom === ZOOMS[0];
    zoomIn.disabled = zoom === ZOOMS[ZOOMS.length - 1];
    fitWords();
    layout();
    reportPosition();
  }

  function step(direction: 1 | -1): void {
    const next = ZOOMS[ZOOMS.indexOf(zoom) + direction];
    if (next === undefined) return;
    const anchor = scroller.scrollTop / Math.max(1, scroller.scrollHeight);
    zoom = next;
    applyZoom();
    scroller.scrollTop = anchor * scroller.scrollHeight;
  }

  function currentSheet(): Sheet {
    const anchorY = scroller.getBoundingClientRect().top + (toolbar.offsetHeight || 40);
    let active = sheets[0];
    for (const sheet of sheets) {
      const el = sheet.box.closest<HTMLElement>(".pdf-sheet") ?? sheet.box;
      if (el.getBoundingClientRect().top <= anchorY) {
        active = sheet;
      }
    }
    return active;
  }

  function updatePageIndicator(): void {
    if (document.activeElement !== pageInput) {
      pageInput.value = String(currentPage);
    }
    reportPosition();
  }

  function jumpToPage(target: number): void {
    const clamped = Math.max(1, Math.min(sheets.length, Math.round(target)));
    currentPage = clamped;
    updatePageIndicator();
    const targetSheet = sheetOf(clamped);
    if (!targetSheet) return;
    const sheetEl = targetSheet.box.closest<HTMLElement>(".pdf-sheet") ?? targetSheet.box;
    scroller.scrollTop += sheetEl.getBoundingClientRect().top - scroller.getBoundingClientRect().top;
  }

  pageInput.addEventListener("change", () => {
    const val = parseInt(pageInput.value, 10);
    if (!Number.isNaN(val)) jumpToPage(val);
    else updatePageIndicator();
  });
  pageInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      const val = parseInt(pageInput.value, 10);
      if (!Number.isNaN(val)) jumpToPage(val);
      pageInput.blur();
    }
  });

  if (options.initialPage !== undefined && options.initialPage > 1) {
    jumpToPage(options.initialPage);
  }

  function alertInMargin(text: string): void {
    const alert = el("p", "error", text);
    alert.setAttribute("role", "alert");
    margin.prepend(alert);
  }

  async function persist(mark: MarkInput): Promise<void> {
    try {
      await saveSourceAnnotation(name, mark);
    } catch (error) {
      if (!signal.aborted) alertInMargin(`Could not save this mark: ${message(error)}`);
    }
  }

  function sheetOf(locator: number): Sheet | undefined {
    return sheets.find((s) => s.page.page === locator);
  }

  function anchorAt(sheet: Sheet, span: Span): Anchor {
    return {
      locator: sheet.page.page,
      span,
      selectors: selectorsAt(sheet.page.text, span),
      rects: rectsForSpan(sheet.words, span),
    };
  }

  /** The selection snapped to whole words on one page, or null. */
  function selected(): Anchor | null {
    const selection = document.getSelection();
    if (!selection || selection.rangeCount === 0 || selection.isCollapsed) return null;
    const range = selection.getRangeAt(0);
    const first = wordAt(range.startContainer, range.startOffset, "start");
    const last = wordAt(range.endContainer, range.endOffset, "end");
    if (!first || !last) return null;
    const box = first.closest<HTMLElement>(".pdf-page");
    if (!box || box !== last.closest(".pdf-page")) return null;
    const sheet = sheetOf(Number(box.dataset.page));
    if (!sheet) return null;
    let [i, j] = [Number(first.dataset.index), Number(last.dataset.index)];
    if (i < j && range.startContainer.nodeType === Node.TEXT_NODE && range.startOffset >= first.textContent!.length) i++;
    if (j > i && range.endContainer.nodeType === Node.TEXT_NODE && range.endOffset === 0) j--;
    if (j < i) return null;
    const start = sheet.words[i].start;
    const end = Math.min(sheet.words[j].end, start + MAX_QUOTE);
    return anchorAt(sheet, { start, end });
  }

  function paint(mark: MarkInput): void {
    const sheet = sheetOf(mark.locator);
    if (!sheet) return;
    for (const rect of mark.rects) {
      const box = el("div", "pdf-highlight");
      box.dataset.annotationId = mark.annotationId;
      box.dataset.color = mark.color;
      placeBox(box, rect);
      sheet.layer.append(box);
    }
  }

  function unpaint(annotationId: string): void {
    for (const node of pages.querySelectorAll<HTMLElement>(".pdf-highlight")) {
      if (node.dataset.annotationId === annotationId) node.remove();
    }
  }

  function firstBox(annotationId: string): HTMLElement | undefined {
    return Array.from(pages.querySelectorAll<HTMLElement>(".pdf-highlight")).find(
      (n) => n.dataset.annotationId === annotationId,
    );
  }

  function render(): void {
    const all = [...entries.values()];
    const placed = all
      .filter((e) => e.placed)
      .sort((a, b) => a.mark.locator - b.mark.locator || (b.mark.rects[0]?.y ?? 0) - (a.mark.rects[0]?.y ?? 0));
    const lost = all.filter((e) => !e.placed);
    if (all.length === 0) {
      // Not a control and never focused: plain text that leaves the column
      // the moment the first mark lands, because `render` replaces it.
      margin.replaceChildren(el("p", "pdf-empty-note", EMPTY_MARGIN));
      return;
    }
    const track = el("div", "margin-track");
    if (lost.length > 0) {
      const group = el("section", "margin-conflicts");
      group.setAttribute("aria-label", "Marks that need attention");
      group.append(el("h2", undefined, "Needs attention"), ...lost.map(conflictCard));
      track.append(group);
    }
    track.append(...placed.map((e) => card(e.mark)));
    margin.replaceChildren(track);
    layout();
  }

  function layout(): void {
    const track = margin.querySelector<HTMLElement>(".margin-track");
    if (!track) return;
    const blocks = Array.from(track.children) as HTMLElement[];
    const trackTop = track.getBoundingClientRect().top;
    const wanted = blocks.map((block) => {
      const id = block.dataset.annotationId;
      const first = id ? firstBox(id) : undefined;
      return first ? first.getBoundingClientRect().top - trackTop : 0;
    });
    const tops = stackTops(wanted, blocks.map((b) => b.offsetHeight), CARD_GAP);
    blocks.forEach((block, i) => (block.style.top = `${tops[i]}px`));
    track.style.height = `${tops.length ? tops[tops.length - 1] + blocks[blocks.length - 1].offsetHeight : 0}px`;
  }

  function follow(): void {
    const track = margin.querySelector<HTMLElement>(".margin-track");
    if (track) track.style.transform = `translateY(${-scroller.scrollTop}px)`;
  }

  function noteField(mark: MarkInput): HTMLTextAreaElement {
    const field = el("textarea", "note-field");
    field.value = mark.note;
    field.placeholder = "Add a note";
    field.rows = 2;
    field.setAttribute("aria-label", "Margin note");
    let timer: ReturnType<typeof setTimeout> | undefined;
    const save = () => {
      clearTimeout(timer);
      const entry = entries.get(mark.annotationId);
      if (!entry || entry.mark.note === field.value) return;
      entry.mark = { ...entry.mark, note: field.value };
      void persist(entry.mark);
    };
    field.addEventListener("input", () => {
      clearTimeout(timer);
      timer = setTimeout(save, NOTE_SAVE_DELAY_MS);
    });
    field.addEventListener("blur", save);
    return field;
  }

  function removeButton(mark: MarkInput, label: string): HTMLButtonElement {
    const button = el("button", "card-action", label);
    button.type = "button";
    button.addEventListener("click", async () => {
      try {
        await deleteAnnotation(mark.annotationId);
      } catch (error) {
        alertInMargin(`Could not delete this mark: ${message(error)}`);
        return;
      }
      entries.delete(mark.annotationId);
      unpaint(mark.annotationId);
      render();
    });
    return button;
  }

  function card(mark: MarkInput): HTMLElement {
    const node = el("div", "note-card");
    node.dataset.annotationId = mark.annotationId;
    node.dataset.color = mark.color;
    const actions = el("div", "card-actions");
    actions.append(removeButton(mark, "Remove"));
    node.append(noteField(mark), actions);
    return node;
  }

  function conflictCard(entry: Entry): HTMLElement {
    const { mark } = entry;
    const node = el("div", "note-card conflicted");
    node.dataset.color = mark.color;
    node.dataset.conflictId = mark.annotationId;
    const why =
      mark.resolution === "ambiguous"
        ? `This passage now appears more than once on page ${mark.locator}.`
        : `This passage is no longer on page ${mark.locator}.`;
    const hint = el("p", "card-hint");
    const reattach = el("button", "card-action", "Re-attach to selection");
    reattach.type = "button";
    reattach.addEventListener("mousedown", (event) => event.preventDefault());
    reattach.addEventListener("click", () => {
      const anchor = selected();
      if (!anchor) {
        hint.textContent = "Select the passage on a page first.";
        return;
      }
      const next = place(entry.mark, anchor);
      entries.set(mark.annotationId, { mark: next, placed: true });
      paint(next);
      document.getSelection()?.removeAllRanges();
      render();
      void persist(next);
    });
    const actions = el("div", "card-actions");
    actions.append(reattach, removeButton(mark, "Delete"));
    node.append(el("blockquote", "lost-quote", mark.quote), el("p", "card-why", why), noteField(mark), hint, actions);
    return node;
  }

  function place(mark: MarkInput, anchor: Anchor): MarkInput {
    return {
      ...mark,
      quote: quoteOf(anchor.selectors),
      selectors: anchor.selectors,
      locator: anchor.locator,
      rects: anchor.rects,
      resolution: "resolved",
    };
  }

  function closePopover(): void {
    popover?.close();
    popover = null;
  }

  function create(color: Color): void {
    const anchor = selected();
    closePopover();
    if (!anchor) return;
    const mark = place(
      { annotationId: crypto.randomUUID(), kind: "highlight", color, quote: "", note: "", selectors: [], resolution: "resolved", locator: 1, rects: [] },
      anchor,
    );
    entries.set(mark.annotationId, { mark, placed: true });
    paint(mark);
    document.getSelection()?.removeAllRanges();
    render();
    for (const c of margin.querySelectorAll<HTMLElement>(".note-card")) {
      if (c.dataset.annotationId === mark.annotationId) c.querySelector("textarea")?.focus();
    }
    void persist(mark);
  }

  function offerHighlight(): void {
    if (popover?.busy()) return;
    closePopover();
    const anchor = selected();
    if (!anchor) return;
    const selection = document.getSelection();
    if (!selection || selection.rangeCount === 0) return;
    const rect = selection.getRangeAt(0).getBoundingClientRect();
    popover = openSelectionPopup({
      host: scroller,
      rect,
      quote: quoteOf(anchor.selectors),
      onColor: create,
      actions: {
        ask: options.actions?.ask,
      },
      targetChapterTitle: options.targetChapterTitle,
      askDisabledReason: options.askDisabledReason,
      onClose: () => (popover = null),
    });
  }

  function onScroll(): void {
    follow();
    const active = currentSheet();
    if (active && active.page.page !== currentPage) {
      currentPage = active.page.page;
      updatePageIndicator();
    }
  }

  zoomOut.addEventListener("click", () => step(-1), { signal });
  zoomIn.addEventListener("click", () => step(1), { signal });
  pages.addEventListener("mouseup", offerHighlight, { signal });
  pages.addEventListener("keyup", offerHighlight, { signal });
  document.addEventListener(
    "selectionchange",
    () => {
      if (popover && !popover.engaged() && !selected()) {
        closePopover();
      }
    },
    { signal },
  );
  // WebKit resolves a drag point in the empty space between positioned words
  // to the page's edges, which drops the selection. While a drag is on, an
  // unselectable block covers the page behind the words and sits in the DOM
  // just after the word holding the moving end, so a gap resolves there.
  // The same technique as pdf.js's text layer `endOfContent`.
  let previous: Range | null = null;
  pages.addEventListener("mousedown", () => pages.classList.add("selecting"), { signal });
  document.addEventListener(
    "mouseup",
    () => {
      pages.classList.remove("selecting");
      for (const end of pages.querySelectorAll(".pdf-end")) end.parentElement?.append(end);
      previous = null;
    },
    { signal },
  );
  document.addEventListener(
    "selectionchange",
    () => {
      const selection = document.getSelection();
      if (!pages.classList.contains("selecting") || !selection || selection.rangeCount === 0) return;
      const range = selection.getRangeAt(0);
      const movedStart =
        previous !== null &&
        (range.compareBoundaryPoints(Range.END_TO_END, previous) === 0 ||
          range.compareBoundaryPoints(Range.START_TO_END, previous) === 0);
      const container = movedStart ? range.startContainer : range.endContainer;
      const word = (container instanceof Element ? container : container.parentElement)?.closest(".pdf-word");
      const end = word?.parentElement?.querySelector(".pdf-end");
      if (word && end) word.parentElement!.insertBefore(end, movedStart ? word : word.nextSibling);
      previous = range.cloneRange();
    },
    { signal },
  );
  scroller.addEventListener("scroll", onScroll, { signal, passive: true });
  window.addEventListener("resize", () => (fitWords(), layout()), { signal });
  signal.addEventListener("abort", closePopover);
  void document.fonts?.ready.then(() => !signal.aborted && applyZoom());

  let marks: Mark[];
  try {
    marks = await listSourceAnnotations(name);
  } catch (error) {
    if (!signal.aborted) alertInMargin(`Could not load marks: ${message(error)}`);
    return;
  }
  if (signal.aborted) return;

  // Re-anchor page by page against the text extracted now. A matched mark
  // takes the rects of the words it covers today; anything else is kept whole
  // and flagged, never deleted.
  const changed: MarkInput[] = [];
  const byPage = new Map<number, MarkInput[]>();
  for (const mark of marks.map(toInput)) byPage.set(mark.locator, [...(byPage.get(mark.locator) ?? []), mark]);
  for (const [locator, group] of byPage) {
    const sheet = sheetOf(locator);
    const { anchored, conflicted } = sheet
      ? reanchor(group, sheet.page.text)
      : { anchored: [], conflicted: group.map((mark) => ({ mark, resolution: "unresolved" as const })) };
    for (const { mark, span } of anchored) {
      const next = place(mark, anchorAt(sheet!, span));
      if (JSON.stringify(next) !== JSON.stringify(mark)) changed.push(next);
      entries.set(mark.annotationId, { mark: next, placed: true });
      paint(next);
    }
    for (const { mark, resolution } of conflicted) {
      const next = { ...mark, resolution };
      if (resolution !== mark.resolution) changed.push(next);
      entries.set(mark.annotationId, { mark: next, placed: false });
    }
  }
  render();
  await Promise.all(changed.map(persist));
}
