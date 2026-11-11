// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import fs from "node:fs";
import path from "node:path";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import readerContract from "./fixtures/reader-contract.json";
import wizardContract from "./fixtures/wizard-contract.json";
import studyContract from "./fixtures/study-contract.json";
import { mountReader } from "./reader";
import { stubMatchMedia } from "./test-media";

describe("responsive margin drawer and persistent toggle under 1080px", () => {
  const stylesCss = fs.readFileSync(path.resolve(__dirname, "styles.css"), "utf-8");
  let root: HTMLElement;

  beforeEach(() => {
    root = document.createElement("div");
    document.body.append(root);
    mockIPC((cmd, args) => {
      const a = (args ?? {}) as Record<string, unknown>;
      if (cmd === "get_outline") return wizardContract.outline_approved;
      if (cmd === "list_chapters") return readerContract.list_chapters;
      if (cmd === "open_chapter") return readerContract.read_chapter_ready;
      if (cmd === "read_chapter") {
        return a.chapterId === "cells" ? readerContract.read_chapter_ready : readerContract.read_chapter_placeholder;
      }
      if (cmd === "list_annotations") return [];
      if (cmd === "list_course_sources") return [];
      if (cmd === "list_pdf_sources") return [];
      if (cmd === "read_study_context") return studyContract.chapter_context;
      if (cmd === "list_chapter_answers") return [];
      if (cmd === "mark_opened") return null;
      throw new Error(`unexpected command ${cmd} with args ${JSON.stringify(a)}`);
    });
  });

  afterEach(() => {
    root.remove();
    clearMocks();
  });

  it("does not set display: none on .margin under 1080px in styles.css", () => {
    const narrowBlock = stylesCss.match(/@media\s*\(max-width:\s*1080px\)[\s\S]*?\n\}/)?.[0] ?? "";
    expect(narrowBlock).not.toContain("display: none");
    expect(narrowBlock).toContain(".margin");
    expect(narrowBlock).toContain("box-shadow: var(--shadow-drawer)");
  });

  it("starts collapsed by default below 1080px without stored preference", async () => {
    const media = stubMatchMedia(true);
    try {
      await mountReader(root, readerContract.list_courses[0], { storage: null });
      const reader = root.querySelector<HTMLElement>(".reader")!;
      expect(reader.dataset.panel).toBe("collapsed");
      const toggle = root.querySelector<HTMLButtonElement>("button.panel-toggle")!;
      expect(toggle.getAttribute("aria-expanded")).toBe("false");
    } finally {
      media.restore();
    }
  });

  it("does not overwrite wide-width stored preference when toggled below 1080px", async () => {
    const media = stubMatchMedia(true);
    let storedValue: string | null = "open";
    const storage = {
      getItem: () => storedValue,
      setItem: (_key: string, val: string) => {
        storedValue = val;
      },
    };
    try {
      await mountReader(root, readerContract.list_courses[0], { storage });
      const reader = root.querySelector<HTMLElement>(".reader")!;
      expect(reader.dataset.panel).toBe("collapsed");
      const toggle = root.querySelector<HTMLButtonElement>("button.panel-toggle")!;
      toggle.click();
      expect(reader.dataset.panel).toBe("open");
      expect(storedValue).toBe("open");
    } finally {
      media.restore();
    }
  });

  it("starts open by default at wide widths (> 1080px)", async () => {
    const media = stubMatchMedia(false);
    try {
      await mountReader(root, readerContract.list_courses[0], { storage: null });
      const reader = root.querySelector<HTMLElement>(".reader")!;
      expect(reader.dataset.panel).toBe("open");
      const toggle = root.querySelector<HTMLButtonElement>("button.panel-toggle")!;
      expect(toggle.getAttribute("aria-expanded")).toBe("true");
    } finally {
      media.restore();
    }
  });

  it("renders a persistent toggle that switches between collapsed and open states", async () => {
    const media = stubMatchMedia(false);
    try {
      await mountReader(root, readerContract.list_courses[0], {
        storage: {
          getItem: () => "collapsed",
          setItem: () => undefined,
        },
      });

      const reader = root.querySelector<HTMLElement>(".reader")!;
      expect(reader).not.toBeNull();
      expect(reader.dataset.panel).toBe("collapsed");

      const toggle = root.querySelector<HTMLButtonElement>("button.panel-toggle")!;
      expect(toggle).not.toBeNull();
      expect(toggle.getAttribute("aria-expanded")).toBe("false");
      expect(toggle.textContent).toBe("‹");
      expect(toggle.getAttribute("aria-label")).toBe("Show the study panel");

      // Click to open the drawer
      toggle.click();
      expect(reader.dataset.panel).toBe("open");
      expect(toggle.getAttribute("aria-expanded")).toBe("true");
      expect(toggle.textContent).toBe("›");
      expect(toggle.getAttribute("aria-label")).toBe("Hide the study panel");

      // Click to collapse the drawer
      toggle.click();
      expect(reader.dataset.panel).toBe("collapsed");
      expect(toggle.getAttribute("aria-expanded")).toBe("false");
      expect(toggle.textContent).toBe("‹");
    } finally {
      media.restore();
    }
  });

  it("collapses when resized to narrow and restores saved preference when widened", async () => {
    const media = stubMatchMedia(false);
    try {
      let storedValue: string | null = "open";
      const storage = {
        getItem: () => storedValue,
        setItem: (_key: string, val: string) => {
          storedValue = val;
        },
      };
      await mountReader(root, readerContract.list_courses[0], { storage });
      const reader = root.querySelector<HTMLElement>(".reader")!;
      expect(reader.dataset.panel).toBe("open");

      // Resize to narrow
      media.setMatches(true);
      expect(reader.dataset.panel).toBe("collapsed");

      // Resize back to wide
      media.setMatches(false);
      expect(reader.dataset.panel).toBe("open");
    } finally {
      media.restore();
    }
  });

  it("removes the media-query listener when the reader unmounts", async () => {
    const controller = new AbortController();
    const media = stubMatchMedia(false);
    try {
      await mountReader(root, readerContract.list_courses[0], {
        storage: null,
        signal: controller.signal,
      });
      expect(media.listenerCount).toBeGreaterThan(0);
      controller.abort();
      expect(media.listenerCount).toBe(0);
    } finally {
      media.restore();
    }
  });
});
