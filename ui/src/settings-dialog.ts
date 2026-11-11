import "./settings.css";
import { el } from "./dom";
import { SETTINGS_DIALOG_ID } from "./tabbar";
import {
  LINE_WIDTHS,
  READING_FONTS,
  TEXT_SIZES,
  THEMES,
  type LineWidth,
  type ReadingFont,
  type Settings,
  type SettingsStore,
  type TextSize,
  type Theme,
} from "./settings";

// The Settings dialog. A change applies the moment it is made, so there is no
// Save button. The dialog reads and writes through the settings store and holds
// no copy of its own: every control is redrawn from the store's current value.

export interface SettingsDialogHandle {
  open(): void;
  close(): void;
  isOpen(): boolean;
}

export interface SettingsDialogOptions {
  /** The dialog closed itself (Escape, the close button or a click outside), so the page can redraw the gear. */
  onClose(): void;
}

const THEME_LABELS: Record<Theme, string> = { system: "System", light: "Light", dark: "Dark" };
const TEXT_SIZE_LABELS: Record<TextSize, string> = {
  small: "Small",
  medium: "Medium",
  large: "Large",
  "x-large": "Extra large",
};
const FONT_LABELS: Record<ReadingFont, string> = { sans: "Sans", serif: "Serif" };
const LINE_WIDTH_LABELS: Record<LineWidth, string> = { narrow: "Narrow", normal: "Normal", wide: "Wide" };

interface Segmented<T extends string> {
  row: HTMLElement;
  show(value: T): void;
}

/** One row: a label, a hint, and a radio group of buttons. Arrow keys move the choice and apply it. */
function segmented<T extends string>(
  label: string,
  hint: string | null,
  options: readonly T[],
  labels: Record<T, string>,
  pick: (value: T) => void,
): Segmented<T> {
  const id = `settings-${label.toLowerCase().replace(/\s+/g, "-")}`;
  const row = el("div", "setting");
  const title = el("span", "setting-label", label);
  title.id = id;
  const group = el("div", "segmented");
  group.setAttribute("role", "radiogroup");
  group.setAttribute("aria-labelledby", id);
  const buttons = options.map((value) => {
    const button = el("button", "segment", labels[value]);
    button.type = "button";
    button.dataset.value = value;
    button.setAttribute("role", "radio");
    button.addEventListener("click", () => pick(value));
    button.addEventListener("keydown", (event) => {
      const step = event.key === "ArrowRight" || event.key === "ArrowDown" ? 1 : event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 0;
      if (step === 0) return;
      event.preventDefault();
      const next = options[(options.indexOf(value) + step + options.length) % options.length];
      pick(next);
      buttons[options.indexOf(next)].focus();
    });
    group.append(button);
    return button;
  });
  row.append(title, group);
  if (hint) row.append(el("p", "setting-hint", hint));
  return {
    row,
    show(current) {
      options.forEach((value, i) => {
        buttons[i].setAttribute("aria-checked", String(value === current));
        buttons[i].tabIndex = value === current ? 0 : -1;
      });
    },
  };
}

function group(title: string, ...rows: HTMLElement[]): HTMLElement {
  const section = el("section", "settings-group");
  section.append(el("h3", undefined, title), ...rows);
  return section;
}

export function mountSettingsDialog(
  root: HTMLElement,
  store: SettingsStore,
  options: SettingsDialogOptions,
): SettingsDialogHandle {
  const theme = segmented("Theme", "System follows your Mac's appearance.", THEMES, THEME_LABELS, (theme) => store.update({ theme }));
  const textSize = segmented("Text size", null, TEXT_SIZES, TEXT_SIZE_LABELS, (textSize) => store.update({ textSize }));
  const preview = el("p", "settings-preview", "The quick brown fox jumps over the lazy dog. Text size, font and line width change the chapter text only.");
  preview.setAttribute("aria-label", "Text size preview");
  textSize.row.append(preview);
  const font = segmented("Reading font", null, READING_FONTS, FONT_LABELS, (readingFont) => store.update({ readingFont }));
  const lineWidth = segmented("Line width", null, LINE_WIDTHS, LINE_WIDTH_LABELS, (lineWidth) => store.update({ lineWidth }));

  const reset = el("button", "settings-reset", "Reset panel widths");
  reset.type = "button";
  reset.addEventListener("click", () => store.update({ leftPanelWidth: null, rightPanelWidth: null }));
  const layoutRow = el("div", "setting");
  layoutRow.append(
    reset,
    el("p", "setting-hint", "Drag the edge of the sidebar or the study panel to resize it. Double-click an edge to reset it."),
  );

  const title = el("h2", undefined, "Settings");
  title.id = "settings-title";
  const close = el("button", "settings-close", "×");
  close.type = "button";
  close.setAttribute("aria-label", "Close settings");
  const head = el("header", "settings-head");
  head.append(title, close);
  const body = el("div", "settings-body");
  body.append(group("Appearance", theme.row, textSize.row, font.row, lineWidth.row), group("Layout", layoutRow));

  const dialog = el("section", "settings");
  dialog.id = SETTINGS_DIALOG_ID;
  dialog.setAttribute("role", "dialog");
  dialog.setAttribute("aria-modal", "true");
  dialog.setAttribute("aria-labelledby", title.id);
  dialog.append(head, body);
  const scrim = el("div", "settings-scrim");
  scrim.hidden = true;
  scrim.append(dialog);
  root.append(scrim);

  function show(settings: Settings): void {
    theme.show(settings.theme);
    textSize.show(settings.textSize);
    font.show(settings.readingFont);
    lineWidth.show(settings.lineWidth);
    reset.disabled = settings.leftPanelWidth === null && settings.rightPanelWidth === null;
  }
  show(store.get());
  store.subscribe(show);

  let opened = false;
  let opener: HTMLElement | null = null;

  function focusable(): HTMLButtonElement[] {
    return Array.from(dialog.querySelectorAll<HTMLButtonElement>("button")).filter((node) => !node.disabled && node.tabIndex >= 0);
  }

  function onKey(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      event.stopPropagation();
      shut();
      options.onClose();
      return;
    }
    if (event.key !== "Tab") return;
    const stops = focusable();
    if (stops.length === 0) return;
    const first = stops[0];
    const last = stops[stops.length - 1];
    const active = document.activeElement;
    if (event.shiftKey && (active === first || !dialog.contains(active))) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (active === last || !dialog.contains(active))) {
      event.preventDefault();
      first.focus();
    }
  }

  function open(): void {
    if (opened) return;
    opened = true;
    opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    show(store.get());
    scrim.hidden = false;
    // Capture phase, so Escape closes this dialog and not a drawer under it.
    document.addEventListener("keydown", onKey, true);
    dialog.querySelector<HTMLElement>('.segment[tabindex="0"]')?.focus();
  }

  function shut(): void {
    if (!opened) return;
    opened = false;
    scrim.hidden = true;
    document.removeEventListener("keydown", onKey, true);
    const back = opener?.isConnected ? opener : document.querySelector<HTMLElement>('[data-focus-key="settings"]');
    back?.focus();
    opener = null;
  }

  close.addEventListener("click", () => {
    shut();
    options.onClose();
  });
  scrim.addEventListener("pointerdown", (event) => {
    if (event.target !== scrim) return;
    shut();
    options.onClose();
  });

  return { open, close: shut, isOpen: () => opened };
}
