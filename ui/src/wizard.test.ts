// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { emit } from "@tauri-apps/api/event";
import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type { LibraryCourse } from "./courses";
import readerContract from "./fixtures/reader-contract.json";
import sourcesContract from "./fixtures/sources-contract.json";
import contract from "./fixtures/wizard-contract.json";
import { mountJobDrawer } from "./job-drawer";
import type { JobFeed } from "./job-feed";
import { resetJobLabels } from "./job-label";
import type { JobRecord } from "./jobs";
import { mountWizard, type WizardOptions } from "./wizard";

type Args = Record<string, unknown>;
type Handler = (args: Args) => unknown;

const made: LibraryCourse = {
  id: "genetics",
  storedId: "gen-1",
  title: "Genetics",
  tags: ["biology"],
  objectives: [],
  chapterCount: 0,
  chapterTitles: [],
  writtenChapterCount: 0,
  createdAt: 1,
  openedAt: null,
  error: null,
  outlineStatus: "approved",
  planning: false,
};

let root: HTMLElement;
let sent: Array<{ cmd: string; args: Args }>;
let handlers: Record<string, Handler>;
let options: WizardOptions;
let controller: AbortController;

function backend(over: Record<string, Handler> = {}): void {
  handlers = {
    list_course_templates: () => contract.list_course_templates,
    create_course: () => made,
    rename_course: (a) => ({ ...made, title: a.title as string }),
    set_course_tags: () => made,
    mark_course_opened: () => made,
    set_course_setup: (a) => a.setup,
    import_files: () => contract.import_files,
    ingest_url: () => sourcesContract.ingest_url,
    generate_outline: () => null,
    get_course_setup: () => null,
    list_course_sources: () => [],
    ...over,
  };
}

async function mount(over: Partial<WizardOptions> = {}): Promise<void> {
  controller = new AbortController();
  options = {
    jobs: null,
    onChange: vi.fn(),
    onPlanned: vi.fn(),
    onCancel: vi.fn(),
    signal: controller.signal,
    ...over,
  };
  await mountWizard(root, options);
}

beforeEach(() => {
  document.body.innerHTML = '<div id="app"></div>';
  root = document.querySelector<HTMLElement>("#app")!;
  sent = [];
  backend();
  mockWindows("main");
  mockIPC(
    (cmd, raw) => {
      const args = (raw ?? {}) as Args;
      sent.push({ cmd, args });
      if (cmd === "plugin:dialog|open") return handlers.dialog?.(args) ?? null;
      const handler = handlers[cmd];
      if (!handler) throw new Error(`unexpected command ${cmd}`);
      return handler(args);
    },
    { shouldMockEvents: true },
  );
});

afterEach(() => {
  controller?.abort();
  resetJobLabels();
  clearMocks();
});

const commands = (): string[] => sent.map((s) => s.cmd);
const button = (name: string): HTMLButtonElement => {
  const found = Array.from(root.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent === name);
  if (!found) throw new Error(`no button "${name}"`);
  return found;
};
const hasButton = (name: string): boolean => Array.from(root.querySelectorAll("button")).some((b) => b.textContent === name);
const field = <T extends HTMLElement>(label: string): T => root.querySelector<T>(`[aria-label="${label}"]`)!;
const radio = (name: string, value: string): HTMLInputElement =>
  root.querySelector<HTMLInputElement>(`input[type="radio"][name="${name}"][value="${value}"]`)!;
const currentStep = (): string => root.querySelector('.wizard-steps [aria-current="step"]')!.textContent!;
const alertText = (): string => root.querySelector('[role="alert"]')?.textContent ?? "";

/** Lets pending promise callbacks run, for a test that expects something not to happen. */
const settle = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 20));

/** A backend answer the test releases by hand, to look at the screen mid-job. */
function gate(): { promise: Promise<unknown>; open: () => void } {
  let release!: (value: unknown) => void;
  const promise = new Promise<unknown>((resolve) => (release = resolve));
  return { promise, open: () => release(null) };
}

function liveFeed(): JobFeed & { add(job: JobRecord): void } {
  let jobs: JobRecord[] = [];
  const listeners = new Set<() => void>();
  return {
    jobs: () => jobs,
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    async start() {},
    stop() {},
    async cancel() {},
    add(job) {
      jobs = [...jobs, job];
      for (const listener of [...listeners]) listener();
    },
  };
}

