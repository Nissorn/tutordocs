import { invoke } from "@tauri-apps/api/core";
import { record, str, strings } from "./api";
import "./quiz.css";

// The typed bridge to the quiz commands in src-tauri/src/quiz.rs, and the quiz
// page: free-text answers, graded by the agent, recorded per objective.

export interface Question {
  id: string;
  objective: string;
  prompt: string;
  referenceAnswer: string;
  combines: string[];
}

export interface Quiz {
  chapterId: string;
  questions: Question[];
}

export type Outcome = "correct" | "partial" | "incorrect" | "ungraded";

export interface Graded {
  questionId: string;
  objective: string;
  result: Outcome;
  final: boolean;
  confidence: number;
  feedback: string;
}

const OUTCOMES: readonly Outcome[] = ["correct", "partial", "incorrect", "ungraded"];

function list(raw: unknown, what: string): unknown[] {
  if (!Array.isArray(raw)) throw new Error(`${what}: expected a list`);
  return raw;
}

export function parseQuiz(raw: unknown): Quiz {
  const r = record(raw, "generate_quiz");
  return {
    chapterId: str(r, "chapterId"),
    questions: list(r.questions, "questions").map((item) => {
      const q = record(item, "question");
      return {
        id: str(q, "id"),
        objective: str(q, "objective"),
        prompt: str(q, "prompt"),
        referenceAnswer: str(q, "referenceAnswer"),
        combines: strings(q, "combines"),
      };
    }),
  };
}

export function parseGraded(raw: unknown): Graded[] {
  return list(raw, "grade_quiz").map((item) => {
    const g = record(item, "graded answer");
    const result = g.result;
    if (!OUTCOMES.includes(result as Outcome)) throw new Error(`grade_quiz: unknown result ${String(result)}`);
    if (typeof g.final !== "boolean" || typeof g.confidence !== "number") {
      throw new Error("grade_quiz: malformed grade");
    }
    return {
      questionId: str(g, "questionId"),
      objective: str(g, "objective"),
      result: result as Outcome,
      final: g.final,
      confidence: g.confidence,
      feedback: str(g, "feedback"),
    };
  });
}

export async function generateQuiz(chapterId: string): Promise<Quiz> {
  return parseQuiz(await invoke<unknown>("generate_quiz", { chapterId }));
}

export async function gradeQuiz(chapterId: string, answers: { question: Question; answer: string }[]): Promise<Graded[]> {
  return parseGraded(await invoke<unknown>("grade_quiz", { chapterId, answers }));
}

const VERDICT_WORDS: Record<Outcome, string> = {
  correct: "Correct",
  partial: "Partly right",
  incorrect: "Not quite",
  ungraded: "Not graded yet",
};

/** Final grades on one quiz. Counted from parsed grades, not a new wire field. */
export interface QuizScore {
  correct: number;
  partial: number;
  incorrect: number;
  total: number;
}

export function quizScore(grades: readonly Pick<Graded, "result" | "final">[]): QuizScore {
  const finished = grades.filter((grade) => grade.final);
  return {
    correct: finished.filter((grade) => grade.result === "correct").length,
    partial: finished.filter((grade) => grade.result === "partial").length,
    incorrect: finished.filter((grade) => grade.result === "incorrect").length,
    total: finished.length,
  };
}

