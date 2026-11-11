import "./outline-review.css";
import { el, message } from "./dom";
import type { JobFeed } from "./job-feed";
import { expectJobLabel } from "./job-label";
import { mountJobStatus, type JobStatusHandle } from "./job-status";
import {
  approveOutline,
  branchOutline,
  cancelChapterQueue,
  getChapterQueue,
  getOutlineVariant,
  listOutlineVariants,
  reviseOutline,
  saveOutline,
  setCurrentVariant,
  type ApprovalChoice,
  type Outline,
  type OutlineVariant,
  type VariantMeta,
} from "./outline-api";
import {
  approvalBlock,
  draftFromOutline,
  edit,
  hasComments,
  isDirty,
  removalBlock,
  saveBlock,
  toOutline,
  type DraftChapter,
  type DraftEdit,
  type OutlineDraft,
} from "./outline-draft";
import type { CourseRef } from "./wizard-model";

// The outline review screen. The user's edits live in an `OutlineDraft`
// (outline-draft.ts), and `phase` says what the screen is doing. Edits change
// the draft; Save, Revise and Approve send it to the backend. Nothing the user
// or the agent wrote is set as markup.

export interface ReviewOptions {
  course: CourseRef;
  /** The app's job feed, for the revise job's progress. */
  jobs: JobFeed | null;
  /** The user is finished here: the outline was approved to write on open, or they went back to the chapters. */
  onDone(): void;
  /** The outline was saved, revised or approved, so the course's summary is out of date. */
  onChange?(outline?: Outline): void;
  /** Aborting retires the screen. Edits not yet saved are saved once. */
  signal?: AbortSignal;
}

type Phase =
  | { kind: "editing" }
  | { kind: "saving" }
  | { kind: "revising" }
  | { kind: "approving"; choice: ApprovalChoice; variantId?: string }
  | { kind: "choosing"; choice: ApprovalChoice; variantId?: string }
  | { kind: "approved" }
  | { kind: "comparing"; left: OutlineVariant | null; right: OutlineVariant | null };

type Notice = { kind: "info" | "error"; text: string };

type PointerDrag = {
  key: string;
  pointerId: number;
  row: HTMLElement;
  startX: number;
  startY: number;
  active: boolean;
  placeholder: HTMLElement | null;
  ghost: HTMLElement | null;
};

const CHOICES: Array<{ value: ApprovalChoice; label: string; detail: string }> = [
  { value: "on_open", label: "Write each chapter when I open it", detail: "Nothing runs until you open a chapter." },
  {
    value: "all",
    label: "Write all chapters now",
    detail: "One agent job per chapter, one at a time, in outline order. Each shows in the job log.",
  },
];

function button(text: string, className: string, onClick: () => void): HTMLButtonElement {
  const b = el("button", className, text);
  b.type = "button";
  b.addEventListener("click", onClick);
  return b;
}

function areaOf(value: string, label: string, className: string, rows: number, onInput: (text: string) => void): HTMLTextAreaElement {
  const area = el("textarea", className);
  area.rows = rows;
  area.value = value;
  area.setAttribute("aria-label", label);
  area.addEventListener("input", () => onInput(area.value));
  return area;
}

