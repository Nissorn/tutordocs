import "./app.css";
import { decodeFilter, encodeFilter, type HomeFilter, type LibraryCourse } from "./courses";
import { listLibraryCourses, markCourseOpened } from "./courses-api";
import { el, message } from "./dom";
import { themeFigures } from "./figure-theme";
import { mountHome, type HomeHandle } from "./home";
import { mountJobDrawer } from "./job-drawer";
import { createJobFeed } from "./job-feed";
import { busyCourses } from "./jobs";
import { mountLibraryTools } from "./library-tools";
import { mountReader } from "./reader";
import { createSettings } from "./settings";
import { mountSettingsDialog } from "./settings-dialog";
import { browserStorage } from "./study-prefs";
import { mountTabBar, type TabBarView } from "./tabbar";
import { mountWizard } from "./wizard";
import {
  activate,
  closeTab,
  decodeTabs,
  dropMissing,
  emptyTabs,
  encodeTabs,
  openCourse,
  remember,
  tabFor,
  type TabsState,
} from "./tabs";

// The page: a tab bar across the top, the active tab's view below it, the job
// log drawer over the right edge, and the one sidebar footer parked in a hidden
// dock between readers. It keeps the master list of courses and the tab set,
// and shows exactly one reader at a time. The Home tab shows either the course
// cards or the New course wizard.

const STORAGE_KEY = "tutordocs.tabs.v1";
/** A scroll offset is saved this long after the last change. */
const SAVE_AFTER_MS = 300;

export interface AppOptions {
  /** Where the open tabs are remembered. Null for none. Defaults to the window's localStorage. */
  storage?: Pick<Storage, "getItem" | "setItem"> | null;
}

function titleOf(course: LibraryCourse): string {
  return course.title.trim() === "" ? course.id : course.title;
}

/**
 * Mounts the app into `root`. Resolves once the course list has loaded and the
 * tab that was open last time is showing. It never rejects.
 */
