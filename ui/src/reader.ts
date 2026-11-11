import {
  listChapters,
  readChapter,
  readPdfSource,
  type ChapterDoc,
  type ChapterSummary,
  type ReadyChapter,
} from "./api";
import { mountAnnotations } from "./annotations";
import { describeProgress, openChapter, regenerateChapter } from "./generate";
import { rewriteForm, rewriteInstruction } from "./rewrite-request";
import type { JobFeed } from "./job-feed";
import { renderMarkdown } from "./markdown";
import { getOutline, isDraftRefusal, type Outline } from "./outline-api";
import { mountOutlineReview } from "./outline-review";
import { mountPdf } from "./pdf-reader";
import { progressPanel } from "./progress";
import { quizPage, type Graded } from "./quiz";
import { expectJobLabel } from "./job-label";
import { listReadableSources, readTextSource, type ReadableSource } from "./sources-api";
import { askAbout, reviseSection } from "./study-api";
import { themeFigures } from "./figure-theme";
import { mountPanelResizers } from "./panel-resize";
import type { SettingsStore } from "./settings";
import { mountStudyPanel } from "./study-panel";
import { browserStorage, type PrefStorage } from "./study-prefs";
import type { TabPosition, ReaderView, QuizTabState, PdfTabState } from "./tabs";
import { mountVisuals } from "./visuals/host";
import { mountWizard } from "./wizard";
import type { CourseRef } from "./wizard-model";
import { type LibraryTools, NO_CHAPTER_MENU } from "./library-tools";

const PLACEHOLDER_NOTE = "This chapter has not been written yet; opening it writes it.";

