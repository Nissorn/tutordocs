// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { mountTabBar, type TabBarHandlers, type TabBarView } from "./tabbar";

let root: HTMLElement;
let handlers: TabBarHandlers;
let bar: ReturnType<typeof mountTabBar>;

const view = (over: Partial<TabBarView> = {}): TabBarView => ({
  home: true,
  tabs: [],
  running: 0,
  drawerOpen: false,
  ...over,
});

beforeEach(() => {
  document.body.innerHTML = '<div id="app"></div>';
  root = document.querySelector<HTMLElement>("#app")!;
  handlers = { onHome: vi.fn(), onSelect: vi.fn(), onClose: vi.fn(), onToggleJobs: vi.fn(), onOpenSettings: vi.fn() };
  bar = mountTabBar(root, handlers);
});

const tabs = (): HTMLElement[] => Array.from(root.querySelectorAll<HTMLElement>("[role='tab']"));

describe("the tab bar", () => {
  it("always has a Home tab, and it has no close button", () => {
    bar.update(view());
    expect(tabs().map((t) => t.textContent)).toEqual(["Home"]);
    expect(root.querySelectorAll("button.tab-close")).toHaveLength(0);
    expect(tabs()[0].getAttribute("aria-selected")).toBe("true");
  });

  it("shows a tab per open course, in order, and marks the active one", () => {
    bar.update(
      view({
        home: false,
        tabs: [
          { courseId: "biology", title: "Cell Biology", active: false, busy: false },
          { courseId: "history", title: "Roman History", active: true, busy: false },
        ],
      }),
    );
    expect(tabs().map((t) => t.textContent)).toEqual(["Home", "Cell Biology", "Roman History"]);
    expect(tabs().map((t) => t.getAttribute("aria-selected"))).toEqual(["false", "false", "true"]);
  });

  it("says a tab is busy, in a way a screen reader gets and without moving the tab", () => {
    bar.update(
      view({
        home: false,
        tabs: [{ courseId: "biology", title: "Cell Biology", active: false, busy: true }],
      }),
    );
    const tab = root.querySelector<HTMLElement>("[data-course-id='biology']")!;
    expect(tab.dataset.busy).toBe("true");
    expect(tab.querySelector(".tab-busy")!.getAttribute("aria-label")).toBe("An agent job is running");
  });

  it("sends clicks to the right handler", () => {
    bar.update(
      view({
        home: false,
        tabs: [
          { courseId: "biology", title: "Cell Biology", active: false, busy: false },
          { courseId: "history", title: "Roman History", active: true, busy: false },
        ],
      }),
    );
    tabs()[0].click();
    expect(handlers.onHome).toHaveBeenCalled();
    tabs()[1].click();
    expect(handlers.onSelect).toHaveBeenCalledWith("biology");
    root.querySelector<HTMLButtonElement>("button[aria-label='Close Roman History']")!.click();
    expect(handlers.onClose).toHaveBeenCalledWith("history");
    expect(handlers.onSelect).toHaveBeenCalledTimes(1);
  });

  it("closes the active tab when it is middle-clicked, as a browser tab does", () => {
    bar.update(view({ home: false, tabs: [{ courseId: "biology", title: "Cell Biology", active: true, busy: false }] }));
    tabs()[1].dispatchEvent(new MouseEvent("auxclick", { button: 1, bubbles: true }));
    expect(handlers.onClose).toHaveBeenCalledWith("biology");
  });
});

