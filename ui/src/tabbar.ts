import { el } from "./dom";

// The bar across the top: Home, one tab per open course, and at the right end
// the Settings gear and the job log control. It draws what it is told. It holds no state of its
// own, so it cannot disagree with the tab set the page keeps.

export interface TabView {
  courseId: string;
  title: string;
  active: boolean;
  /** An agent job is running for this course. */
  busy: boolean;
}

export interface TabBarView {
  /** Home is the active tab. */
  home: boolean;
  tabs: TabView[];
  /** Jobs running anywhere, for the log control. */
  running: number;
  /** Attention dot when at least one job failed, staying until drawer opens. */
  failed?: boolean;
  drawerOpen: boolean;
  /** The Settings dialog is open. */
  settingsOpen?: boolean;
}

export interface TabBarHandlers {
  onHome(): void;
  onSelect(courseId: string): void;
  onClose(courseId: string): void;
  onToggleJobs(): void;
  onOpenSettings(): void;
}

export interface TabBarHandle {
  update(view: TabBarView): void;
}

export const JOB_DRAWER_ID = "job-drawer";
export const SETTINGS_DIALOG_ID = "settings-dialog";

const SVG_NS = "http://www.w3.org/2000/svg";

/** The gear, drawn as a stroked outline so it takes the button's colour. */
function gearIcon(): SVGSVGElement {
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("width", "16");
  svg.setAttribute("height", "16");
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "1.8");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  svg.setAttribute("aria-hidden", "true");
  const hub = document.createElementNS(SVG_NS, "circle");
  hub.setAttribute("cx", "12");
  hub.setAttribute("cy", "12");
  hub.setAttribute("r", "3");
  const teeth = document.createElementNS(SVG_NS, "path");
  teeth.setAttribute(
    "d",
    "M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z",
  );
  svg.append(hub, teeth);
  return svg;
}

export function mountTabBar(root: HTMLElement, handlers: TabBarHandlers): TabBarHandle {
  const bar = el("div", "tabbar");
  root.append(bar);

  let checkActiveOverflow: (() => void) | null = null;
  window.addEventListener("resize", () => {
    checkActiveOverflow?.();
  });

  function tabButton(label: string, selected: boolean, focusKey: string, onClick: () => void): HTMLButtonElement {
    const button = el("button", "tab-main", label);
    button.type = "button";
    button.setAttribute("role", "tab");
    button.setAttribute("aria-selected", String(selected));
    button.dataset.focusKey = focusKey;
    button.addEventListener("click", onClick);
    return button;
  }

  function update(view: TabBarView): void {
    const focused = document.activeElement instanceof HTMLElement ? document.activeElement.dataset.focusKey : undefined;
    const strip = el("div", "tabs");
    strip.setAttribute("role", "tablist");
    strip.setAttribute("aria-label", "Open courses");

    const home = el("div", "tab tab-home");
    home.setAttribute("role", "presentation");
    home.dataset.active = String(view.home);
    home.append(tabButton("Home", view.home, "home", handlers.onHome));
    strip.append(home);

    for (const tab of view.tabs) {
      const item = el("div", "tab");
      item.setAttribute("role", "presentation");
      item.dataset.courseId = tab.courseId;
      item.dataset.active = String(tab.active);
      item.dataset.busy = String(tab.busy);
      const main = tabButton(tab.title, tab.active, `tab:${tab.courseId}`, () => handlers.onSelect(tab.courseId));
      main.title = tab.title;
      if (tab.busy) {
        const dot = el("span", "tab-busy");
        dot.setAttribute("role", "img");
        dot.setAttribute("aria-label", "An agent job is running");
        main.append(dot);
      }
      const close = el("button", "tab-close", "×");
      close.type = "button";
      close.setAttribute("aria-label", `Close ${tab.title}`);
      close.dataset.focusKey = `close:${tab.courseId}`;
      close.addEventListener("click", () => handlers.onClose(tab.courseId));
      item.addEventListener("auxclick", (event) => {
        if (event.button === 1) handlers.onClose(tab.courseId);
      });
      item.append(main, close);
      strip.append(item);
    }

    const stripWrapper = el("div", "tabs-wrapper");
    const overflowLeft = el("button", "tab-overflow-btn tab-overflow-left", "‹");
    overflowLeft.type = "button";
    overflowLeft.setAttribute("aria-label", "Scroll tabs left");
    overflowLeft.hidden = true;
    overflowLeft.addEventListener("click", () => {
      strip.scrollBy({ left: -160, behavior: "smooth" });
    });

    const overflowRight = el("button", "tab-overflow-btn tab-overflow-right", "›");
    overflowRight.type = "button";
    overflowRight.setAttribute("aria-label", "Scroll tabs right");
    overflowRight.hidden = true;
    overflowRight.addEventListener("click", () => {
      strip.scrollBy({ left: 160, behavior: "smooth" });
    });

    function checkOverflow(): void {
      const hasLeft = strip.scrollLeft > 0;
      const hasRight = strip.scrollLeft + strip.clientWidth < strip.scrollWidth - 1;
      overflowLeft.hidden = !hasLeft;
      overflowRight.hidden = !hasRight;
      strip.dataset.overflowLeft = String(hasLeft);
      strip.dataset.overflowRight = String(hasRight);
    }

    strip.addEventListener("scroll", checkOverflow);
    checkActiveOverflow = checkOverflow;
    stripWrapper.append(overflowLeft, strip, overflowRight);

    const jobs = el("button", "jobs-toggle");
    jobs.type = "button";
    jobs.dataset.focusKey = "jobs";
    const labelParts = ["Agent jobs"];
    if (view.running > 0) labelParts.push(`${view.running} running`);
    if (view.failed) labelParts.push("failed job");
    jobs.setAttribute("aria-label", labelParts.join(", "));
    jobs.setAttribute("aria-expanded", String(view.drawerOpen));
    jobs.setAttribute("aria-controls", JOB_DRAWER_ID);
    jobs.append(el("span", "jobs-label", "Jobs"));
    if (view.running > 0) jobs.append(el("span", "jobs-count", String(view.running)));
    if (view.failed) {
      const dot = el("span", "jobs-dot jobs-attention");
      dot.setAttribute("role", "img");
      dot.setAttribute("aria-label", "A job failed");
      jobs.append(dot);
    }
    jobs.addEventListener("click", handlers.onToggleJobs);

    const settings = el("button", "settings-toggle");
    settings.type = "button";
    settings.dataset.focusKey = "settings";
    settings.setAttribute("aria-label", "Settings");
    settings.setAttribute("aria-haspopup", "dialog");
    settings.setAttribute("aria-expanded", String(view.settingsOpen === true));
    settings.setAttribute("aria-controls", SETTINGS_DIALOG_ID);
    settings.title = "Settings (\u2318,)";
    settings.append(gearIcon());
    settings.addEventListener("click", handlers.onOpenSettings);

    bar.replaceChildren(stripWrapper, settings, jobs);
    if (focused) bar.querySelector<HTMLElement>(`[data-focus-key="${CSS.escape(focused)}"]`)?.focus();

    const activeTab = strip.querySelector<HTMLElement>('.tab[data-active="true"]');
    if (activeTab && typeof activeTab.scrollIntoView === "function") {
      activeTab.scrollIntoView({ block: "nearest", inline: "nearest" });
    }
    checkOverflow();
  }

  return { update };
}
