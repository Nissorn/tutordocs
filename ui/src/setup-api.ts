import { invoke } from "@tauri-apps/api/core";
import { list, numOrNull, oneOf, record, str, strOrNull, number } from "./api";

// The typed bridge to the course setup, template and import commands in
// notes/31-contract.md (ticket 33). Every response is parsed here into the
// contract's types, so nothing past this file re-checks a shape.

export const LEVELS = ["new", "some", "strong"] as const;
export type Level = (typeof LEVELS)[number];
export const LANGUAGES = ["th", "en"] as const;
export type Language = (typeof LANGUAGES)[number];

export interface CourseSetup {
  /** A template id from `listCourseTemplates`. */
  template: string;
  level: Level;
  hoursPerWeek: number | null;
  language: Language;
  /** The user's free text, "" when none. */
  guide: string;
}

export interface Template {
  id: string;
  title: string;
  description: string;
}

const SOURCE_KINDS = ["pdf", "markdown", "text", "url", "other"] as const;
export type SourceKind = (typeof SOURCE_KINDS)[number];

/** A file in the course's `sources/`. The contract calls it `Source`; `sources-api.ts` already owns that name. */
export interface CourseSource {
  name: string;
  path: string;
  kind: SourceKind;
  url: string | null;
  fetchedAt: string | null;
  bytes: number;
}

/** An imported file always names its source, and a skipped one has no source to name. */
export type ImportResult =
  | { path: string; status: "imported"; source: CourseSource }
  | { path: string; status: "skipped"; reason: string | null };

function parseTemplate(raw: unknown): Template {
  const r = record(raw, "template");
  return { id: str(r, "id"), title: str(r, "title"), description: str(r, "description") };
}

function parseSetup(raw: unknown): CourseSetup {
  const r = record(raw, "course setup");
  return {
    template: str(r, "template"),
    level: oneOf(r, "level", LEVELS),
    hoursPerWeek: numOrNull(r, "hoursPerWeek"),
    language: oneOf(r, "language", LANGUAGES),
    guide: str(r, "guide"),
  };
}

function parseSource(raw: unknown): CourseSource {
  const r = record(raw, "source");
  return {
    name: str(r, "name"),
    path: str(r, "path"),
    kind: oneOf(r, "kind", SOURCE_KINDS),
    url: strOrNull(r, "url"),
    fetchedAt: strOrNull(r, "fetchedAt"),
    bytes: number(r, "bytes"),
  };
}

function parseImportResult(raw: unknown): ImportResult {
  const r = record(raw, "import result");
  const path = str(r, "path");
  const status = oneOf(r, "status", ["imported", "skipped"] as const);
  if (status === "skipped") return { path, status, reason: strOrNull(r, "reason") };
  if (r.source === null || r.source === undefined) throw new Error(`import_files: ${path} was imported without a source`);
  return { path, status, source: parseSource(r.source) };
}

export async function listCourseTemplates(): Promise<Template[]> {
  return list(await invoke<unknown>("list_course_templates"), "list_course_templates").map(parseTemplate);
}

/** The saved setup, or null for a course made without one. */
export async function getCourseSetup(courseId: string): Promise<CourseSetup | null> {
  const raw = await invoke<unknown>("get_course_setup", { courseId });
  return raw === null ? null : parseSetup(raw);
}

export async function setCourseSetup(courseId: string, setup: CourseSetup): Promise<CourseSetup> {
  return parseSetup(await invoke<unknown>("set_course_setup", { courseId, setup }));
}

/** `paths` are absolute: a file, or a folder the backend walks. */
export async function importFiles(courseId: string, paths: string[]): Promise<ImportResult[]> {
  return list(await invoke<unknown>("import_files", { courseId, paths }), "import_files").map(parseImportResult);
}

export async function listCourseSources(courseId: string): Promise<CourseSource[]> {
  return list(await invoke<unknown>("list_course_sources", { courseId }), "list_course_sources").map(parseSource);
}