describe("the job log control", () => {
  const toggle = (): HTMLButtonElement => root.querySelector<HTMLButtonElement>("button.jobs-toggle")!;

  it("is one button that names the log and says whether it is open", () => {
    bar.update(view());
    expect(toggle().getAttribute("aria-label")).toBe("Agent jobs");
    expect(toggle().getAttribute("aria-expanded")).toBe("false");
    bar.update(view({ drawerOpen: true }));
    expect(toggle().getAttribute("aria-expanded")).toBe("true");
    toggle().click();
    expect(handlers.onToggleJobs).toHaveBeenCalled();
  });

  it("counts the running jobs on the button", () => {
    bar.update(view({ running: 2 }));
    expect(toggle().getAttribute("aria-label")).toBe("Agent jobs, 2 running");
    expect(toggle().querySelector(".jobs-count")!.textContent).toBe("2");
    bar.update(view({ running: 0 }));
    expect(toggle().querySelector(".jobs-count")).toBeNull();
  });

  it("shows an attention dot when a job failed", () => {
    bar.update(view({ failed: true }));
    expect(toggle().getAttribute("aria-label")).toContain("failed job");
    expect(toggle().querySelector(".jobs-dot")).not.toBeNull();
    bar.update(view({ failed: false }));
    expect(toggle().querySelector(".jobs-dot")).toBeNull();
  });
});

describe("tab strip overflow and scrolling", () => {
  it("scrolls the active tab into view when rendered", () => {
    bar.update(
      view({
        home: false,
        tabs: [
          { courseId: "a", title: "Course A", active: false, busy: false },
          { courseId: "b", title: "Course B", active: true, busy: false },
        ],
      }),
    );
    const activeTab = root.querySelector<HTMLElement>('.tab[data-active="true"]')!;
    expect(activeTab).not.toBeNull();
    expect(activeTab.dataset.courseId).toBe("b");
  });

  it("updates overflow affordance buttons when tabs do not fit", () => {
    bar.update(
      view({
        home: false,
        tabs: Array.from({ length: 8 }, (_, i) => ({
          courseId: `course-${i}`,
          title: `Course ${i}`,
          active: i === 0,
          busy: false,
        })),
      }),
    );
    const strip = root.querySelector<HTMLElement>(".tabs")!;
    Object.defineProperty(strip, "clientWidth", { value: 300, configurable: true });
    Object.defineProperty(strip, "scrollWidth", { value: 800, configurable: true });
    Object.defineProperty(strip, "scrollLeft", { value: 50, writable: true, configurable: true });
    strip.dispatchEvent(new Event("scroll"));

    const leftBtn = root.querySelector<HTMLButtonElement>("button.tab-overflow-left")!;
    const rightBtn = root.querySelector<HTMLButtonElement>("button.tab-overflow-right")!;
    expect(leftBtn.hidden).toBe(false);
    expect(rightBtn.hidden).toBe(false);

    leftBtn.click();
    rightBtn.click();
  });

  it("re-runs overflow check on window resize", () => {
    bar.update(
      view({
        home: false,
        tabs: Array.from({ length: 8 }, (_, i) => ({
          courseId: `course-${i}`,
          title: `Course ${i}`,
          active: i === 0,
          busy: false,
        })),
      }),
    );
    const strip = root.querySelector<HTMLElement>(".tabs")!;
    Object.defineProperty(strip, "clientWidth", { value: 300, configurable: true });
    Object.defineProperty(strip, "scrollWidth", { value: 800, configurable: true });
    Object.defineProperty(strip, "scrollLeft", { value: 50, writable: true, configurable: true });
    strip.dispatchEvent(new Event("scroll"));

    const leftBtn = root.querySelector<HTMLButtonElement>("button.tab-overflow-left")!;
    expect(leftBtn.hidden).toBe(false);

    Object.defineProperty(strip, "scrollLeft", { value: 0, writable: true, configurable: true });
    window.dispatchEvent(new Event("resize"));
    expect(leftBtn.hidden).toBe(true);
  });
});

describe("redrawing", () => {
  it("keeps keyboard focus on the same tab when the bar redraws", () => {
    const one = { courseId: "biology", title: "Cell Biology", active: true, busy: false };
    bar.update(view({ home: false, tabs: [one] }));
    tabs()[1].focus();
    bar.update(view({ home: false, tabs: [{ ...one, busy: true }] }));
    expect(document.activeElement).toBe(tabs()[1]);
  });
});
