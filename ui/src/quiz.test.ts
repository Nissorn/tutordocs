// @vitest-environment happy-dom
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import contract from "./fixtures/quiz-contract.json";
import { parseProgress, reviewWords } from "./progress";
import { parseGraded, parseQuiz, quizPage, quizScore, scoreSummary, type Graded } from "./quiz";

type Args = Record<string, unknown>;
const calls: { cmd: string; args: Args }[] = [];

function backend(handlers: Record<string, (args: Args) => unknown>): void {
  mockIPC((cmd, raw) => {
    const args = (raw ?? {}) as Args;
    calls.push({ cmd, args });
    const handler = handlers[cmd];
    if (!handler) throw new Error(`unexpected command ${cmd}`);
    return handler(args);
  });
}

async function settle(): Promise<void> {
  for (let i = 0; i < 5; i++) await new Promise((r) => setTimeout(r, 0));
}

beforeEach(() => {
  document.body.innerHTML = "";
  calls.length = 0;
});

afterEach(() => clearMocks());

describe("the wire contract", () => {
  it("parses the quiz and grades Rust sends", () => {
    const quiz = parseQuiz(contract.generate_quiz);
    expect(quiz.questions[0].referenceAnswer).toBe(contract.generate_quiz.questions[0].referenceAnswer);
    expect(quiz.questions[0].combines).toHaveLength(2);
    expect(parseGraded(contract.grade_quiz)[0]).toMatchObject({ result: "correct", final: true });
  });

  it("refuses a grade with an unknown result", () => {
    expect(() => parseGraded([{ ...contract.grade_quiz[0], result: "excellent" }])).toThrow();
  });
});

