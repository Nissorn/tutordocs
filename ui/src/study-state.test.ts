import { describe, expect, it, vi } from "vitest";
import contract from "./fixtures/study-contract.json";
import { emptyStudy, flow, flowController, studyReduce, type Flow, type FlowEvent, type StudyModel } from "./study-state";

const answers = contract.answers;

describe("the panel's sections", () => {
  it("starts every section loading except the notes, which the chapter text fills", () => {
    expect(emptyStudy()).toEqual({
      goals: { state: "loading" },
      sources: { state: "loading" },
      notes: { state: "loading" },
      answers: { state: "loading" },
    });
  });

  it("turns the chapter context into ready goals and source chips, marking which sources open", () => {
    const model = studyReduce(emptyStudy(), { type: "context", context: contract.chapter_context, readable: ["cell-notes.pdf"] });
    expect(model.goals).toEqual({
      state: "ready",
      value: { summary: "What a cell is, and why a boundary matters.", objectives: contract.chapter_context.objectives },
    });
    expect(model.sources).toEqual({
      state: "ready",
      value: [
        { name: "cell-notes.pdf", opens: true },
        { name: "membranes.md", opens: false },
      ],
    });
  });

  it("reads a chapter with no goals and no sources as empty, not as an empty list", () => {
    const model = studyReduce(emptyStudy(), {
      type: "context",
      context: { objectives: [], summary: "", sources: [] },
      readable: [],
    });
    expect(model.goals).toEqual({ state: "empty" });
    expect(model.sources).toEqual({ state: "empty" });
  });

  it("shows a summary alone as ready goals", () => {
    const model = studyReduce(emptyStudy(), {
      type: "context",
      context: { objectives: [], summary: "One line.", sources: [] },
      readable: [],
    });
    expect(model.goals).toEqual({ state: "ready", value: { summary: "One line.", objectives: [] } });
  });

  it("fails goals and sources together when the context cannot be read, and leaves the answers alone", () => {
    const loaded = studyReduce(emptyStudy(), { type: "answers", answers });
    const model = studyReduce(loaded, { type: "context-failed", error: new Error("no such chapter") });
    expect(model.goals).toEqual({ state: "error", message: "no such chapter" });
    expect(model.sources).toEqual({ state: "error", message: "no such chapter" });
    expect(model.answers.state).toBe("ready");
  });

  it("keeps the answers in the order Rust lists them and calls none empty", () => {
    expect(studyReduce(emptyStudy(), { type: "answers", answers }).answers).toEqual({ state: "ready", value: answers });
    expect(studyReduce(emptyStudy(), { type: "answers", answers: [] }).answers).toEqual({ state: "empty" });
  });

  it("puts a new answer first, whether the list was empty, ready, or failed", () => {
    const fresh = contract.ask_about_whole_chapter;
    const fromEmpty = studyReduce(studyReduce(emptyStudy(), { type: "answers", answers: [] }), { type: "answer", answer: fresh });
    expect(fromEmpty.answers).toEqual({ state: "ready", value: [fresh] });
    const fromReady = studyReduce(studyReduce(emptyStudy(), { type: "answers", answers }), { type: "answer", answer: fresh });
    expect(fromReady.answers).toEqual({ state: "ready", value: [fresh, ...answers] });
    const fromFailed = studyReduce(studyReduce(emptyStudy(), { type: "answers-failed", error: "boom" }), { type: "answer", answer: fresh });
    expect(fromFailed.answers).toEqual({ state: "ready", value: [fresh] });
  });

  it("does not list the same answer twice when it arrives again", () => {
    const loaded = studyReduce(emptyStudy(), { type: "answers", answers });
    const again = studyReduce(loaded, { type: "answer", answer: answers[1] });
    expect(again.answers).toEqual({ state: "ready", value: [answers[1], answers[0]] });
  });

  it("describes the notes by how many marks the chapter holds", () => {
    const none = studyReduce(emptyStudy(), { type: "notes", count: 0 });
    expect(none.notes).toEqual({ state: "empty" });
    const some = studyReduce(emptyStudy(), { type: "notes", count: 3 });
    expect(some.notes).toEqual({ state: "ready", value: 3 });
    const bad = studyReduce(emptyStudy(), { type: "notes-failed", error: "no marks" });
    expect(bad.notes).toEqual({ state: "error", message: "no marks" });
  });

  it("goes back to loading when goals and sources, or the questions, are read again", () => {
    const failed = studyReduce(studyReduce(emptyStudy(), { type: "context-failed", error: "x" }), { type: "answers", answers });
    const again = studyReduce(failed, { type: "context-loading" });
    expect(again.goals).toEqual({ state: "loading" });
    expect(again.sources).toEqual({ state: "loading" });
    expect(again.answers.state).toBe("ready");
    expect(studyReduce(again, { type: "answers-loading" }).answers).toEqual({ state: "loading" });
  });

  it("turns anything thrown into a message, including the plain string Tauri rejects with", () => {
    const fromString: StudyModel = studyReduce(emptyStudy(), { type: "answers-failed", error: "the store is locked" });
    expect(fromString.answers).toEqual({ state: "error", message: "the store is locked" });
  });
});

