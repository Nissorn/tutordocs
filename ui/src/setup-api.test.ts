// @vitest-environment happy-dom
import { afterEach, describe, expect, it } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import contract from "./fixtures/wizard-contract.json";
import {
  getCourseSetup,
  importFiles,
  listCourseSources,
  listCourseTemplates,
  setCourseSetup,
  type CourseSetup,
} from "./setup-api";

afterEach(() => {
  clearMocks();
});

type Sent = { cmd: string; args: Record<string, unknown> };

function backend(replies: Record<string, unknown>): Sent[] {
  const sent: Sent[] = [];
  mockIPC((cmd, raw) => {
    sent.push({ cmd, args: (raw ?? {}) as Record<string, unknown> });
    if (!(cmd in replies)) throw new Error(`unexpected command ${cmd}`);
    return replies[cmd];
  });
  return sent;
}

describe("templates", () => {
  it("lists the templates the backend offers, in its order", async () => {
    backend({ list_course_templates: contract.list_course_templates });
    const templates = await listCourseTemplates();
    expect(templates.map((t) => t.id)).toEqual(["interview", "exam", "overview", "custom"]);
    expect(templates[0]).toEqual({
      id: "interview",
      title: "Interview prep",
      description: "Questions you are likely to be asked, with model answers and the reasoning behind them.",
    });
  });

  it("rejects a template without a title", async () => {
    backend({ list_course_templates: [{ id: "x", description: "d" }] });
    await expect(listCourseTemplates()).rejects.toThrow(/title/);
  });
});

describe("course setup", () => {
  it("reads a saved setup and reads none as null", async () => {
    const sent = backend({ get_course_setup: contract.course_setup });
    await expect(getCourseSetup("cell-biology")).resolves.toEqual(contract.course_setup);
    expect(sent[0]).toEqual({ cmd: "get_course_setup", args: { courseId: "cell-biology" } });
    clearMocks();
    backend({ get_course_setup: null });
    await expect(getCourseSetup("cell-biology")).resolves.toBeNull();
  });

  it("keeps hours per week as null when none were given", async () => {
    backend({ get_course_setup: contract.course_setup_no_hours });
    const setup = await getCourseSetup("c");
    expect(setup?.hoursPerWeek).toBeNull();
    expect(setup?.language).toBe("th");
  });

  it("sends the whole setup under courseId and returns what the backend saved", async () => {
    const sent = backend({ set_course_setup: contract.course_setup });
    const setup: CourseSetup = { ...(contract.course_setup as CourseSetup) };
    await expect(setCourseSetup("cell-biology", setup)).resolves.toEqual(contract.course_setup);
    expect(sent[0]).toEqual({ cmd: "set_course_setup", args: { courseId: "cell-biology", setup } });
  });

  it("rejects a level or language the contract does not name", async () => {
    backend({ get_course_setup: { ...contract.course_setup, level: "expert" } });
    await expect(getCourseSetup("c")).rejects.toThrow(/level/);
    clearMocks();
    backend({ get_course_setup: { ...contract.course_setup, language: "fr" } });
    await expect(getCourseSetup("c")).rejects.toThrow(/language/);
  });

  it("rejects hours that are neither a number nor null", async () => {
    backend({ get_course_setup: { ...contract.course_setup, hoursPerWeek: "six" } });
    await expect(getCourseSetup("c")).rejects.toThrow(/hoursPerWeek/);
  });
});

describe("importing files", () => {
  it("sends the absolute paths and reads each result, imported or skipped", async () => {
    const sent = backend({ import_files: contract.import_files });
    const paths = ["/Users/me/notes"];
    const results = await importFiles("cell-biology", paths);
    expect(sent[0]).toEqual({ cmd: "import_files", args: { courseId: "cell-biology", paths } });
    expect(results.map((r) => r.status)).toEqual(["imported", "imported", "skipped", "skipped"]);
    const first = results[0];
    if (first.status !== "imported") throw new Error("expected an imported result");
    expect(first.source).toEqual({
      name: "membranes.pdf",
      path: "sources/membranes.pdf",
      kind: "pdf",
      url: null,
      fetchedAt: null,
      bytes: 482113,
    });
    const dup = results[2];
    if (dup.status !== "skipped") throw new Error("expected a skipped result");
    expect(dup.reason).toBe("duplicate");
    expect(dup.path).toBe("/Users/me/notes/summary-copy.md");
  });

  it("refuses an imported result that carries no source", async () => {
    backend({ import_files: [{ path: "/a.md", status: "imported", reason: null, source: null }] });
    await expect(importFiles("c", ["/a.md"])).rejects.toThrow(/source/);
  });

  it("refuses a status the contract does not name", async () => {
    backend({ import_files: [{ path: "/a.md", status: "queued", reason: null, source: null }] });
    await expect(importFiles("c", ["/a.md"])).rejects.toThrow(/status/);
  });

  it("lists a course's sources with their kind and size", async () => {
    const sent = backend({ list_course_sources: contract.list_course_sources });
    const sources = await listCourseSources("cell-biology");
    expect(sent[0]).toEqual({ cmd: "list_course_sources", args: { courseId: "cell-biology" } });
    expect(sources.map((s) => [s.name, s.kind, s.bytes])).toEqual([
      ["membranes.pdf", "pdf", 482113],
      ["example-com-a.md", "url", 5120],
    ]);
    expect(sources[1].url).toBe("https://example.com/a");
  });

  it("rejects a source kind the contract does not name", async () => {
    backend({ list_course_sources: [{ ...contract.list_course_sources[0], kind: "video" }] });
    await expect(listCourseSources("c")).rejects.toThrow(/kind/);
  });
});
