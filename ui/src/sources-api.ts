import { invoke } from "@tauri-apps/api/core";
import { oneOf, record, str, strOrNull } from "./api";
import { listCourseSources } from "./setup-api";

// The typed bridge to the source commands in src-tauri/src/sources.rs.

export interface Source {
  name: string;
  path: string;
  /** Where the page came from. Null for a file the user dropped in. */
  url: string | null;
  fetchedAt: string | null;
}

/** Fetch `url` through the chosen agent and add it to the library as markdown. */
export async function ingestUrl(url: string): Promise<Source> {
  const r = record(await invoke<unknown>("ingest_url", { url }), "ingest_url");
  return {
    name: str(r, "name"),
    path: str(r, "path"),
    url: strOrNull(r, "url"),
    fetchedAt: strOrNull(r, "fetchedAt"),
  };
}

/** The kinds of source the reader can open: a PDF in the PDF view, the others as read-only text. */
export type ReadableKind = "pdf" | "markdown" | "text";

export interface ReadableSource {
  name: string;
  kind: ReadableKind;
}

/**
 * Every source in the course the reader can show, in the order the backend
 * lists them. A captured web page is stored as markdown, so it reads as
 * markdown; a kind the reader cannot show is left out rather than listed dead.
 */
export async function listReadableSources(courseId: string): Promise<ReadableSource[]> {
  const readable: ReadableSource[] = [];
  for (const source of await listCourseSources(courseId)) {
    if (source.kind === "pdf" || source.kind === "markdown" || source.kind === "text") {
      readable.push({ name: source.name, kind: source.kind });
    } else if (source.kind === "url") {
      readable.push({ name: source.name, kind: "markdown" });
    }
  }
  return readable;
}

export interface TextSource {
  name: string;
  kind: "markdown" | "text";
  text: string;
}

export { readPdfSource } from "./api";

/** A markdown or text source's contents, by its plain file name. */
export async function readTextSource(courseId: string, name: string): Promise<TextSource> {
  const r = record(await invoke<unknown>("read_text_source", { courseId, name }), "read_text_source");
  return { name: str(r, "name"), kind: oneOf(r, "kind", ["markdown", "text"] as const), text: str(r, "text") };
}
