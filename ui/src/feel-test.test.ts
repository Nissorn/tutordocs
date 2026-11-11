// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import contract from "./fixtures/reader-contract.json";
import wizardContract from "./fixtures/wizard-contract.json";
import { mountReader } from "./reader";
import { mountLibraryTools } from "./library-tools";
import first from "./fixtures/feel/chapter-v1.md?raw";
import second from "./fixtures/feel/chapter-v2.md?raw";

// Ticket 13's feel test with the display locked: the real reader and re-anchor
// code, the recorded chapter and its recorded rewrite, an in-memory store.
// The expected outcome is fixed by how the rewrite was written, not by running
// the function: two passages stay, two move to a new section, one is removed.

type Stored = Record<string, unknown> & { annotationId: string; note: string; quote: string; resolution: string };

const PASSAGES = [
  { quote: "Every value has exactly one owner, and the value is dropped when that owner goes out of scope.", note: "Core rule", fate: "kept" },
  { quote: "A move copies only the pointer, length and capacity, and leaves the heap bytes where they are.", note: "Cheap, no heap copy", fate: "moved" },
  { quote: "Integers, booleans and other types that live entirely on the stack implement Copy, so assigning them leaves the source usable.", note: "", fate: "kept" },
  { quote: "Passing a value to a function moves it into the parameter, so the caller loses access unless the function hands it back.", note: "", fate: "moved" },
  { quote: "Call clone when you really want two independent values, and accept that it copies the heap data.", note: "Ask when clone is worth it", fate: "removed" },
] as const;

let root: HTMLElement;

beforeEach(() => {
  document.body.innerHTML = '<div id="app"></div>';
  root = document.querySelector<HTMLElement>("#app")!;
});

afterEach(() => {
  clearMocks();
  document.getSelection()?.removeAllRanges();
});

const prose = () => root.querySelector<HTMLElement>("main.document .prose")!;
const margin = () => root.querySelector<HTMLElement>("aside.margin")!;

function textNode(container: Element, contains: string): Text {
  const walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if ((node as Text).data.includes(contains)) return node as Text;
  }
  throw new Error(`no text node containing "${contains}"`);
}

function select(quote: string): void {
  const node = textNode(prose(), quote);
  const from = node.data.indexOf(quote);
  const range = document.createRange();
  range.setStart(node, from);
  range.setEnd(node, from + quote.length);
  const selection = document.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
  prose().dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
}

describe("five marks across a regeneration", () => {
  it("re-anchors the four whose quote survives and surfaces the removed one with its quote and note", async () => {
    const db = new Map<string, Stored>();
    const deletes: string[] = [];
    const chapter = (markdown: string) => ({ ...contract.read_chapter_ready, markdown });
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
          return chapter(first);
        case "regenerate_chapter":
          return chapter(second);
        case "list_annotations":
          return [...db.values()];
        case "list_course_sources":
          return [];
        case "save_annotation": {
          const mark = { ...(args.mark as Stored), createdAt: 1, updatedAt: 2 };
          db.set(mark.annotationId, structuredClone(mark));
          return mark;
        }
        case "delete_annotation":
          deletes.push(args.annotationId as string);
          return db.delete(args.annotationId as string);
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });
    const tools = mountLibraryTools(document.createElement("div"));
    await tools.ready;
    await mountReader(root, contract.list_courses[0], { tools });

    for (const [i, p] of PASSAGES.entries()) {
      select(p.quote);
      root.querySelector<HTMLButtonElement>('[role="toolbar"] button[data-color="yellow"]')!.click();
      await vi.waitFor(() => expect(db.size).toBe(i + 1));
      if (p.note) {
        const field = margin().querySelectorAll<HTMLTextAreaElement>("textarea.note-field")[0];
        const card = Array.from(margin().querySelectorAll<HTMLElement>(".note-card")).find((c) =>
          Array.from(db.values()).some((m) => m.quote === p.quote && m.annotationId === c.dataset.annotationId),
        );
        const target = card?.querySelector<HTMLTextAreaElement>("textarea.note-field") ?? field;
        target.value = p.note;
        target.dispatchEvent(new Event("input", { bubbles: true }));
        target.dispatchEvent(new FocusEvent("blur"));
        await vi.waitFor(() => expect([...db.values()].find((m) => m.quote === p.quote)?.note).toBe(p.note));
      }
    }
    const idOf = (quote: string) => [...db.values()].find((m) => m.quote === quote)!.annotationId;
    const ids = PASSAGES.map((p) => idOf(p.quote));
    expect(root.querySelectorAll("mark.hl").length).toBeGreaterThanOrEqual(5);

    Array.from(root.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent === "Rewrite chapter")!.click();
    Array.from(root.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent === "Confirm rewrite")!.click();
    await vi.waitFor(() => expect(prose().textContent).toContain("Ownership is the rule"));
    await vi.waitFor(() => expect(margin().textContent).toContain("Needs attention"));

    const paintedIds = new Set(Array.from(prose().querySelectorAll<HTMLElement>("mark.hl")).map((m) => m.dataset.annotationId));
    const paintedText = (id: string) =>
      Array.from(prose().querySelectorAll(`mark[data-annotation-id="${id}"]`)).map((m) => m.textContent).join("");
    const conflicts = Array.from(margin().querySelectorAll<HTMLElement>(".note-card.conflicted"));

    const reanchored = PASSAGES.filter((_, i) => paintedIds.has(ids[i]));
    const conflicted = PASSAGES.filter((_, i) => !paintedIds.has(ids[i]));
    console.log(`FEEL reanchored=${reanchored.length} conflicted=${conflicted.length} cards=${conflicts.length} deletes=${deletes.length}`);

    expect(reanchored.map((p) => p.fate)).toEqual(["kept", "moved", "kept", "moved"]);
    expect(conflicted.map((p) => p.fate)).toEqual(["removed"]);
    PASSAGES.forEach((p, i) => {
      if (p.fate !== "removed") expect(paintedText(ids[i])).toBe(p.quote);
    });
    expect(textNode(prose(), PASSAGES[1].quote).parentElement!.closest("p")!.textContent).toContain("Think of a String");
    expect(textNode(prose(), PASSAGES[3].quote).parentElement!.closest("p")!.previousElementSibling!.textContent).toContain("Ownership is the rule");

    expect(conflicts).toHaveLength(1);
    expect(conflicts[0].querySelector("blockquote.lost-quote")!.textContent).toBe(PASSAGES[4].quote);
    expect(conflicts[0].querySelector<HTMLTextAreaElement>("textarea.note-field")!.value).toBe(PASSAGES[4].note);
    expect(conflicts[0].textContent).toContain("no longer in the chapter");
    expect(Array.from(conflicts[0].querySelectorAll("button")).map((b) => b.textContent)).toEqual(["Re-attach to selection", "Delete"]);

    const lost = db.get(ids[4])!;
    await vi.waitFor(() => expect(db.get(ids[4])!.resolution).toBe("unresolved"));
    expect(lost.quote).toBe(PASSAGES[4].quote);
    expect(db.get(ids[4])!.note).toBe(PASSAGES[4].note);
    expect(deletes).toEqual([]);
    for (const i of [0, 1]) {
      const card = Array.from(margin().querySelectorAll<HTMLElement>(".note-card")).find((c) => c.dataset.annotationId === ids[i])!;
      expect(card.querySelector<HTMLTextAreaElement>("textarea.note-field")!.value).toBe(PASSAGES[i].note);
    }
  });
});
