// @vitest-environment happy-dom
import { afterEach, describe, expect, it } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import contract from "./fixtures/study-contract.json";
import { askAbout, chapterContext, listAnswers, reviseSection } from "./study-api";

afterEach(() => {
  clearMocks();
});

/** Replies with `reply` and records the command and arguments the wrapper sent. */
function backend(reply: unknown): { calls: { cmd: string; args: unknown }[] } {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    return reply;
  });
  return { calls };
}

describe("chapter_context", () => {
  it("sends the course and chapter, and reads goals and sources", async () => {
    const { calls } = backend(contract.chapter_context);
    await expect(chapterContext("cell-biology", "cells")).resolves.toEqual(contract.chapter_context);
    expect(calls).toEqual([{ cmd: "chapter_context", args: { courseId: "cell-biology", chapterId: "cells" } }]);
  });

  it.each([
    ["objectives that are not a list", { ...contract.chapter_context, objectives: "one" }],
    ["a summary that is missing", { objectives: [], sources: [] }],
    ["a source that is not a name", { ...contract.chapter_context, sources: [4] }],
    ["a bare string", "ok"],
  ])("refuses %s", async (_label, reply) => {
    backend(reply);
    await expect(chapterContext("cell-biology", "cells")).rejects.toThrow();
  });
});

describe("list_answers", () => {
  it("keeps the order Rust sends, newest first, and reads every field", async () => {
    const { calls } = backend(contract.answers);
    const answers = await listAnswers("cell-biology", "cells");
    expect(answers.map((a) => a.id)).toEqual([2, 1]);
    expect(answers[0]).toEqual(contract.answers[0]);
    expect(answers[1].quote).toBeNull();
    expect(calls).toEqual([{ cmd: "list_answers", args: { courseId: "cell-biology", chapterId: "cells" } }]);
  });

  it.each([
    ["an id that is a string", [{ ...contract.answers[0], id: "2" }]],
    ["a quote that is a number", [{ ...contract.answers[0], quote: 7 }]],
    ["a missing askedAt", [{ ...contract.answers[0], askedAt: undefined }]],
    ["an object where a list belongs", { answers: [] }],
  ])("refuses %s", async (_label, reply) => {
    backend(reply);
    await expect(listAnswers("cell-biology", "cells")).rejects.toThrow();
  });
});

describe("ask_about", () => {
  it("sends the quote and question, and returns the stored answer", async () => {
    const { calls } = backend(contract.answers[0]);
    const answer = await askAbout("cell-biology", "cells", "basic unit of life", "Why is the cell called the basic unit?");
    expect(answer).toEqual(contract.answers[0]);
    expect(calls).toEqual([
      {
        cmd: "ask_about",
        args: {
          courseId: "cell-biology",
          chapterId: "cells",
          quote: "basic unit of life",
          question: "Why is the cell called the basic unit?",
        },
      },
    ]);
  });

  it("sends a null quote when the question is about the whole chapter", async () => {
    const { calls } = backend(contract.ask_about_whole_chapter);
    const answer = await askAbout("cell-biology", "cells", null, "How does a cell stay different?");
    expect(answer.quote).toBeNull();
    expect((calls[0].args as Record<string, unknown>).quote).toBeNull();
  });
});

describe("revise_section", () => {
  it("sends the quote and instruction, and returns the revised chapter", async () => {
    const { calls } = backend(contract.revise_section);
    const doc = await reviseSection("cell-biology", "cells", "A membrane keeps the inside", "Add an example");
    expect(doc).toEqual(contract.revise_section);
    expect(calls).toEqual([
      {
        cmd: "revise_section",
        args: {
          courseId: "cell-biology",
          chapterId: "cells",
          quote: "A membrane keeps the inside",
          instruction: "Add an example",
        },
      },
    ]);
  });

  it("refuses a reply that is still a placeholder, so the chapter is never blanked", async () => {
    backend({ id: "cells", title: "Cells", state: "placeholder" });
    await expect(reviseSection("cell-biology", "cells", "q", "i")).rejects.toThrow(/placeholder/);
  });

  it("passes a Rust error through as the failure", async () => {
    mockIPC(() => {
      throw "the agent exited with status 1";
    });
    await expect(reviseSection("cell-biology", "cells", "q", "i")).rejects.toBe("the agent exited with status 1");
  });
});
