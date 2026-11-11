// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import readerContract from "./fixtures/reader-contract.json";
import wizardContract from "./fixtures/wizard-contract.json";
import agentsContract from "./fixtures/agents-contract.json";
import sourcesContract from "./fixtures/sources-contract.json";
import { mountLibraryTools } from "./library-tools";
import { mountReader } from "./reader";
import { stubWideMatchMedia } from "./test-media";

function backend(extra: Record<string, unknown> = {}): void {
  mockIPC((cmd) => {
    if (Object.prototype.hasOwnProperty.call(extra, cmd)) return extra[cmd];
    switch (cmd) {
      case "list_courses":
        return readerContract.list_courses;
      case "get_outline":
        return wizardContract.outline_approved;
      case "list_chapters":
        return readerContract.list_chapters;
      case "read_chapter":
        return readerContract.read_chapter_ready;
      case "list_annotations":
      case "list_course_sources":
        return [];
      case "list_agent_drivers":
        return agentsContract.list_agent_drivers;
      case "driver_tuning":
        return { model: null, effort: null };
      case "ingest_url":
        return sourcesContract.ingest_url;
      default:
        throw new Error(`unexpected command ${cmd}`);
    }
  });
}

let root: HTMLElement;
let before: Element[];
let toolsInstance: ReturnType<typeof mountLibraryTools>;
let restoreMedia: (() => void) | undefined;

const inFooter = (control: Element): boolean => control.closest(".library-tools") !== null;

const CONTROLS = "button, a[href], input, select, textarea, summary, [role='button'], [tabindex]";

function controls(): Element[] {
  return Array.from(root.querySelectorAll(CONTROLS));
}

beforeEach(async () => {
  restoreMedia = stubWideMatchMedia();
  document.body.innerHTML = '<div id="app"></div>';
  root = document.querySelector<HTMLElement>("#app")!;
  backend();
  const dock = document.createElement("div");
  dock.hidden = true;
  document.body.append(dock);
  toolsInstance = mountLibraryTools(dock);
  await toolsInstance.ready;
  await mountReader(root, readerContract.list_courses[0], { tools: toolsInstance });
  before = controls().filter((control) => !inFooter(control));
});

afterEach(() => {
  restoreMedia?.();
  clearMocks();
});

