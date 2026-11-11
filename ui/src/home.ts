import "./home.css";
import {
  allTags,
  describeOpened,
  filterCourses,
  homeOrder,
  parseTagInput,
  type HomeFilter,
  type LibraryCourse,
  type TrashEntry,
} from "./courses";
import { emptyTrash, listTrash, renameCourse, restoreCourse, setCourseTags, trashCourse } from "./courses-api";
import { el, message } from "./dom";
import { formatBytes, formatRelativeTime } from "./format";

// The Home tab: every course on the shelf as a card, with a search box,
// a tag filter, lifecycle states, course renaming, and a trash view.

export interface HomeOptions {
  /** The search and tag the user had last time, so leaving Home and coming back keeps them. */
  filter: HomeFilter;
  onFilter(filter: HomeFilter): void;
  onOpen(courseId: string): void;
  /** The user asked to make a course. The page shows the wizard. */
  onNewCourse(): void;
  /** A course's title or tags changed. The page keeps the master list. */
  onChange(course: LibraryCourse): void;
  /** A course was trashed. */
  onTrash?(courseId: string): void;
  /** A course was restored from trash. */
  onRestore?(course: LibraryCourse): void;
  /** Unix seconds. */
  now?: () => number;
}

export interface HomeHandle {
  /** Shows a new list. Keeps the search text and an edit in progress. */
  update(courses: LibraryCourse[]): void;
  /** Displays a notice that an outline job completed. */
  showCompletionNotice(course: LibraryCourse): void;
}

const sameTag = (a: string | null, b: string | null): boolean =>
  a !== null && b !== null && a.toLocaleLowerCase() === b.toLocaleLowerCase();

function titleOf(course: LibraryCourse): string {
  return course.title.trim() === "" ? course.id : course.title;
}

function progressText(course: LibraryCourse): string {
  if (course.chapterCount === 0) return "No chapters yet";
  return `${course.writtenChapterCount} of ${course.chapterCount} chapter${course.chapterCount === 1 ? "" : "s"} written`;
}

function alertNote(text: string): HTMLElement {
  const note = el("p", "card-error", text);
  note.setAttribute("role", "alert");
  return note;
}

