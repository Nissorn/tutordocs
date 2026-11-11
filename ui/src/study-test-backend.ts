import { mockIPC } from "@tauri-apps/api/mocks";
import contract from "./fixtures/reader-contract.json";
import study from "./fixtures/study-contract.json";
import type { PrefStorage } from "./study-prefs";

// A stand-in for the Rust side of one course, for the tests that mount the
// reader: the reader contract plus the commands of notes/31-contract.md.

export type Handler = (args: Record<string, unknown>) => unknown;

export interface Backend {
  calls: { cmd: string; args: Record<string, unknown> }[];
  sent(cmd: string): Record<string, unknown>[];
}

/** Every command answers from the contract. An override replaces one command's handler. */
export function backend(overrides: Partial<Record<string, Handler>> = {}): Backend {
  const calls: Backend["calls"] = [];
  const defaults: Record<string, Handler> = {
    list_chapters: () => contract.list_chapters.map((c) => ({ ...c, placeholder: false })),
    read_chapter: (a) => ({
      ...contract.read_chapter_ready,
      id: String(a.chapterId),
      title: a.chapterId === "cells" ? "Cells" : "Membranes",
    }),
    list_annotations: () => [],
    list_course_sources: () => [
      { name: "cell-notes.pdf", path: "/library/sources/cell-notes.pdf", kind: "pdf", url: null, fetchedAt: null, bytes: 1 },
      { name: "syllabus.txt", path: "/library/sources/syllabus.txt", kind: "text", url: null, fetchedAt: null, bytes: 1 },
    ],
    chapter_context: () => study.chapter_context,
    list_answers: () => study.answers,
    ask_about: () => study.ask_about_whole_chapter,
    revise_section: () => study.revise_section,
  };
  mockIPC((cmd, raw) => {
    const args = (raw ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args });
    const handler = overrides[cmd] ?? defaults[cmd];
    if (!handler) throw new Error(`unexpected command ${cmd}`);
    return handler(args);
  });
  return { calls, sent: (cmd) => calls.filter((c) => c.cmd === cmd).map((c) => c.args) };
}

export function memoryStorage(initial: Record<string, string> = {}): PrefStorage & { held: Map<string, string> } {
  const held = new Map(Object.entries(initial));
  return { held, getItem: (k) => held.get(k) ?? null, setItem: (k, v) => void held.set(k, v) };
}
