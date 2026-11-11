import "./annotations.css";
import { reanchor, selectorsAt, type Selector, type Span } from "./anchor";
import {
  COLORS,
  deleteAnnotation,
  listAnnotations,
  saveAnnotation,
  type Color,
  type Mark,
  type MarkInput,
} from "./annotation-api";
import { openSelectionPopup, type SelectionActions, type SelectionPopup } from "./selection-popup";

// Highlights and margin notes over one rendered chapter. Every position indexes
// the canonical text: the concatenated text nodes under the `.prose` element,
// less the hidden MathML copy KaTeX writes for each formula. Capture,
// re-anchor and paint all read that one string.

/** The store caps an exact quote at 2000 characters; stay inside it. */
const MAX_QUOTE = 2000;
const NOTE_SAVE_DELAY_MS = 600;

interface Entry {
  mark: MarkInput;
  span: Span | null;
}

/** Where the chapter's note cards are drawn, and how the marks report to the panel around them. */
export interface NotesSink {
  host: HTMLElement;
  count(count: number): void;
  failed(error: unknown): void;
}

const mounted = new WeakMap<HTMLElement, AbortController>();

/** Ends whatever owned this margin before and returns the new owner's lifetime. */
export function claimMargin(margin: HTMLElement): AbortSignal {
  mounted.get(margin)?.abort();
  const lifetime = new AbortController();
  mounted.set(margin, lifetime);
  return lifetime.signal;
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string, text?: string) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

/**
 * The text nodes a reader sees, in document order. KaTeX repeats every formula
 * as MathML for screen readers, and that copy holds the glyphs again plus the
 * TeX source, so it is left out of the chapter's text.
 */
function* chapterText(node: Node): Generator<Text> {
  for (const child of Array.from(node.childNodes)) {
    if (child.nodeType === Node.TEXT_NODE) yield child as Text;
    else if (
      child instanceof Element &&
      !child.classList.contains("katex-mathml") &&
      !child.classList.contains("visual-notice") &&
      !child.classList.contains("visual-placeholder")
    )
      yield* chapterText(child);
  }
}

function canonicalText(prose: HTMLElement): string {
  let text = "";
  for (const node of chapterText(prose)) text += node.data;
  return text;
}

function textOffset(prose: HTMLElement, node: Node, offset: number): number {
  const point = document.createRange();
  point.setStart(node, offset);
  let pos = 0;
  for (const text of chapterText(prose)) {
    if (text === node) return pos + offset;
    if (point.comparePoint(text, text.length) > 0) break;
    pos += text.length;
  }
  return pos;
}

/** The selection as a span of canonical text, trimmed of edge whitespace. */
function selectedSpan(prose: HTMLElement): Span | null {
  const selection = document.getSelection();
  if (!selection || selection.rangeCount === 0 || selection.isCollapsed) return null;
  const range = selection.getRangeAt(0);
  if (!prose.contains(range.startContainer) || !prose.contains(range.endContainer)) return null;
  const text = canonicalText(prose);
  let start = textOffset(prose, range.startContainer, range.startOffset);
  let end = textOffset(prose, range.endContainer, range.endOffset);
  while (start < end && /\s/.test(text[start])) start++;
  while (end > start && /\s/.test(text[end - 1])) end--;
  end = Math.min(end, start + MAX_QUOTE);
  return start < end ? { start, end } : null;
}

function paint(prose: HTMLElement, span: Span, mark: MarkInput): void {
  const pieces: [Text, number, number][] = [];
  let pos = 0;
  for (const text of chapterText(prose)) {
    if (pos >= span.end) break;
    const from = Math.max(span.start, pos) - pos;
    const to = Math.min(span.end, pos + text.data.length) - pos;
    if (from < to && text.data.slice(from, to).trim() !== "") pieces.push([text, from, to]);
    pos += text.data.length;
  }
  for (const [text, from, to] of pieces) {
    const inside = from > 0 ? text.splitText(from) : text;
    if (to - from < inside.data.length) inside.splitText(to - from);
    const wrap = el("mark", "hl");
    wrap.dataset.annotationId = mark.annotationId;
    wrap.dataset.color = mark.color;
    inside.replaceWith(wrap);
    wrap.append(inside);
  }
}