export function scoreSummary(score: QuizScore): string {
  return `Score: ${score.correct} correct, ${score.partial} partly right, ${score.incorrect} not quite, of ${score.total}.`;
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string, text?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export interface QuizTabState {
  chapterId?: string;
  quiz?: Quiz;
  answers?: Record<string, string>;
  grades?: Graded[];
}

/**
 * The quiz page for one written chapter. It asks the agent for questions,
 * takes free-text answers, and shows each grade with its feedback. An answer
 * the grader could not judge stays open to answer again. `onGraded` runs after
 * every grading that recorded something, so progress can refresh.
 */
export function quizPage(
  chapterId: string,
  title: string,
  onBack: () => void,
  onGraded: () => void,
  onOutline: () => void = () => {},
  restore?: QuizTabState,
  onChange?: (state: QuizTabState) => void,
): HTMLElement {
  const page = el("section", "quiz");
  const back = el("button", "quiet-button", "Back to the chapter");
  back.type = "button";
  back.addEventListener("click", onBack);
  const status = el("p", "generation-status");
  status.setAttribute("role", "status");
  page.append(back, el("h1", undefined, `Quiz: ${title}`), status);

  let generation = 0;
  let activeQuiz: Quiz | undefined = restore?.quiz;
  let recordedGrades: Graded[] = restore?.grades ? [...restore.grades] : [];

  function emitChange(): void {
    if (!onChange || !activeQuiz) return;
    const answers: Record<string, string> = {};
    const fieldsets = page.querySelectorAll<HTMLElement>("fieldset.quiz-question");
    for (const fs of fieldsets) {
      const qid = fs.dataset.questionId;
      const val = fs.querySelector<HTMLTextAreaElement>("textarea.quiz-answer")?.value ?? "";
      if (qid) answers[qid] = val;
    }
    onChange({
      quiz: activeQuiz,
      answers,
      grades: recordedGrades,
    });
  }

  function load(): void {
    const mine = ++generation;
    recordedGrades = [];
    activeQuiz = undefined;
    page.querySelector(".quiz-form")?.remove();
    page.querySelector(".quiz-retry")?.remove();
    status.className = "generation-status";
    status.textContent = "The agent is writing questions that make you connect ideas…";
    generateQuiz(chapterId).then(
      (quiz) => {
        if (mine !== generation) return;
        activeQuiz = quiz;
        status.textContent = "Answer in your own words. The agent grades the reasoning, not the keywords.";
        page.append(form(quiz));
        emitChange();
      },
      (error: unknown) => {
        if (mine !== generation) return;
        status.className = "generation-status error";
        status.textContent = `The quiz could not be written: ${message(error)}`;
        const retry = el("button", "quiet-button quiz-retry", "Try again");
        retry.type = "button";
        retry.addEventListener("click", () => load());
        page.append(retry);
      },
    );
  }

  function form(quiz: Quiz, initialAnswers?: Record<string, string>, initialGrades?: Graded[]): HTMLFormElement {
    const f = el("form", "quiz-form");
    const fields = quiz.questions.map((question, n) => {
      const item = el("fieldset", "quiz-question");
      item.dataset.questionId = question.id;
      item.append(el("legend", "quiz-objective", question.objective));
      item.append(el("p", "quiz-prompt", `${n + 1}. ${question.prompt}`));
      const answer = el("textarea", "quiz-answer");
      answer.rows = 4;
      answer.setAttribute("aria-label", `Answer to question ${n + 1}`);
      if (initialAnswers && initialAnswers[question.id] !== undefined) {
        answer.value = initialAnswers[question.id];
      }
      answer.addEventListener("input", () => emitChange());

      const verdict = el("div", "quiz-verdict");
      verdict.setAttribute("aria-live", "polite");
      item.append(answer, verdict);
      f.append(item);
      return { question, answer, verdict, done: false, result: "ungraded" as Outcome };
    });

    if (initialGrades) {
      for (const grade of initialGrades) {
        const field = fields.find((x) => x.question.id === grade.questionId);
        if (!field) continue;
        field.done = grade.final;
        field.result = grade.result;
        field.answer.readOnly = grade.final;
        field.verdict.dataset.result = grade.result;
        field.verdict.replaceChildren(
          el("strong", undefined, VERDICT_WORDS[grade.result]),
          el("span", undefined, ` ${grade.feedback}`),
        );
        if (grade.final) {
          const reference = el("details", "quiz-reference");
          reference.append(el("summary", undefined, "A full answer"), el("p", undefined, field.question.referenceAnswer));
          field.verdict.append(reference);
        }
      }
    }

    const submit = el("button", "outline-submit", "Check my answers");
    submit.type = "submit";
    const formStatus = el("p", "generation-status");
    formStatus.setAttribute("role", "status");
    f.append(submit, formStatus);

    const left = fields.filter((field) => !field.done).length;
    if (left === 0 && fields.length > 0) {
      submit.remove();
      formStatus.className = "quiz-score";
      formStatus.textContent = scoreSummary(quizScore(fields.map((field) => ({ result: field.result, final: field.done }))));
      const again = el("button", "outline-submit", "New questions");
      again.type = "button";
      again.addEventListener("click", () => load());
      const outline = el("button", "quiet-button", "Back to the outline");
      outline.type = "button";
      outline.addEventListener("click", onOutline);
      f.append(again, outline);
    }

    f.addEventListener("submit", (event) => {
      event.preventDefault();
      const open = fields.filter((field) => !field.done);
      if (open.length === 0) return;
      submit.disabled = true;
      formStatus.className = "generation-status";
      formStatus.textContent = "The agent is grading…";
      gradeQuiz(
        chapterId,
        open.map((field) => ({ question: field.question, answer: field.answer.value })),
      ).then(
        (grades) => {
          for (const grade of grades) {
            const field = open.find((x) => x.question.id === grade.questionId);
            if (!field) continue;
            field.done = grade.final;
            field.result = grade.result;
            field.answer.readOnly = grade.final;
            field.verdict.dataset.result = grade.result;
            field.verdict.replaceChildren(
              el("strong", undefined, VERDICT_WORDS[grade.result]),
              el("span", undefined, ` ${grade.feedback}`),
            );
            if (grade.final) {
              const reference = el("details", "quiz-reference");
              reference.append(el("summary", undefined, "A full answer"), el("p", undefined, field.question.referenceAnswer));
              field.verdict.append(reference);
            }
          }

          for (const g of grades) {
            const idx = recordedGrades.findIndex((x) => x.questionId === g.questionId);
            if (idx >= 0) recordedGrades[idx] = g;
            else recordedGrades.push(g);
          }
          emitChange();

          const left = fields.filter((field) => !field.done).length;
          if (left === 0) {
            submit.remove();
            formStatus.className = "quiz-score";
            formStatus.textContent = scoreSummary(quizScore(fields.map((field) => ({ result: field.result, final: field.done }))));
            const again = el("button", "outline-submit", "New questions");
            again.type = "button";
            again.addEventListener("click", () => load());
            const outline = el("button", "quiet-button", "Back to the outline");
            outline.type = "button";
            outline.addEventListener("click", onOutline);
            f.append(again, outline);
          } else {
            submit.disabled = false;
            formStatus.className = "generation-status";
            formStatus.textContent = "Answer the open questions again, then check.";
          }
          if (grades.some((grade) => grade.final)) onGraded();
        },
        (error: unknown) => {
          submit.disabled = false;
          formStatus.className = "generation-status error";
          formStatus.textContent = `Your answers could not be graded: ${message(error)}`;
        },
      );
    });
    return f;
  }

  if (restore?.quiz) {
    status.textContent = "Answer in your own words. The agent grades the reasoning, not the keywords.";
    page.append(form(restore.quiz, restore.answers, restore.grades));
    emitChange();
  } else {
    load();
  }

  return page;
}