export function mountOutlineReview(root: HTMLElement, outline: Outline, options: ReviewOptions): void {
  const { course, signal } = options;
  let saved = outline;
  let draft: OutlineDraft = draftFromOutline(outline);
  let phase: Phase = { kind: "editing" };
  let notice: Notice | null = null;
  let stopStatus: JobStatusHandle | null = null;
  let pointerDrag: PointerDrag | null = null;
  /** Run after the next full draw, to put focus back where the user was. */
  let focusAfter: (() => HTMLElement | null | undefined) | null = null;

  let variants: VariantMeta[] | null = null;
  let selectedVariantId = "current";
  let currentVariantId = "current";
  let branching = false;
  let lastCompare: { left: OutlineVariant | null; right: OutlineVariant | null } | null = null;

  const section = el("section", "review");
  section.setAttribute("aria-label", "Outline review");
  const head = el("header", "review-head");
  const variantBar = el("div", "variant-bar");
  const intro = el("p", "review-intro");
  const fields = el("fieldset", "review-fields");
  fields.append(el("legend", "visually-hidden", "Outline"));
  const compareHost = el("div", "compare-host");
  compareHost.hidden = true;
  // The job, the approval choice, any error and the buttons share one dock that sticks to the bottom, so none of them is off screen below a long outline.
  const dock = el("div", "review-dock");
  const panel = el("div", "review-panel");
  const noticeHost = el("div", "review-notices");
  const footer = el("div", "review-footer");
  dock.append(panel, noticeHost, footer);
  section.append(head, variantBar, intro, fields, compareHost, dock);
  root.replaceChildren(section);

  const busy = (): boolean => phase.kind === "saving" || phase.kind === "revising" || phase.kind === "approving";
  const dirty = (): boolean => isDirty(draft, saved);
  const isComparing = (): boolean =>
    phase.kind === "comparing" || (phase.kind === "choosing" && phase.variantId !== undefined);

  // Editing.

  function apply(change: DraftEdit, structural: boolean, after?: () => HTMLElement | null | undefined): void {
    const result = edit(draft, change);
    draft = result.draft;
    if (result.refused) notice = { kind: "error", text: result.refused };
    else if (notice?.kind === "error") notice = null;
    if (structural || result.refused) {
      focusAfter = after ?? null;
      draw();
    } else {
      drawFooter();
      drawNotices();
    }
  }

  const rowOf = (key: string): HTMLElement | null => fields.querySelector(`li.draft-chapter[data-key="${CSS.escape(key)}"]`);
  const move = (chapter: DraftChapter, to: number): void =>
    apply({ kind: "move", key: chapter.key, to }, true, () => rowOf(chapter.key)?.querySelector<HTMLElement>(".drag-handle"));

  // Ends a pointer drag. On commit, the placeholder's slot among the chapter
  // rows is the destination; the dragged row still occupies its old slot in
  // the DOM, so it is discounted when it sits before the placeholder.
  function endPointerDrag(commit: boolean): void {
    const drag = pointerDrag;
    if (!drag) return;
    pointerDrag = null;
    drag.ghost?.remove();
    let to = -1;
    const placeholder = drag.placeholder;
    if (commit && drag.active && placeholder && placeholder.parentElement) {
      let index = 0;
      let countedDragged = false;
      for (const child of Array.from(placeholder.parentElement.children)) {
        if (child === placeholder) break;
        if (child.classList.contains("draft-chapter")) {
          index += 1;
          if (child === drag.row) countedDragged = true;
        }
      }
      to = countedDragged ? index - 1 : index;
    }
    placeholder?.remove();
    drag.row.classList.remove("dragging");
    if (to >= 0 && to !== draft.chapters.findIndex((c) => c.key === drag.key)) {
      apply({ kind: "move", key: drag.key, to }, true, () => rowOf(drag.key)?.querySelector<HTMLElement>(".drag-handle"));
    }
  }

  function chapterRow(chapter: DraftChapter, index: number): HTMLElement {
    const n = index + 1;
    const row = el("li", "draft-chapter");
    row.dataset.key = chapter.key;
    row.dataset.written = String(chapter.written);

    const top = el("div", "draft-top");
    // Tauri's native file-drop handler swallows dragover and drop before they
    // reach the page (measured in notes/46-premise), so reorder uses pointer
    // events with capture instead of HTML5 drag and drop.
    const handle = el("div", "drag-handle", "⋮⋮");
    const locked = busy();
    handle.setAttribute("role", "button");
    handle.tabIndex = locked ? -1 : 0;
    handle.style.minWidth = "44px";
    handle.style.minHeight = "44px";
    if (locked) handle.setAttribute("aria-disabled", "true");
    handle.setAttribute("aria-label", `Move chapter ${n}. Drag, or press the up and down arrow keys.`);
    handle.addEventListener("keydown", (event) => {
      if (busy()) return;
      if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
      event.preventDefault();
      move(chapter, index + (event.key === "ArrowDown" ? 1 : -1));
    });
    handle.addEventListener("pointerdown", (event) => {
      if (busy() || event.button !== 0) return;
      event.preventDefault();
      handle.focus();
      handle.setPointerCapture(event.pointerId);
      pointerDrag = {
        key: chapter.key,
        pointerId: event.pointerId,
        row,
        startX: event.clientX,
        startY: event.clientY,
        active: false,
        placeholder: null,
        ghost: null,
      };
    });
    handle.addEventListener("pointermove", (event) => {
      const drag = pointerDrag;
      if (!drag || drag.pointerId !== event.pointerId) return;
      if (!drag.active) {
        if (Math.hypot(event.clientX - drag.startX, event.clientY - drag.startY) < 6) return;
        drag.active = true;
        const placeholder = el("li", "drag-placeholder");
        placeholder.style.height = `${drag.row.offsetHeight}px`;
        drag.row.before(placeholder);
        drag.row.classList.add("dragging");
        drag.placeholder = placeholder;
        const title = draft.chapters.find((c) => c.key === drag.key)?.title.trim() || "Chapter";
        const ghost = el("div", "drag-ghost", title);
        document.body.append(ghost);
        drag.ghost = ghost;
      }
      event.preventDefault();
      drag.ghost!.style.left = `${event.clientX + 12}px`;
      drag.ghost!.style.top = `${event.clientY + 12}px`;
      const under = document.elementFromPoint(event.clientX, event.clientY)?.closest("li.draft-chapter");
      if (under && under !== drag.row && drag.placeholder) {
        const rect = under.getBoundingClientRect();
        if (event.clientY < rect.top + rect.height / 2) under.before(drag.placeholder);
        else under.after(drag.placeholder);
      }
    });
    handle.addEventListener("pointerup", (event) => {
      if (!pointerDrag || pointerDrag.pointerId !== event.pointerId) return;
      endPointerDrag(true);
    });
    handle.addEventListener("pointercancel", () => endPointerDrag(false));
    handle.addEventListener("lostpointercapture", () => endPointerDrag(false));

    const title = el("input", "draft-title");
    title.type = "text";
    title.value = chapter.title;
    title.placeholder = "Chapter title";
    title.setAttribute("aria-label", `Title of chapter ${n}`);
    title.addEventListener("input", () => apply({ kind: "title", key: chapter.key, text: title.value }, false));

    top.append(handle, el("span", "draft-number", String(n)), title);
    if (chapter.written) {
      top.append(el("span", "written-badge", "Written"));
    }
    const blocked = removalBlock(chapter);
    const remove = button("Remove", "quiet-button draft-remove", () =>
      apply({ kind: "remove-chapter", key: chapter.key }, true, () => {
        const titles = fields.querySelectorAll<HTMLElement>("input.draft-title");
        return titles[Math.min(index, titles.length - 1)] ?? fields.querySelector<HTMLElement>(".add-chapter");
      }),
    );
    remove.setAttribute("aria-label", `Remove chapter ${n}`);
    remove.disabled = busy() || blocked !== null;
    top.append(remove);
    row.append(top);
    if (blocked) {
      const reason = el("p", "draft-reason", blocked);
      reason.id = `reason-${chapter.key}`;
      remove.setAttribute("aria-describedby", reason.id);
      row.append(reason);
    }

    row.append(
      areaOf(chapter.summary, `Summary of chapter ${n}`, "draft-summary", 2, (text) =>
        apply({ kind: "summary", key: chapter.key, text }, false),
      ),
    );

    const chips = el("ul", "source-chips");
    chips.setAttribute("aria-label", `Sources of chapter ${n}`);
    if (chapter.sources.length === 0) chips.append(el("li", "source-none", "No sources"));
    for (const name of chapter.sources) chips.append(el("li", "source-chip", name));
    row.append(chips);

    row.append(
      areaOf(chapter.comment, `Comment on chapter ${n} for the AI`, "draft-comment", 2, (text) =>
        apply({ kind: "comment", key: chapter.key, text }, false),
      ),
    );
    row.querySelector<HTMLTextAreaElement>("textarea.draft-comment")!.placeholder = "Comment for the AI (optional)";
    return row;
  }

  function objectiveRow(text: string, index: number): HTMLElement {
    const row = el("li", "draft-objective-row");
    const input = el("input", "draft-objective");
    input.type = "text";
    input.value = text;
    input.placeholder = "What the learner can do afterwards";
    input.setAttribute("aria-label", `Objective ${index + 1}`);
    input.addEventListener("input", () => apply({ kind: "objective", index, text: input.value }, false));
    const remove = button("Remove", "quiet-button draft-remove-objective", () =>
      apply({ kind: "remove-objective", index }, true, () => fields.querySelector<HTMLElement>(".add-objective")),
    );
    remove.setAttribute("aria-label", `Remove objective ${index + 1}`);
    remove.disabled = busy();
    row.append(input, remove);
    return row;
  }

  // Drawing.

  function drawHead(): void {
    const title = el("h1", undefined, draft.title);
    const status = el("span", "review-status", draft.status === "draft" ? "Draft" : "Approved");
    status.dataset.status = draft.status;
    head.replaceChildren(title, status);
    intro.textContent =
      draft.status === "draft"
        ? "Nothing is written yet. Rename, reorder, add or remove chapters, leave comments for the AI, then approve."
        : "Add or reorder chapters. Chapters that are already written stay as they are.";
  }

  function drawFields(): void {
    const objectives = el("section", "review-section");
    objectives.append(el("h2", undefined, "Objectives"));
    const objectiveList = el("ul", "draft-objectives");
    objectiveList.append(...draft.objectives.map(objectiveRow));
    const addObjective = button("Add an objective", "quiet-button add-objective", () =>
      apply({ kind: "add-objective" }, true, () => fields.querySelector<HTMLElement>("li.draft-objective-row:last-child input")),
    );
    addObjective.disabled = busy();
    objectives.append(objectiveList, addObjective);

    const chapters = el("section", "review-section");
    chapters.append(el("h2", undefined, "Chapters"));
    const list = el("ol", "draft-chapters");
    list.append(...draft.chapters.map(chapterRow));
    const addChapter = button("Add a chapter", "quiet-button add-chapter", () =>
      apply({ kind: "add-chapter" }, true, () => fields.querySelector<HTMLElement>("li.draft-chapter:last-child input.draft-title")),
    );
    addChapter.disabled = busy();
    chapters.append(list, addChapter);
    fields.replaceChildren(fields.querySelector("legend")!, objectives, chapters);
    fields.disabled = false;
  }

  function drawPanel(): void {
    stopStatus?.stop();
    stopStatus = null;
    panel.replaceChildren();
    if (phase.kind === "revising") {
      stopStatus = mountJobStatus(panel, options.jobs, {
        label: "Revising the outline",
        kind: "outline",
        courseIds: [course.id, course.storedId],
      });
    } else if (phase.kind === "choosing" || phase.kind === "approving") {
      panel.append(approvePanel(phase.choice, phase.kind === "approving"));
    } else if (phase.kind === "approved") {
      panel.append(approvedPanel());
    }
  }

  function approvePanel(selected: ApprovalChoice, working: boolean): HTMLElement {
    const box = el("section", "approve-panel");
    box.setAttribute("role", "group");
    box.setAttribute("aria-label", "Approve the outline");
    box.append(el("p", "approve-lead", "How should the chapters be written?"));
    let choice = selected;
    for (const option of CHOICES) {
      const label = el("label", "approve-choice");
      const input = el("input");
      input.type = "radio";
      input.name = "generate";
      input.value = option.value;
      input.checked = option.value === selected;
      input.disabled = working;
      input.addEventListener("change", () => {
        if (input.checked) choice = option.value;
      });
      const text = el("span", "approve-text");
      text.append(el("span", "approve-label", option.label), el("span", "approve-detail", option.detail));
      label.append(input, text);
      box.append(label);
    }
    const actions = el("div", "approve-actions");
    const confirm = button("Approve outline", "outline-submit", () =>
      void approve(choice, phase.kind === "choosing" || phase.kind === "approving" ? phase.variantId : undefined)
    );
    const cancel = button("Cancel", "quiet-button", () => {
      if (lastCompare && isComparing()) {
        phase = { kind: "comparing", left: lastCompare.left, right: lastCompare.right };
      } else {
        phase = { kind: "editing" };
      }
      draw();
    });
    confirm.disabled = cancel.disabled = working;
    actions.append(confirm, cancel);
    box.append(actions);
    return box;
  }

  function approvedPanel(): HTMLElement {
    const box = el("section", "approved-panel");
    const detail = el(
      "p",
      "approve-detail",
      "The agent is writing every chapter, one at a time, in outline order. Follow along in the job log.",
    );
    box.append(el("p", "approve-lead", "Approved."), detail);
    let stopped = false;
    let settled = false;
    let closed = false;
    let lastRunning: string | null = draft.chapters.find((c) => !c.written && c.id)?.id ?? null;
    const chapterName = (id: string): string => draft.chapters.find((c) => c.id === id)?.title.trim() || id;
    const actions = el("div", "approve-actions");
    const stop = button("Stop after the running chapter", "quiet-button", () => {
      stopped = true;
      stop.disabled = true;
      cancelChapterQueue(course.id).then(
        () => {
          if (!settled) {
            notice = { kind: "info", text: "The queue will stop after the chapter that is running." };
            drawNotices();
          }
          void refreshQueue();
        },
        (error: unknown) => {
          stopped = false;
          stop.disabled = false;
          notice = { kind: "error", text: `The queue could not be stopped: ${message(error)}` };
          drawNotices();
        },
      );
    });
    actions.append(button("Open the course", "outline-submit", () => options.onDone()), stop);
    box.append(actions);

    let timer: ReturnType<typeof setInterval> | undefined;

    async function refreshQueue(): Promise<void> {
      if (closed) return;
      const cancelledJob = options.jobs
        ?.jobs()
        .some(
          (j) =>
            j.kind === "chapter" &&
            (j.courseId === course.id || j.courseId === course.storedId) &&
            j.status === "cancelled",
        );
      if (cancelledJob) stopped = true;
      try {
        const queue = await getChapterQueue(course.id);
        if (closed) return;
        if (queue.running !== null) {
          lastRunning = queue.running;
          detail.textContent = `Writing ${chapterName(queue.running)} (${queue.pending.length} left) — the agent is writing every chapter, one at a time, in outline order. Follow along in the job log.`;
        } else if (queue.pending.length === 0) {
          const wasSettled = settled;
          settled = true;
          stop.disabled = true;
          if (stopped) {
            if (notice?.kind === "info") {
              notice = null;
              drawNotices();
            }
            detail.textContent = lastRunning ? `Stopped after ${chapterName(lastRunning)}.` : "Stopped.";
          } else {
            detail.textContent = "All chapters written.";
          }
          if (!wasSettled) options.onChange?.(saved);
          if (timer !== undefined) clearInterval(timer);
        }
      } catch {
        // Leave the default status message when the queue cannot be read.
      }
    }

    const unsubscribe = options.jobs?.subscribe(() => {
      void refreshQueue();
    });
    // The queue drops its running chapter after that chapter's job has already
    // reported itself finished, so the last job event can still see it running.
    // Keep reading the queue until the banner has somewhere to settle.
    timer = setInterval(() => void refreshQueue(), 300);
    stopStatus = {
      stop() {
        closed = true;
        clearInterval(timer);
        unsubscribe?.();
      },
    };
    void refreshQueue();
    return box;
  }

  function drawNotices(): void {
    noticeHost.replaceChildren();
    if (!notice) return;
    if (notice.kind === "error") {
      const alert = el("p", "review-error", notice.text);
      alert.setAttribute("role", "alert");
      noticeHost.append(alert);
    } else {
      const info = el("p", "review-notice", notice.text);
      info.setAttribute("role", "status");
      noticeHost.append(info);
    }
  }

  async function loadVariants(): Promise<void> {
    try {
      const list = await listOutlineVariants(course.id);
      variants = list;
      const curr = list.find((v) => v.current);
      if (curr) {
        currentVariantId = curr.id;
      } else if (list.length > 0) {
        currentVariantId = list[0].id;
      }
      if (!variants.some((v) => v.id === selectedVariantId)) {
        selectedVariantId = currentVariantId;
      }
    } catch (error) {
      notice = { kind: "error", text: `Could not load outline variants: ${message(error)}` };
    }
    draw();
  }
  void loadVariants();

  const activeVariantArg = (id: string = selectedVariantId): string | undefined =>
    id !== currentVariantId ? id : undefined;

  function drawVariantBar(): void {
    variantBar.replaceChildren();
    if (variants === null || isComparing() || phase.kind === "approved") {
      variantBar.hidden = true;
      return;
    }
    variantBar.hidden = false;

    const labelTag = el("span", "variant-bar-label", "Variant:");
    const select = el("select", "variant-select");
    select.setAttribute("aria-label", "Select outline variant");
    select.disabled = busy();

    for (const v of variants) {
      const opt = el("option", undefined, v.id === currentVariantId ? `${v.label} (Active)` : v.label);
      opt.value = v.id;
      opt.selected = v.id === selectedVariantId;
      select.append(opt);
    }
    select.addEventListener("change", () => void switchVariant(select.value));

    variantBar.append(labelTag, select);

    if (branching) {
      const form = el("div", "variant-branch-form");
      const input = el("input", "variant-branch-label");
      input.type = "text";
      input.placeholder = "Variant label, e.g. Draft B";
      input.value = "Draft B";
      input.setAttribute("aria-label", "New variant label");

      const createBtn = button("Create branch", "outline-submit variant-branch-confirm", () => {
        const val = input.value.trim();
        if (!val) {
          notice = { kind: "error", text: "Variant label is required." };
          drawNotices();
          return;
        }
        void branch(val);
      });
      const cancelBtn = button("Cancel", "quiet-button variant-branch-cancel", () => {
        branching = false;
        draw();
      });
      const hint = el("span", "variant-branch-hint", "Written chapters stay in every draft.");
      form.append(input, createBtn, cancelBtn, hint);
      variantBar.append(form);
    } else {
      const branchBtn = button("Branch draft", "quiet-button variant-branch", () => {
        branching = true;
        draw();
      });
      branchBtn.disabled = busy();
      variantBar.append(branchBtn);

      if (selectedVariantId !== currentVariantId) {
        const makeCurrentBtn = button("Set as current", "quiet-button variant-set-current", () => {
          void makeCurrent();
        });
        makeCurrentBtn.disabled = busy();
        variantBar.append(makeCurrentBtn);
      }

      if (variants.length >= 2) {
        const compareBtn = button("Compare", "quiet-button variant-compare", () => {
          void startCompare();
        });
        compareBtn.disabled = busy();
        variantBar.append(compareBtn);
      }
    }
  }

  function drawCompareView(): void {
    compareHost.replaceChildren();
    if (!isComparing()) {
      compareHost.hidden = true;
      return;
    }
    compareHost.hidden = false;

    const head = el("div", "compare-header");
    head.append(
      el("h2", "compare-title", "Compare outline variants"),
      button("Back to editing", "quiet-button compare-close", () => {
        lastCompare = null;
        phase = { kind: "editing" };
        draw();
      })
    );

    const currentCompare = phase.kind === "comparing" ? phase : lastCompare;
    const left = currentCompare?.left ?? null;
    const right = currentCompare?.right ?? null;

    if (!left || !right) {
      compareHost.append(head, el("p", "compare-loading", "Loading variants…"));
      return;
    }

    const container = el("div", "compare-view");

    function renderColumn(side: "left" | "right", variant: OutlineVariant): HTMLElement {
      const otherVariant = side === "left" ? right! : left!;
      const col = el("div", "compare-column");
      col.dataset.variantId = variant.id;

      const colHead = el("div", "compare-col-header");
      const select = el("select", "compare-variant-select");
      select.setAttribute("aria-label", `Select variant for ${side} column`);
      if (variants) {
        for (const v of variants) {
          const opt = el("option", undefined, v.label);
          opt.value = v.id;
          opt.selected = v.id === variant.id;
          select.append(opt);
        }
      }
      select.addEventListener("change", () => void updateCompareVariant(side, select.value));

      const titleRow = el("div", "compare-title-row");
      const title = el("h3", undefined, variant.label);
      titleRow.append(title);
      if (variant.id === currentVariantId) {
        const badge = el("span", "review-status", "Active");
        badge.dataset.status = "approved";
        titleRow.append(badge);
      }

      const actions = el("div", "compare-actions");
      const approveBtn = button(`Approve ${variant.label}`, "outline-submit compare-approve", () => {
        openApproval(variant.id);
      });
      approveBtn.dataset.variantId = variant.id;
      const editBtn = button("Edit this draft", "quiet-button compare-edit", () => {
        selectedVariantId = variant.id;
        saved = { status: draft.status, title: draft.title, objectives: variant.objectives, chapters: variant.chapters };
        draft = draftFromOutline(saved);
        lastCompare = null;
        phase = { kind: "editing" };
        draw();
      });
      actions.append(approveBtn, editBtn);
      colHead.append(select, titleRow, actions);

      const objSection = el("div", "compare-objectives-section");
      objSection.append(el("h4", undefined, "Objectives"));
      const objList = el("ul", "compare-objectives");
      for (const obj of variant.objectives) {
        objList.append(el("li", undefined, obj));
      }
      objSection.append(objList);

      const chapSection = el("div", "compare-chapters-section");
      chapSection.append(el("h4", undefined, "Chapters"));
      const chapList = el("ol", "compare-chapters");
      variant.chapters.forEach((ch, idx) => {
        const row = el("li", "compare-chapter");
        const top = el("div", "compare-chapter-top");
        top.append(
          el("span", "draft-number", String(idx + 1)),
          el("strong", "compare-chapter-title", ch.title)
        );

        const match = otherVariant.chapters.find((c) => c.id && c.id === ch.id);
        let diffLabel: string | null = null;
        if (!match) {
          diffLabel = "Only in this draft";
        } else if (match.title !== ch.title || match.summary !== ch.summary) {
          diffLabel = "Changed";
        }
        if (diffLabel) {
          top.append(el("span", "compare-diff-badge", diffLabel));
        }

        if (ch.written) {
          top.append(el("span", "written-badge", "Written"));
        }
        const summary = el("p", "compare-summary", ch.summary || "No summary");
        const chips = el("ul", "source-chips");
        if (ch.sources.length === 0) chips.append(el("li", "source-none", "No sources"));
        else for (const s of ch.sources) chips.append(el("li", "source-chip", s));
        row.append(top, summary, chips);
        chapList.append(row);
      });
      chapSection.append(chapList);

      col.append(colHead, objSection, chapSection);
      return col;
    }

    container.append(
      renderColumn("left", left),
      renderColumn("right", right)
    );
    compareHost.append(head, container);
  }

  async function branch(label: string): Promise<void> {
    phase = { kind: "saving" };
    notice = null;
    draw();
    try {
      const newVariant = await branchOutline(course.id, selectedVariantId, label);
      await loadVariants();
      selectedVariantId = newVariant.id;
      saved = {
        status: draft.status,
        title: draft.title,
        objectives: newVariant.objectives,
        chapters: newVariant.chapters,
      };
      draft = draftFromOutline(saved);
      branching = false;
      notice = { kind: "info", text: `Branched "${newVariant.label}".` };
    } catch (error) {
      notice = { kind: "error", text: `Could not branch outline: ${message(error)}` };
    }
    phase = { kind: "editing" };
    draw();
  }

  async function switchVariant(variantId: string): Promise<void> {
    if (variantId === selectedVariantId) return;
    phase = { kind: "saving" };
    notice = null;
    draw();
    if (dirty() && !(await persist())) {
      phase = { kind: "editing" };
      draw();
      return;
    }
    try {
      const variant = await getOutlineVariant(course.id, variantId);
      selectedVariantId = variantId;
      saved = {
        status: draft.status,
        title: draft.title,
        objectives: variant.objectives,
        chapters: variant.chapters,
      };
      draft = draftFromOutline(saved);
    } catch (error) {
      notice = { kind: "error", text: `Could not load variant: ${message(error)}` };
    }
    phase = { kind: "editing" };
    draw();
  }

  async function makeCurrent(): Promise<void> {
    phase = { kind: "saving" };
    notice = null;
    draw();
    if (dirty() && !(await persist())) {
      phase = { kind: "editing" };
      draw();
      return;
    }
    try {
      await setCurrentVariant(course.id, selectedVariantId);
      currentVariantId = selectedVariantId;
      await loadVariants();
      notice = { kind: "info", text: "Set as current variant." };
    } catch (error) {
      notice = { kind: "error", text: `Could not set current variant: ${message(error)}` };
    }
    phase = { kind: "editing" };
    draw();
  }

  async function startCompare(): Promise<void> {
    if (dirty()) {
      phase = { kind: "saving" };
      notice = null;
      draw();
      const ok = await persist();
      if (!ok) {
        phase = { kind: "editing" };
        draw();
        return;
      }
    }
    const other = variants?.find((v) => v.id !== selectedVariantId) ?? variants?.[0];
    const leftId = selectedVariantId;
    const rightId = other ? other.id : selectedVariantId;
    phase = { kind: "comparing", left: null, right: null };
    notice = null;
    draw();
    try {
      const [left, right] = await Promise.all([
        getOutlineVariant(course.id, leftId),
        getOutlineVariant(course.id, rightId),
      ]);
      phase = { kind: "comparing", left, right };
      draw();
    } catch (error) {
      notice = { kind: "error", text: `Could not load variants: ${message(error)}` };
      phase = { kind: "editing" };
      draw();
    }
  }

  async function updateCompareVariant(side: "left" | "right", variantId: string): Promise<void> {
    if (phase.kind !== "comparing") return;
    try {
      const loaded = await getOutlineVariant(course.id, variantId);
      if (phase.kind === "comparing") {
        phase = {
          kind: "comparing",
          left: side === "left" ? loaded : phase.left,
          right: side === "right" ? loaded : phase.right,
        };
        draw();
      }
    } catch (error) {
      notice = { kind: "error", text: `Could not load variant: ${message(error)}` };
      draw();
    }
  }

  function drawFooter(): void {
    footer.replaceChildren();
    footer.hidden = phase.kind === "approved" || isComparing();
    if (footer.hidden) return;
    const state = el("p", "review-save", busy() ? (phase.kind === "saving" ? "Saving…" : "") : dirty() ? "Unsaved changes" : "All changes saved");
    state.setAttribute("role", "status");
    const actions = el("div", "review-actions");
    const saveButton = button("Save", "quiet-button", () => void saveClicked());
    saveButton.disabled = busy() || !dirty();
    const reviseButton = button("Revise with my comments", "quiet-button", () => void revise());
    const noComments = !hasComments(draft);
    reviseButton.disabled = busy() || noComments || phase.kind === "choosing";
    actions.append(saveButton, reviseButton);
    if (draft.status === "draft") {
      const approveButton = button("Approve", "outline-submit", () => openApproval());
      approveButton.disabled = busy() || phase.kind === "choosing";
      actions.append(approveButton);
    } else {
      const backButton = button("Back to the chapters", "outline-submit", () => void finish());
      backButton.disabled = busy();
      actions.append(backButton);
    }
    footer.append(state);
    if (noComments && !busy()) {
      footer.append(el("p", "review-hint", "Add a comment to a chapter to revise."));
    }
    footer.append(actions);
  }

  function draw(): void {
    endPointerDrag(false);
    drawHead();
    drawVariantBar();
    if (isComparing()) {
      intro.hidden = true;
      fields.hidden = true;
      compareHost.hidden = false;
      drawCompareView();
    } else {
      intro.hidden = false;
      fields.hidden = false;
      compareHost.hidden = true;
      drawFields();
    }
    drawPanel();
    drawNotices();
    drawFooter();
    const target = focusAfter?.();
    focusAfter = null;
    target?.focus();
  }

  // Talking to the backend.

  /** Sends the draft. On success the backend's copy becomes the draft, which is how an added chapter gets its id. */
  async function persist(): Promise<boolean> {
    const block = saveBlock(draft);
    if (block !== null) {
      notice = { kind: "error", text: block };
      return false;
    }
    try {
      saved = await saveOutline(course.id, toOutline(draft), activeVariantArg(selectedVariantId));
      draft = draftFromOutline(saved);
      options.onChange?.(saved);
      return true;
    } catch (error) {
      notice = { kind: "error", text: `The outline could not be saved: ${message(error)}` };
      return false;
    }
  }

  async function saveClicked(): Promise<void> {
    phase = { kind: "saving" };
    notice = null;
    draw();
    await persist();
    phase = { kind: "editing" };
    draw();
  }

  async function revise(): Promise<void> {
    phase = { kind: "revising" };
    notice = null;
    draw();
    if (dirty() && !(await persist())) {
      phase = { kind: "editing" };
      draw();
      return;
    }
    const claim = expectJobLabel(options.jobs, [course.id, course.storedId], "outline", "Outline revise");
    try {
      saved = await reviseOutline(course.id, activeVariantArg(selectedVariantId));
      draft = draftFromOutline(saved);
      notice = { kind: "info", text: "Revised. Your comments were applied and cleared." };
      options.onChange?.(saved);
    } catch (error) {
      claim.release(options.jobs?.jobs() ?? []);
      notice = { kind: "error", text: `The outline could not be revised: ${message(error)}` };
    }
    phase = { kind: "editing" };
    draw();
  }

  function openApproval(variantId?: string): void {
    const block = approvalBlock(draft);
    if (block !== null && (!variantId || variantId === selectedVariantId)) {
      notice = { kind: "error", text: block };
      drawNotices();
      return;
    }
    if (phase.kind === "comparing") {
      lastCompare = { left: phase.left, right: phase.right };
    }
    notice = null;
    phase = { kind: "choosing", choice: "on_open", variantId };
    draw();
  }

  async function approve(choice: ApprovalChoice, variantId?: string): Promise<void> {
    const targetVariantId = variantId ?? (phase.kind === "choosing" || phase.kind === "approving" ? phase.variantId : undefined) ?? selectedVariantId;
    phase = { kind: "approving", choice, variantId: targetVariantId };
    notice = null;
    draw();
    if (targetVariantId === selectedVariantId && dirty() && !(await persist())) {
      phase = { kind: "choosing", choice, variantId: targetVariantId };
      draw();
      return;
    }
    try {
      saved = await approveOutline(course.id, choice, activeVariantArg(targetVariantId));
      draft = draftFromOutline(saved);
      options.onChange?.(saved);
    } catch (error) {
      notice = { kind: "error", text: `The outline could not be approved: ${message(error)}` };
      phase = { kind: "choosing", choice, variantId: targetVariantId };
      draw();
      return;
    }
    if (choice === "on_open") {
      options.onDone();
      return;
    }
    phase = { kind: "approved" };
    draw();
  }

  /** Back to the chapters of an approved course, saving what changed first. */
  async function finish(): Promise<void> {
    if (dirty()) {
      phase = { kind: "saving" };
      notice = null;
      draw();
      const ok = await persist();
      phase = { kind: "editing" };
      draw();
      if (!ok) return;
    }
    options.onDone();
  }

  signal?.addEventListener("abort", () => {
    endPointerDrag(false);
    stopStatus?.stop();
    if (phase.kind === "editing" && dirty()) void persist();
  });

  draw();
}