type Case = [string, Flow<string>, FlowEvent<string>, Flow<string>];

describe("a fix or an ask, from idle to done", () => {
  const idle: Flow<string> = { phase: "idle" };
  const editing = (text: string): Flow<string> => ({ phase: "editing", text });
  const running = (text: string): Flow<string> => ({ phase: "running", text });
  const failed = (text: string, error: string): Flow<string> => ({ phase: "failed", text, error });
  const done: Flow<string> = { phase: "done", result: "answer" };

  const cases: Case[] = [
    ["opens an empty box", idle, { type: "open" }, editing("")],
    ["opens a box by typing into it", idle, { type: "edit", text: "why" }, editing("why")],
    ["keeps what is typed", editing(""), { type: "edit", text: "add an example" }, editing("add an example")],
    ["will not run an empty box", editing("   "), { type: "submit" }, editing("   ")],
    ["runs a typed instruction, trimmed", editing("  add an example "), { type: "submit" }, running("add an example")],
    ["finishes with the result", running("x"), { type: "succeed", result: "answer" }, done],
    ["fails and keeps the text for another try", running("x"), { type: "fail", error: "exit 1" }, failed("x", "exit 1")],
    ["goes back to editing from a failure when the user types", failed("x", "e"), { type: "edit", text: "xy" }, editing("xy")],
    ["tries again from a failure without retyping", failed("x", "e"), { type: "submit" }, running("x")],
    ["opens a fresh box after an answer", done, { type: "open" }, editing("")],
    ["closes while editing", editing("x"), { type: "close" }, idle],
    ["closes after a failure", failed("x", "e"), { type: "close" }, idle],
    ["closes after an answer", done, { type: "close" }, idle],
  ];

  it.each(cases)("%s", (_name, from, event, to) => {
    expect(flow(from, event)).toEqual(to);
  });

  it("cannot be closed or reopened while the job runs, and ignores results that arrive with no job", () => {
    const busy = running("x");
    expect(flow(busy, { type: "close" })).toBe(busy);
    expect(flow(busy, { type: "open" })).toBe(busy);
    expect(flow(busy, { type: "edit", text: "y" })).toBe(busy);
    expect(flow(busy, { type: "submit" })).toBe(busy);
    expect(flow(idle, { type: "succeed", result: "a" })).toBe(idle);
    expect(flow(editing("x"), { type: "fail", error: "e" })).toEqual(editing("x"));
  });
});

describe("a flow that runs a job", () => {
  function setup(run: (text: string) => Promise<string>) {
    const seen: string[] = [];
    const controller = flowController<string>(run, (next) => seen.push(next.phase));
    return { controller, seen };
  }

  it("runs the job once with the trimmed text, and ends done with its result", async () => {
    const run = vi.fn(async (text: string) => `answer to ${text}`);
    const { controller, seen } = setup(run);
    controller.send({ type: "open" });
    controller.send({ type: "edit", text: "  why?  " });
    controller.send({ type: "submit" });
    controller.send({ type: "submit" });
    expect(run).toHaveBeenCalledTimes(1);
    expect(run).toHaveBeenCalledWith("why?");
    await vi.waitFor(() => expect(controller.state.phase).toBe("done"));
    expect(controller.state).toEqual({ phase: "done", result: "answer to why?" });
    expect(seen).toEqual(["editing", "editing", "running", "done"]);
  });

  it("does not run an empty draft", () => {
    const run = vi.fn(async () => "x");
    const { controller } = setup(run);
    controller.send({ type: "open" });
    controller.send({ type: "submit" });
    expect(run).not.toHaveBeenCalled();
    expect(controller.state.phase).toBe("editing");
  });

  it("fails with the message of an Error or of a plain string, and keeps the text to retry", async () => {
    let attempt = 0;
    const run = vi.fn(async (_text: string) => {
      attempt++;
      if (attempt === 1) throw "exit 1";
      if (attempt === 2) throw new Error("timed out");
      return "fine";
    });
    const { controller } = setup(run);
    controller.send({ type: "open" });
    controller.send({ type: "edit", text: "go" });
    controller.send({ type: "submit" });
    await vi.waitFor(() => expect(controller.state.phase).toBe("failed"));
    expect(controller.state).toEqual({ phase: "failed", text: "go", error: "exit 1" });
    controller.send({ type: "submit" });
    await vi.waitFor(() => expect(controller.state).toEqual({ phase: "failed", text: "go", error: "timed out" }));
    controller.send({ type: "submit" });
    await vi.waitFor(() => expect(controller.state).toEqual({ phase: "done", result: "fine" }));
    expect(run.mock.calls.map((c) => c[0])).toEqual(["go", "go", "go"]);
  });

  it("lets a running job finish even when the user tries to close", async () => {
    let finish: (value: string) => void = () => {};
    const { controller } = setup(() => new Promise((resolve) => (finish = resolve)));
    controller.send({ type: "open" });
    controller.send({ type: "edit", text: "go" });
    controller.send({ type: "submit" });
    controller.send({ type: "close" });
    expect(controller.state.phase).toBe("running");
    finish("ok");
    await vi.waitFor(() => expect(controller.state).toEqual({ phase: "done", result: "ok" }));
  });
});
