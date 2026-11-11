// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createSettings, type SettingsStore } from "./settings";
import { mountSettingsDialog, type SettingsDialogHandle } from "./settings-dialog";

let root: HTMLElement;
let html: HTMLElement;
let store: SettingsStore;
let dialog: SettingsDialogHandle;
let closed: () => void;

const scrim = (): HTMLElement => root.querySelector<HTMLElement>(".settings-scrim")!;
const radio = (group: string, value: string): HTMLButtonElement =>
  root.querySelector<HTMLButtonElement>(`[role="radiogroup"][aria-labelledby="settings-${group}"] [data-value="${value}"]`)!;

beforeEach(() => {
  document.body.innerHTML = '<div id="app"></div>';
  root = document.querySelector<HTMLElement>("#app")!;
  html = document.createElement("html");
  store = createSettings(null, html);
  closed = vi.fn<() => void>();
  dialog = mountSettingsDialog(root, store, { onClose: closed });
});

afterEach(() => {
  dialog.close();
});

describe("the Settings dialog", () => {
  it("is closed until it is opened, and says what it is", () => {
    expect(scrim().hidden).toBe(true);
    dialog.open();
    expect(scrim().hidden).toBe(false);
    const box = root.querySelector<HTMLElement>('[role="dialog"]')!;
    expect(box.getAttribute("aria-modal")).toBe("true");
    expect(box.querySelector("h2")!.textContent).toBe("Settings");
    expect(Array.from(box.querySelectorAll("h3"), (h) => h.textContent)).toEqual(["Appearance", "Layout"]);
  });

  it("shows the current choice in each group and applies a new one at once", () => {
    dialog.open();
    expect(radio("theme", "system").getAttribute("aria-checked")).toBe("true");
    radio("theme", "dark").click();
    expect(html.dataset.theme).toBe("dark");
    expect(radio("theme", "dark").getAttribute("aria-checked")).toBe("true");
    expect(radio("theme", "system").getAttribute("aria-checked")).toBe("false");

    radio("text-size", "x-large").click();
    expect(html.style.getPropertyValue("--text-scale")).toBe("1.25");
    radio("reading-font", "serif").click();
    expect(html.style.getPropertyValue("--reading-font")).toContain("serif");
    radio("line-width", "wide").click();
    expect(html.style.getPropertyValue("--line-width")).toBe("52rem");
  });

  it("offers each theme, text size, font and line width the settings know", () => {
    const labels = (group: string): (string | null)[] =>
      Array.from(root.querySelectorAll(`[aria-labelledby="settings-${group}"] .segment`), (b) => b.textContent);
    expect(labels("theme")).toEqual(["System", "Light", "Dark"]);
    expect(labels("text-size")).toEqual(["Small", "Medium", "Large", "Extra large"]);
    expect(labels("reading-font")).toEqual(["Sans", "Serif"]);
    expect(labels("line-width")).toEqual(["Narrow", "Normal", "Wide"]);
  });

  it("moves the choice with the arrow keys and wraps at the ends", () => {
    dialog.open();
    const system = radio("theme", "system");
    system.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
    expect(store.get().theme).toBe("light");
    radio("theme", "light").dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft", bubbles: true }));
    radio("theme", "system").dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft", bubbles: true }));
    expect(store.get().theme).toBe("dark");
  });

  it("keeps only the chosen radio in the tab order", () => {
    dialog.open();
    radio("text-size", "large").click();
    expect(radio("text-size", "large").tabIndex).toBe(0);
    expect(radio("text-size", "medium").tabIndex).toBe(-1);
  });

  it("previews the text size with the same properties the chapter reads", () => {
    dialog.open();
    expect(root.querySelector(".settings-preview")).not.toBeNull();
  });

  it("resets both panel widths and disables the button when there is nothing to reset", () => {
    dialog.open();
    const reset = root.querySelector<HTMLButtonElement>(".settings-reset")!;
    expect(reset.disabled).toBe(true);
    store.update({ leftPanelWidth: 300, rightPanelWidth: 360 });
    expect(reset.disabled).toBe(false);
    reset.click();
    expect(store.get().leftPanelWidth).toBeNull();
    expect(store.get().rightPanelWidth).toBeNull();
    expect(reset.disabled).toBe(true);
  });

  it("closes on Escape, on the close button and on a click outside, and tells the page each time", () => {
    dialog.open();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(dialog.isOpen()).toBe(false);
    expect(scrim().hidden).toBe(true);

    dialog.open();
    root.querySelector<HTMLButtonElement>(".settings-close")!.click();
    expect(dialog.isOpen()).toBe(false);

    dialog.open();
    scrim().dispatchEvent(new MouseEvent("pointerdown", { bubbles: true }));
    expect(dialog.isOpen()).toBe(false);
    expect(closed).toHaveBeenCalledTimes(3);
  });

  it("stays open for a press inside the dialog", () => {
    dialog.open();
    root.querySelector(".settings")!.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true }));
    expect(dialog.isOpen()).toBe(true);
  });

  it("puts focus on the chosen theme when it opens and hands it back to the opener when it closes", () => {
    const opener = document.createElement("button");
    document.body.append(opener);
    opener.focus();
    dialog.open();
    expect(document.activeElement).toBe(radio("theme", "system"));
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(document.activeElement).toBe(opener);
  });

  it("keeps Tab inside the dialog", () => {
    dialog.open();
    const stops = Array.from(root.querySelectorAll<HTMLButtonElement>(".settings button")).filter((b) => !b.disabled && b.tabIndex >= 0);
    stops[stops.length - 1].focus();
    const forward = new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true });
    document.dispatchEvent(forward);
    expect(forward.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(stops[0]);
    const back = new KeyboardEvent("keydown", { key: "Tab", shiftKey: true, bubbles: true, cancelable: true });
    document.dispatchEvent(back);
    expect(document.activeElement).toBe(stops[stops.length - 1]);
  });

  it("does not let Escape reach a listener under it, such as the job drawer", () => {
    const under = vi.fn();
    document.addEventListener("keydown", under);
    dialog.open();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    document.removeEventListener("keydown", under);
    expect(under).not.toHaveBeenCalled();
  });
});