describe("clutter budget", () => {
  it("adds one overflow menu button to the resting reader chrome, revealing tools when open", () => {
    const resting = controls().filter((c) => inFooter(c) && c.closest("[hidden]") === null);
    expect(resting.map((control) => control.tagName + "." + control.className)).toEqual([
      "BUTTON.quiet-button overflow-menu-button",
    ]);
    const added = controls().filter(inFooter);
    expect(added.map((control) => control.tagName + "." + control.className)).toEqual([
      "BUTTON.quiet-button overflow-menu-button",
      "BUTTON.quiet-button menu-action menu-action-rewrite",
      "BUTTON.quiet-button menu-action menu-action-quiz",
      "INPUT.url-input",
      "BUTTON.agent-button",
    ]);
  });

  it("adds the model and effort selects only while the panel is open", () => {
    const resting = controls();
    root.querySelector<HTMLButtonElement>("button.agent-button")!.click();
    const open = controls().filter((control) => !resting.includes(control));
    expect(open.map((control) => control.className)).toEqual(["agent-select", "model-select", "effort-select"]);
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(controls()).toEqual(resting);
  });

  it("keeps both in the sidebar footer, removes nothing, and puts neither in the study panel", () => {
    const added = controls().filter(inFooter);
    expect(added.every((control) => control.closest("nav.sidebar > .library-tools") !== null)).toBe(true);
    expect(before.every((control) => control.isConnected)).toBe(true);
    expect(root.querySelector("aside.margin .url-input, aside.margin .agent-button")).toBeNull();
  });

  it("puts the tools in one footer group after the chapter tree", () => {
    const sidebar = root.querySelector("nav.sidebar")!;
    const footer = sidebar.lastElementChild!;
    expect(footer.className).toBe("library-tools");
    expect(footer.getAttribute("role")).toBe("group");
    expect(sidebar.querySelector(".course")!.compareDocumentPosition(footer)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
  });

  it("loads the picker's state without disturbing the open chapter", () => {
    expect(root.querySelector("button.agent-button")!.textContent).toBe("Claude \u00b7 CLI default");
    expect(root.querySelector<HTMLElement>("main.document")!.dataset.state).toBe("ready");
  });

  it("keeps one sidebar footer, and the five standing controls, after a trip to the outline and back", async () => {
    const typed = "https://example.com/cells";
    const url = root.querySelector<HTMLInputElement>("input.url-input");
    if (!url) throw new Error("url box missing before the outline");
    url.value = typed;

    const named = (name: string): HTMLButtonElement => {
      const found = Array.from(root.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent === name);
      if (!found) throw new Error(`no ${name} button`);
      return found;
    };
    named("Edit outline").click();
    await vi.waitFor(() => expect(root.querySelector<HTMLElement>("main.document")?.dataset.state).toBe("review"));
    expect(root.querySelectorAll(".library-tools")).toHaveLength(1);
    named("Back to the chapters").click();
    await vi.waitFor(() =>
      expect(root.querySelector<HTMLElement>("main.document")?.dataset.state).toBe("ready"),
    );

    const standing = [
      "button: Edit outline",
      "button: Chapter and course actions",
    ];
    const labels = Array.from(root.querySelectorAll<HTMLElement>(CONTROLS))
      .filter((c) => c.closest("[hidden]") === null)
      .map((control) => {
        const kind = control.tagName.toLowerCase();
        const name =
          control.getAttribute("aria-label") || (control as HTMLInputElement).placeholder || control.textContent || "";
        return `${kind}: ${name}`;
      });
    expect(labels.filter((label) => standing.includes(label))).toEqual(standing);
    expect(root.querySelectorAll(".library-tools")).toHaveLength(1);
    expect(root.querySelectorAll("input.url-input")).toHaveLength(1);
    expect(root.querySelectorAll("button.agent-button")).toHaveLength(1);
    expect(url.isConnected).toBe(true);
    expect(root.querySelector("input.url-input")).toBe(url);
    expect(url.value).toBe(typed);
  });
});

describe("overflow menu interactions and accessibility", () => {
  it("toggles the menu open and closed on trigger click, updating aria-expanded and aria-controls", () => {
    const trigger = root.querySelector<HTMLButtonElement>("button.overflow-menu-button")!;
    const dropdown = root.querySelector<HTMLElement>(".overflow-menu-dropdown")!;

    expect(trigger.getAttribute("aria-expanded")).toBe("false");
    expect(trigger.getAttribute("aria-controls")).toBe(dropdown.id);
    expect(trigger.getAttribute("aria-haspopup")).toBeNull();
    expect(dropdown.getAttribute("role")).toBe("region");
    expect(dropdown.hidden).toBe(true);

    trigger.click();
    expect(trigger.getAttribute("aria-expanded")).toBe("true");
    expect(dropdown.hidden).toBe(false);

    trigger.click();
    expect(trigger.getAttribute("aria-expanded")).toBe("false");
    expect(dropdown.hidden).toBe(true);
  });

  it("closes the menu on Escape and restores focus to the trigger button", () => {
    const trigger = root.querySelector<HTMLButtonElement>("button.overflow-menu-button")!;
    const dropdown = root.querySelector<HTMLElement>(".overflow-menu-dropdown")!;

    trigger.click();
    expect(dropdown.hidden).toBe(false);

    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(dropdown.hidden).toBe(true);
    expect(trigger.getAttribute("aria-expanded")).toBe("false");
    expect(document.activeElement).toBe(trigger);
  });

  it("closes the menu on outside click and restores focus to the trigger button", () => {
    const trigger = root.querySelector<HTMLButtonElement>("button.overflow-menu-button")!;
    const dropdown = root.querySelector<HTMLElement>(".overflow-menu-dropdown")!;

    trigger.click();
    expect(dropdown.hidden).toBe(false);

    document.body.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    expect(dropdown.hidden).toBe(true);
    expect(trigger.getAttribute("aria-expanded")).toBe("false");
    expect(document.activeElement).toBe(trigger);
  });

  it("does not close on inside click, typing, or pressing Enter in the url input", () => {
    const trigger = root.querySelector<HTMLButtonElement>("button.overflow-menu-button")!;
    const dropdown = root.querySelector<HTMLElement>(".overflow-menu-dropdown")!;
    const urlInput = dropdown.querySelector<HTMLInputElement>("input.url-input")!;

    trigger.click();
    expect(dropdown.hidden).toBe(false);

    urlInput.focus();
    urlInput.value = "https://example.com/test";
    urlInput.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    expect(dropdown.hidden).toBe(false);
  });

  it("closes and restores focus after invoking an action like Quiz me", async () => {
    const trigger = root.querySelector<HTMLButtonElement>("button.overflow-menu-button")!;
    const dropdown = root.querySelector<HTMLElement>(".overflow-menu-dropdown")!;
    const quizBtn = dropdown.querySelector<HTMLButtonElement>("button.menu-action-quiz")!;

    trigger.click();
    expect(dropdown.hidden).toBe(false);

    quizBtn.click();
    expect(dropdown.hidden).toBe(true);
    expect(trigger.getAttribute("aria-expanded")).toBe("false");
    expect(document.activeElement).toBe(trigger);

    await vi.waitFor(() => {
      expect(root.querySelector<HTMLElement>("main.document")?.dataset.state).toBe("quiz");
    });
  });

  it("disables Rewrite and Quiz with a short visible reason when no written chapter is open", () => {
    const trigger = root.querySelector<HTMLButtonElement>("button.overflow-menu-button")!;
    const dropdown = root.querySelector<HTMLElement>(".overflow-menu-dropdown")!;
    const rewriteBtn = dropdown.querySelector<HTMLButtonElement>("button.menu-action-rewrite")!;
    const quizBtn = dropdown.querySelector<HTMLButtonElement>("button.menu-action-quiz")!;
    const note = dropdown.querySelector<HTMLElement>(".menu-action-note")!;

    expect(rewriteBtn.getAttribute("role")).toBeNull();
    expect(quizBtn.getAttribute("role")).toBeNull();

    // Trigger updateActions with kind: "none"
    toolsInstance.updateActions({
      kind: "none",
      reason: "Available when reading a chapter",
    });

    trigger.click();
    expect(rewriteBtn.disabled).toBe(true);
    expect(rewriteBtn.getAttribute("aria-disabled")).toBe("true");
    expect(quizBtn.disabled).toBe(true);
    expect(quizBtn.getAttribute("aria-disabled")).toBe("true");
    expect(note.hidden).toBe(false);
    expect(note.textContent).toBe("Available when reading a chapter");

    // Re-enable
    toolsInstance.updateActions({
      kind: "chapter",
      title: "Cells",
      onRewrite: vi.fn(),
      onQuiz: vi.fn(),
    });

    expect(rewriteBtn.disabled).toBe(false);
    expect(rewriteBtn.getAttribute("aria-disabled")).toBeNull();
    expect(quizBtn.disabled).toBe(false);
    expect(quizBtn.getAttribute("aria-disabled")).toBeNull();
    expect(note.hidden).toBe(true);
  });
});