type DocumentState = "empty" | "loading" | "ready" | "placeholder" | "writing" | "review" | "setup" | "quiz" | "error";

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function label(title: string, id: string): string {
  return title.trim() === "" ? id : title;
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function skeleton(): HTMLElement {
  const bars = el("div", "skeleton");
  bars.setAttribute("aria-hidden", "true");
  bars.append(el("span"), el("span"), el("span"));
  return bars;
}

/** The course a reader shows. Its `id` is the folder id every course command takes. */
export interface ReaderCourse {
  id: string;
  title: string;
  /** The id inside course.json, which job records carry. The page passes it. Without it jobs are matched by `id`. */
  storedId?: string;
  /** The course's tags, kept when its setup is finished. */
  tags?: string[];
}

export interface ReaderOptions {
  /** The sidebar footer handle, which app.ts passes. When omitted, no footer is shown. */
  tools?: LibraryTools;
  /**
   * Where the user was. A chapter that is still a placeholder is only opened
   * when `rejoin` says a chapter job is already running, because opening a
   * placeholder starts a job.
   */
  restore?: TabPosition & { rejoin: boolean };
  /** Called whenever the chapter on screen or its scroll offset changes. */
  onPosition?: (position: TabPosition) => void;
  /** An outline was planned or a chapter was written, so the course's summary on Home is out of date. */
  onChanged?: () => void;
  /** The app's job feed, which the outline screens read for the running agent job. */
  jobs?: JobFeed | null;
  /** Aborting retires this reader: its pending jobs stop painting. They keep running in Rust. */
  signal?: AbortSignal;
  /** Where the study panel's collapse choice is remembered. Null for none. Defaults to the window's localStorage. */
  storage?: PrefStorage | null;
  /** The settings the sidebar and study panel widths are kept in. Omitted, the columns are not resizable. */
  settings?: SettingsStore;
}

/**
 * Mounts the three-column reader for one course into `root` and resolves once
 * its chapters have loaded and the first chapter (if any) is showing. It never
 * rejects: a course that cannot be opened is shown in the document column. A
 * restore that rejoins a running chapter job resolves when that job ends.
 */
export async function mountReader(root: HTMLElement, course: ReaderCourse, options: ReaderOptions = {}): Promise<void> {
  const { tools, restore, onPosition, onChanged, signal } = options;
  const ref: CourseRef = { id: course.id, storedId: course.storedId ?? course.id };
  const storage = options.storage === undefined ? browserStorage() : options.storage;
  const shell = el("div", "reader");
  const sidebar = el("nav", "sidebar");
  sidebar.setAttribute("aria-label", "Library");
  const page = el("main", "document");
  const margin = el("aside", "margin");
  margin.setAttribute("aria-label", "Study panel");
  shell.append(sidebar, page, margin);
  root.replaceChildren(shell);
  if (options.settings) mountPanelResizers(shell, { settings: options.settings, signal });
  // Appended now, not after the course loads, so the footer is never out of the page while it waits.
  if (tools) sidebar.append(tools.footer);

  let current: string | null = null;
  let lastChapterId: string | null = null;
  let currentView: ReaderView = "chapter";
  let currentQuizDraft: QuizTabState | undefined = restore?.view === "quiz" ? restore.quiz : undefined;
  let currentPdfState: PdfTabState | undefined = restore?.view === "pdf" ? restore.pdf : undefined;

  const report = (): void => {
    if (signal?.aborted) return;
    if (currentView === "quiz") {
      if (currentQuizDraft) {
        onPosition?.({
          chapterId: current,
          scrollTop: 0,
          view: "quiz",
          quiz: currentQuizDraft,
        });
      }
    } else if (currentView === "pdf") {
      if (currentPdfState) {
        onPosition?.({
          chapterId: null,
          scrollTop: page.scrollTop,
          view: "pdf",
          pdf: currentPdfState,
        });
      }
    } else if (currentView === "review") {
      onPosition?.({
        chapterId: null,
        scrollTop: page.scrollTop,
        view: "review",
      });
    } else {
      onPosition?.({ chapterId: current, scrollTop: page.scrollTop, view: "chapter" });
    }
  };
  page.addEventListener("scroll", report, { passive: true });

  // A page of links must never navigate the app window away from the reader.
  page.addEventListener("click", (event) => {
    if (event.target instanceof Element && event.target.closest("a")) event.preventDefault();
  });

  /** The sources the reader can open. A chip in the study panel opens one of these. */
  let readable: ReadableSource[] = [];
  let currentChapters: ChapterSummary[] = [];
  let courseSection: HTMLElement | null = null;
  let latestOpen = 0;
  signal?.addEventListener("abort", () => (latestOpen = -1));
  const progress = progressPanel();

  /** The outline review or the setup wizard on screen, retired whenever anything else takes the page. */
  let viewRun: AbortController | null = null;
  signal?.addEventListener("abort", () => viewRun?.abort());

  function show(state: DocumentState, ...children: Node[]): void {
    viewRun?.abort();
    viewRun = null;
    delete shell.dataset.mode;
    page.dataset.state = state;
    margin.replaceChildren();
    delete shell.dataset.panel;
    const article = el("article", "page");
    article.append(...children);
    page.replaceChildren(article);
  }

  function markCurrent(id: string): void {
    for (const button of sidebar.querySelectorAll<HTMLButtonElement>("button.chapter")) {
      if ((button.dataset.chapterId ?? button.dataset.sourceName) === id) button.setAttribute("aria-current", "page");
      else button.removeAttribute("aria-current");
    }
  }

  function markWritten(chapterId: string): void {
    const button = sidebar.querySelector<HTMLButtonElement>(`button.chapter[data-chapter-id="${CSS.escape(chapterId)}"]`);
    if (button) button.dataset.state = "ready";
    currentChapters = currentChapters.map((c) => (c.id === chapterId ? { ...c, placeholder: false } : c));
  }

  async function showChapter(courseId: string, chapter: ReadyChapter, scrollTop = 0): Promise<void> {
    lastChapterId = chapter.id;
    currentChapters = currentChapters.map((c) => (c.id === chapter.id ? { ...c, placeholder: false } : c));
    currentView = "chapter";
    current = chapter.id;
    currentQuizDraft = undefined;
    currentPdfState = undefined;
    const body = el("div", "prose");
    body.innerHTML = renderMarkdown(chapter.markdown, chapter.id);
    themeFigures(body);
    // The tools sit outside .prose so the text marks anchor to stays the chapter's.
    const chapterTools = rewriteTools(courseId, chapter);
    show("ready", chapterTools.node, body);
    const run = new AbortController();
    viewRun = run;
    signal?.addEventListener("abort", () => run.abort(), { once: true });
    tools?.updateActions({
      kind: "chapter",
      title: label(chapter.title, chapter.id),
      onRewrite: () => chapterTools.promptRewrite(),
      onQuiz: () => openQuiz(chapter, undefined, page.scrollTop),
    });
    page.scrollTop = scrollTop;
    void mountVisuals(body, chapter.id);
    const study = mountStudyPanel({
      margin,
      courseId,
      chapterId: chapter.id,
      storage,
      readable: readable.map((source) => source.name),
      openSource: (name) => void openSource(name),
      onLayout: (layout) => (shell.dataset.panel = layout),
      signal: run.signal,
    });
    await mountAnnotations(chapter.id, body, margin, study.notes, {
      fix: async (quote, instruction) => {
        const mine = latestOpen;
        const claim = expectJobLabel(options.jobs ?? null, [ref.id, ref.storedId], "rewrite", "Section revise");
        let doc;
        try {
          doc = await reviseSection(courseId, chapter.id, quote, instruction);
        } catch (error) {
          claim.release(options.jobs?.jobs() ?? []);
          throw error;
        }
        if (mine === latestOpen) await showChapter(courseId, doc, page.scrollTop);
      },
      ask: (quote, question) => study.ask(quote, question),
    });
  }

  /** Opening a placeholder writes it; a second open joins the same job in Rust. */
  async function writeChapter(courseId: string, chapter: ChapterDoc, mine: number): Promise<void> {
    tools?.updateActions({
      kind: "none",
      reason: "Chapter has not been written yet",
    });
    const title = () => el("h1", undefined, label(chapter.title, chapter.id));
    const status = el("p", "generation-status", "Starting the agent…");
    status.setAttribute("role", "status");
    show("writing", title(), status, skeleton());
    try {
      const doc = await openChapter(courseId, chapter.id, (p) => (status.textContent = describeProgress(p)));
      markWritten(chapter.id);
      onChanged?.();
      if (mine === latestOpen) await showChapter(courseId, doc);
    } catch (error) {
      if (mine !== latestOpen) return;
      if (isDraftRefusal(error)) return void showOutline();
      const alert = el("p", "error", `The chapter could not be written: ${message(error)}`);
      alert.setAttribute("role", "alert");
      const retry = el("button", "quiet-button", "Try again");
      retry.type = "button";
      retry.addEventListener("click", () => void open(courseId, { ...chapter, placeholder: true }));
      show("placeholder", title(), el("p", "placeholder-note", PLACEHOLDER_NOTE), alert, retry);
    }
  }

  function openQuiz(chapter: ChapterSummary | ReadyChapter, quizDraft?: QuizTabState, backScroll = 0): void {
    latestOpen++;
    current = chapter.id;
    currentView = "quiz";
    tools?.updateActions(NO_CHAPTER_MENU);
    currentQuizDraft = quizDraft ?? {
      answers: {},
      grades: [],
    };
    currentPdfState = undefined;
    markCurrent(chapter.id);
    const back = () => {
      currentView = "chapter";
      currentQuizDraft = undefined;
      if ("markdown" in chapter) void showChapter(course.id, chapter as ReadyChapter, backScroll);
      else void open(course.id, chapter, backScroll);
    };
    show(
      "quiz",
      quizPage(
        chapter.id,
        label(chapter.title, chapter.id),
        back,
        () => void progress.refresh(),
        () => void showOutline(),
        currentQuizDraft,
        (draft) => {
          currentQuizDraft = draft;
          report();
        },
      ),
    );
    // The quiz has no use for the study panel, so the reader collapses the
    // margin column for it; `show` above cleared any earlier mode.
    shell.dataset.mode = "quiz";
    page.scrollTop = 0;
    report();
  }

  /**
   * Regeneration replaces the chapter in Rust only when the new job succeeds,
   * then shows it again, which re-anchors every mark and lists the ones that no
   * longer fit under "Needs attention".
   */
  function rewriteTools(courseId: string, chapter: ReadyChapter): { node: HTMLElement; promptRewrite: () => void } {
    const tools = el("div", "chapter-tools");
    const status = el("span", "generation-status");
    status.setAttribute("role", "status");

    const rewrite = el("button", "quiet-button chapter-rewrite", "Rewrite chapter");
    rewrite.type = "button";
    rewrite.title = `Rewrite ${label(chapter.title, chapter.id)}`;
    rewrite.addEventListener("click", () => promptRewrite());

    function promptRewrite(): void {
      if (tools.querySelector(".rewrite-confirm-box")) {
        tools.querySelector<HTMLButtonElement>(".rewrite-confirm")?.focus();
        return;
      }
      const confirmBox = el("div", "rewrite-confirm-box");
      const prompt = el("span", "rewrite-confirm-prompt", "Rewrite this chapter? Rewriting replaces the text.");
      const form = rewriteForm();
      const actions = el("div", "rewrite-actions");
      const confirmBtn = el("button", "quiet-button rewrite-confirm", "Confirm rewrite");
      confirmBtn.type = "button";
      const cancelBtn = el("button", "quiet-button rewrite-cancel", "Cancel");
      cancelBtn.type = "button";

      cancelBtn.addEventListener("click", () => {
        confirmBox.remove();
      });

      confirmBtn.addEventListener("click", async () => {
        const mine = latestOpen;
        const currentScroll = page.scrollTop;
        confirmBtn.disabled = true;
        cancelBtn.disabled = true;
        form.setDisabled(true);
        status.className = "generation-status";
        status.textContent = "Starting the agent…";
        try {
          const doc = await regenerateChapter(
            courseId,
            chapter.id,
            (p) => (status.textContent = describeProgress(p)),
            rewriteInstruction(form.request()),
          );
          if (mine === latestOpen) await showChapter(courseId, doc, currentScroll);
        } catch (error) {
          confirmBtn.disabled = false;
          cancelBtn.disabled = false;
          form.setDisabled(false);
          status.className = "generation-status error";
          status.textContent = `The chapter could not be rewritten, so it is unchanged: ${message(error)}`;
        }
      });

      actions.append(confirmBtn, cancelBtn);
      confirmBox.append(prompt, form.node, actions);
      tools.prepend(confirmBox);
      confirmBtn.focus();
    }

    tools.append(rewrite, status);
    return { node: tools, promptRewrite };
  }

  /** Starts the course over from the backend, so it shows what the user's last action left. */
  function reload(): void {
    if (!signal?.aborted) void mountReader(root, course, { ...options, restore: undefined });
  }

  /** Puts the outline review or the wizard in the document column. Saving a review's unsaved edits rides on the abort that retires it. */
  function showView(state: "review" | "setup", mountInto: (host: HTMLElement, runSignal: AbortSignal) => void): void {
    latestOpen++;
    viewRun?.abort();
    const run = new AbortController();
    viewRun = run;
    markCurrent("");
    current = null;
    currentView = state === "review" ? "review" : "chapter";
    currentQuizDraft = undefined;
    currentPdfState = undefined;
    tools?.updateActions(NO_CHAPTER_MENU);
    report();
    shell.dataset.mode = state;
    delete shell.dataset.panel;
    page.dataset.state = state;
    margin.replaceChildren();
    const host = el("div", `${state}-host`);
    page.replaceChildren(host);
    mountInto(host, run.signal);
  }

  function openReview(outline: Outline): void {
    showView("review", (host, runSignal) =>
      mountOutlineReview(host, outline, {
        course: ref,
        jobs: options.jobs ?? null,
        onDone: reload,
        onChange: (updated) => {
          onChanged?.();
          if (updated) {
            const isDraft = updated.status === "draft";
            updateSidebarCourse(currentChapters, isDraft);
            void listChapters(course.id)
              .then((fresh) => {
                if (signal?.aborted) return;
                currentChapters = fresh;
                updateSidebarCourse(currentChapters, isDraft);
              })
              .catch(() => undefined);
          }
        },
        signal: runSignal,
      }),
    );
  }

  /** A course with no outline is set up in the wizard, with its title and anything it already has filled in. */
  function openSetup(): void {
    showView("setup", (host, runSignal) => {
      void mountWizard(host, {
        existing: { ...ref, title: course.title, tags: course.tags ?? [] },
        jobs: options.jobs ?? null,
        onChange: () => onChanged?.(),
        onPlanned: () => {
          onChanged?.();
          if (!runSignal.aborted) reload();
        },
        signal: runSignal,
      });
    });
  }

  /** Reads the outline and shows it, or says why it cannot. */
  async function showOutline(): Promise<void> {
    const mine = ++latestOpen;
    delete shell.dataset.mode;
    markCurrent("");
    current = null;
    report();
    try {
      const outline = await getOutline(course.id);
      if (mine === latestOpen) openReview(outline);
    } catch (error) {
      if (mine !== latestOpen) return;
      const alert = el("p", "error", `Could not read the outline: ${message(error)}`);
      alert.setAttribute("role", "alert");
      show("error", alert);
    }
  }

  async function open(courseId: string, chapter: ChapterSummary, scrollTop = 0): Promise<void> {
    const mine = ++latestOpen;
    currentView = "chapter";
    currentQuizDraft = undefined;
    currentPdfState = undefined;
    markCurrent(chapter.id);
    show("loading", el("h1", undefined, label(chapter.title, chapter.id)), skeleton());
    page.scrollTop = 0;
    current = chapter.id;
    report();
    try {
      const doc = await readChapter(courseId, chapter.id);
      if (mine !== latestOpen) return;
      if (doc.state === "placeholder") await writeChapter(courseId, doc, mine);
      else await showChapter(courseId, doc, scrollTop);
    } catch (error) {
      if (mine !== latestOpen) return;
      if (isDraftRefusal(error)) return void showOutline();
      const alert = el("p", "error", `Could not open this chapter: ${message(error)}`);
      alert.setAttribute("role", "alert");
      show("error", el("h1", undefined, label(chapter.title, chapter.id)), alert);
    }
  }

  async function openSource(name: string, restoredPdf?: PdfTabState): Promise<void> {
    const mine = ++latestOpen;
    markCurrent(name);
    show("loading", el("h1", undefined, name), skeleton());
    page.scrollTop = 0;
    current = null;
    currentView = "pdf";
    currentQuizDraft = undefined;
    currentPdfState = restoredPdf ?? { sourceName: name, page: 1, zoom: 1 };
    tools?.updateActions(NO_CHAPTER_MENU);
    report();
    const kind = readable.find((source) => source.name === name)?.kind ?? "pdf";
    try {
      if (kind === "pdf") {
        const source = await readPdfSource(course.id, name);
        if (mine !== latestOpen) return;
        const body = el("div", "pdf-source");
        show("ready", el("h1", undefined, name), body);
        const writtenChapters = currentChapters.filter((c) => !c.placeholder);
        const targetChapter = (lastChapterId ? currentChapters.find((c) => c.id === lastChapterId && !c.placeholder) : null) ?? writtenChapters[0];
        await mountPdf(source, body, margin, {
          initialPage: currentPdfState.page,
          initialZoom: currentPdfState.zoom,
          onPosition: (pos) => {
            currentPdfState = {
              sourceName: name,
              page: pos.page,
              zoom: pos.zoom,
            };
            report();
          },
          targetChapterTitle: targetChapter?.title,
          askDisabledReason: targetChapter ? undefined : "No chapter is written yet",
          actions: targetChapter
            ? {
                ask: async (quote: string, question: string) => askAbout(course.id, targetChapter.id, quote, question),
              }
            : undefined,
        });
      } else {
        const source = await readTextSource(course.id, name);
        if (mine !== latestOpen) return;
        show("ready", el("h1", undefined, name), sourceText(source.kind, source.text));
      }
    } catch (error) {
      if (mine !== latestOpen) return;
      const alert = el("p", "error", `Could not open this source: ${message(error)}`);
      alert.setAttribute("role", "alert");
      show("error", el("h1", undefined, name), alert);
    }
  }

  /** A source the user imported, read-only: markdown is rendered, text keeps its own line breaks and spacing. */
  function sourceText(kind: "markdown" | "text", text: string): HTMLElement {
    if (kind === "text") return el("pre", "source-text", text);
    const body = el("div", "source-text prose");
    body.innerHTML = renderMarkdown(text);
    return body;
  }

  function renderSources(sources: ReadableSource[]): HTMLElement[] {
    if (sources.length === 0) return [];
    const section = el("section", "course");
    section.append(el("h2", "course-title", "Sources"));
    const items = el("ul", "chapters");
    for (const { name } of sources) {
      const button = el("button", "chapter", name);
      button.type = "button";
      button.dataset.sourceName = name;
      button.dataset.state = "ready";
      button.addEventListener("click", () => void openSource(name));
      const item = el("li");
      item.append(button);
      items.append(item);
    }
    section.append(items);
    return [section];
  }

  function renderCourse(chapters: ChapterSummary[], draft: boolean): HTMLElement {
    const section = el("section", "course");
    section.append(el("h2", "course-title", label(course.title, course.id)));
    if (draft) {
      section.append(el("p", "sidebar-note", "Draft outline. Chapters open after you approve it."));
      return section;
    }
    if (chapters.length > 0) {
      const edit = el("button", "quiet-button", "Edit outline");
      edit.type = "button";
      edit.addEventListener("click", () => void showOutline());
      const tools = el("div", "course-tools");
      tools.append(edit);
      section.append(tools);
    }
    const items = el("ul", "chapters");
    for (const chapter of chapters) {
      const button = el("button", "chapter", label(chapter.title, chapter.id));
      button.type = "button";
      button.dataset.chapterId = chapter.id;
      button.dataset.state = chapter.placeholder ? "placeholder" : "ready";
      if (chapter.placeholder) {
        button.title = "Writes on open";
        button.setAttribute("aria-description", "Writes on open");
      }
      button.addEventListener("click", () => void open(course.id, chapter));
      const item = el("li");
      item.append(button);
      items.append(item);
    }
    section.append(items, progress.node);
    return section;
  }

  function updateSidebarCourse(chapters: ChapterSummary[], draft: boolean): void {
    const next = renderCourse(chapters, draft);
    if (courseSection && courseSection.parentElement === sidebar) {
      courseSection.replaceWith(next);
    }
    courseSection = next;
  }

  /** The outline's status, or null when the backend cannot say, which reads as approved as it did before outlines had one. */
  async function readOutline(): Promise<Outline | null> {
    try {
      return await getOutline(course.id);
    } catch (error) {
      console.warn(`could not read the outline of ${course.id}: ${message(error)}`);
      return null;
    }
  }

  show("loading", skeleton());
  try {
    const chapters = await listChapters(course.id);
    const outline = await readOutline();
    const sources = await listReadableSources(course.id);
    if (signal?.aborted) return;
    readable = sources;
    currentChapters = chapters;
    const draft = outline?.status === "draft";
    courseSection = renderCourse(chapters, draft);
    sidebar.prepend(courseSection, ...renderSources(sources));
    if (draft && outline) return openReview(outline);
    void progress.refresh();
    if (restore?.view === "review") {
      return await showOutline();
    }
    // Opening a placeholder will start a generation job, so a chapter that is
    // not written is only opened to rejoin one that is already running.
    const back = restore?.chapterId ? chapters.find((chapter) => chapter.id === restore.chapterId) : undefined;
    if (restore?.view === "quiz" && back) {
      return openQuiz(back, restore.quiz, restore.scrollTop ?? 0);
    }
    if (restore?.view === "pdf" && restore.pdf?.sourceName) {
      return await openSource(restore.pdf.sourceName, restore.pdf);
    }
    const first = chapters.find((chapter) => !chapter.placeholder);
    if (back && (!back.placeholder || restore?.rejoin)) {
      await open(course.id, back, restore?.scrollTop ?? 0);
    } else if (first) {
      await open(course.id, first);
    } else if (chapters.length === 0) {
      openSetup();
    } else {
      show("empty", el("p", "empty-note", "Choose a chapter from the sidebar."));
    }
  } catch (error) {
    if (signal?.aborted) return;
    const alert = el("p", "error", `Could not open this course: ${message(error)}`);
    alert.setAttribute("role", "alert");
    show("error", alert);
  }
}
