import { describe, expect, it } from "vitest";
import contract from "./fixtures/wizard-contract.json";
import type { ImportResult, Template } from "./setup-api";
import {
  back,
  basename,
  beginImport,
  beginUrl,
  blocker,
  describeSkip,
  failImport,
  failUrl,
  forward,
  importedSourceNames,
  parseHours,
  resumeWizard,
  settleImport,
  settleUrl,
  startWizard,
  tally,
  waiting,
  toSetup,
  type Wizard,
} from "./wizard-model";

const templates = contract.list_course_templates as Template[];
const results = contract.import_files as ImportResult[];

function wizard(over: Partial<Wizard> = {}): Wizard {
  return { ...startWizard(templates), ...over };
}

describe("the steps", () => {
  it("starts on the goal with the lightest template chosen", () => {
    const w = startWizard(templates);
    expect(w.step).toBe("goal");
    expect(w.goal.template).toBe("overview");
    expect(w.course).toBeNull();
  });

  it("falls back to the first template when there is no overview", () => {
    expect(startWizard(templates.filter((t) => t.id !== "overview")).goal.template).toBe("interview");
  });

  it("walks goal, materials, guide, plan and back again", () => {
    let w = wizard({ goal: { ...wizard().goal, title: "Cells" } });
    const seen = [w.step];
    for (let i = 0; i < 3; i++) {
      w = forward(w);
      seen.push(w.step);
    }
    expect(seen).toEqual(["goal", "materials", "guide", "plan"]);
    expect(forward(w).step).toBe("plan");
    const walked = [w.step];
    for (let i = 0; i < 3; i++) {
      w = back(w);
      walked.push(w.step);
    }
    expect(walked).toEqual(["plan", "guide", "materials", "goal"]);
    expect(back(w).step).toBe("goal");
  });

  it("will not leave the goal without a title", () => {
    const w = wizard();
    expect(blocker(w)).toBe("Name the course first.");
    expect(forward(w).step).toBe("goal");
    expect(blocker(wizard({ goal: { ...w.goal, title: "   " } }))).toBe("Name the course first.");
  });

  it("will not leave the goal with hours that are not a whole number from 1 to 168", () => {
    const goal = { ...wizard().goal, title: "Cells" };
    for (const hours of ["abc", "0", "-3", "200", "6.5"]) {
      expect(blocker(wizard({ goal: { ...goal, hours } })), hours).toMatch(/hours per week/i);
    }
    expect(blocker(wizard({ goal: { ...goal, hours: "" } }))).toBeNull();
    expect(blocker(wizard({ goal: { ...goal, hours: "6" } }))).toBeNull();
  });

  it("holds the step while the course is being made", () => {
    const goal = { ...wizard().goal, title: "Cells" };
    const w = wizard({ goal, busy: "creating" });
    expect(blocker(w)).toBe("Creating the course…");
    expect(forward(w).step).toBe("goal");
  });

  it("holds the materials step while a file is still importing", () => {
    const w = wizard({ step: "materials", materials: beginImport([], ["/a.pdf"]) });
    expect(blocker(w)).toMatch(/importing/i);
    expect(forward(w).step).toBe("materials");
  });

  it("lets the user move on from the materials step with no files at all", () => {
    expect(blocker(wizard({ step: "materials" }))).toBeNull();
  });

  it("does not go back while the plan runs", () => {
    const w = wizard({ step: "plan", plan: { phase: "running" } });
    expect(back(w).step).toBe("plan");
    expect(back({ ...w, plan: { phase: "failed", reason: "x" } }).step).toBe("guide");
  });
});

describe("waiting", () => {
  it("is true while the course is being made or a file is importing, and false otherwise", () => {
    expect(waiting(wizard())).toBe(false);
    expect(waiting(wizard({ busy: "creating" }))).toBe(true);
    expect(waiting(wizard({ materials: beginImport([], ["/a.pdf"]) }))).toBe(true);
    expect(waiting(wizard({ goal: { ...wizard().goal, title: "" } }))).toBe(false);
  });
});

describe("hours", () => {
  it("reads blank as none, a whole number from 1 to 168 as hours, anything else as invalid", () => {
    expect(parseHours("")).toEqual({ kind: "none" });
    expect(parseHours("  ")).toEqual({ kind: "none" });
    expect(parseHours("6")).toEqual({ kind: "some", value: 6 });
    expect(parseHours(" 12 ")).toEqual({ kind: "some", value: 12 });
    expect(parseHours("168")).toEqual({ kind: "some", value: 168 });
    expect(parseHours("2.5")).toEqual({ kind: "invalid" });
    expect(parseHours("0")).toEqual({ kind: "invalid" });
    expect(parseHours("six")).toEqual({ kind: "invalid" });
    expect(parseHours("169")).toEqual({ kind: "invalid" });
  });
});

describe("the setup the wizard sends", () => {
  it("carries the template, level, hours, language and guide", () => {
    const w = wizard({
      goal: { title: "Cells", tags: "", template: "exam", level: "strong", hours: "6", language: "th" },
      guide: "Skip plant biology.",
    });
    expect(toSetup(w)).toEqual({
      template: "exam",
      level: "strong",
      hoursPerWeek: 6,
      language: "th",
      guide: "Skip plant biology.",
    });
  });

  it("sends null hours when the box was left empty", () => {
    expect(toSetup(wizard()).hoursPerWeek).toBeNull();
  });
});

