import type { Outline, OutlineChapter, OutlineStatus } from "./outline-api";

// The review screen's edits as data. A draft is the outline the user is
// shaping; `edit` is the only way it changes, and it is a pure function, so the
// rules (a written chapter stays, a move stays in range) live here and not in
// the DOM handlers. The screen sends `toOutline(draft)` to save_outline.

/** A chapter row. `key` identifies the row on screen, whether or not the backend has given it an id yet. */
export interface DraftChapter extends OutlineChapter {
  key: string;
}

export interface OutlineDraft {
  status: OutlineStatus;
  title: string;
  objectives: string[];
  chapters: DraftChapter[];
  /** Counts the chapters added in this draft, so each gets a key of its own. */
  added: number;
}

export type DraftEdit =
  | { kind: "title"; key: string; text: string }
  | { kind: "summary"; key: string; text: string }
  | { kind: "comment"; key: string; text: string }
  | { kind: "move"; key: string; to: number }
  | { kind: "add-chapter" }
  | { kind: "remove-chapter"; key: string }
  | { kind: "objective"; index: number; text: string }
  | { kind: "add-objective" }
  | { kind: "remove-objective"; index: number };

export interface EditResult {
  draft: OutlineDraft;
  /** Why the edit was not made, when it was refused. */
  refused: string | null;
}

export function draftFromOutline(outline: Outline): OutlineDraft {
  return {
    status: outline.status,
    title: outline.title,
    objectives: [...outline.objectives],
    chapters: outline.chapters.map((chapter, i) => ({ ...chapter, key: chapter.id ?? `loaded-${i}` })),
    added: 0,
  };
}

export function toOutline(draft: OutlineDraft): Outline {
  return {
    status: draft.status,
    title: draft.title,
    objectives: draft.objectives.map((o) => o.trim()).filter((o) => o !== ""),
    chapters: draft.chapters.map(({ key: _key, ...chapter }) => chapter),
  };
}

/** Why a chapter cannot be removed, or null. The backend refuses it too. */
export function removalBlock(chapter: OutlineChapter): string | null {
  return chapter.written ? `"${chapter.title}" is already written, so it cannot be removed.` : null;
}

export function hasComments(draft: OutlineDraft): boolean {
  return draft.chapters.some((c) => c.comment.trim() !== "");
}

/** Why the draft cannot be saved yet, or null. */
export function saveBlock(draft: OutlineDraft): string | null {
  const untitled = draft.chapters.findIndex((c) => c.title.trim() === "");
  return untitled === -1 ? null : `Chapter ${untitled + 1} needs a title.`;
}

/** Why the outline cannot be approved yet, or null. */
export function approvalBlock(draft: OutlineDraft): string | null {
  return draft.chapters.length === 0 ? "Add at least one chapter first." : saveBlock(draft);
}

export function isDirty(draft: OutlineDraft, saved: Outline): boolean {
  return JSON.stringify(toOutline(draft)) !== JSON.stringify(toOutline(draftFromOutline(saved)));
}

function patch(draft: OutlineDraft, key: string, change: Partial<OutlineChapter>): OutlineDraft {
  return { ...draft, chapters: draft.chapters.map((c) => (c.key === key ? { ...c, ...change } : c)) };
}

function replaceAt<T>(items: T[], index: number, value: T): T[] {
  return items.map((item, i) => (i === index ? value : item));
}

export function edit(draft: OutlineDraft, change: DraftEdit): EditResult {
  const done = (next: OutlineDraft): EditResult => ({ draft: next, refused: null });
  switch (change.kind) {
    case "title":
      return done(patch(draft, change.key, { title: change.text }));
    case "summary":
      return done(patch(draft, change.key, { summary: change.text }));
    case "comment":
      return done(patch(draft, change.key, { comment: change.text }));
    case "move": {
      const from = draft.chapters.findIndex((c) => c.key === change.key);
      if (from === -1) return done(draft);
      const to = Math.max(0, Math.min(draft.chapters.length - 1, change.to));
      const chapters = [...draft.chapters];
      const [moved] = chapters.splice(from, 1);
      chapters.splice(to, 0, moved);
      return done({ ...draft, chapters });
    }
    case "add-chapter":
      return done({
        ...draft,
        added: draft.added + 1,
        chapters: [
          ...draft.chapters,
          { key: `added-${draft.added + 1}`, id: null, title: "", summary: "", sources: [], comment: "", written: false },
        ],
      });
    case "remove-chapter": {
      const target = draft.chapters.find((c) => c.key === change.key);
      if (!target) return done(draft);
      const refused = removalBlock(target);
      if (refused) return { draft, refused };
      return done({ ...draft, chapters: draft.chapters.filter((c) => c.key !== change.key) });
    }
    case "objective":
      return done({ ...draft, objectives: replaceAt(draft.objectives, change.index, change.text) });
    case "add-objective":
      return done({ ...draft, objectives: [...draft.objectives, ""] });
    case "remove-objective":
      return done({ ...draft, objectives: draft.objectives.filter((_, i) => i !== change.index) });
  }
}