describe("the quiz page", () => {
  it("sends the learner's free text with its question and shows the grade", async () => {
    backend({
      generate_quiz: () => contract.generate_quiz,
      grade_quiz: () => contract.grade_quiz,
    });
    const onGraded = vi.fn();
    const page = quizPage("moves", "Moves", () => {}, onGraded);
    document.body.append(page);
    await settle();

    const answer = page.querySelector<HTMLTextAreaElement>("textarea.quiz-answer")!;
    answer.value = contract.learner_answer;
    page.querySelector("form")!.requestSubmit();
    await settle();

    const grade = calls.find((c) => c.cmd === "grade_quiz")!;
    expect(grade.args).toEqual({
      chapterId: "moves",
      answers: [{ question: contract.generate_quiz.questions[0], answer: contract.learner_answer }],
    });
    expect(page.querySelector(".quiz-verdict")!.textContent).toContain("Correct");
    expect(answer.readOnly).toBe(true);
    expect(onGraded).toHaveBeenCalledTimes(1);
  });

  it("keeps an answer the grader could not judge open, and sends only that one again", async () => {
    const two = {
      ...contract.generate_quiz,
      questions: [contract.generate_quiz.questions[0], { ...contract.generate_quiz.questions[0], id: "q2" }],
    };
    const unsure = { ...contract.grade_quiz[0], questionId: "q2", result: "ungraded", final: false, confidence: 0.6 };
    backend({
      generate_quiz: () => two,
      grade_quiz: (args) => ((args.answers as unknown[]).length === 2 ? [contract.grade_quiz[0], unsure] : [unsure]),
    });
    const page = quizPage("moves", "Moves", () => {}, () => {});
    document.body.append(page);
    await settle();

    const [first, second] = page.querySelectorAll<HTMLTextAreaElement>("textarea.quiz-answer");
    first.value = "a";
    second.value = "b";
    page.querySelector("form")!.requestSubmit();
    await settle();

    expect(second.readOnly).toBe(false);
    expect(page.querySelectorAll(".quiz-verdict")[1].textContent).toContain("Not graded yet");

    page.querySelector("form")!.requestSubmit();
    await settle();
    const resent = calls.filter((c) => c.cmd === "grade_quiz")[1].args.answers as { question: { id: string } }[];
    expect(resent.map((a) => a.question.id)).toEqual(["q2"]);
  });

  it("says why when the quiz cannot be written", async () => {
    backend({
      generate_quiz: () => {
        throw new Error("this chapter is not written yet, so it has no quiz");
      },
    });
    const page = quizPage("later", "Later", () => {}, () => {});
    await settle();
    expect(page.querySelector(".generation-status")!.textContent).toContain("not written yet");
  });

  it("offers Try again after a failed generation and writes a new quiz", async () => {
    let attempts = 0;
    backend({
      generate_quiz: () => {
        attempts += 1;
        if (attempts === 1) throw new Error("the agent stopped");
        return contract.generate_quiz;
      },
    });
    const page = quizPage("moves", "Moves", () => {}, () => {});
    document.body.append(page);
    await settle();

    const retry = button(page, "Try again");
    expect(page.querySelector(".generation-status")!.textContent).toContain("could not be written");
    retry.click();
    await settle();

    expect(attempts).toBe(2);
    expect(page.querySelector(".quiz-prompt")!.textContent).toContain(contract.generate_quiz.questions[0].prompt);
    expect(buttonOrNull(page, "Try again")).toBeNull();
  });

  it("counts a finished quiz from its final grades", () => {
    expect(
      quizScore([
        { result: "correct", final: true },
        { result: "partial", final: true },
        { result: "incorrect", final: true },
        { result: "ungraded", final: false },
      ]),
    ).toEqual({ correct: 1, partial: 1, incorrect: 1, total: 3 });
    expect(scoreSummary({ correct: 1, partial: 1, incorrect: 1, total: 3 })).toBe(
      "Score: 1 correct, 1 partly right, 1 not quite, of 3.",
    );
  });

  it("offers new questions, the score and a way back to the outline when every answer is final", async () => {
    const second = {
      ...contract.generate_quiz,
      questions: [{ ...contract.generate_quiz.questions[0], id: "q9", prompt: "A second set of questions." }],
    };
    let generation = 0;
    backend({
      generate_quiz: () => {
        generation += 1;
        return generation === 1 ? contract.generate_quiz : second;
      },
      grade_quiz: () => contract.grade_quiz,
    });
    const onOutline = vi.fn();
    const page = quizPage("moves", "Moves", () => {}, () => {}, onOutline);
    document.body.append(page);
    await settle();

    page.querySelector<HTMLTextAreaElement>("textarea.quiz-answer")!.value = "because ownership moved";
    page.querySelector("form")!.requestSubmit();
    await settle();

    expect(page.querySelector(".quiz-score")!.textContent).toBe("Score: 1 correct, 0 partly right, 0 not quite, of 1.");
    expect(page.querySelector<HTMLButtonElement>("button.outline-submit")?.textContent).toBe("New questions");

    button(page, "Back to the outline").click();
    expect(onOutline).toHaveBeenCalledTimes(1);

    button(page, "New questions").click();
    await settle();
    expect(generation).toBe(2);
    expect(page.querySelector(".quiz-prompt")!.textContent).toContain("A second set of questions.");
    expect(page.querySelector(".quiz-score")).toBeNull();
  });

  it("restores a quiz without regenerating when given existing questions, typed answers, and grades", async () => {
    const generateFn = vi.fn(() => contract.generate_quiz);
    backend({ generate_quiz: generateFn });
    const onChange = vi.fn();
    const restored = {
      quiz: contract.generate_quiz,
      answers: { [contract.generate_quiz.questions[0].id]: "typed draft answer" },
      grades: [contract.grade_quiz[0] as Graded],
    };

    const page = quizPage("moves", "Moves", () => {}, () => {}, () => {}, restored, onChange);
    document.body.append(page);
    await settle();

    // Does not call generate_quiz since quiz was already restored
    expect(generateFn).not.toHaveBeenCalled();

    // Restores typed answer and grade verdict
    const answer = page.querySelector<HTMLTextAreaElement>("textarea.quiz-answer")!;
    expect(answer.value).toBe("typed draft answer");
    expect(page.querySelector(".quiz-verdict")!.textContent).toContain("Correct");
    expect(answer.readOnly).toBe(true);
  });

  it("reports draft answers on input to onChange callback", async () => {
    backend({ generate_quiz: () => contract.generate_quiz });
    const onChange = vi.fn();
    const page = quizPage("moves", "Moves", () => {}, () => {}, () => {}, undefined, onChange);
    document.body.append(page);
    await settle();

    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({
        quiz: contract.generate_quiz,
      }),
    );

    const answer = page.querySelector<HTMLTextAreaElement>("textarea.quiz-answer")!;
    answer.value = "my draft";
    answer.dispatchEvent(new Event("input"));

    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({
        answers: expect.objectContaining({ [contract.generate_quiz.questions[0].id]: "my draft" }),
      }),
    );
  });
});

