import "./panel-resize.css";
import { el } from "./dom";
import { STEP, clampWidth, defaultWidth, parseTracks, widthRange, type Fit, type Side } from "./panel-width";
import type { Settings, SettingsStore } from "./settings";

// The separators between the reader's columns. Each sits on the inner edge of
// the sidebar or the study panel and is a keyboard-operable `separator`. A drag
// uses pointer events with pointer capture, not HTML5 drag and drop, because
// the macOS webview under Tauri swallows `dragover` and `drop`. The widths live
// in the settings store. This module fits them to the window it is drawn in and
// writes them to the reader as `--sidebar-width` and `--panel-width`.

/** What the reader looks like right now, in pixels. */
export interface Measure {
  container: number;
  /** The sidebar's column. */
  left: number;
  /** The study panel's column, 0 when it takes no column (a drawer, the quiz, the outline review). */
  right: number;
}

export interface ResizerOptions {
  settings: SettingsStore;
  /** Removes the listeners when the reader that owns these separators is retired. */
  signal?: AbortSignal;
  /** Reads the reader's layout. Defaults to the computed grid of `shell`. */
  measure?: () => Measure;
}

const PROPERTY: Record<Side, string> = { left: "--sidebar-width", right: "--panel-width" };
const LABEL: Record<Side, string> = { left: "Resize the sidebar", right: "Resize the study panel" };

function stored(settings: Settings, side: Side): number | null {
  return side === "left" ? settings.leftPanelWidth : settings.rightPanelWidth;
}

function change(side: Side, width: number | null): Partial<Settings> {
  return side === "left" ? { leftPanelWidth: width } : { rightPanelWidth: width };
}

function gridMeasure(shell: HTMLElement): Measure {
  const tracks = parseTracks(getComputedStyle(shell).gridTemplateColumns);
  return { container: shell.clientWidth, left: tracks[0] ?? 0, right: tracks.length >= 3 ? tracks[2] : 0 };
}

export function mountPanelResizers(shell: HTMLElement, options: ResizerOptions): void {
  const { settings, signal } = options;
  const measure = options.measure ?? (() => gridMeasure(shell));

  const fit = (side: Side): Fit => {
    const m = measure();
    return { container: m.container, other: side === "left" ? m.right : m.left };
  };

  /** The width a side has now: what is drawn, or what would be drawn if nothing is yet. */
  function widthNow(side: Side): number {
    const m = measure();
    const drawn = side === "left" ? m.left : m.right;
    if (drawn > 0) return Math.round(drawn);
    return stored(settings.get(), side) ?? defaultWidth(side, window.innerWidth);
  }

  const handles: Record<Side, HTMLElement> = { left: el("div", "panel-resizer panel-resizer-left"), right: el("div", "panel-resizer panel-resizer-right") };

  function refresh(side: Side): void {
    const range = widthRange(side, fit(side));
    const handle = handles[side];
    handle.setAttribute("aria-valuemin", String(range.min));
    handle.setAttribute("aria-valuemax", String(range.max));
    handle.setAttribute("aria-valuenow", String(widthNow(side)));
  }

  /** Draws the saved widths, fitted to this window. The saved values themselves stay as the user set them. */
  function layout(): void {
    const m = measure();
    const left = stored(settings.get(), "left");
    const right = stored(settings.get(), "right");
    const drawnLeft = left === null ? null : clampWidth("left", left, { container: m.container, other: m.right });
    const drawnRight = right === null ? null : clampWidth("right", right, { container: m.container, other: drawnLeft ?? m.left });
    for (const [side, width] of [["left", drawnLeft], ["right", drawnRight]] as const) {
      if (width === null) shell.style.removeProperty(PROPERTY[side]);
      else shell.style.setProperty(PROPERTY[side], `${width}px`);
    }
    refresh("left");
    refresh("right");
  }

  function commit(side: Side, requested: number): void {
    settings.update(change(side, clampWidth(side, requested, fit(side))));
  }

  for (const side of ["left", "right"] as const) {
    const handle = handles[side];
    handle.setAttribute("role", "separator");
    handle.setAttribute("aria-orientation", "vertical");
    handle.setAttribute("aria-label", LABEL[side]);
    handle.tabIndex = 0;
    handle.dataset.side = side;
    let dragging = false;

    handle.addEventListener("pointerdown", (event) => {
      if (event.button !== 0) return;
      event.preventDefault();
      dragging = true;
      handle.setPointerCapture(event.pointerId);
      shell.dataset.resizing = side;
    });
    handle.addEventListener("pointermove", (event) => {
      if (!dragging) return;
      const edge = shell.getBoundingClientRect();
      commit(side, side === "left" ? event.clientX - edge.left : edge.right - event.clientX);
    });
    const stop = (): void => {
      dragging = false;
      delete shell.dataset.resizing;
    };
    handle.addEventListener("pointerup", stop);
    handle.addEventListener("pointercancel", stop);
    handle.addEventListener("lostpointercapture", stop);

    handle.addEventListener("keydown", (event) => {
      const direction = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
      if (direction === 0) return;
      event.preventDefault();
      // An arrow moves the edge: right grows the sidebar and shrinks the study panel.
      commit(side, widthNow(side) + (side === "left" ? direction : -direction) * STEP);
    });
    handle.addEventListener("dblclick", () => settings.update(change(side, null)));
  }

  shell.append(handles.left, handles.right);
  layout();

  const unsubscribe = settings.subscribe(layout);
  window.addEventListener("resize", layout);
  signal?.addEventListener("abort", () => {
    unsubscribe();
    window.removeEventListener("resize", layout);
  });
}
