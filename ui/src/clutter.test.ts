// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import readerContract from "./fixtures/reader-contract.json";
import wizardContract from "./fixtures/wizard-contract.json";
import agentsContract from "./fixtures/agents-contract.json";
import studyContract from "./fixtures/study-contract.json";
import { mountApp } from "./app";
import { stubWideMatchMedia } from "./test-media";

// Ticket 13's clutter test: every control the whole app mounts on a chapter
// view, named, in the order the page lays them out. DOM only, no layout, so
// this counts what exists, and styles.css hides only the margin column below
// 1080px. A control inside a `hidden` part of the page is not counted: the job
// log drawer is in the page from launch but closed, and shows nothing until it
// is opened. The agent picker is one button; its panel with three selects is in
// the page only while open, so it is not a standing control.
//
// Ticket 26 added the tab bar. With one course open it holds the Home tab, the
// course's tab with its close button, and the job log control: four. Every
// further open course adds two more, its tab and its close button.
//
// Ticket 35 turned the right column into the study panel. It adds three
// standing controls: the toggle that collapses it, the question box and its
// Ask button. A source chip is a button only for a PDF in the library, one per
// source like a chapter button, so the count leaves chips out and the mock
// names no PDF. A selection now offers two more actions beside the five colors.
//
// Ticket 58 added the visible Rewrite chapter shortcut in the .chapter-tools
// row, one more standing control on a chapter view.
//
// Ticket 65 added the Settings gear to the tab bar and the two panel
// separators, which are keyboard-operable: three more standing controls.

const CONTROLS = "button, a[href], input, select, textarea, summary, [role='button'], [tabindex]";

let root: HTMLElement;
let restoreMedia: (() => void) | undefined;
const saved = new Map<string, Record<string, unknown>>();

beforeEach(async () => {
  restoreMedia = stubWideMatchMedia();
  document.body.innerHTML = '<div id="app"></div>';
  root = document.querySelector<HTMLElement>("#app")!;
  saved.clear();
  mockIPC(
    (cmd, raw) => {
      const args = (raw ?? {}) as Record<string, unknown>;
      switch (cmd) {
        case "list_library_courses":
          return readerContract.list_library_courses;
        case "mark_course_opened":
          return readerContract.list_library_courses[0];
        case "list_agent_jobs":
        case "course_progress":
        case "list_course_sources":
          return [];
        case "get_outline":
          return wizardContract.outline_approved;
        case "list_chapters":
          return readerContract.list_chapters;
        case "read_chapter":
          return readerContract.read_chapter_ready;
        case "list_annotations":
          return [...saved.values()];
        case "chapter_context":
          return studyContract.chapter_context;
        case "list_answers":
          return [];
        case "list_agent_drivers":
          return agentsContract.list_agent_drivers;
        case "driver_tuning":
          return { model: null, effort: null };
        case "save_annotation": {
          const mark = { ...(args.mark as Record<string, unknown> & { annotationId: string }), createdAt: 1, updatedAt: 2 };
          saved.set(mark.annotationId, mark);
          return mark;
        }
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    },
    { shouldMockEvents: true },
  );
  await mountApp(root, { storage: null });
  root.querySelector<HTMLButtonElement>("button.card-open")!.click();
  await vi.waitFor(() => expect(root.querySelector<HTMLElement>("main.document")?.dataset.state).toBe("ready"));
  await vi.waitFor(() => expect(root.querySelector("button.agent-button")?.textContent).toContain("Claude"));
});

afterEach(() => {
  restoreMedia?.();
  clearMocks();
  document.getSelection()?.removeAllRanges();
});

function names(): string[] {
  return Array.from(root.querySelectorAll<HTMLElement>(CONTROLS))
    .filter((c) => c.closest("[hidden]") === null)
    .map((c) => {
      const kind = c.tagName.toLowerCase();
      const name = c.getAttribute("aria-label") || (c as HTMLInputElement).placeholder || c.textContent || "";
      return `${kind}: ${name}`;
    });
}

describe("controls on a chapter view", () => {
  it("lists every control the app mounts, with and without a selection and a mark", async () => {
    const resting = names();
    console.log(`CLUTTER resting (${resting.length}): ${resting.join(" | ")}`);
    const chapters = root.querySelectorAll("button.chapter").length;
    expect(resting.length - chapters).toBe(13);
    expect(resting).toEqual([
      "button: Home",
      "button: Cell Biology",
      "button: Close Cell Biology",
      "button: Settings",
      "button: Agent jobs",
      "button: Edit outline",
      "button: Cells",
      "button: Membranes",
      "button: Chapter and course actions",
      "button: Rewrite chapter",
      "button: Hide the study panel",
      "textarea: Ask about this chapter",
      "button: Ask",
      "div: Resize the sidebar",
      "div: Resize the study panel",
    ]);

    const prose = root.querySelector<HTMLElement>("main.document .prose")!;
    const walker = document.createTreeWalker(prose, NodeFilter.SHOW_TEXT);
    let line: Text | null = null;
    for (let node = walker.nextNode(); node && !line; node = walker.nextNode()) {
      if ((node as Text).data.includes("basic unit")) line = node as Text;
    }
    const range = document.createRange();
    range.setStart(line!, line!.data.indexOf("basic unit"));
    range.setEnd(line!, line!.data.indexOf("basic unit") + "basic unit".length);
    document.getSelection()!.removeAllRanges();
    document.getSelection()!.addRange(range);
    prose.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    const selecting = names();
    console.log(`CLUTTER selecting (${selecting.length}): ${selecting.slice(resting.length).join(" | ")}`);
    expect(selecting.length - resting.length).toBe(7);

    root.querySelector<HTMLButtonElement>('[role="toolbar"] button[data-color="yellow"]')!.click();
    await vi.waitFor(() => expect(saved.size).toBe(1));
    const marked = names();
    console.log(`CLUTTER one mark (${marked.length}): ${marked.filter((n) => !resting.includes(n)).join(" | ")}`);
    // Ticket 52: note card swatches are behind one color button, adding 3 controls (textarea, color button, remove)
    expect(marked.length - resting.length).toBe(3);
  });

  it("adds three selects while the agent panel is open and none once it is closed", async () => {
    const resting = names();
    root.querySelector<HTMLButtonElement>("button.overflow-menu-button")!.click();
    root.querySelector<HTMLButtonElement>("button.agent-button")!.click();
    const open = root.querySelectorAll("[role='dialog'] select").length;
    console.log(`CLUTTER agent panel open (${resting.length + open}): ${open} selects added`);
    expect(open).toBe(3);
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(names()).toEqual(resting);
  });

  it("adds nothing to the reading view while the job log is closed, and a close button and the job rows only once it opens", () => {
    const resting = names();
    expect(root.querySelector<HTMLElement>("aside.job-drawer")!.hidden).toBe(true);
    root.querySelector<HTMLButtonElement>("button.jobs-toggle")!.click();
    const open = names().filter((n) => !resting.includes(n));
    console.log(`CLUTTER job log open: ${open.join(" | ")}`);
    expect(open).toEqual(["button: Close the job log"]);
    root.querySelector<HTMLButtonElement>("button.job-close")!.click();
    expect(names()).toEqual(resting);
  });
});