export async function mountApp(root: HTMLElement, options: AppOptions = {}): Promise<void> {
  const storage = options.storage === undefined ? browserStorage() : options.storage;

  const barHost = el("header", "app-bar");
  const stage = el("div", "stage");
  const dock = el("div", "footer-dock");
  dock.hidden = true;
  root.replaceChildren(barHost, stage, dock);

  const settings = createSettings(storage);
  settings.subscribe(() => themeFigures(document));
  const tools = mountLibraryTools(dock);
  const feed = createJobFeed();
  let courses: LibraryCourse[] = [];
  const stored = loadSession();
  let tabs: TabsState = stored.tabs;
  let filter: HomeFilter = stored.filter;
  let home: HomeHandle | null = null;
  let reader: AbortController | null = null;
  /** The New course wizard on the Home tab, or null while Home shows the cards. */
  let wizard: AbortController | null = null;
  /** What the stage shows: a course id, null for Home, undefined before the first show. */
  let showing: string | null | undefined;
  let token = 0;
  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  let running = new Set<string>();
  let seenFailedJobIds = new Set<string>();

  function persist(): void {
    if (saveTimer !== null) clearTimeout(saveTimer);
    saveTimer = null;
    try {
      const envelope = {
        ...(encodeTabs(tabs) as Record<string, unknown>),
        filter: encodeFilter(filter),
      };
      storage?.setItem(STORAGE_KEY, JSON.stringify(envelope));
    } catch {
      // The tabs are only a convenience, so a full or blocked store costs nothing but the memory.
    }
  }

  function persistSoon(): void {
    if (saveTimer === null) saveTimer = setTimeout(persist, SAVE_AFTER_MS);
  }

  function loadSession(): { tabs: TabsState; filter: HomeFilter } {
    try {
      const raw = storage?.getItem(STORAGE_KEY);
      if (!raw) return { tabs: emptyTabs, filter: { query: "", tag: null } };
      const parsed = JSON.parse(raw);
      const parsedFilter = decodeFilter(
        typeof parsed === "object" && parsed !== null ? (parsed as Record<string, unknown>).filter : undefined,
      );
      return { tabs: decodeTabs(parsed), filter: parsedFilter };
    } catch {
      return { tabs: emptyTabs, filter: { query: "", tag: null } };
    }
  }

  const courseOf = (id: string): LibraryCourse | undefined => courses.find((c) => c.id === id);
  const planningIds = new Set<string>();

  function decorateCourses(list: LibraryCourse[]): LibraryCourse[] {
    const runningJobs = feed.jobs().filter((j) => j.status === "running" && j.kind === "outline");
    const runningIds = new Set([
      ...runningJobs.map((j) => j.courseId),
      ...planningIds,
    ]);
    return list.map((c) => {
      const isPlanning = c.planning || runningIds.has(c.id) || (c.storedId !== "" && runningIds.has(c.storedId));
      return isPlanning ? { ...c, planning: true } : c;
    });
  }

  const bar = mountTabBar(barHost, {
    onHome: () => (wizard ? void show() : go(activate(tabs, null))),
    onSelect: (id) => go(activate(tabs, id)),
    onClose: (id) => go(closeTab(tabs, id)),
    onToggleJobs: () => {
      if (drawer.isOpen()) {
        drawer.close();
      } else {
        seenFailedJobIds = new Set(feed.jobs().filter((j) => j.status === "failed").map((j) => j.id));
        drawer.open();
      }
      drawBar();
    },
    onOpenSettings: toggleSettings,
  });

  const drawer = mountJobDrawer(root, feed, { courses: () => courses, onClose: () => drawBar() });
  const settingsDialog = mountSettingsDialog(root, settings, { onClose: () => drawBar() });

  function toggleSettings(): void {
    if (settingsDialog.isOpen()) settingsDialog.close();
    else settingsDialog.open();
    drawBar();
  }

  // Cmd+, opens Settings from anywhere in the window. The listener retires itself
  // once the page it belongs to has been replaced.
  function onShortcut(event: KeyboardEvent): void {
    if (!root.isConnected) {
      document.removeEventListener("keydown", onShortcut);
      return;
    }
    if (event.metaKey && !event.altKey && !event.ctrlKey && event.key === ",") {
      event.preventDefault();
      toggleSettings();
    }
  }
  document.addEventListener("keydown", onShortcut);

  function drawBar(): void {
    if (drawer.isOpen()) {
      for (const j of feed.jobs()) {
        if (j.status === "failed") seenFailedJobIds.add(j.id);
      }
    }
    const busy = busyCourses(feed.jobs(), courses);
    const failedJobs = feed.jobs().filter((j) => j.status === "failed");
    const hasFailed = !drawer.isOpen() && failedJobs.some((j) => !seenFailedJobIds.has(j.id));
    const view: TabBarView = {
      home: tabs.active === null,
      tabs: tabs.open.map((tab) => ({
        courseId: tab.courseId,
        title: courseOf(tab.courseId) ? titleOf(courseOf(tab.courseId)!) : tab.courseId,
        active: tab.courseId === tabs.active,
        busy: busy.has(tab.courseId),
      })),
      running: feed.jobs().filter((j) => j.status === "running").length,
      failed: hasFailed,
      drawerOpen: drawer.isOpen(),
      settingsOpen: settingsDialog.isOpen(),
    };
    bar.update(view);
  }

  /** Takes a new tab set, saves it, and shows whatever it now makes active. */
  function go(next: TabsState): void {
    if (next === tabs) return;
    tabs = next;
    persist();
    drawBar();
    if (tabs.active !== showing) void show();
  }

  function replaceCourse(next: LibraryCourse): void {
    courses = courses.some((c) => c.id === next.id) ? courses.map((c) => (c.id === next.id ? next : c)) : [...courses, next];
    home?.update(decorateCourses(courses));
    drawBar();
  }

  async function refreshCourses(): Promise<void> {
    try {
      courses = await listLibraryCourses();
    } catch (error) {
      console.warn(`could not refresh the course list: ${message(error)}`);
      return;
    }
    home?.update(decorateCourses(courses));
    const kept = dropMissing(tabs, courses.map((c) => c.id));
    if (kept !== tabs) go(kept);
    else drawBar();
  }

  function showError(text: string): void {
    const alert = el("p", "error", text);
    alert.setAttribute("role", "alert");
    const box = el("div", "stage-error");
    box.append(alert);
    stage.replaceChildren(box);
  }

  /** Replaces the Home cards with the wizard. A plan that finishes after the user left only refreshes the list. */
  function showWizard(): void {
    const run = new AbortController();
    wizard?.abort();
    wizard = run;
    home = null;
    const host = el("div", "wizard-stage");
    stage.replaceChildren(host);
    void mountWizard(host, {
      jobs: feed,
      onChange: replaceCourse,
      onPlanStart: (ref) => {
        planningIds.add(ref.id);
        if (ref.storedId) planningIds.add(ref.storedId);
        home?.update(decorateCourses(courses));
      },
      onCancel: () => void show(),
      onPlanned: (made) => {
        planningIds.delete(made.id);
        planningIds.delete(made.storedId);
        const here = wizard === run && !run.signal.aborted;
        void refreshCourses().then(() => {
          if (here) {
            go(openCourse(tabs, made.id));
          } else {
            const course = courses.find((c) => c.id === made.id || c.storedId === made.storedId) ?? {
              id: made.id,
              storedId: made.storedId,
              title: made.id,
              tags: [],
              objectives: [],
              chapterCount: 0,
              chapterTitles: [],
              writtenChapterCount: 0,
              createdAt: 0,
              openedAt: null,
              error: null,
              outlineStatus: "draft",
              planning: false,
            };
            if (tabs.active === null && home) {
              home.showCompletionNotice(course);
            }
          }
        });
      },
      signal: run.signal,
    });
  }

  async function show(): Promise<void> {
    const mine = ++token;
    const target = tabs.active;
    reader?.abort();
    reader = null;
    wizard?.abort();
    wizard = null;
    home = null;
    dock.append(tools.footer);
    showing = target;
    if (target === null) {
      home = mountHome(stage, decorateCourses(courses), {
        filter,
        onFilter: (next) => {
          filter = next;
          persistSoon();
        },
        onOpen: (id) => go(openCourse(tabs, id)),
        onNewCourse: showWizard,
        onChange: replaceCourse,
        onTrash: (id) => {
          tabs = dropMissing(tabs, courses.filter((c) => c.id !== id).map((c) => c.id));
          persist();
          drawBar();
          void refreshCourses();
        },
        onRestore: (_restored) => {
          void refreshCourses();
        },
      });
      void refreshCourses();
      return;
    }
    try {
      replaceCourse(await markCourseOpened(target));
    } catch (error) {
      if (mine === token) showError(`Could not open this course: ${message(error)}`);
      return;
    }
    if (mine !== token) return;
    const course = courseOf(target);
    const saved = tabFor(tabs, target);
    if (!course || !saved) return;
    const controller = new AbortController();
    reader = controller;
    const rejoin = feed
      .jobs()
      .some((j) => j.status === "running" && j.kind === "chapter" && (j.courseId === course.id || j.courseId === course.storedId));
    const mounting = mountReader(
      stage,
      { id: course.id, storedId: course.storedId, title: titleOf(course), tags: course.tags },
      {
        tools,
        jobs: feed,
        restore: { ...saved, rejoin },
        onPosition: (position) => {
          const next = remember(tabs, target, position);
          if (next === tabs) return;
          tabs = next;
          persistSoon();
        },
        onChanged: () => void refreshCourses(),
        signal: controller.signal,
        storage,
        settings,
      },
    );
    // A restore that rejoins a running chapter job resolves only when the job ends.
    if (!rejoin) await mounting;
  }

  feed.subscribe(() => {
    const runningJobs = feed.jobs().filter((j) => j.status === "running");
    const now = new Set(runningJobs.map((j) => j.id));
    const finishedJobIds = [...running].filter((id) => !now.has(id));
    running = now;
    drawBar();
    if (tabs.active === null && home) {
      home.update(decorateCourses(courses));
    }
    if (finishedJobIds.length > 0) {
      planningIds.clear();
      void refreshCourses().then(() => {
        if (tabs.active === null && home) {
          const allJobs = feed.jobs();
          for (const finishedId of finishedJobIds) {
            const job = allJobs.find((j) => j.id === finishedId);
            if (job && job.kind === "outline" && job.status === "succeeded") {
              const course = courses.find((c) => c.id === job.courseId || c.storedId === job.courseId);
              if (course) home.showCompletionNotice(course);
            }
          }
        }
      });
    }
  });
  feed.start().then(drawBar, (error: unknown) => console.warn(`job log unavailable: ${message(error)}`));

  try {
    courses = await listLibraryCourses();
  } catch (error) {
    drawBar();
    showError(`Could not open the library: ${message(error)}`);
    return;
  }
  tabs = dropMissing(tabs, courses.map((c) => c.id));
  drawBar();
  await show();
}
