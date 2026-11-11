import { invoke } from "@tauri-apps/api/core";
import { chapterDoc, list, number, record, str, strOrNull, strings, type ReadyChapter } from "./api";

// The typed bridge to the section revision and question commands of
// notes/31-contract.md (built by ticket 34). Every reply is parsed here, so
// nothing past this file holds a raw payload.

export interface Answer {
  id: number;
  /** The passage the question was about, or null for the whole chapter. */
  quote: string | null;
  question: string;
  /** Markdown written by the agent. Render it with renderMarkdown and nothing else. */
  answer: string;
  /** File names of the course sources the answer cited. */
  sources: string[];
  askedAt: string;
}

export interface ChapterContext {
  objectives: string[];
  summary: string;
  /** The outline's sources for the chapter plus any `sources/<file>` its text names. */
  sources: string[];
}

function parseAnswer(raw: unknown): Answer {
  const r = record(raw, "answer");
  return {
    id: number(r, "id"),
    quote: strOrNull(r, "quote"),
    question: str(r, "question"),
    answer: str(r, "answer"),
    sources: strings(r, "sources"),
    askedAt: str(r, "askedAt"),
  };
}

export async function chapterContext(courseId: string, chapterId: string): Promise<ChapterContext> {
  const r = record(await invoke<unknown>("chapter_context", { courseId, chapterId }), "chapter_context");
  return { objectives: strings(r, "objectives"), summary: str(r, "summary"), sources: strings(r, "sources") };
}

/** Newest first, as Rust sends them. */
export async function listAnswers(courseId: string, chapterId: string): Promise<Answer[]> {
  return list(await invoke<unknown>("list_answers", { courseId, chapterId }), "list_answers").map(parseAnswer);
}

/** Runs a read-only agent job of kind `ask`. A null quote asks about the whole chapter. */
export async function askAbout(
  courseId: string,
  chapterId: string,
  quote: string | null,
  question: string,
): Promise<Answer> {
  return parseAnswer(await invoke<unknown>("ask_about", { courseId, chapterId, quote, question }));
}

/** Runs an agent job of kind `rewrite` on the section holding `quote`. Resolves to the chapter as revised. */
export async function reviseSection(
  courseId: string,
  chapterId: string,
  quote: string,
  instruction: string,
): Promise<ReadyChapter> {
  const doc = chapterDoc(
    await invoke<unknown>("revise_section", { courseId, chapterId, quote, instruction }),
    "revise_section",
  );
  if (doc.state !== "ready") throw new Error("revise_section: the chapter came back as a placeholder");
  return doc;
}
