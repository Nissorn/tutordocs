import { COLORS, type Color } from "./annotation-api";
import { el } from "./dom";
import { renderMarkdown } from "./markdown";
import type { Answer } from "./study-api";
import { flowController, type Flow, type FlowController } from "./study-state";

// The popup beside a selected passage: five highlight colors, "Fix this
// section" and "Ask". The last two open a box that runs one job. It lives in
// the document column, so it works when the study panel is collapsed or the
// whole right column is hidden below 1080px.

export interface SelectionActions {
  /** Revises the section holding `quote`. Resolves once the chapter on screen is the revised one. */
  fix?(quote: string, instruction: string): Promise<void>;
  ask?(quote: string, question: string): Promise<Answer>;
}

export interface PopupOptions {
  /** The element the popup is appended to. */
  host: HTMLElement;
  /** The selection's box, which the popup sits under. */
  rect: DOMRect;
  /** The selected text, as the chapter shows it. */
  quote: string;
  onColor(color: Color): void;
  actions: SelectionActions;
  /** When asking from a source, notes which chapter the answer will be saved to. */
  targetChapterTitle?: string | null;
  /** When asking is unavailable, provides a reason why the Ask button is disabled. */
  askDisabledReason?: string | null;
  /** Called when the popup removes itself, so its owner can let go of it. */
  onClose(): void;
}

export interface SelectionPopup {
  /** True while a fix or an ask is running. The popup must stay until it ends. */
  busy(): boolean;
  /** True once the user chose Fix or Ask. Losing the text selection no longer dismisses it. */
  engaged(): boolean;
  close(): void;
}

const QUOTE_SHOWN = 160;

type Kind = "fix" | "ask";

const COPY: Record<Kind, { label: string; placeholder: string; field: string; submit: string; running: string }> = {
  fix: {
    label: "Fix this section",
    placeholder: "What should change? For example: add an example, shorten it, explain it more simply.",
    field: "What should change in this section?",
    submit: "Revise section",
    running: "Revising this section…",
  },
  ask: {
    label: "Ask about this passage",
    placeholder: "What do you want to know about this passage?",
    field: "Your question about this passage",
    submit: "Ask",
    running: "Asking…",
  },
};

function shortened(quote: string): string {
  const flat = quote.replace(/\s+/g, " ");
  return flat.length > QUOTE_SHOWN ? `${flat.slice(0, QUOTE_SHOWN).trimEnd()}…` : flat;
}

function button(className: string, text: string): HTMLButtonElement {
  const node = el("button", className, text);
  node.type = "button";
  return node;
}