function sourceCss(name: string): string {
  const path = [join(process.cwd(), "src", name), join(process.cwd(), "ui/src", name)].find((candidate) => existsSync(candidate));
  if (!path) throw new Error(`missing ${name}`);
  return readFileSync(path, "utf8");
}

describe("verdict and mastery colors", () => {
  const quizCss = sourceCss("quiz.css");
  const stylesCss = sourceCss("styles.css");
  const darkAt = stylesCss.indexOf("@media (prefers-color-scheme: dark)");

  it("uses theme tokens, with a warn color that changes in the dark appearance", () => {
    expect(quizCss).not.toMatch(/#448361|#cb912f/i);
    expect(quizCss).toMatch(/\.quiz-verdict\[data-result="correct"\][\s\S]*border-left-color:\s*var\(--ok-ink\)/);
    expect(quizCss).toMatch(/\.quiz-verdict\[data-result="partial"\][\s\S]*border-left-color:\s*var\(--warn-ink\)/);
    expect(quizCss).toMatch(/\.progress-bar span[\s\S]*background:\s*var\(--ok-ink\)/);
    expect(quizCss).toMatch(/\.progress-item\[data-due="true"\] \.progress-meta[\s\S]*color:\s*var\(--warn-ink\)/);
    const lightness = (css: string) => Number(css.match(/--warn-ink:\s*oklch\(([\d.]+)%/)?.[1]);
    const light = lightness(stylesCss.slice(0, darkAt));
    const dark = lightness(stylesCss.slice(darkAt));
    expect(light).toBeLessThan(55);
    expect(dark).toBeGreaterThan(70);
  });
});

function button(page: ParentNode, label: string): HTMLButtonElement {
  const found = buttonOrNull(page, label);
  if (!found) throw new Error(`no button ${label}`);
  return found;
}

function buttonOrNull(page: ParentNode, label: string): HTMLButtonElement | null {
  return [...page.querySelectorAll("button")].find((node) => node.textContent === label) ?? null;
}

describe("progress", () => {
  const day = 86_400;
  const base = { objective: "Explain moves", mastery: 0.5, attempts: 1, due: false };

  it("words when to review", () => {
    expect(reviewWords({ ...base, attempts: 0, nextReviewAt: null }, 0)).toBe("Not assessed yet");
    expect(reviewWords({ ...base, nextReviewAt: 0, due: true }, 10)).toBe("Review now");
    expect(reviewWords({ ...base, nextReviewAt: 3 * day }, 0)).toBe("Review in 3 days");
    expect(reviewWords({ ...base, nextReviewAt: day / 2 }, 0)).toBe("Review tomorrow");
  });

  it("refuses a malformed objective", () => {
    expect(parseProgress([{ ...base, nextReviewAt: null }])).toHaveLength(1);
    expect(() => parseProgress([{ ...base, nextReviewAt: "soon" }])).toThrow();
  });
});