function type(input: HTMLInputElement | HTMLTextAreaElement, text: string): void {
  input.value = text;
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

async function toMaterials(title = "Genetics"): Promise<void> {
  type(field<HTMLInputElement>("Course title"), title);
  button("Next").click();
  await vi.waitFor(() => expect(currentStep()).toContain("Materials"));
}

async function toGuide(): Promise<void> {
  await toMaterials();
  button("Next").click();
  await vi.waitFor(() => expect(currentStep()).toContain("Guide"));
}

describe("the goal step", () => {
  it("opens on the goal with the four steps listed and the quick overview chosen", async () => {
    await mount();
    expect(Array.from(root.querySelectorAll(".wizard-steps li"), (li) => li.textContent)).toEqual([
      "1 Goal",
      "2 Materials",
      "3 Guide",
      "4 Plan",
    ]);
    expect(currentStep()).toContain("Goal");
    expect(Array.from(root.querySelectorAll(".template-card"), (c) => c.querySelector(".template-title")!.textContent)).toEqual([
      "Interview prep",
      "Exam review",
      "Quick overview",
      "Custom",
    ]);
    expect(radio("template", "overview").checked).toBe(true);
    expect(sent.map((s) => s.cmd)).toEqual(["list_course_templates"]);
  });

  it("asks for a title instead of creating a course without one", async () => {
    await mount();
    button("Next").click();
    expect(alertText()).toBe("Name the course first.");
    expect(currentStep()).toContain("Goal");
    expect(commands()).not.toContain("create_course");
  });

  it("refuses hours that are not a number", async () => {
    await mount();
    type(field("Course title"), "Genetics");
    type(field("Hours per week"), "lots");
    button("Next").click();
    expect(alertText()).toMatch(/hours per week/i);
    expect(commands()).not.toContain("create_course");
  });

  it("creates the course from the title and tags and moves on to the materials", async () => {
    await mount();
    type(field("Course title"), "  Genetics ");
    type(field("Tags"), "biology, year2");
    button("Next").click();
    await vi.waitFor(() => expect(currentStep()).toContain("Materials"));
    expect(sent.find((s) => s.cmd === "create_course")!.args).toEqual({ title: "Genetics", tags: ["biology", "year2"] });
    expect(options.onChange).toHaveBeenCalledWith(made);
  });

  it("shows why the course could not be created and stays on the goal", async () => {
    backend({
      create_course: () => {
        throw new Error("disk is full");
      },
    });
    await mount();
    type(field("Course title"), "Genetics");
    button("Next").click();
    await vi.waitFor(() => expect(alertText()).toContain("disk is full"));
    expect(currentStep()).toContain("Goal");
    expect(button("Next").disabled).toBe(false);
  });

  it("creates the course once however often the user goes back and forward", async () => {
    await mount();
    await toMaterials();
    button("Back").click();
    expect(currentStep()).toContain("Goal");
    expect(field<HTMLInputElement>("Course title").value).toBe("Genetics");
    expect(field<HTMLInputElement>("Course title").disabled).toBe(false);
    button("Next").click();
    await vi.waitFor(() => expect(currentStep()).toContain("Materials"));
    expect(commands().filter((c) => c === "create_course")).toHaveLength(1);
  });

  it("renames the course through rename_course when title is edited after creation", async () => {
    await mount();
    await toMaterials();
    button("Back").click();
    expect(field<HTMLInputElement>("Course title").disabled).toBe(false);
    type(field("Course title"), "Epigenetics");
    button("Next").click();
    await vi.waitFor(() => expect(commands()).toContain("rename_course"));
    expect(sent.find((s) => s.cmd === "rename_course")!.args).toEqual({
      courseId: "genetics",
      title: "Epigenetics",
    });
  });

  it("saves changed tags on the course it already made", async () => {
    await mount();
    await toMaterials();
    button("Back").click();
    type(field("Tags"), "biology, exam");
    button("Next").click();
    await vi.waitFor(() => expect(commands()).toContain("set_course_tags"));
    expect(sent.find((s) => s.cmd === "set_course_tags")!.args).toEqual({ courseId: "genetics", tags: ["biology", "exam"] });
  });

  it("leaves the wizard from Cancel on the first step", async () => {
    await mount();
    button("Cancel").click();
    expect(options.onCancel).toHaveBeenCalled();
  });

  it("shows why the templates could not load and tries again", async () => {
    let calls = 0;
    backend({
      list_course_templates: () => {
        if (++calls === 1) throw new Error("backend not ready");
        return contract.list_course_templates;
      },
    });
    await mount();
    expect(alertText()).toContain("backend not ready");
    button("Try again").click();
    await vi.waitFor(() => expect(root.querySelectorAll(".template-card")).toHaveLength(4));
  });
});

describe("the materials step", () => {
  it("opens the file picker for files and imports what the user chose", async () => {
    backend({ dialog: () => ["/Users/me/notes/membranes.pdf", "/Users/me/notes/summary.md"] });
    await mount();
    await toMaterials();
    button("Choose files").click();
    await vi.waitFor(() => expect(commands()).toContain("import_files"));
    expect(sent.find((s) => s.cmd === "plugin:dialog|open")!.args).toEqual({ options: { multiple: true, directory: false } });
    expect(sent.find((s) => s.cmd === "import_files")!.args).toEqual({
      courseId: "genetics",
      paths: ["/Users/me/notes/membranes.pdf", "/Users/me/notes/summary.md"],
    });
  });

  it("opens the picker for a folder", async () => {
    backend({ dialog: () => ["/Users/me/notes"] });
    await mount();
    await toMaterials();
    button("Choose a folder").click();
    await vi.waitFor(() => expect(commands()).toContain("import_files"));
    expect(sent.find((s) => s.cmd === "plugin:dialog|open")!.args).toEqual({ options: { multiple: true, directory: true } });
  });

  it("imports nothing when the picker is cancelled", async () => {
    backend({ dialog: () => null });
    await mount();
    await toMaterials();
    button("Choose files").click();
    await vi.waitFor(() => expect(commands()).toContain("plugin:dialog|open"));
    await settle();
    expect(commands()).not.toContain("import_files");
  });

  it("lists each file with its status, and the reason for each one skipped", async () => {
    backend({ dialog: () => ["/Users/me/notes"] });
    await mount();
    await toMaterials();
    button("Choose a folder").click();
    await vi.waitFor(() => expect(root.querySelectorAll("li.material")).toHaveLength(4));
    const rows = Array.from(root.querySelectorAll<HTMLElement>("li.material"), (li) => ({
      name: li.querySelector(".material-name")!.textContent,
      state: li.dataset.state,
      status: li.querySelector(".material-status")!.textContent,
      reason: li.querySelector(".material-reason")?.textContent ?? null,
    }));
    expect(rows).toEqual([
      { name: "membranes.pdf", state: "imported", status: "Imported", reason: null },
      { name: "summary.md", state: "imported", status: "Imported", reason: null },
      { name: "summary-copy.md", state: "skipped", status: "Skipped", reason: "Already in this course, same contents." },
      { name: "lecture.mp4", state: "skipped", status: "Skipped", reason: "unsupported file type .mp4" },
    ]);
    expect(root.querySelector(".materials-summary")!.textContent).toBe("2 imported, 2 skipped");
  });

  it("shows a row as importing until the backend answers", async () => {
    let finish!: (value: unknown) => void;
    backend({
      dialog: () => ["/Users/me/notes/a.pdf"],
      import_files: () => new Promise((resolve) => (finish = resolve)),
    });
    await mount();
    await toMaterials();
    button("Choose files").click();
    await vi.waitFor(() => expect(root.querySelector<HTMLElement>("li.material")?.dataset.state).toBe("importing"));
    expect(button("Next").disabled).toBe(true);
    finish([contract.import_files[0]]);
    await vi.waitFor(() => expect(root.querySelector<HTMLElement>("li.material")?.dataset.state).toBe("imported"));
    expect(button("Next").disabled).toBe(false);
  });

  it("shows the backend's reason when the whole import fails", async () => {
    backend({
      dialog: () => ["/Users/me/a.pdf"],
      import_files: () => {
        throw new Error("the library is read-only");
      },
    });
    await mount();
    await toMaterials();
    button("Choose files").click();
    await vi.waitFor(() => expect(root.querySelector<HTMLElement>("li.material")?.dataset.state).toBe("failed"));
    expect(root.querySelector(".material-reason")!.textContent).toContain("the library is read-only");
  });

  it("imports what is dropped on the window while this step shows", async () => {
    await mount();
    await toMaterials();
    await emit("tauri://drag-drop", { paths: ["/Users/me/notes/membranes.pdf"], position: { x: 10, y: 10 } });
    await vi.waitFor(() => expect(commands()).toContain("import_files"));
    expect(sent.find((s) => s.cmd === "import_files")!.args.paths).toEqual(["/Users/me/notes/membranes.pdf"]);
  });

  it("marks the step while files are dragged over it and clears the mark on leave", async () => {
    await mount();
    await toMaterials();
    const step = root.querySelector<HTMLElement>(".wizard-body")!;
    await emit("tauri://drag-enter", { paths: ["/a.pdf"], position: { x: 1, y: 1 } });
    await vi.waitFor(() => expect(step.dataset.dragging).toBe("true"));
    await emit("tauri://drag-leave", null);
    await vi.waitFor(() => expect(step.dataset.dragging).toBeUndefined());
  });

  it("ignores a drop while another step shows, even with the course made", async () => {
    await mount();
    await toGuide();
    await emit("tauri://drag-drop", { paths: ["/Users/me/a.pdf"], position: { x: 1, y: 1 } });
    await settle();
    expect(commands()).not.toContain("import_files");
  });

  it("fetches a pasted URL as the course's current source", async () => {
    await mount();
    await toMaterials();
    type(field("Web page URL"), " https://example.com/a ");
    button("Add page").click();
    await vi.waitFor(() => expect(root.querySelector<HTMLElement>("li.material")?.dataset.state).toBe("imported"));
    const order = commands().filter((c) => c === "mark_course_opened" || c === "ingest_url");
    expect(order).toEqual(["mark_course_opened", "ingest_url"]);
    expect(sent.find((s) => s.cmd === "ingest_url")!.args).toEqual({ url: "https://example.com/a" });
    expect(root.querySelector(".material-name")!.textContent).toBe("https://example.com/a");
    expect(field<HTMLInputElement>("Web page URL").value).toBe("");
  });

  it("shows why a page could not be fetched", async () => {
    backend({
      ingest_url: () => {
        throw new Error("the page needs a login");
      },
    });
    await mount();
    await toMaterials();
    type(field("Web page URL"), "https://example.com/private");
    button("Add page").click();
    await vi.waitFor(() => expect(root.querySelector<HTMLElement>("li.material")?.dataset.state).toBe("failed"));
    expect(root.querySelector(".material-reason")!.textContent).toContain("the page needs a login");
  });

  it("lets the user go on with no files at all", async () => {
    await mount();
    await toMaterials();
    expect(root.querySelector(".materials-empty")!.textContent).toContain("without");
    button("Next").click();
    await vi.waitFor(() => expect(currentStep()).toContain("Guide"));
  });
});

describe("the guide step", () => {
  it("shows the chosen template's description beside the free text box", async () => {
    await mount();
    radio("template", "exam").click();
    await toGuide();
    const note = root.querySelector(".template-note")!;
    expect(note.querySelector(".template-title")!.textContent).toBe("Exam review");
    expect(note.textContent).toContain("Each topic condensed into what to remember");
    expect(field("Guide").tagName).toBe("TEXTAREA");
  });

  it("keeps what the user typed when they go back and forward", async () => {
    backend({ dialog: () => ["/Users/me/notes"] });
    await mount();
    type(field("Hours per week"), "6");
    await toMaterials();
    button("Choose a folder").click();
    await vi.waitFor(() => expect(root.querySelectorAll("li.material")).toHaveLength(4));
    button("Next").click();
    await vi.waitFor(() => expect(currentStep()).toContain("Guide"));
    type(field<HTMLTextAreaElement>("Guide"), "Skip plant biology.");
    button("Back").click();
    expect(root.querySelectorAll("li.material")).toHaveLength(4);
    button("Back").click();
    expect(field<HTMLInputElement>("Hours per week").value).toBe("6");
    button("Next").click();
    await vi.waitFor(() => expect(currentStep()).toContain("Materials"));
    button("Next").click();
    await vi.waitFor(() => expect(currentStep()).toContain("Guide"));
    expect(field<HTMLTextAreaElement>("Guide").value).toBe("Skip plant biology.");
  });

  it("runs no agent job on the way to the plan step", async () => {
    await mount();
    await toGuide();
    expect(commands()).not.toContain("generate_outline");
    expect(button("Plan the course")).toBeTruthy();
  });
});

describe("the plan step", () => {
  async function plan(): Promise<void> {
    radio("template", "exam").click();
    radio("level", "strong").click();
    radio("language", "th").click();
    type(field("Hours per week"), "6");
    backend({ ...handlers, dialog: () => ["/Users/me/notes"] });
    await toMaterials();
    button("Choose a folder").click();
    await vi.waitFor(() => expect(root.querySelectorAll("li.material")).toHaveLength(4));
    button("Next").click();
    await vi.waitFor(() => expect(currentStep()).toContain("Guide"));
    type(field<HTMLTextAreaElement>("Guide"), "Skip plant biology.");
    button("Plan the course").click();
  }

  it("saves the setup, makes the course current, plans, and hands the course on", async () => {
    await mount();
    await plan();
    await vi.waitFor(() => expect(options.onPlanned).toHaveBeenCalled());
    const jobSteps = commands().filter((c) => ["set_course_setup", "mark_course_opened", "generate_outline"].includes(c));
    expect(jobSteps).toEqual(["set_course_setup", "mark_course_opened", "generate_outline"]);
    expect(sent.find((s) => s.cmd === "set_course_setup")!.args).toEqual({
      courseId: "genetics",
      setup: { template: "exam", level: "strong", hoursPerWeek: 6, language: "th", guide: "Skip plant biology." },
    });
    expect(sent.find((s) => s.cmd === "generate_outline")!.args).toEqual({
      courseId: "genetics",
      topic: "Genetics",
      sources: ["membranes.pdf", "summary.md"],
    });
    expect(options.onPlanned).toHaveBeenCalledWith({ id: "genetics", storedId: "gen-1" });
  });

  it("labels the planning job Outline in the drawer", async () => {
    const feed = liveFeed();
    const job = gate();
    backend({ generate_outline: () => job.promise });
    await mount({ jobs: feed });
    await plan();
    await vi.waitFor(() => expect(commands()).toContain("generate_outline"));
    feed.add({
      id: "job-1",
      courseId: "gen-1",
      kind: "outline",
      driver: "claude",
      model: null,
      effort: null,
      command: "claude -p <prompt, 8 chars>",
      startedAt: "2026-11-09T12:00:00Z",
      endedAt: null,
      status: "running",
      lines: [],
    });
    const drawer = mountJobDrawer(root, feed, { courses: () => [], onClose: () => {} });
    drawer.open();
    expect(root.querySelector("aside.job-drawer .job-kind")!.textContent).toBe("Outline");
    job.open();
    await vi.waitFor(() => expect(options.onPlanned).toHaveBeenCalled());
  });

  it("shows the planning job while it runs and offers leave button back to home", async () => {
    const job = gate();
    backend({ generate_outline: () => job.promise });
    await mount();
    await plan();
    await vi.waitFor(() => expect(currentStep()).toContain("Plan"));
    await vi.waitFor(() => expect(root.querySelector(".job-status")).not.toBeNull());
    expect(root.querySelector(".job-status")!.textContent).toContain("Planning the course");
    expect(hasButton("Back")).toBe(false);
    expect(hasButton("Back to Home, plan keeps running")).toBe(true);
    button("Back to Home, plan keeps running").click();
    expect(options.onCancel).toHaveBeenCalled();
    job.open();
    await vi.waitFor(() => expect(options.onPlanned).toHaveBeenCalled());
  });

  it("states the choices being planned from", async () => {
    const job = gate();
    backend({ generate_outline: () => job.promise });
    await mount();
    await plan();
    await vi.waitFor(() => expect(root.querySelector(".plan-summary")).not.toBeNull());
    const text = root.querySelector(".plan-summary")!.textContent!;
    expect(text).toContain("Genetics");
    expect(text).toContain("Exam review");
    expect(text).toContain("6 hours");
    expect(text).toContain("2 sources");
    job.open();
  });

  it("shows why planning failed, then tries again or goes back to the guide", async () => {
    let calls = 0;
    backend({
      generate_outline: () => {
        if (++calls === 1) throw new Error("the agent's outline was not usable");
        return null;
      },
    });
    await mount();
    await plan();
    await vi.waitFor(() => expect(alertText()).toContain("the agent's outline was not usable"));
    expect(options.onPlanned).not.toHaveBeenCalled();
    button("Try again").click();
    await vi.waitFor(() => expect(options.onPlanned).toHaveBeenCalled());
    expect(calls).toBe(2);
  });

  it("goes back to the guide after a failure", async () => {
    backend({
      generate_outline: () => {
        throw new Error("no agent");
      },
    });
    await mount();
    await plan();
    await vi.waitFor(() => expect(hasButton("Back")).toBe(true));
    button("Back").click();
    expect(currentStep()).toContain("Guide");
    expect(field<HTMLTextAreaElement>("Guide").value).toBe("Skip plant biology.");
  });

  it("reads the job from the feed the app holds", async () => {
    const feed = {
      jobs: () => [
        {
          id: "job-1",
          courseId: "gen-1",
          kind: "outline",
          driver: "claude",
          model: null,
          effort: null,
          command: "claude -p",
          startedAt: new Date().toISOString(),
          endedAt: null,
          status: "running",
          lines: [{ at: "x", kind: "text", text: "Planning six chapters." }],
        },
      ],
      subscribe: () => () => undefined,
    } as unknown as JobFeed;
    const job = gate();
    backend({ generate_outline: () => job.promise });
    await mount({ jobs: feed });
    await plan();
    await vi.waitFor(() => expect(root.querySelector(".job-status-lines li")?.textContent).toBe("Planning six chapters."));
    job.open();
  });
});

describe("finishing the setup of a course that exists", () => {
  const existing: LibraryCourse = { ...made, id: "cell-biology", storedId: "cb-1", title: "Cell Biology", tags: ["science"] };

  it("fills in the saved setup and sources, fixes the title, and makes no new course", async () => {
    backend({
      get_course_setup: () => contract.course_setup,
      list_course_sources: () => contract.list_course_sources,
    });
    await mount({ existing });
    expect(field<HTMLInputElement>("Course title").value).toBe("Cell Biology");
    expect(field<HTMLInputElement>("Course title").disabled).toBe(false);
    expect(radio("template", "exam").checked).toBe(true);
    expect(radio("level", "some").checked).toBe(true);
    expect(field<HTMLInputElement>("Hours per week").value).toBe("6");
    button("Next").click();
    await vi.waitFor(() => expect(currentStep()).toContain("Materials"));
    expect(Array.from(root.querySelectorAll(".material-name"), (n) => n.textContent)).toEqual(["membranes.pdf", "example-com-a.md"]);
    expect(commands()).not.toContain("create_course");
  });

  it("imports into the course it was given", async () => {
    backend({ dialog: () => ["/Users/me/x.md"] });
    await mount({ existing });
    button("Next").click();
    await vi.waitFor(() => expect(currentStep()).toContain("Materials"));
    button("Choose files").click();
    await vi.waitFor(() => expect(commands()).toContain("import_files"));
    expect(sent.find((s) => s.cmd === "import_files")!.args.courseId).toBe("cell-biology");
  });

  it("reads the reader's own course fixture without trouble", async () => {
    await mount({ existing: readerContract.list_library_courses[0] as LibraryCourse });
    expect(field<HTMLInputElement>("Course title").disabled).toBe(false);
  });
});

describe("leaving the screen", () => {
  it("stops listening for drops once its signal aborts", async () => {
    await mount();
    await toMaterials();
    controller.abort();
    await emit("tauri://drag-drop", { paths: ["/Users/me/a.pdf"], position: { x: 1, y: 1 } });
    await settle();
    expect(commands()).not.toContain("import_files");
  });
});
