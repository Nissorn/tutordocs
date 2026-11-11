// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { mountPanelResizers, type Measure } from "./panel-resize";
import { BOUNDS, MIN_DOCUMENT } from "./panel-width";
import { createSettings, type SettingsStore } from "./settings";

let shell: HTMLElement;
let store: SettingsStore;
let container: number;
let rightColumn: boolean;
let controller: AbortController;

const px = (name: string): number => Number.parseFloat(shell.style.getPropertyValue(name));

/** A reader whose columns are its saved widths or the defaults, like the stylesheet draws them. */
function measure(): Measure {
  return {
    container,
    left: px("--sidebar-width") || 240,
    right: rightColumn ? px("--panel-width") || 300 : 0,
  };
}

const handle = (side: "left" | "right"): HTMLElement => shell.querySelector<HTMLElement>(`.panel-resizer-${side}`)!;
const key = (side: "left" | "right", name: string): void => {
  handle(side).dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true, cancelable: true }));
};
const pointer = (side: "left" | "right", type: string, clientX = 0, button = 0): void => {
  handle(side).dispatchEvent(new MouseEvent(type, { clientX, button, bubbles: true, cancelable: true }));
};

beforeEach(() => {
  document.body.replaceChildren();
  shell = document.createElement("div");
  shell.className = "reader";
  shell.getBoundingClientRect = () => ({ left: 0, right: container, top: 0, bottom: 800, width: container, height: 800, x: 0, y: 0, toJSON: () => ({}) });
  document.body.append(shell);
  container = 1400;
  rightColumn = true;
  store = createSettings(null, document.createElement("html"));
  controller = new AbortController();
  mountPanelResizers(shell, { settings: store, signal: controller.signal, measure });
  for (const side of ["left", "right"] as const) handle(side).setPointerCapture = vi.fn();
});

describe("the panel separators", () => {
  it("are vertical separators with a label, a place in the tab order and their current width", () => {
    for (const [side, now] of [["left", 240], ["right", 300]] as const) {
      const node = handle(side);
      expect(node.getAttribute("role")).toBe("separator");
      expect(node.getAttribute("aria-orientation")).toBe("vertical");
      expect(node.getAttribute("aria-label")).toBeTruthy();
      expect(node.tabIndex).toBe(0);
      expect(node.getAttribute("aria-valuenow")).toBe(String(now));
      expect(Number(node.getAttribute("aria-valuemin"))).toBe(BOUNDS[side].min);
    }
  });

  it("step the sidebar by 16 pixels with the arrow keys and save the width", () => {
    key("left", "ArrowRight");
    expect(store.get().leftPanelWidth).toBe(256);
    expect(shell.style.getPropertyValue("--sidebar-width")).toBe("256px");
    expect(handle("left").getAttribute("aria-valuenow")).toBe("256");
    key("left", "ArrowLeft");
    key("left", "ArrowLeft");
    expect(store.get().leftPanelWidth).toBe(224);
  });

  it("move the study panel's edge, so Left widens it and Right narrows it", () => {
    key("right", "ArrowLeft");
    expect(store.get().rightPanelWidth).toBe(316);
    key("right", "ArrowRight");
    key("right", "ArrowRight");
    expect(store.get().rightPanelWidth).toBe(284);
    expect(handle("right").getAttribute("aria-valuenow")).toBe("284");
  });

  it("stop at the smallest width", () => {
    for (let i = 0; i < 20; i++) key("left", "ArrowLeft");
    expect(store.get().leftPanelWidth).toBe(BOUNDS.left.min);
  });

  it("go back to the default on a double-click", () => {
    key("left", "ArrowRight");
    handle("left").dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    expect(store.get().leftPanelWidth).toBeNull();
    expect(shell.style.getPropertyValue("--sidebar-width")).toBe("");
    expect(handle("left").getAttribute("aria-valuenow")).toBe("240");
  });

  it("follow a drag, capturing the pointer, and stop when it is released", () => {
    pointer("left", "pointerdown", 240);
    expect(handle("left").setPointerCapture).toHaveBeenCalled();
    expect(shell.dataset.resizing).toBe("left");
    pointer("left", "pointermove", 320);
    expect(store.get().leftPanelWidth).toBe(320);
    pointer("left", "pointerup");
    expect(shell.dataset.resizing).toBeUndefined();
    pointer("left", "pointermove", 380);
    expect(store.get().leftPanelWidth).toBe(320);
  });

  it("measure the study panel from the window's right edge", () => {
    pointer("right", "pointerdown", 1100);
    pointer("right", "pointermove", 1000);
    expect(store.get().rightPanelWidth).toBe(400);
  });

  it("ignore a drag that starts with another mouse button", () => {
    pointer("left", "pointerdown", 240, 2);
    pointer("left", "pointermove", 320);
    expect(store.get().leftPanelWidth).toBeNull();
  });

  it("clamp a drag to the side's limits", () => {
    pointer("left", "pointerdown", 240);
    pointer("left", "pointermove", 5000);
    expect(store.get().leftPanelWidth).toBe(BOUNDS.left.max);
    pointer("left", "pointermove", 10);
    expect(store.get().leftPanelWidth).toBe(BOUNDS.left.min);
  });

  it("never squeeze the reading column below its floor", () => {
    container = 1000;
    pointer("left", "pointerdown", 240);
    pointer("left", "pointermove", 900);
    expect(store.get().leftPanelWidth).toBe(1000 - 300 - MIN_DOCUMENT);
  });

  it("count a missing study panel column as taking no room", () => {
    rightColumn = false;
    container = 700;
    pointer("left", "pointerdown", 240);
    pointer("left", "pointermove", 900);
    expect(store.get().leftPanelWidth).toBe(700 - MIN_DOCUMENT);
  });

  it("draw a saved width that is too wide for this window at the width that fits, and keep the saved value", () => {
    store.update({ leftPanelWidth: 420 });
    expect(shell.style.getPropertyValue("--sidebar-width")).toBe("420px");
    container = 1000;
    window.dispatchEvent(new Event("resize"));
    expect(shell.style.getPropertyValue("--sidebar-width")).toBe(`${1000 - 300 - MIN_DOCUMENT}px`);
    expect(store.get().leftPanelWidth).toBe(420);
    container = 1800;
    window.dispatchEvent(new Event("resize"));
    expect(shell.style.getPropertyValue("--sidebar-width")).toBe("420px");
  });

  it("redraw when the settings change from elsewhere, as Reset panel widths does", () => {
    store.update({ leftPanelWidth: 300, rightPanelWidth: 380 });
    expect(shell.style.getPropertyValue("--panel-width")).toBe("380px");
    store.update({ leftPanelWidth: null, rightPanelWidth: null });
    expect(shell.style.getPropertyValue("--sidebar-width")).toBe("");
    expect(shell.style.getPropertyValue("--panel-width")).toBe("");
  });

  it("let go of the settings and the window once the reader is retired", () => {
    controller.abort();
    store.update({ leftPanelWidth: 300 });
    expect(shell.style.getPropertyValue("--sidebar-width")).toBe("");
  });
});
