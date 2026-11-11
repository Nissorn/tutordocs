import { BOUNDS } from "./panel-width";
import type { PrefStorage } from "./study-prefs";

// Everything the Settings dialog can change, and the one place that turns it
// into page state. Saved data is untrusted: it is parsed field by field and
// each bad field falls back to its default. Applying settings sets `data-theme`
// on <html> and a few CSS custom properties. Stylesheets read those properties,
// and nothing else reads the settings object, with one exception: the panel
// separators own the two panel widths, because a width has to be fitted to the
// window it is drawn in.

export const THEMES = ["system", "light", "dark"] as const;
export const TEXT_SIZES = ["small", "medium", "large", "x-large"] as const;
export const READING_FONTS = ["sans", "serif"] as const;
export const LINE_WIDTHS = ["narrow", "normal", "wide"] as const;

export type Theme = (typeof THEMES)[number];
export type TextSize = (typeof TEXT_SIZES)[number];
export type ReadingFont = (typeof READING_FONTS)[number];
export type LineWidth = (typeof LINE_WIDTHS)[number];

export interface Settings {
  theme: Theme;
  textSize: TextSize;
  readingFont: ReadingFont;
  lineWidth: LineWidth;
  /** The sidebar's width in pixels, or null for the stylesheet's default. */
  leftPanelWidth: number | null;
  /** The study panel's width in pixels, or null for the stylesheet's default. */
  rightPanelWidth: number | null;
}

export const DEFAULT_SETTINGS: Settings = {
  theme: "system",
  textSize: "medium",
  readingFont: "serif",
  lineWidth: "normal",
  leftPanelWidth: null,
  rightPanelWidth: null,
};

const TEXT_SCALE: Record<TextSize, number> = { small: 0.9, medium: 1, large: 1.125, "x-large": 1.25 };
const LINE_WIDTH_REM: Record<LineWidth, number> = { narrow: 36, normal: 42, wide: 52 };
/**
 * The bundled reading serif first, then the system serifs for the scripts it
 * does not cover. Sans needs no property, because the stylesheet's own reading
 * sans is what shows when `--reading-font` is absent.
 */
const SERIF_STACK = '"Newsreader", "New York", "Iowan Old Style", Charter, Georgia, serif';

const KEY = "tutordocs.settings.v1";

function oneOf<T extends string>(allowed: readonly T[], value: unknown, fallback: T): T {
  return allowed.find((candidate) => candidate === value) ?? fallback;
}

function width(side: "left" | "right", value: unknown): number | null {
  if (typeof value !== "number" || !Number.isFinite(value)) return null;
  const { min, max } = BOUNDS[side];
  return Math.round(Math.min(max, Math.max(min, value)));
}

export function parseSettings(raw: unknown): Settings {
  const data = typeof raw === "object" && raw !== null && !Array.isArray(raw) ? (raw as Record<string, unknown>) : {};
  return {
    theme: oneOf(THEMES, data.theme, DEFAULT_SETTINGS.theme),
    textSize: oneOf(TEXT_SIZES, data.textSize, DEFAULT_SETTINGS.textSize),
    readingFont: oneOf(READING_FONTS, data.readingFont, DEFAULT_SETTINGS.readingFont),
    lineWidth: oneOf(LINE_WIDTHS, data.lineWidth, DEFAULT_SETTINGS.lineWidth),
    leftPanelWidth: width("left", data.leftPanelWidth),
    rightPanelWidth: width("right", data.rightPanelWidth),
  };
}

export function readSettings(storage: PrefStorage | null): Settings {
  try {
    const raw = storage?.getItem(KEY);
    return raw ? parseSettings(JSON.parse(raw)) : DEFAULT_SETTINGS;
  } catch {
    return DEFAULT_SETTINGS;
  }
}

export function writeSettings(storage: PrefStorage | null, settings: Settings): void {
  try {
    storage?.setItem(KEY, JSON.stringify(settings));
  } catch {
    // A choice is a convenience, so a full or blocked store costs only the memory.
  }
}

function setOrRemove(root: HTMLElement, name: string, value: string | null): void {
  if (value === null) root.style.removeProperty(name);
  else root.style.setProperty(name, value);
}

/** Puts the settings on the page. Calling it again with the same settings changes nothing. */
export function applySettings(root: HTMLElement, settings: Settings): void {
  if (settings.theme === "system") delete root.dataset.theme;
  else root.dataset.theme = settings.theme;
  root.style.setProperty("--text-scale", String(TEXT_SCALE[settings.textSize]));
  root.style.setProperty("--line-width", `${LINE_WIDTH_REM[settings.lineWidth]}rem`);
  setOrRemove(root, "--reading-font", settings.readingFont === "serif" ? SERIF_STACK : null);
}

export interface SettingsStore {
  get(): Settings;
  /** Merges a change, applies it to the page, saves it and tells every listener. */
  update(change: Partial<Settings>): void;
  /** Returns the function that stops listening. */
  subscribe(listener: (settings: Settings) => void): () => void;
}

/** The page's settings. Applies what was saved before it returns, so the first paint is already themed. */
export function createSettings(storage: PrefStorage | null, root: HTMLElement = document.documentElement): SettingsStore {
  let current = readSettings(storage);
  const listeners = new Set<(settings: Settings) => void>();
  applySettings(root, current);
  return {
    get: () => current,
    update(change) {
      current = parseSettings({ ...current, ...change });
      applySettings(root, current);
      writeSettings(storage, current);
      for (const listener of listeners) listener(current);
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
  };
}