function byId<T extends HTMLElement>(scope: ParentNode, selector: string, annotationId: string): T[] {
  return Array.from(scope.querySelectorAll<T>(selector)).filter((node) => node.dataset.annotationId === annotationId);
}

function unpaint(prose: HTMLElement, annotationId: string): void {
  for (const wrap of byId(prose, "mark.hl", annotationId)) wrap.replaceWith(...wrap.childNodes);
  prose.normalize();
}

function focusNote(margin: HTMLElement, annotationId: string): void {
  const panelToggle = margin.querySelector<HTMLButtonElement>("button.panel-toggle[aria-expanded='false']");
  if (panelToggle) panelToggle.click();
  const notesSection = margin.querySelector<HTMLElement>('[data-section="notes"]');
  if (notesSection?.dataset.collapsed === "true") {
    notesSection.querySelector<HTMLElement>(".section-head")?.click();
  }

  byId(margin, ".note-card", annotationId)[0]?.querySelector("textarea")?.focus();
}

function quoteOf(selectors: Selector[]): string {
  const quote = selectors.find((s) => s.type === "TextQuoteSelector");
  return quote?.type === "TextQuoteSelector" ? quote.exact : "";
}

function toInput({ createdAt: _c, updatedAt: _u, ...mark }: Mark): MarkInput {
  return mark;
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Tops for cards in document order, each pushed below the one above it. The PDF view lays its margin out with this. */
export function stackTops(wanted: number[], heights: number[], gap: number): number[] {
  let floor = -Infinity;
  return wanted.map((top, i) => {
    const placed = Math.max(top, floor);
    floor = placed + heights[i] + gap;
    return placed;
  });
}

/**
 * Loads the chapter's marks, re-anchors them against the text now on screen,
 * persists what moved or conflicted, paints the highlights and fills the
 * study panel's notes. Resolves once the notes show the result. `margin` is
 * the column that owns the mount, so the next chapter or a PDF replaces it.
 */
export async function mountAnnotations(
  chapterId: string,
  prose: HTMLElement,
  margin: HTMLElement,
  notes: NotesSink,
  actions: SelectionActions,
): Promise<void> {
  const signal = claimMargin(margin);
  const page = prose.closest<HTMLElement>(".document") ?? prose;
  const entries = new Map<string, Entry>();
  let popover: SelectionPopup | null = null;

  function alertInNotes(text: string): void {
    const alert = el("p", "error", text);
    alert.setAttribute("role", "alert");
    notes.host.prepend(alert);
  }

  async function persist(mark: MarkInput): Promise<void> {
    try {
      await saveAnnotation(chapterId, mark);
    } catch (error) {
      if (!signal.aborted) alertInNotes(`Could not save this mark: ${message(error)}`);
    }
  }

  let marks: Mark[];
  try {
    marks = await listAnnotations(chapterId);
  } catch (error) {
    if (!signal.aborted) notes.failed(error);
    return;
  }
  if (signal.aborted || !prose.isConnected) return;

  const text = canonicalText(prose);
  const { anchored, conflicted } = reanchor(marks.map(toInput), text);
  const changed: MarkInput[] = [];
  for (const { mark, span, selectors } of anchored) {
    const next = { ...mark, quote: quoteOf(selectors), selectors, resolution: "resolved" as const };
    if (JSON.stringify(next) !== JSON.stringify(mark)) changed.push(next);
    entries.set(mark.annotationId, { mark: next, span });
  }
  for (const { mark, resolution } of conflicted) {
    const next = { ...mark, resolution };
    if (resolution !== mark.resolution) changed.push(next);
    entries.set(mark.annotationId, { mark: next, span: null });
  }
  for (const { mark, span } of entries.values()) if (span) paint(prose, span, mark);
  render();
  listen();
  await Promise.all(changed.map(persist));

  function listen(): void {
    prose.addEventListener("mouseup", offerHighlight, { signal });
    prose.addEventListener("keyup", offerHighlight, { signal });
    document.addEventListener(
      "selectionchange",
      () => {
        if (popover && !popover.engaged() && !selectedSpan(prose)) closePopover();
      },
      { signal },
    );
    prose.addEventListener(
      "click",
      (event) => {
        const hit = event.target instanceof Element ? event.target.closest<HTMLElement>("mark.hl") : null;
        const id = hit?.dataset.annotationId;
        if (id) focusNote(margin, id);
      },
      { signal },
    );
    signal.addEventListener("abort", () => closePopover(true));
  }

  function render(): void {
    const placed = [...entries.values()]
      .filter((e): e is Entry & { span: Span } => e.span !== null)
      .sort((a, b) => a.span.start - b.span.start);
    const lost = [...entries.values()].filter((e) => e.span === null);
    notes.count(entries.size);
    if (entries.size === 0) {
      notes.host.replaceChildren();
      return;
    }

    let list = notes.host.querySelector<HTMLElement>(":scope > .note-list");
    if (!list) {
      list = el("div", "note-list");
      notes.host.replaceChildren(list);
    }

    const existingCards = new Map<string, HTMLElement>();
    for (const cardNode of list.querySelectorAll<HTMLElement>(".note-card:not(.conflicted)")) {
      if (cardNode.dataset.annotationId) {
        existingCards.set(cardNode.dataset.annotationId, cardNode);
      }
    }

    const existingConflicts = new Map<string, HTMLElement>();
    for (const cardNode of list.querySelectorAll<HTMLElement>(".note-card.conflicted")) {
      if (cardNode.dataset.conflictId) {
        existingConflicts.set(cardNode.dataset.conflictId, cardNode);
      }
    }

    const nextChildren: HTMLElement[] = [];

    if (lost.length > 0) {
      let group = list.querySelector<HTMLElement>(":scope > .margin-conflicts");
      if (!group) {
        group = el("section", "margin-conflicts");
        group.setAttribute("aria-label", "Marks that need attention");
        group.append(el("h2", undefined, "Needs attention"));
      }
      const conflictNodes: HTMLElement[] = [group.querySelector("h2")!];
      for (const entry of lost) {
        let node = existingConflicts.get(entry.mark.annotationId);
        if (!node) {
          node = conflictCard(entry);
        }
        conflictNodes.push(node);
      }
      group.replaceChildren(...conflictNodes);
      nextChildren.push(group);
    }

    for (const entry of placed) {
      let node = existingCards.get(entry.mark.annotationId);
      if (!node) {
        node = card(entry.mark);
      } else {
        if (node.dataset.color !== entry.mark.color) {
          node.dataset.color = entry.mark.color;
          for (const s of node.querySelectorAll<HTMLButtonElement>("button.swatch")) {
            s.setAttribute("aria-current", String(s.dataset.color === entry.mark.color));
          }
        }
      }
      nextChildren.push(node);
    }

    list.replaceChildren(...nextChildren);
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
        alertInNotes(`Could not delete this mark: ${message(error)}`);
        return;
      }
      entries.delete(mark.annotationId);
      unpaint(prose, mark.annotationId);
      render();
    });
    return button;
  }

  function swatchesFor(mark: MarkInput, onRecolor: (color: Color) => void): HTMLElement {
    const bar = el("div", "card-swatches");
    bar.setAttribute("role", "group");
    bar.setAttribute("aria-label", "Highlight colors");
    for (const color of COLORS) {
      const swatch = el("button", "swatch");
      swatch.type = "button";
      swatch.dataset.color = color;
      swatch.setAttribute("aria-label", `Change color to ${color}`);
      swatch.setAttribute("aria-current", String(color === mark.color));
      swatch.addEventListener("click", () => onRecolor(color));
      bar.append(swatch);
    }
    return bar;
  }

  function colorPicker(mark: MarkInput, onRecolor: (color: Color) => void): HTMLElement {
    const wrap = el("div", "card-color-picker");
    const toggle = el("button", "card-action card-color-toggle", "Color");
    toggle.type = "button";
    toggle.setAttribute("aria-expanded", "false");
    toggle.setAttribute("aria-label", "Change color");

    function close(): void {
      if (swatches.hidden) return;
      swatches.hidden = true;
      toggle.setAttribute("aria-expanded", "false");
      document.removeEventListener("pointerdown", onOutside, true);
      document.removeEventListener("keydown", onKeyDown);
      toggle.focus();
    }

    function open(): void {
      swatches.hidden = false;
      toggle.setAttribute("aria-expanded", "true");
      document.addEventListener("pointerdown", onOutside, true);
      document.addEventListener("keydown", onKeyDown);
    }

    const onOutside = (event: Event): void => {
      if (!wrap.contains(event.target as Node)) {
        close();
      }
    };

    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === "Escape") {
        event.stopPropagation();
        close();
      }
    };

    const swatches = swatchesFor(mark, (color) => {
      onRecolor(color);
      close();
    });
    swatches.hidden = true;

    toggle.addEventListener("click", () => {
      if (swatches.hidden) {
        open();
      } else {
        close();
      }
    });

    wrap.append(toggle, swatches);
    return wrap;
  }

  function recolor(annotationId: string, color: Color): void {
    const entry = entries.get(annotationId);
    if (!entry || entry.mark.color === color) return;
    const next = { ...entry.mark, color };
    entry.mark = next;
    for (const card of byId(notes.host, ".note-card", annotationId)) {
      card.dataset.color = color;
      for (const s of card.querySelectorAll<HTMLButtonElement>("button.swatch")) {
        s.setAttribute("aria-current", String(s.dataset.color === color));
      }
    }
    for (const wrap of byId(prose, "mark.hl", annotationId)) {
      wrap.dataset.color = color;
    }
    void persist(next);
  }

  function card(mark: MarkInput): HTMLElement {
    const node = el("div", "note-card");
    node.dataset.annotationId = mark.annotationId;
    node.dataset.color = mark.color;
    const actions = el("div", "card-actions");
    actions.append(colorPicker(mark, (color) => recolor(mark.annotationId, color)), removeButton(mark, "Remove"));
    node.append(el("p", "note-quote", mark.quote), noteField(mark), actions);
    return node;
  }

  function conflictCard(entry: Entry): HTMLElement {
    const { mark } = entry;
    const node = el("div", "note-card conflicted");
    node.dataset.color = mark.color;
    node.dataset.conflictId = mark.annotationId;
    const why =
      mark.resolution === "ambiguous"
        ? "This passage now appears more than once."
        : "This passage is no longer in the chapter.";
    const reattach = el("button", "card-action", "Re-attach to selection");
    reattach.type = "button";
    reattach.addEventListener("mousedown", (event) => event.preventDefault());
    reattach.addEventListener("click", () => {
      const span = selectedSpan(prose);
      if (!span) {
        hint.textContent = "Select the passage in the chapter first.";
        return;
      }
      const selectors = selectorsAt(canonicalText(prose), span);
      const next = { ...entry.mark, quote: quoteOf(selectors), selectors, resolution: "resolved" as const };
      entries.set(mark.annotationId, { mark: next, span });
      paint(prose, span, next);
      document.getSelection()?.removeAllRanges();
      render();
      void persist(next);
    });
    const hint = el("p", "card-hint");
    const actions = el("div", "card-actions");
    actions.append(reattach, removeButton(mark, "Delete"));
    node.append(el("blockquote", "lost-quote", mark.quote), el("p", "card-why", why), noteField(mark), hint, actions);
    return node;
  }

  function closePopover(force = false): void {
    if (!popover || (!force && popover.busy())) return;
    popover.close();
  }

  function create(color: Color): void {
    const span = selectedSpan(prose);
    closePopover(true);
    if (!span) return;
    const selectors = selectorsAt(canonicalText(prose), span);
    const mark: MarkInput = {
      annotationId: crypto.randomUUID(),
      kind: "highlight",
      color,
      quote: quoteOf(selectors),
      note: "",
      selectors,
      resolution: "resolved",
      locator: 1,
      rects: [],
    };
    entries.set(mark.annotationId, { mark, span });
    paint(prose, span, mark);
    document.getSelection()?.removeAllRanges();
    render();
    focusNote(margin, mark.annotationId);
    void persist(mark);
  }

  function offerHighlight(): void {
    if (popover?.busy()) return;
    closePopover();
    const span = selectedSpan(prose);
    if (!span) return;
    popover = openSelectionPopup({
      host: page,
      rect: document.getSelection()!.getRangeAt(0).getBoundingClientRect(),
      quote: canonicalText(prose).slice(span.start, span.end),
      onColor: create,
      actions,
      onClose: () => (popover = null),
    });
  }

}