export function mountHome(root: HTMLElement, initial: LibraryCourse[], options: HomeOptions): HomeHandle {
  let courses = initial;
  let filter = options.filter;
  let editing: { id: string; text: string } | null = null;
  let renaming: { id: string; title: string } | null = null;
  let openMenuId: string | null = null;
  let activeConfirm: { id: string; kind: "trash" | "discard" } | null = null;
  const tagErrors = new Map<string, string>();
  const actionErrors = new Map<string, string>();

  let view: "courses" | "trash" = "courses";
  let trashEntries: TrashEntry[] = [];
  let trashConfirm: { trashId: string } | null = null;
  let trashError: string | null = null;

  let completionNotice: LibraryCourse | null = null;

  const now = (): number => options.now?.() ?? Math.floor(Date.now() / 1000);

  const section = el("section", "home");
  section.setAttribute("aria-label", "Courses");
  const welcome = el("div", "home-welcome");
  const header = el("header", "home-head");
  const search = el("input", "home-search");
  search.type = "search";
  search.placeholder = "Search courses, tags and chapters";
  search.setAttribute("aria-label", "Search courses");
  search.spellcheck = false;
  search.setAttribute("autocorrect", "off");
  search.setAttribute("autocapitalize", "off");
  search.value = filter.query;
  const newButton = el("button", "quiet-button home-new", "New course");
  newButton.type = "button";
  newButton.addEventListener("click", () => options.onNewCourse());
  const trashButton = el("button", "quiet-button home-trash", "Trash");
  trashButton.type = "button";
  trashButton.addEventListener("click", () => {
    view = "trash";
    render();
    void loadTrash();
  });
  header.append(el("h1", undefined, "Courses"), search, newButton, trashButton);

  const tagBar = el("div", "tag-bar");
  tagBar.setAttribute("role", "group");
  tagBar.setAttribute("aria-label", "Filter by tag");
  const count = el("p", "home-count");
  count.setAttribute("role", "status");
  const none = el("p", "home-empty");
  const grid = el("ul", "cards");
  root.replaceChildren(section);

  function setFilter(next: HomeFilter): void {
    filter = next;
    options.onFilter(filter);
    renderBody();
  }

  function toggleTag(tag: string): void {
    setFilter({ ...filter, tag: sameTag(filter.tag, tag) ? null : tag });
  }

  function chip(tag: string): HTMLButtonElement {
    const button = el("button", "tag-chip", tag);
    button.type = "button";
    button.setAttribute("aria-pressed", String(sameTag(filter.tag, tag)));
    button.addEventListener("click", () => toggleTag(tag));
    return button;
  }

  function replace(next: LibraryCourse): void {
    courses = courses.some((c) => c.id === next.id) ? courses.map((c) => (c.id === next.id ? next : c)) : [...courses, next];
  }

  async function saveTags(course: LibraryCourse): Promise<void> {
    if (!editing || editing.id !== course.id) return;
    try {
      const saved = await setCourseTags(course.id, parseTagInput(editing.text));
      editing = null;
      tagErrors.delete(course.id);
      replace(saved);
      options.onChange(saved);
    } catch (error) {
      tagErrors.set(course.id, `Could not save the tags: ${message(error)}`);
    }
    render();
  }

  async function saveRename(course: LibraryCourse): Promise<void> {
    if (!renaming || renaming.id !== course.id) return;
    const newTitle = renaming.title.trim();
    if (newTitle === "" || newTitle === course.title) {
      renaming = null;
      render();
      return;
    }
    try {
      const saved = await renameCourse(course.id, newTitle);
      renaming = null;
      actionErrors.delete(course.id);
      replace(saved);
      options.onChange(saved);
      render();
    } catch (error) {
      actionErrors.set(course.id, `Could not rename course: ${message(error)}`);
      renaming = null;
      render();
    }
  }

  async function loadTrash(): Promise<void> {
    try {
      trashEntries = await listTrash();
      trashError = null;
    } catch (error) {
      trashError = `Could not load trash: ${message(error)}`;
    }
    render();
  }

  function tagEditor(course: LibraryCourse): HTMLElement {
    const input = el("input", "tag-input");
    input.type = "text";
    input.value = editing?.text ?? "";
    input.placeholder = "Tags, separated by commas. Enter saves.";
    input.setAttribute("aria-label", `Tags for ${titleOf(course)}`);
    input.addEventListener("input", () => {
      if (editing) editing.text = input.value;
    });
    input.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        void saveTags(course);
      } else if (event.key === "Escape") {
        event.stopPropagation();
        editing = null;
        tagErrors.delete(course.id);
        render();
      }
    });
    return input;
  }

  function card(course: LibraryCourse): HTMLElement {
    const article = el("article", "card");
    article.dataset.courseId = course.id;

    const headRow = el("div", "card-head-row");
    const open = el("button", "card-open", titleOf(course));
    open.type = "button";
    open.addEventListener("click", () => options.onOpen(course.id));
    const heading = el("h2", "card-title");
    heading.append(open);
    headRow.append(heading);

    const menuBtn = el("button", "quiet-button card-menu-button", "⋯");
    menuBtn.type = "button";
    menuBtn.setAttribute("aria-label", `Actions for ${titleOf(course)}`);
    menuBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      openMenuId = openMenuId === course.id ? null : course.id;
      render();
    });
    headRow.append(menuBtn);
    article.append(headRow);

    if (openMenuId === course.id) {
      const menu = el("div", "card-menu");
      const renameBtn = el("button", "quiet-button card-menu-item card-action-rename", "Rename");
      renameBtn.type = "button";
      renameBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        openMenuId = null;
        renaming = { id: course.id, title: course.title };
        render();
        article.querySelector<HTMLInputElement>(".card-rename-input")?.focus();
      });
      const trashBtn = el("button", "quiet-button card-menu-item card-action-trash", "Move to trash");
      trashBtn.type = "button";
      trashBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        openMenuId = null;
        activeConfirm = { id: course.id, kind: "trash" };
        render();
      });
      menu.append(renameBtn, trashBtn);
      article.append(menu);
    }

    if (renaming?.id === course.id) {
      const renameRow = el("div", "card-rename");
      const renameInput = el("input", "card-rename-input");
      renameInput.type = "text";
      renameInput.value = renaming.title;
      renameInput.setAttribute("aria-label", `New title for ${titleOf(course)}`);
      renameInput.addEventListener("input", () => {
        if (renaming) renaming.title = renameInput.value;
      });
      const saveBtn = el("button", "quiet-button card-rename-save", "Save");
      saveBtn.type = "button";
      saveBtn.addEventListener("click", async (e) => {
        e.stopPropagation();
        await saveRename(course);
      });
      const cancelBtn = el("button", "quiet-button card-rename-cancel", "Cancel");
      cancelBtn.type = "button";
      cancelBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        renaming = null;
        render();
      });
      renameInput.addEventListener("keydown", async (e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          await saveRename(course);
        } else if (e.key === "Escape") {
          e.stopPropagation();
          renaming = null;
          render();
        }
      });
      renameRow.append(renameInput, saveBtn, cancelBtn);
      article.append(renameRow);
    }

    const tagRow = el("div", "card-tags");
    if (editing?.id === course.id) {
      tagRow.append(tagEditor(course));
    } else {
      const list = el("ul", "card-tag-list");
      for (const tag of course.tags) {
        const item = el("li");
        item.append(chip(tag));
        list.append(item);
      }
      const edit = el("button", "card-edit-tags", course.tags.length === 0 ? "Add tags" : "Edit tags");
      edit.type = "button";
      edit.setAttribute("aria-label", `${edit.textContent} for ${titleOf(course)}`);
      edit.addEventListener("click", () => {
        editing = { id: course.id, text: course.tags.join(", ") };
        render();
        grid.querySelector<HTMLInputElement>(`[data-course-id="${CSS.escape(course.id)}"] .tag-input`)?.focus();
      });
      tagRow.append(list, edit);
    }
    article.append(tagRow);

    const status = el("span", "card-status");
    if (course.planning) {
      status.classList.add("card-status-planning");
      status.textContent = "Planning";
    } else if (course.chapterCount === 0) {
      status.classList.add("card-status-setup");
      status.textContent = "Setup not finished";
    } else if (course.outlineStatus === "draft") {
      status.classList.add("card-status-draft");
      status.textContent = "Draft";
    } else {
      status.classList.add("card-status-approved");
      status.textContent = "Approved";
    }

    const meta = el("div", "card-meta");
    const bar = el("div", "card-bar");
    bar.setAttribute("aria-hidden", "true");
    const fill = el("span");
    fill.style.width = `${course.chapterCount === 0 ? 0 : Math.round((course.writtenChapterCount / course.chapterCount) * 100)}%`;
    bar.append(fill);
    meta.append(
      bar,
      status,
      el("p", "card-progress", progressText(course)),
      el("p", "card-opened", describeOpened(course.openedAt, now())),
    );
    article.append(meta);

    if (course.chapterCount === 0) {
      const setupActions = el("div", "card-setup-actions");
      const cont = el("button", "quiet-button card-continue", "Continue");
      cont.type = "button";
      cont.addEventListener("click", (e) => {
        e.stopPropagation();
        options.onOpen(course.id);
      });
      const discard = el("button", "quiet-button card-discard", "Discard");
      discard.type = "button";
      if (course.planning) {
        discard.disabled = true;
        discard.title = "Planning. Discard when it finishes.";
        const reason = el("span", "card-discard-reason", "Planning. Discard when it finishes.");
        setupActions.append(cont, discard, reason);
      } else {
        discard.addEventListener("click", (e) => {
          e.stopPropagation();
          activeConfirm = { id: course.id, kind: "discard" };
          render();
        });
        setupActions.append(cont, discard);
      }
      article.append(setupActions);
    }

    if (activeConfirm?.id === course.id) {
      const kind = activeConfirm.kind;
      const confirmBox = el("div", "confirm-prompt");
      confirmBox.setAttribute("role", "alertdialog");
      const promptText = el(
        "span",
        undefined,
        kind === "trash" ? `Move ${titleOf(course)} to trash?` : `Discard setup for ${titleOf(course)}?`,
      );
      const confirmBtn = el("button", `danger-button confirm-${kind}`, kind === "trash" ? "Move to trash" : "Discard");
      confirmBtn.type = "button";
      confirmBtn.addEventListener("click", async (e) => {
        e.stopPropagation();
        try {
          await trashCourse(course.id);
          courses = courses.filter((c) => c.id !== course.id);
          activeConfirm = null;
          actionErrors.delete(course.id);
          render();
          options.onTrash?.(course.id);
        } catch (error) {
          actionErrors.set(course.id, `Could not ${kind} course: ${message(error)}`);
          activeConfirm = null;
          render();
        }
      });
      const cancelBtn = el("button", `quiet-button cancel-${kind}`, "Cancel");
      cancelBtn.type = "button";
      cancelBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        activeConfirm = null;
        render();
      });
      confirmBox.append(promptText, confirmBtn, cancelBtn);
      article.append(confirmBox);
    }

    if (course.error) article.append(alertNote(course.error));
    const tagError = tagErrors.get(course.id);
    if (tagError) article.append(alertNote(tagError));
    const actionError = actionErrors.get(course.id);
    if (actionError) article.append(alertNote(actionError));

    article.addEventListener("click", (event) => {
      if (event.target instanceof Element && !event.target.closest("button, input, a, .confirm-prompt, .card-menu")) {
        options.onOpen(course.id);
      }
    });
    return article;
  }

  function renderTrashView(): HTMLElement {
    const trashSection = el("div", "trash-view");
    const head = el("header", "trash-head");
    const backBtn = el("button", "quiet-button home-back-courses", "Back to courses");
    backBtn.type = "button";
    backBtn.addEventListener("click", () => {
      view = "courses";
      render();
    });
    head.append(el("h1", undefined, "Trash"), backBtn);
    trashSection.append(head);

    if (trashError) {
      trashSection.append(alertNote(trashError));
    }

    if (trashEntries.length === 0) {
      const emptyNote = el("p", "home-empty", "Trash is empty.");
      trashSection.append(emptyNote);
      return trashSection;
    }

    const list = el("ul", "trash-list");
    for (const entry of trashEntries) {
      const item = el("li", "trash-item");
      item.dataset.trashId = entry.trashId;

      const info = el("div", "trash-info");
      const title = el("h2", "trash-title", entry.title.trim() === "" ? entry.courseId : entry.title);
      const timeSpan = el("span", "trash-time", `Trashed ${formatRelativeTime(entry.trashedAt, now() * 1000)}`);
      timeSpan.title = entry.trashedAt;
      const meta = el("p", "trash-meta");
      meta.append(
        `${entry.chapterCount} chapters (${entry.writtenChapterCount} written) · ${formatBytes(entry.bytes)} · `,
        timeSpan,
      );
      info.append(title, meta);
      item.append(info);

      const actions = el("div", "trash-actions");
      const restoreBtn = el("button", "quiet-button trash-restore", "Restore");
      restoreBtn.type = "button";
      restoreBtn.addEventListener("click", async () => {
        try {
          const restored = await restoreCourse(entry.trashId);
          trashEntries = trashEntries.filter((e) => e.trashId !== entry.trashId);
          trashConfirm = null;
          replace(restored);
          options.onRestore?.(restored);
          render();
        } catch (error) {
          trashError = `Could not restore course: ${message(error)}`;
          render();
        }
      });

      const emptyBtn = el("button", "quiet-button trash-empty", "Empty");
      emptyBtn.type = "button";
      emptyBtn.addEventListener("click", () => {
        trashConfirm = { trashId: entry.trashId };
        render();
      });
      actions.append(restoreBtn, emptyBtn);
      item.append(actions);

      if (trashConfirm?.trashId === entry.trashId) {
        const confirmBox = el("div", "confirm-prompt");
        confirmBox.setAttribute("role", "alertdialog");
        const promptText = el("span", undefined, `Permanently delete ${entry.title}?`);
        const confirmBtn = el("button", "danger-button confirm-empty", "Empty permanently");
        confirmBtn.type = "button";
        confirmBtn.addEventListener("click", async () => {
          try {
            await emptyTrash(entry.trashId);
            trashEntries = trashEntries.filter((e) => e.trashId !== entry.trashId);
            trashConfirm = null;
            render();
          } catch (error) {
            trashError = `Could not empty course: ${message(error)}`;
            render();
          }
        });
        const cancelBtn = el("button", "quiet-button cancel-empty", "Cancel");
        cancelBtn.type = "button";
        cancelBtn.addEventListener("click", () => {
          trashConfirm = null;
          render();
        });
        confirmBox.append(promptText, confirmBtn, cancelBtn);
        item.append(confirmBox);
      }

      list.append(item);
    }
    trashSection.append(list);
    return trashSection;
  }

  function renderBody(): void {
    const tagsInUse = allTags(courses);
    tagBar.hidden = tagsInUse.length === 0;
    tagBar.replaceChildren(...tagsInUse.map(chip));
    const shown = homeOrder(filterCourses(courses, filter));
    count.hidden = courses.length === 0;
    count.textContent =
      shown.length === courses.length
        ? `${courses.length} course${courses.length === 1 ? "" : "s"}`
        : `${shown.length} of ${courses.length} courses`;
    none.hidden = shown.length > 0 || courses.length === 0;
    none.textContent = "No course matches the search or tag. Clear them to see every course.";
    grid.replaceChildren(
      ...shown.map((course) => {
        const item = el("li");
        item.append(card(course));
        return item;
      }),
    );
  }

  function render(): void {
    section.replaceChildren();

    if (completionNotice !== null) {
      const notice = el("div", "home-notice");
      notice.setAttribute("role", "status");
      notice.append(el("span", undefined, `Outline ready for ${titleOf(completionNotice)}.`));
      const openBtn = el("button", "outline-submit notice-open", "Open course");
      openBtn.type = "button";
      const cid = completionNotice.id;
      openBtn.addEventListener("click", () => {
        completionNotice = null;
        render();
        options.onOpen(cid);
      });
      const dismissBtn = el("button", "quiet-button notice-dismiss", "Dismiss");
      dismissBtn.type = "button";
      dismissBtn.addEventListener("click", () => {
        completionNotice = null;
        render();
      });
      notice.append(openBtn, dismissBtn);
      section.append(notice);
    }

    if (view === "trash") {
      section.append(renderTrashView());
      return;
    }

    const empty = courses.length === 0;
    search.hidden = empty;
    welcome.hidden = !empty;
    welcome.textContent = empty
      ? "No courses yet. Start your first course: set a goal, add your own materials, and the agent plans it. You review the outline before anything is written."
      : "";
    section.append(header, welcome, tagBar, count, none, grid);
    renderBody();
  }

  search.addEventListener("input", () => setFilter({ ...filter, query: search.value }));

  render();

  return {
    update(next) {
      const wasEmpty = courses.length === 0;
      courses = next;
      if (wasEmpty !== (courses.length === 0)) render();
      else renderBody();
    },
    showCompletionNotice(course) {
      completionNotice = course;
      render();
    },
  };
}