describe("materials", () => {
  const paths = ["/Users/me/notes", "/Users/me/other.md"];

  it("shows each dropped path as importing, newest last", () => {
    const m = beginImport([], paths);
    expect(m.map((x) => [x.path, x.label, x.state.phase])).toEqual([
      ["/Users/me/notes", "notes", "importing"],
      ["/Users/me/other.md", "other.md", "importing"],
    ]);
  });

  it("swaps the importing rows for one row per result, imported or skipped with its reason", () => {
    const m = settleImport(beginImport([], ["/Users/me/notes"]), ["/Users/me/notes"], results);
    expect(m.map((x) => [x.label, x.state])).toEqual([
      ["membranes.pdf", { phase: "imported", sourceName: "membranes.pdf" }],
      ["summary.md", { phase: "imported", sourceName: "summary.md" }],
      ["summary-copy.md", { phase: "skipped", reason: "duplicate" }],
      ["lecture.mp4", { phase: "skipped", reason: "unsupported file type .mp4" }],
    ]);
  });

  it("keeps an imported row when the same path comes back as a duplicate", () => {
    const once = settleImport(beginImport([], ["/Users/me/notes/summary.md"]), ["/Users/me/notes/summary.md"], [results[1]]);
    const again = settleImport(
      beginImport(once, ["/Users/me/notes/summary.md"]),
      ["/Users/me/notes/summary.md"],
      [{ path: "/Users/me/notes/summary.md", status: "skipped", reason: "duplicate" }],
    );
    expect(again.map((x) => [x.label, x.state.phase])).toEqual([["summary.md", "imported"]]);
  });

  it("marks the dropped paths failed when the whole import call fails", () => {
    const m = failImport(beginImport([], paths), paths, "disk is full");
    expect(m.map((x) => x.state)).toEqual([
      { phase: "failed", reason: "disk is full" },
      { phase: "failed", reason: "disk is full" },
    ]);
  });

  it("counts each state", () => {
    const m = settleImport(beginImport([], ["/Users/me/notes"]), ["/Users/me/notes"], results);
    expect(tally(m)).toEqual({ importing: 0, imported: 2, skipped: 2, failed: 0 });
  });

  it("names the imported sources for the planner and nothing else", () => {
    const m = settleImport(beginImport([], ["/Users/me/notes"]), ["/Users/me/notes"], results);
    expect(importedSourceNames(wizard({ materials: m }))).toEqual(["membranes.pdf", "summary.md"]);
  });

  it("words a duplicate for the reader and passes any other reason through", () => {
    expect(describeSkip("duplicate")).toBe("Already in this course, same contents.");
    expect(describeSkip("unsupported file type .mp4")).toBe("unsupported file type .mp4");
    expect(describeSkip(null)).toBe("Skipped.");
  });

  it("takes the last part of a path, whichever way it slashes", () => {
    expect(basename("/Users/me/notes/a.pdf")).toBe("a.pdf");
    expect(basename("/Users/me/notes/")).toBe("notes");
    expect(basename("C:\\Users\\me\\a.pdf")).toBe("a.pdf");
  });
});

describe("a web page", () => {
  it("shows the URL as fetching, then as the source the backend made from it", () => {
    const fetching = beginUrl([], "https://example.com/a");
    expect(fetching).toEqual([
      { path: "https://example.com/a", label: "https://example.com/a", origin: "url", state: { phase: "importing" } },
    ]);
    const done = settleUrl(fetching, "https://example.com/a", "example-com-a.md");
    expect(done[0].state).toEqual({ phase: "imported", sourceName: "example-com-a.md" });
  });

  it("keeps the reason when the fetch fails, and lets the same URL be tried again", () => {
    const failed = failUrl(beginUrl([], "https://x.test"), "https://x.test", "the page needs a login");
    expect(failed[0].state).toEqual({ phase: "failed", reason: "the page needs a login" });
    expect(beginUrl(failed, "https://x.test")).toHaveLength(1);
    expect(beginUrl(failed, "https://x.test")[0].state.phase).toBe("importing");
  });
});

describe("resuming a course that exists", () => {
  it("opens on the goal with the title fixed and the saved setup and sources filled in", () => {
    const w = resumeWizard(
      templates,
      { id: "cell-biology", storedId: "cb-1", title: "Cell Biology", tags: ["science", "exam"] },
      contract.course_setup as never,
      contract.list_course_sources as never,
    );
    expect(w.step).toBe("goal");
    expect(w.course).toEqual({ id: "cell-biology", storedId: "cb-1" });
    expect(w.goal).toEqual({
      title: "Cell Biology",
      tags: "science, exam",
      template: "exam",
      level: "some",
      hours: "6",
      language: "en",
    });
    expect(w.guide).toBe("Focus on membranes. Skip plant biology.");
    expect(w.materials.map((m) => [m.label, m.state.phase])).toEqual([
      ["membranes.pdf", "imported"],
      ["example-com-a.md", "imported"],
    ]);
  });

  it("keeps the defaults when the course never had a setup", () => {
    const w = resumeWizard(templates, { id: "c", storedId: "c", title: "C", tags: [] }, null, []);
    expect(w.goal.template).toBe("overview");
    expect(w.goal.level).toBe("new");
    expect(w.goal.language).toBe("en");
    expect(w.goal.hours).toBe("");
  });
});