export function openSelectionPopup(options: PopupOptions): SelectionPopup {
  const { host, rect, quote, onColor, actions, onClose } = options;
  const popup = el("div", "mark-popover");
  let kind: Kind | null = null;
  let closed = false;
  let flow: FlowController<unknown> | null = null;
  // The selection rect is a snapshot. Scrolling the host moves the passage, so
  // the anchor moves with it and the popup stays beside that passage.
  const anchor = { left: rect.left, top: rect.top, bottom: rect.bottom };
  let seenTop = host.scrollTop;
  let seenLeft = host.scrollLeft;

  const active = (): FlowController<unknown> | null => flow;

  function place(): void {
    const width = popup.offsetWidth;
    const height = popup.offsetHeight;
    const left = Math.max(8, Math.min(anchor.left, window.innerWidth - width - 8));
    const below = anchor.bottom + 6;
    const above = anchor.top - height - 6;
    const roomBelow = window.innerHeight - 8 - below;
    // Flip above the passage only when that side fits. Clamping to 8px is what
    // parked a tall answer over the chapter title.
    const top = height <= roomBelow || above < 8 ? below : above;
    popup.style.left = `${left}px`;
    popup.style.top = `${top}px`;
  }

  function onHostScroll(event: Event): void {
    if (event.target !== host) return;
    const dy = host.scrollTop - seenTop;
    const dx = host.scrollLeft - seenLeft;
    seenTop = host.scrollTop;
    seenLeft = host.scrollLeft;
    if (dy === 0 && dx === 0) return;
    anchor.top -= dy;
    anchor.bottom -= dy;
    anchor.left -= dx;
    const top = Number.parseFloat(popup.style.top);
    const left = Number.parseFloat(popup.style.left);
    if (Number.isFinite(top)) popup.style.top = `${top - dy}px`;
    if (Number.isFinite(left)) popup.style.left = `${left - dx}px`;
  }

  function close(): void {
    if (closed) return;
    closed = true;
    host.removeEventListener("scroll", onHostScroll);
    popup.remove();
    onClose();
  }

  function drawBar(): void {
    popup.dataset.mode = "bar";
    popup.setAttribute("role", "toolbar");
    popup.setAttribute("aria-label", "Selection actions");
    const items: HTMLElement[] = COLORS.map((color) => {
      const swatch = el("button", "swatch");
      swatch.type = "button";
      swatch.dataset.color = color;
      swatch.setAttribute("aria-label", `Highlight ${color}`);
      swatch.addEventListener("click", () => onColor(color));
      return swatch;
    });
    const hasFix = typeof actions.fix === "function";
    const hasAsk = typeof actions.ask === "function";
    const showAsk = hasAsk || Boolean(options.askDisabledReason);
    if (hasFix || showAsk) {
      const divider = el("span", "popover-divider");
      divider.setAttribute("aria-hidden", "true");
      items.push(divider);
    }
    if (hasFix) {
      const fixBtn = button("popover-action", "Fix this section");
      fixBtn.dataset.action = "fix";
      fixBtn.addEventListener("click", () => begin("fix"));
      items.push(fixBtn);
    }
    if (showAsk) {
      const askBtn = button("popover-action", "Ask");
      askBtn.dataset.action = "ask";
      if (options.askDisabledReason) {
        askBtn.disabled = true;
        askBtn.title = options.askDisabledReason;
        askBtn.setAttribute("aria-disabled", "true");
        const reason = el("span", "popover-disabled-reason", options.askDisabledReason);
        items.push(askBtn, reason);
      } else {
        if (options.targetChapterTitle) {
          askBtn.title = `Saved with chapter ${options.targetChapterTitle}`;
        }
        askBtn.addEventListener("click", () => begin("ask"));
        items.push(askBtn);
      }
    }
    // Keep the text selected while a button is pressed, so the highlight still has its passage.
    for (const item of items) item.addEventListener("mousedown", (event) => event.preventDefault());
    popup.replaceChildren(...items);
    place();
  }

  /** Builds the box once. Each phase after that only changes what it shows, so typing keeps its focus. */
  function begin(which: Kind): void {
    kind = which;
    const copy = COPY[which];
    popup.dataset.mode = which;
    popup.setAttribute("role", "dialog");
    popup.setAttribute("aria-label", copy.label);

    const field = el("textarea", "popup-field");
    field.rows = 3;
    field.placeholder = copy.placeholder;
    field.setAttribute("aria-label", copy.field);
    const status = el("div", "popup-status");
    status.setAttribute("role", "status");
    const cancel = button("popup-cancel", "Cancel");
    const submit = el("button", "popup-submit", copy.submit);
    submit.type = "submit";
    const buttons = el("div", "popup-actions");
    buttons.append(cancel, submit);
    const form = el("form", "popup-form");
    form.append(field, status, buttons);

    const result = el("div", "popup-result");
    result.hidden = true;
    const answerText = el("div", "popup-answer prose");
    const sources = el("p", "popup-sources");
    const again = button("popup-again", "Ask another");
    const done = button("popup-close", "Close");
    const resultActions = el("div", "popup-actions");
    resultActions.append(again, done);
    result.append(answerText, sources, resultActions);

    const passage = el("blockquote", "popup-quote", shortened(quote));
    const parts: HTMLElement[] = [passage];
    if (which === "ask" && options.targetChapterTitle) {
      parts.push(el("p", "popup-hint popup-chapter-hint", `Saved with chapter ${options.targetChapterTitle}`));
    }
    parts.push(form, result);
    popup.replaceChildren(...parts);
    // Place the box once, here. Phase changes must not move it: the answer is
    // height-capped with its own scroll, so a tall answer stays beside the
    // passage instead of flipping over the chapter title.
    place();

    function draw(state: Flow<unknown>): void {
      popup.dataset.phase = state.phase;
      const running = state.phase === "running";
      field.disabled = running;
      if (state.phase === "running") field.value = state.text;
      buttons.hidden = running;
      submit.textContent = state.phase === "failed" ? "Try again" : copy.submit;
      status.replaceChildren();
      if (running) {
        status.append(el("p", "popup-running", copy.running), el("p", "popup-hint", "Follow the job in the Agent jobs log."));
      } else if (state.phase === "failed") {
        const alert = el("p", "error", `${which === "fix" ? "The section was not changed" : "The question could not be answered"}: ${state.error}`);
        alert.setAttribute("role", "alert");
        status.append(alert);
      }
      const showing = state.phase === "done" && which === "ask";
      form.hidden = showing;
      result.hidden = !showing;
      if (showing) {
        const answer = (state as Extract<Flow<Answer>, { phase: "done" }>).result;
        // The agent wrote this, so it goes through the same renderer as a chapter and nowhere else.
        answerText.innerHTML = renderMarkdown(answer.answer);
        sources.textContent = answer.sources.length > 0 ? `Sources: ${answer.sources.join(", ")}` : "";
        sources.className = "popup-sources";
        sources.hidden = answer.sources.length === 0;
      } else if (state.phase === "editing" && field.value !== state.text) {
        field.value = state.text;
      }
    }

    const onChange = (next: Flow<unknown>, previous: Flow<unknown>): void => {
      if (closed || next.phase === previous.phase) return;
      // A fix that worked has already put the revised chapter on screen, so there is nothing left to show.
      if (next.phase === "done" && which === "fix") close();
      else draw(next);
    };
    if (which === "fix") {
      if (!actions.fix) throw new Error("Missing fix action");
      flow = flowController<void>((text) => actions.fix!(quote, text), onChange);
    } else {
      if (!actions.ask) throw new Error("Missing ask action");
      flow = flowController<Answer>((text) => actions.ask!(quote, text), onChange);
    }
    const currentFlow = flow!;

    field.addEventListener("input", () => currentFlow.send({ type: "edit", text: field.value }));
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      currentFlow.send({ type: "edit", text: field.value });
      currentFlow.send({ type: "submit" });
    });
    field.addEventListener("keydown", (event) => {
      if (event.key !== "Enter" || event.shiftKey || event.isComposing) return;
      event.preventDefault();
      form.requestSubmit();
    });
    cancel.addEventListener("click", close);
    done.addEventListener("click", close);
    again.addEventListener("click", () => currentFlow.send({ type: "open" }));

    currentFlow.send({ type: "open" });
    field.focus();
  }

  popup.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && active()?.state.phase !== "running") close();
  });

  drawBar();
  host.append(popup);
  place();
  host.addEventListener("scroll", onHostScroll);

  return {
    busy: () => active()?.state.phase === "running",
    engaged: () => kind !== null,
    close,
  };
}
