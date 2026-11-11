import "./library-tools.css";
import { mountAgentPicker } from "./agent-picker";
import { el } from "./dom";
import { mountUrlBox } from "./url-box";

// The reader chrome overflow menu in the sidebar footer: holds Rewrite chapter,
// Quiz me, the agent picker and the URL box. Only the menu trigger button is
// visible at rest, consolidating standing controls to at most 9.

export type ChapterMenu =
  | { kind: "chapter"; title: string; onRewrite(): void; onQuiz(): void }
  | { kind: "none"; reason: string };

export const NO_CHAPTER_MENU: ChapterMenu = {
  kind: "none",
  reason: "Available when reading a chapter",
};

export interface LibraryTools {
  footer: HTMLElement;
  ready: Promise<void>;
  updateActions(menu: ChapterMenu): void;
}

/**
 * Builds the overflow menu footer inside `dock`, an element that stays in the page.
 * `ready` resolves once the agent picker has checked which agents are installed.
 */
export function mountLibraryTools(dock: HTMLElement): LibraryTools {
  const footer = el("div", "library-tools");
  footer.setAttribute("role", "group");
  footer.setAttribute("aria-label", "Chapter and course actions");
  dock.append(footer);

  const button = el("button", "quiet-button overflow-menu-button", "Chapter and course actions");
  button.type = "button";
  button.setAttribute("aria-expanded", "false");
  button.setAttribute("aria-controls", "overflow-menu-panel");
  button.setAttribute("aria-label", "Chapter and course actions");

  const dropdown = el("div", "overflow-menu-dropdown");
  dropdown.id = "overflow-menu-panel";
  dropdown.hidden = true;
  dropdown.setAttribute("role", "region");
  dropdown.setAttribute("aria-label", "Chapter and course actions");

  const chapterSection = el("div", "menu-section menu-chapter-actions");
  chapterSection.setAttribute("aria-label", "Chapter actions");

  const rewriteBtn = el("button", "quiet-button menu-action menu-action-rewrite", "Rewrite chapter");
  rewriteBtn.type = "button";

  const quizBtn = el("button", "quiet-button menu-action menu-action-quiz", "Quiz me");
  quizBtn.type = "button";

  const note = el("span", "menu-action-note");
  note.setAttribute("aria-live", "polite");

  chapterSection.append(rewriteBtn, quizBtn, note);

  const divider = el("div", "menu-divider");

  const toolsSection = el("div", "menu-section menu-tools");
  toolsSection.setAttribute("aria-label", "Course tools");

  mountUrlBox(toolsSection);
  const ready = mountAgentPicker(toolsSection);

  dropdown.append(chapterSection, divider, toolsSection);
  footer.append(button, dropdown);

  let currentMenu: ChapterMenu = NO_CHAPTER_MENU;
  let isOpen = false;

  function updateActions(menu: ChapterMenu): void {
    currentMenu = menu;
    if (menu.kind === "chapter") {
      rewriteBtn.disabled = false;
      rewriteBtn.removeAttribute("aria-disabled");
      rewriteBtn.title = `Rewrite ${menu.title}`;
      quizBtn.disabled = false;
      quizBtn.removeAttribute("aria-disabled");
      quizBtn.title = `Quiz me on ${menu.title}`;
      note.textContent = "";
      note.hidden = true;
    } else {
      rewriteBtn.disabled = true;
      rewriteBtn.setAttribute("aria-disabled", "true");
      quizBtn.disabled = true;
      quizBtn.setAttribute("aria-disabled", "true");
      rewriteBtn.title = menu.reason;
      quizBtn.title = menu.reason;
      note.textContent = menu.reason;
      note.hidden = false;
    }
  }

  // Initial state: no chapter active
  updateActions(NO_CHAPTER_MENU);

  function openMenu(): void {
    if (isOpen) return;
    isOpen = true;
    button.setAttribute("aria-expanded", "true");
    dropdown.hidden = false;
    document.addEventListener("pointerdown", onOutside, true);
    document.addEventListener("keydown", onKeyDown, true);
  }

  function closeMenu(restoreFocus = true): void {
    if (!isOpen) return;
    isOpen = false;
    button.setAttribute("aria-expanded", "false");
    dropdown.hidden = true;
    document.removeEventListener("pointerdown", onOutside, true);
    document.removeEventListener("keydown", onKeyDown, true);
    if (restoreFocus) {
      button.focus();
    }
  }

  function onOutside(event: Event): void {
    const target = event.target as Node;
    if (!footer.contains(target)) {
      closeMenu(true);
    }
  }

  function onKeyDown(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      // If agent picker dialog is open inside dropdown, let it handle its own Escape first
      if (dropdown.querySelector(".agent-panel") !== null) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      closeMenu(true);
    }
  }

  button.addEventListener("click", () => {
    if (isOpen) closeMenu(true);
    else openMenu();
  });

  rewriteBtn.addEventListener("click", () => {
    if (rewriteBtn.disabled || currentMenu.kind !== "chapter") return;
    const run = currentMenu.onRewrite;
    closeMenu(true);
    run();
  });

  quizBtn.addEventListener("click", () => {
    if (quizBtn.disabled || currentMenu.kind !== "chapter") return;
    const run = currentMenu.onQuiz;
    closeMenu(true);
    run();
  });

  return { footer, ready, updateActions };
}
