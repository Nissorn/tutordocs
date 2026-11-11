// @vitest-environment happy-dom
import { afterEach, describe, expect, it } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import readerContract from "./fixtures/reader-contract.json";
import contract from "./fixtures/wizard-contract.json";
import {
  approveOutline,
  branchOutline,
  cancelChapterQueue,
  getOutline,
  getOutlineVariant,
  isDraftRefusal,
  listOutlineVariants,
  planOutline,
  reviseOutline,
  saveOutline,
  setCurrentVariant,
} from "./outline-api";

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

describe("reading an outline", () => {
  it("reads the draft with its chapters, sources, comments and written flags", async () => {
    const sent = backend({ get_outline: contract.outline_draft });
    const outline = await getOutline("cell-biology");
    expect(sent[0]).toEqual({ cmd: "get_outline", args: { courseId: "cell-biology" } });
    expect(outline.status).toBe("draft");
    expect(outline.objectives).toHaveLength(2);
    expect(outline.chapters.map((c) => [c.id, c.written, c.sources])).toEqual([
      ["cells", false, ["membranes.pdf"]],
      ["membranes", false, ["membranes.pdf", "summary.md"]],
      ["nucleus", false, []],
    ]);
  });

  it("keeps a null id for a chapter the user added", async () => {
    backend({
      get_outline: {
        ...contract.outline_draft,
        chapters: [{ id: null, title: "New", summary: "", sources: [], comment: "", written: false }],
      },
    });
    expect((await getOutline("c")).chapters[0].id).toBeNull();
  });

  it("rejects a status the contract does not name", async () => {
    backend({ get_outline: { ...contract.outline_draft, status: "published" } });
    await expect(getOutline("c")).rejects.toThrow(/status/);
  });

  it("rejects a chapter whose written flag is missing", async () => {
    const { written: _written, ...rest } = contract.outline_draft.chapters[0];
    backend({ get_outline: { ...contract.outline_draft, chapters: [rest] } });
    await expect(getOutline("c")).rejects.toThrow(/written/);
  });
});

describe("saving, revising and approving", () => {
  it("saves the user's outline under courseId and returns the saved one", async () => {
    const sent = backend({ save_outline: contract.outline_draft });
    const draft = JSON.parse(JSON.stringify(contract.outline_draft));
    const saved = await saveOutline("cell-biology", draft);
    expect(sent[0]).toEqual({ cmd: "save_outline", args: { courseId: "cell-biology", outline: draft } });
    expect(saved).toEqual(contract.outline_draft);
  });

  it("revises with only the course id, since the comments are already saved", async () => {
    const sent = backend({ revise_outline: contract.outline_draft });
    await reviseOutline("cell-biology");
    expect(sent[0]).toEqual({ cmd: "revise_outline", args: { courseId: "cell-biology" } });
  });

  it("approves with the choice in the contract's words", async () => {
    const sent = backend({ approve_outline: { ...contract.outline_draft, status: "approved" } });
    const approved = await approveOutline("cell-biology", "all");
    await approveOutline("cell-biology", "on_open");
    expect(sent.map((s) => s.args)).toEqual([
      { courseId: "cell-biology", generate: "all" },
      { courseId: "cell-biology", generate: "on_open" },
    ]);
    expect(approved.status).toBe("approved");
  });

  it("stops the chapter queue by course id", async () => {
    const sent = backend({ cancel_chapter_queue: null });
    await cancelChapterQueue("cell-biology");
    expect(sent[0]).toEqual({ cmd: "cancel_chapter_queue", args: { courseId: "cell-biology" } });
  });
});

describe("planning", () => {
  it("makes the course current, then plans with the topic and the imported source names", async () => {
    const sent = backend({ mark_course_opened: readerContract.list_library_courses[0], generate_outline: null });
    await planOutline("cell-biology", "Cell Biology", ["membranes.pdf"]);
    expect(sent.map((s) => s.cmd)).toEqual(["mark_course_opened", "generate_outline"]);
    expect(sent[0].args).toEqual({ courseId: "cell-biology" });
    expect(sent[1].args).toEqual({ courseId: "cell-biology", topic: "Cell Biology", sources: ["membranes.pdf"] });
  });
});

describe("the draft refusal", () => {
  it("recognises the backend's reason, as a string or an Error", () => {
    expect(isDraftRefusal("outline is a draft")).toBe(true);
    expect(isDraftRefusal(new Error("could not open chapter: outline is a draft"))).toBe(true);
    expect(isDraftRefusal(new Error("disk is full"))).toBe(false);
  });
});

describe("outline variants", () => {
  const sampleMeta = {
    id: "current",
    label: "Current",
    parentId: null,
    createdAt: "2026-11-09T10:00:00Z",
    chapterCount: 3,
    writtenChapterCount: 1,
    current: true,
  };

  const sampleVariant = {
    id: "draft-b",
    label: "Draft B",
    parentId: "current",
    createdAt: "2026-11-09T10:05:00Z",
    objectives: ["Learn Rust"],
    chapters: [
      {
        id: "intro",
        title: "Introduction",
        summary: "Getting started",
        sources: ["guide.md"],
        comment: "",
        written: true,
      },
    ],
  };

  it("lists outline variants for a course", async () => {
    const sent = backend({ list_outline_variants: [sampleMeta] });
    const list = await listOutlineVariants("cell-biology");
    expect(sent[0]).toEqual({ cmd: "list_outline_variants", args: { courseId: "cell-biology" } });
    expect(list).toEqual([sampleMeta]);
  });

  it("reads a specific outline variant", async () => {
    const sent = backend({ get_outline_variant: sampleVariant });
    const variant = await getOutlineVariant("cell-biology", "draft-b");
    expect(sent[0]).toEqual({
      cmd: "get_outline_variant",
      args: { courseId: "cell-biology", variantId: "draft-b" },
    });
    expect(variant).toEqual(sampleVariant);
  });

  it("branches a new outline variant with label", async () => {
    const sent = backend({ branch_outline: sampleVariant });
    const variant = await branchOutline("cell-biology", "current", "Draft B");
    expect(sent[0]).toEqual({
      cmd: "branch_outline",
      args: { courseId: "cell-biology", fromVariantId: "current", label: "Draft B" },
    });
    expect(variant.id).toBe("draft-b");
  });

  it("swaps the current variant", async () => {
    const sent = backend({ set_current_variant: sampleVariant });
    const swapped = await setCurrentVariant("cell-biology", "draft-b");
    expect(sent[0]).toEqual({
      cmd: "set_current_variant",
      args: { courseId: "cell-biology", variantId: "draft-b" },
    });
    expect(swapped.id).toBe("draft-b");
  });

  it("passes variantId to save, revise, and approve when provided", async () => {
    const sent = backend({
      save_outline: contract.outline_draft,
      revise_outline: contract.outline_draft,
      approve_outline: { ...contract.outline_draft, status: "approved" },
    });
    const draft = JSON.parse(JSON.stringify(contract.outline_draft));
    await saveOutline("cell-biology", draft, "draft-b");
    await reviseOutline("cell-biology", "draft-b");
    await approveOutline("cell-biology", "on_open", "draft-b");

    expect(sent[0]).toEqual({
      cmd: "save_outline",
      args: { courseId: "cell-biology", outline: draft, variantId: "draft-b" },
    });
    expect(sent[1]).toEqual({
      cmd: "revise_outline",
      args: { courseId: "cell-biology", variantId: "draft-b" },
    });
    expect(sent[2]).toEqual({
      cmd: "approve_outline",
      args: { courseId: "cell-biology", generate: "on_open", variantId: "draft-b" },
    });
  });
});
