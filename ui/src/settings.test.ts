// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";
import { BOUNDS } from "./panel-width";
import {
  DEFAULT_SETTINGS,
  applySettings,
  createSettings,
  parseSettings,
  readSettings,
  writeSettings,
  type Settings,
} from "./settings";
import type { PrefStorage } from "./study-prefs";

function memory(): PrefStorage & { held: Map<string, string> } {
  const held = new Map<string, string>();
  return { held, getItem: (k) => held.get(k) ?? null, setItem: (k, v) => void held.set(k, v) };
}

describe("parsing saved settings", () => {
  it("gives the defaults for missing or unusable data", () => {
    for (const raw of [undefined, null, "dark", 7, [], true]) {
      expect(parseSettings(raw)).toEqual(DEFAULT_SETTINGS);
    }
  });

  it("keeps each good field and defaults each bad one on its own", () => {
    const parsed = parseSettings({ theme: "purple", textSize: "large", readingFont: 3, lineWidth: "wide" });
    expect(parsed).toEqual({ ...DEFAULT_SETTINGS, textSize: "large", lineWidth: "wide" });
  });

  it("accepts every listed value", () => {
    const all: Settings = {
      theme: "dark",
      textSize: "x-large",
      readingFont: "serif",
      lineWidth: "narrow",
      leftPanelWidth: 300,
      rightPanelWidth: 400,
    };
    expect(parseSettings(all)).toEqual(all);
  });

  it("turns a bad width into the default and pulls a stray one inside the limits", () => {
    expect(parseSettings({ leftPanelWidth: "300", rightPanelWidth: Number.NaN })).toEqual(DEFAULT_SETTINGS);
    expect(parseSettings({ leftPanelWidth: Infinity, rightPanelWidth: -5 }).rightPanelWidth).toBe(BOUNDS.right.min);
    expect(parseSettings({ leftPanelWidth: 5000 }).leftPanelWidth).toBe(BOUNDS.left.max);
    expect(parseSettings({ leftPanelWidth: 250.4 }).leftPanelWidth).toBe(250);
  });
});

describe("remembering settings", () => {
  it("reads back what was written", () => {
    const store = memory();
    const mine: Settings = { ...DEFAULT_SETTINGS, theme: "light", leftPanelWidth: 280 };
    writeSettings(store, mine);
    expect(readSettings(store)).toEqual(mine);
  });

  it("falls back to the defaults for bad JSON, a missing key or no storage", () => {
    const store = memory();
    expect(readSettings(store)).toEqual(DEFAULT_SETTINGS);
    store.setItem("tutordocs.settings.v1", "{oops");
    expect(readSettings(store)).toEqual(DEFAULT_SETTINGS);
    expect(readSettings(null)).toEqual(DEFAULT_SETTINGS);
    expect(() => writeSettings(null, DEFAULT_SETTINGS)).not.toThrow();
  });

  it("survives a store whose reads and writes both throw", () => {
    const blocked: PrefStorage = {
      getItem: () => {
        throw new Error("SecurityError");
      },
      setItem: () => {
        throw new Error("QuotaExceededError");
      },
    };
    expect(readSettings(blocked)).toEqual(DEFAULT_SETTINGS);
    expect(() => writeSettings(blocked, DEFAULT_SETTINGS)).not.toThrow();
  });
});

describe("applying settings to the page", () => {
  const root = (): HTMLElement => document.createElement("html");

  it("sets data-theme for light and dark and removes it for system", () => {
    const html = root();
    applySettings(html, { ...DEFAULT_SETTINGS, theme: "dark" });
    expect(html.dataset.theme).toBe("dark");
    applySettings(html, { ...DEFAULT_SETTINGS, theme: "light" });
    expect(html.dataset.theme).toBe("light");
    applySettings(html, DEFAULT_SETTINGS);
    expect(html.hasAttribute("data-theme")).toBe(false);
  });

  it("sets the reading scale for each text size", () => {
    const html = root();
    const scale = (textSize: Settings["textSize"]): string => {
      applySettings(html, { ...DEFAULT_SETTINGS, textSize });
      return html.style.getPropertyValue("--text-scale");
    };
    expect([scale("small"), scale("medium"), scale("large"), scale("x-large")]).toEqual(["0.9", "1", "1.125", "1.25"]);
  });

  it("sets a serif stack for serif and drops the property for sans", () => {
    const html = root();
    applySettings(html, { ...DEFAULT_SETTINGS, readingFont: "serif" });
    expect(html.style.getPropertyValue("--reading-font")).toContain("serif");
    applySettings(html, { ...DEFAULT_SETTINGS, readingFont: "sans" });
    expect(html.style.getPropertyValue("--reading-font")).toBe("");
  });

  it("reads in the bundled serif by default, with the system serifs behind it", () => {
    const html = root();
    expect(DEFAULT_SETTINGS.readingFont).toBe("serif");
    applySettings(html, DEFAULT_SETTINGS);
    expect(html.style.getPropertyValue("--reading-font")).toMatch(/^"Newsreader", /);
  });

  it("sets the line width in rem", () => {
    const html = root();
    applySettings(html, { ...DEFAULT_SETTINGS, lineWidth: "narrow" });
    expect(html.style.getPropertyValue("--line-width")).toBe("36rem");
    applySettings(html, { ...DEFAULT_SETTINGS, lineWidth: "wide" });
    expect(html.style.getPropertyValue("--line-width")).toBe("52rem");
  });
});

describe("the settings store", () => {
  it("applies the saved settings when it is created", () => {
    const storage = memory();
    writeSettings(storage, { ...DEFAULT_SETTINGS, theme: "dark" });
    const html = document.createElement("html");
    createSettings(storage, html);
    expect(html.dataset.theme).toBe("dark");
  });

  it("applies, saves and announces a change", () => {
    const storage = memory();
    const html = document.createElement("html");
    const settings = createSettings(storage, html);
    const heard = vi.fn();
    settings.subscribe(heard);
    settings.update({ theme: "light", textSize: "large" });
    expect(html.dataset.theme).toBe("light");
    expect(readSettings(storage)).toMatchObject({ theme: "light", textSize: "large" });
    expect(settings.get()).toMatchObject({ theme: "light", textSize: "large" });
    expect(heard).toHaveBeenCalledTimes(1);
    expect(heard.mock.calls[0][0]).toMatchObject({ theme: "light" });
  });

  it("keeps a width inside the limits even when the caller asks for more", () => {
    const settings = createSettings(memory(), document.createElement("html"));
    settings.update({ leftPanelWidth: 10 });
    expect(settings.get().leftPanelWidth).toBe(BOUNDS.left.min);
  });

  it("stops announcing to a listener that unsubscribed", () => {
    const settings = createSettings(null, document.createElement("html"));
    const heard = vi.fn();
    const stop = settings.subscribe(heard);
    stop();
    settings.update({ theme: "dark" });
    expect(heard).not.toHaveBeenCalled();
  });

  it("works with no storage", () => {
    const settings = createSettings(null, document.createElement("html"));
    settings.update({ theme: "dark" });
    expect(settings.get().theme).toBe("dark");
  });
});
