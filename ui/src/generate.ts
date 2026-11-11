import { Channel, invoke } from "@tauri-apps/api/core";
import { chapterDoc, type ReadyChapter } from "./api";
import "./generate.css";

// The typed bridge to the chapter generation commands in src-tauri/src/generate.rs,
// plus the words the reader shows while the agent writes. Planning the outline
// is outline-api.ts.

/** What the running job did last, as the Rust engine reports it. */
export type Progress =
  | { kind: "started" }
  | { kind: "tool"; name: string }
  | { kind: "text"; chars: number };

/** Writes the chapter if it is a placeholder; joins the job if one is running. */
export function openChapter(
  courseId: string,
  chapterId: string,
  onProgress: (p: Progress) => void,
): Promise<ReadyChapter> {
  return runChapterJob("open_chapter", courseId, chapterId, onProgress);
}

/**
 * Writes the chapter again, replacing it only if the new job succeeds.
 * `instruction` is the learner's request for the rewrite; without one the
 * rewrite is plain.
 */
export function regenerateChapter(
  courseId: string,
  chapterId: string,
  onProgress: (p: Progress) => void,
  instruction?: string,
): Promise<ReadyChapter> {
  return runChapterJob("regenerate_chapter", courseId, chapterId, onProgress, instruction);
}

async function runChapterJob(
  cmd: "open_chapter" | "regenerate_chapter",
  courseId: string,
  chapterId: string,
  onProgress: (p: Progress) => void,
  instruction?: string,
): Promise<ReadyChapter> {
  const channel = new Channel<unknown>((raw) => {
    const p = progress(raw);
    if (p) onProgress(p);
  });
  const args = { courseId, chapterId, onProgress: channel, ...(instruction ? { instruction } : {}) };
  const doc = chapterDoc(await invoke<unknown>(cmd, args), cmd);
  if (doc.state !== "ready") throw new Error(`${cmd}: the chapter is still a placeholder`);
  return doc;
}

/** A progress event, or `undefined` for a shape this build does not know. */
function progress(raw: unknown): Progress | undefined {
  if (typeof raw !== "object" || raw === null) return undefined;
  const r = raw as Record<string, unknown>;
  if (r.kind === "started") return { kind: "started" };
  if (r.kind === "tool" && typeof r.name === "string") return { kind: "tool", name: r.name };
  if (r.kind === "text" && typeof r.chars === "number") return { kind: "text", chars: r.chars };
  return undefined;
}

const TOOL_WORDS: Record<string, string> = {
  Grep: "Searching the library",
  Glob: "Looking through the library",
  Read: "Reading a source",
  Write: "Writing the chapter",
  Edit: "Revising the chapter",
  WebFetch: "Reading a web page",
  WebSearch: "Searching the web",
};

/** One line for the status under the chapter title. */
export function describeProgress(p: Progress): string {
  switch (p.kind) {
    case "started":
      return "The agent has started.";
    case "tool":
      return `${TOOL_WORDS[p.name] ?? `Using ${p.name}`}…`;
    case "text":
      return "Drafting…";
  }
}
