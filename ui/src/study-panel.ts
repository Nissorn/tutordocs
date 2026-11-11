import "./study-panel.css";
import type { NotesSink } from "./annotations";
import { el } from "./dom";
import { renderMarkdown } from "./markdown";
import { askAbout, chapterContext, listAnswers, type Answer } from "./study-api";
import { readCollapsed, writeCollapsed, type PrefStorage } from "./study-prefs";
import {
  emptyStudy,
  flowController,
  studyReduce,
  type Flow,
  type Goals,
  type Section,
  type SourceChip,
  type StudyEvent,
} from "./study-state";

// The right column of a written chapter. It draws a StudyModel: each section is
// loading, ready, empty or error, and the notes section hosts the cards that
// annotations.ts draws. Nothing here holds state the model does not.

export interface StudyPanelOptions {
  /** The `aside.margin` the panel fills. */
  margin: HTMLElement;
  courseId: string;
  chapterId: string;
  storage: PrefStorage | null;
  /** The sources in the library the reader can open: a chip for one of these is a button. */
  readable: readonly string[];
  openSource(name: string): void;
  /** Tells the page how wide the column is: collapsed to a rail, or open. */
  onLayout(layout: "open" | "collapsed"): void;
  /** Retires listeners when the panel unmounts. */
  signal?: AbortSignal;
}

export interface StudyPanel {
  /** Where the note cards go, and how the marks report their count. */
  notes: NotesSink;
  /** Asks about a passage, or the whole chapter for a null quote, and files the answer in the panel. */
  ask(quote: string | null, question: string): Promise<Answer>;
}

const COPY = {
  goals: "No goals were written for this chapter.",
  sources: "No sources are tied to this chapter.",
  notes: "No notes yet. Select a passage to highlight it and add a note.",
  answers: "No questions yet. Ask below, or select a passage and choose Ask.",
};

function bars(): HTMLElement {
  const node = el("div", "study-skeleton");
  node.setAttribute("aria-hidden", "true");
  node.append(el("span"), el("span"), el("span"));
  return node;
}

function sourceChip(chip: SourceChip, open: (name: string) => void): HTMLElement {
  if (!chip.opens) return el("span", "chip", chip.name);
  const button = el("button", "chip", chip.name);
  button.type = "button";
  button.title = `Open ${chip.name}`;
  button.addEventListener("click", () => open(chip.name));
  return button;
}

function chipList(chips: SourceChip[], open: (name: string) => void): HTMLElement {
  const list = el("ul", "chips");
  for (const chip of chips) {
    const item = el("li");
    item.append(sourceChip(chip, open));
    list.append(item);
  }
  return list;
}

