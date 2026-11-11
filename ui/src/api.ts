import { invoke } from "@tauri-apps/api/core";
import type { PdfWord, Rect } from "./pdf-geometry";

// The typed bridge to the Rust commands in src-tauri/src/reader.rs. Every
// response is checked on arrival, because the shapes cross a process boundary.

export interface CourseSummary {
  id: string;
  title: string;
  objectives: string[];
}

export interface ChapterSummary {
  id: string;
  title: string;
  placeholder: boolean;
}

export type ChapterDoc = { id: string; title: string } & (
  | { state: "ready"; markdown: string }
  | { state: "placeholder" }
);

export type ReadyChapter = Extract<ChapterDoc, { state: "ready" }>;

export async function listCourses(): Promise<CourseSummary[]> {
  const raw = await invoke<unknown>("list_courses");
  return list(raw, "list_courses").map((item) => {
    const r = record(item, "course");
    return { id: str(r, "id"), title: str(r, "title"), objectives: strings(r, "objectives") };
  });
}

export async function listChapters(courseId: string): Promise<ChapterSummary[]> {
  const raw = await invoke<unknown>("list_chapters", { courseId });
  return list(raw, "list_chapters").map((item) => {
    const r = record(item, "chapter");
    return { id: str(r, "id"), title: str(r, "title"), placeholder: bool(r, "placeholder") };
  });
}

export async function readChapter(courseId: string, chapterId: string): Promise<ChapterDoc> {
  return chapterDoc(await invoke<unknown>("read_chapter", { courseId, chapterId }), "read_chapter");
}

export function chapterDoc(raw: unknown, what: string): ChapterDoc {
  const r = record(raw, what);
  const head = { id: str(r, "id"), title: str(r, "title") };
  switch (r.state) {
    case "ready":
      return { ...head, state: "ready", markdown: str(r, "markdown") };
    case "placeholder":
      return { ...head, state: "placeholder" };
    default:
      throw new Error(`${what}: unknown chapter state ${JSON.stringify(r.state)}`);
  }
}

export function record(value: unknown, what: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(`${what}: expected an object`);
  }
  return value as Record<string, unknown>;
}

export function list(value: unknown, what: string): unknown[] {
  if (!Array.isArray(value)) throw new Error(`${what}: expected a list`);
  return value;
}

export function str(r: Record<string, unknown>, key: string): string {
  const value = r[key];
  if (typeof value !== "string") throw new Error(`expected "${key}" to be a string`);
  return value;
}

export function bool(r: Record<string, unknown>, key: string): boolean {
  const value = r[key];
  if (typeof value !== "boolean") throw new Error(`expected "${key}" to be a boolean`);
  return value;
}

export function strOrNull(r: Record<string, unknown>, key: string): string | null {
  const value = r[key];
  if (value === null) return null;
  if (typeof value !== "string") throw new Error(`expected "${key}" to be a string or null`);
  return value;
}

export function numOrNull(r: Record<string, unknown>, key: string): number | null {
  const value = r[key];
  if (value === null) return null;
  if (typeof value !== "number") throw new Error(`expected "${key}" to be a number or null`);
  return value;
}

/** A string field that must be one of `allowed`, so a variant this build does not know is refused. */
export function oneOf<T extends string>(r: Record<string, unknown>, key: string, allowed: readonly T[]): T {
  const value = str(r, key);
  if (!(allowed as readonly string[]).includes(value)) throw new Error(`unknown ${key} ${JSON.stringify(value)}`);
  return value as T;
}

export function strings(r: Record<string, unknown>, key: string): string[] {
  const value = r[key];
  if (!Array.isArray(value) || value.some((v) => typeof v !== "string")) {
    throw new Error(`expected "${key}" to be a list of strings`);
  }
  return value as string[];
}

// PDF sources, from src-tauri/src/pdf_sources.rs.

export interface PdfPage {
  page: number;
  width: number;
  height: number;
  text: string;
  /** Set when some text on the page is known to sit at the wrong place. */
  caveat: "transformedFormText" | null;
  words: PdfWord[];
}

export interface PdfSource {
  name: string;
  /** Ticket 03's ratio of glyph box height to em, from the Rust constant. */
  glyphHeightFactor: number;
  pages: PdfPage[];
}

export async function readPdfSource(courseId: string, name: string): Promise<PdfSource> {
  const r = record(await invoke<unknown>("read_pdf_source", { courseId, name }), "read_pdf_source");
  return {
    name: str(r, "name"),
    glyphHeightFactor: number(r, "glyphHeightFactor"),
    pages: list(r.pages, "pages").map((item) => {
      const p = record(item, "page");
      const caveat = p.caveat;
      if (caveat !== null && caveat !== "transformedFormText") {
        throw new Error(`read_pdf_source: unknown caveat ${JSON.stringify(caveat)}`);
      }
      return {
        page: number(p, "page"),
        width: number(p, "width"),
        height: number(p, "height"),
        text: str(p, "text"),
        caveat,
        words: list(p.words, "words").map((w) => {
          const word = record(w, "word");
          return { quote: str(word, "quote"), rect: parseRect(word.rect) };
        }),
      };
    }),
  };
}

export function parseRect(value: unknown): Rect {
  const r = record(value, "rect");
  return { x: number(r, "x"), y: number(r, "y"), w: number(r, "w"), h: number(r, "h") };
}

export function number(r: Record<string, unknown>, key: string): number {
  const value = r[key];
  if (typeof value !== "number") throw new Error(`expected "${key}" to be a number`);
  return value;
}
