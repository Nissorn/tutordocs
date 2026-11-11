import { invoke } from "@tauri-apps/api/core";
import { bool, list, number, oneOf, record, str, strings, strOrNull } from "./api";
import { markCourseOpened } from "./courses-api";

// The typed bridge to the outline commands in notes/31-contract.md (ticket 31).
// Every response is parsed here into the contract's types.

export const OUTLINE_STATUSES = ["draft", "approved"] as const;
export type OutlineStatus = (typeof OUTLINE_STATUSES)[number];

export interface OutlineChapter {
  /** Null for a chapter the user added. The backend assigns an id on save. */
  id: string | null;
  title: string;
  summary: string;
  /** Source file names this chapter draws on. */
  sources: string[];
  /** The user's note to the AI, "" when none. */
  comment: string;
  /** True when the chapter text exists. The UI never changes it. */
  written: boolean;
}

export interface Outline {
  status: OutlineStatus;
  title: string;
  objectives: string[];
  chapters: OutlineChapter[];
}

export interface VariantMeta {
  id: string;
  label: string;
  parentId: string | null;
  createdAt: string;
  chapterCount: number;
  writtenChapterCount: number;
  current: boolean;
}

export interface OutlineVariant {
  id: string;
  label: string;
  parentId: string | null;
  createdAt: string;
  objectives: string[];
  chapters: OutlineChapter[];
}

/** What `approve_outline` does about writing: each chapter on open, or every unwritten chapter now. */
export type ApprovalChoice = "on_open" | "all";

export interface ChapterQueue {
  running: string | null;
  pending: string[];
}

export function parseChapterQueue(raw: unknown): ChapterQueue {
  const r = record(raw, "chapter queue");
  return {
    running: strOrNull(r, "running"),
    pending: strings(r, "pending"),
  };
}

function parseChapter(raw: unknown): OutlineChapter {
  const r = record(raw, "outline chapter");
  return {
    id: strOrNull(r, "id"),
    title: str(r, "title"),
    summary: str(r, "summary"),
    sources: strings(r, "sources"),
    comment: str(r, "comment"),
    written: bool(r, "written"),
  };
}

export function parseOutline(raw: unknown): Outline {
  const r = record(raw, "outline");
  return {
    status: oneOf(r, "status", OUTLINE_STATUSES),
    title: str(r, "title"),
    objectives: strings(r, "objectives"),
    chapters: list(r.chapters, "outline chapters").map(parseChapter),
  };
}

export function parseVariantMeta(raw: unknown): VariantMeta {
  const r = record(raw, "variant meta");
  return {
    id: str(r, "id"),
    label: str(r, "label"),
    parentId: strOrNull(r, "parentId"),
    createdAt: str(r, "createdAt"),
    chapterCount: number(r, "chapterCount"),
    writtenChapterCount: number(r, "writtenChapterCount"),
    current: bool(r, "current"),
  };
}

export function parseOutlineVariant(raw: unknown): OutlineVariant {
  const r = record(raw, "outline variant");
  return {
    id: str(r, "id"),
    label: str(r, "label"),
    parentId: strOrNull(r, "parentId"),
    createdAt: str(r, "createdAt"),
    objectives: strings(r, "objectives"),
    chapters: list(r.chapters, "variant chapters").map(parseChapter),
  };
}

export async function getOutline(courseId: string): Promise<Outline> {
  return parseOutline(await invoke<unknown>("get_outline", { courseId }));
}

export async function listOutlineVariants(courseId: string): Promise<VariantMeta[]> {
  const raw = await invoke<unknown>("list_outline_variants", { courseId });
  return list(raw, "list_outline_variants").map(parseVariantMeta);
}

export async function getOutlineVariant(courseId: string, variantId: string): Promise<OutlineVariant> {
  return parseOutlineVariant(await invoke<unknown>("get_outline_variant", { courseId, variantId }));
}

export async function branchOutline(courseId: string, fromVariantId: string, label: string): Promise<OutlineVariant> {
  return parseOutlineVariant(await invoke<unknown>("branch_outline", { courseId, fromVariantId, label }));
}

export async function setCurrentVariant(courseId: string, variantId: string): Promise<OutlineVariant> {
  return parseOutlineVariant(await invoke<unknown>("set_current_variant", { courseId, variantId }));
}

/** Refused for removing a written chapter or naming an id the course does not have. */
export async function saveOutline(courseId: string, outline: Outline, variantId?: string): Promise<Outline> {
  const args: Record<string, unknown> = { courseId, outline };
  if (variantId !== undefined) args.variantId = variantId;
  return parseOutline(await invoke<unknown>("save_outline", args));
}

/** Runs an agent job that applies the saved comments and clears them. */
export async function reviseOutline(courseId: string, variantId?: string): Promise<Outline> {
  const args: Record<string, unknown> = { courseId };
  if (variantId !== undefined) args.variantId = variantId;
  return parseOutline(await invoke<unknown>("revise_outline", args));
}

export async function approveOutline(courseId: string, generate: ApprovalChoice, variantId?: string): Promise<Outline> {
  const args: Record<string, unknown> = { courseId, generate };
  if (variantId !== undefined) args.variantId = variantId;
  return parseOutline(await invoke<unknown>("approve_outline", args));
}

export async function cancelChapterQueue(courseId: string): Promise<void> {
  await invoke("cancel_chapter_queue", { courseId });
}

export async function getChapterQueue(courseId: string): Promise<ChapterQueue> {
  return parseChapterQueue(await invoke<unknown>("get_chapter_queue", { courseId }));
}

/**
 * Has the agent plan the course and write a draft outline. The response is not
 * read: the draft is read back with `getOutline`.
 */
export async function planOutline(courseId: string, topic: string, sources: string[]): Promise<void> {
  await markCourseOpened(courseId);
  await invoke("generate_outline", { courseId, topic, sources });
}

/** The backend's reason when a chapter is opened before the outline is approved. */
export function isDraftRefusal(error: unknown): boolean {
  return (error instanceof Error ? error.message : String(error)).includes("outline is a draft");
}