function asked(iso: string): string {
  const at = new Date(iso);
  return Number.isNaN(at.getTime())
    ? iso
    : at.toLocaleString(undefined, { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
}

function isNarrow(): boolean {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return false;
  return window.matchMedia("(max-width: 1080px)").matches;
}

export function mountStudyPanel(options: StudyPanelOptions): StudyPanel {
  const { margin, courseId, chapterId, storage, readable, openSource, onLayout } = options;
  let model = emptyStudy();
  let collapsed = isNarrow() ? true : readCollapsed(storage);

  const panel = el("section", "study-panel");
  const head = el("header", "study-head");
  const toggle = el("button", "panel-toggle");
  toggle.type = "button";
  head.append(el("h2", "study-title", "Study panel"), toggle);
  const body = el("div", "study-body");
  const scroll = el("div", "study-scroll");

  const notesHost = el("div", "study-notes");

  function box(name: string, title: string): { box: HTMLElement; body: HTMLElement } {
    const node = el("section", "study-section");
    node.dataset.section = name;
    node.dataset.state = "loading";
    const header = el("div", "section-head");
    header.setAttribute("aria-label", `${title} section`);
    const h2 = el("h2", undefined, title);
    const indicator = el("span", "section-indicator", "▾");
    indicator.setAttribute("aria-hidden", "true");
    header.append(h2, indicator);

    const content = el("div", "section-body");
    node.append(header, content);

    let sectionCollapsed = false;
    const updateCollapse = () => {
      node.dataset.collapsed = String(sectionCollapsed);
      indicator.textContent = sectionCollapsed ? "▸" : "▾";
      content.hidden = sectionCollapsed;
      if (name === "notes") {
        notesHost.hidden = sectionCollapsed;
      }
    };
    header.addEventListener("click", () => {
      sectionCollapsed = !sectionCollapsed;
      updateCollapse();
    });

    return { box: node, body: content };
  }
  const goals = box("goals", "Goals");
  const sources = box("sources", "Sources");
  const notes = box("notes", "Notes");
  const answers = box("answers", "Questions");
  notes.box.append(notesHost);
  scroll.append(goals.box, sources.box, notes.box, answers.box);

  /** One section's body for each state. `ready` draws the section's own content. */
  function draw<T>(
    part: { box: HTMLElement; body: HTMLElement },
    section: Section<T>,
    empty: string,
    ready: (value: T) => Node,
    retry?: () => void,
  ): void {
    part.box.dataset.state = section.state;
    switch (section.state) {
      case "loading":
        part.body.replaceChildren(bars());
        break;
      case "empty":
        part.body.replaceChildren(el("p", "study-empty", empty));
        break;
      case "ready":
        part.body.replaceChildren(ready(section.value));
        break;
      case "error": {
        const alert = el("p", "error", section.message);
        alert.setAttribute("role", "alert");
        part.body.replaceChildren(alert);
        if (retry) {
          const again = el("button", "retry", "Try again");
          again.type = "button";
          again.addEventListener("click", retry);
          part.body.append(again);
        }
      }
    }
  }

  function goalsView({ summary, objectives }: Goals): Node {
    const fragment = document.createDocumentFragment();
    if (summary.trim() !== "") fragment.append(el("p", "study-summary", summary));
    if (objectives.length > 0) {
      const list = el("ul", "study-goals");
      for (const objective of objectives) list.append(el("li", undefined, objective));
      fragment.append(list);
    }
    return fragment;
  }

  function answerView(answer: Answer): HTMLElement {
    const node = el("article", "answer");
    node.dataset.answerId = String(answer.id);
    node.append(el("p", "answer-question", answer.question));
    if (answer.quote !== null) node.append(el("blockquote", "answer-quote", answer.quote));
    // The agent wrote this, so it goes through the same renderer as a chapter and nowhere else.
    const text = el("div", "answer-body prose");
    text.innerHTML = renderMarkdown(answer.answer);
    const meta = el("footer", "answer-meta");
    if (answer.sources.length > 0) {
      const cited = el("span", "answer-sources");
      cited.append(
        "Sources",
        chipList(
          answer.sources.map((name) => ({ name, opens: readable.includes(name) })),
          openSource,
        ),
      );
      meta.append(cited);
    }
    const time = el("time", undefined, asked(answer.askedAt));
    time.dateTime = answer.askedAt;
    meta.append(time);
    node.append(text, meta);
    return node;
  }

  function drawNotes(): void {
    const state = model.notes;
    notes.box.dataset.state = state.state;
    if (state.state === "empty") notes.body.replaceChildren(el("p", "study-empty", COPY.notes));
    else if (state.state === "error") {
      const alert = el("p", "error", `Could not load marks: ${state.message}`);
      alert.setAttribute("role", "alert");
      notes.body.replaceChildren(alert);
    } else notes.body.replaceChildren();
  }

  /** Redraws only the sections the event changed, so a note being typed or an answer being read is left alone. */
  function dispatch(event: StudyEvent): void {
    const before = model;
    model = studyReduce(model, event);
    if (model.goals !== before.goals) draw(goals, model.goals, COPY.goals, goalsView, loadContext);
    if (model.sources !== before.sources) {
      draw(sources, model.sources, COPY.sources, (chips) => chipList(chips, openSource), loadContext);
    }
    if (model.notes !== before.notes) drawNotes();
    if (model.answers !== before.answers) {
      if (event.type === "answer") {
        answers.box.dataset.state = "ready";
        let list = answers.body.querySelector<HTMLElement>(":scope > .answers");
        if (!list) {
          list = el("div", "answers");
          answers.body.replaceChildren(list);
        }
        const existingNode = list.querySelector<HTMLElement>(`article.answer[data-answer-id="${CSS.escape(String(event.answer.id))}"]`);
        if (!existingNode) {
          const newNode = answerView(event.answer);
          list.prepend(newNode);
          newNode.scrollIntoView?.({ behavior: "smooth", block: "nearest" });
        }
      } else {
        draw(
          answers,
          model.answers,
          COPY.answers,
          (items) => {
            const list = el("div", "answers");
            list.append(...items.map(answerView));
            return list;
          },
          loadAnswers,
        );
      }
    }
  }

  function loadContext(): void {
    dispatch({ type: "context-loading" });
    chapterContext(courseId, chapterId).then(
      (context) => dispatch({ type: "context", context, readable }),
      (error: unknown) => dispatch({ type: "context-failed", error }),
    );
  }

  function loadAnswers(): void {
    dispatch({ type: "answers-loading" });
    listAnswers(courseId, chapterId).then(
      (items) => dispatch({ type: "answers", answers: items }),
      (error: unknown) => dispatch({ type: "answers-failed", error }),
    );
  }

  async function ask(quote: string | null, question: string): Promise<Answer> {
    const answer = await askAbout(courseId, chapterId, quote, question);
    dispatch({ type: "answer", answer });
    return answer;
  }

  const form = el("form", "ask-box");
  const field = el("textarea");
  field.rows = 2;
  field.placeholder = "Ask about this chapter";
  field.setAttribute("aria-label", "Ask about this chapter");
  const submit = el("button", undefined, "Ask");
  submit.type = "submit";
  const status = el("p", "ask-status");
  status.setAttribute("role", "status");
  const actions = el("div", "ask-actions");
  actions.append(status, submit);
  form.append(field, actions);

  function drawAsk(state: Flow<Answer>): void {
    const running = state.phase === "running";
    field.disabled = running;
    submit.disabled = running;
    status.replaceChildren();
    if (running) status.textContent = "Asking…";
    else if (state.phase === "failed") {
      const alert = el("p", "error", `The question could not be answered: ${state.error}`);
      alert.setAttribute("role", "alert");
      status.append(alert);
    } else if (state.phase === "done") {
      field.value = "";
      status.textContent = "Answer added above.";
    }
  }

  const asking = flowController<Answer>((text) => ask(null, text), drawAsk);
  field.addEventListener("input", () => asking.send({ type: "edit", text: field.value }));
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    asking.send({ type: "edit", text: field.value });
    asking.send({ type: "submit" });
  });
  field.addEventListener("keydown", (event) => {
    if (event.key !== "Enter" || event.shiftKey || event.isComposing) return;
    event.preventDefault();
    form.requestSubmit();
  });

  function drawCollapse(): void {
    body.hidden = collapsed;
    toggle.setAttribute("aria-expanded", String(!collapsed));
    toggle.setAttribute("aria-label", collapsed ? "Show the study panel" : "Hide the study panel");
    toggle.textContent = collapsed ? "‹" : "›";
    onLayout(collapsed ? "collapsed" : "open");
  }
  toggle.addEventListener("click", () => {
    collapsed = !collapsed;
    if (!isNarrow()) {
      writeCollapsed(storage, collapsed);
    }
    drawCollapse();
  });

  let removeMediaListener: (() => void) | undefined;
  if (typeof window !== "undefined" && typeof window.matchMedia === "function") {
    const mq = window.matchMedia("(max-width: 1080px)");
    const onMediaChange = (event: MediaQueryListEvent | MediaQueryList) => {
      if (event.matches) {
        if (!collapsed) {
          collapsed = true;
          drawCollapse();
        }
      } else {
        const widePref = readCollapsed(storage);
        if (collapsed !== widePref) {
          collapsed = widePref;
          drawCollapse();
        }
      }
    };
    mq.addEventListener?.("change", onMediaChange);
    removeMediaListener = () => {
      mq.removeEventListener?.("change", onMediaChange);
    };
    options.signal?.addEventListener("abort", () => removeMediaListener?.(), { once: true });
  }

  body.append(scroll, form);
  panel.append(head, body);
  margin.replaceChildren(panel);
  drawCollapse();
  drawNotes();
  loadContext();
  loadAnswers();

  return {
    notes: {
      host: notesHost,
      count: (count) => dispatch({ type: "notes", count }),
      failed: (error) => dispatch({ type: "notes-failed", error }),
    },
    ask,
  };
}
