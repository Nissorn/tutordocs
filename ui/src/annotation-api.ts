import { invoke } from "@tauri-apps/api/core";
import type { Selector } from "./anchor";
import { list, parseRect, record, str } from "./api";
import type { Rect } from "./pdf-geometry";

// The typed bridge to the annotation commands in src-tauri/src/annotations.rs
// and to the PDF source marks in src-tauri/src/pdf_sources.rs, which share Mark.

export const COLORS = ["yellow", "green", "blue", "pink", "purple"] as const;
export type Color = (typeof COLORS)[number];
export type Resolution = "resolved" | "unresolved" | "ambiguous";

export interface Mark {
  annotationId: string;
  kind: "highlight" | "note" | "bookmark";
  color: Color;
  quote: string;
  note: string;
  selectors: Selector[];
  resolution: Resolution;
  /** The page of a PDF source; 1 for a chapter. */
  locator: number;
  /** Normalized page rects of a PDF mark; empty for a chapter. */
  rects: Rect[];
  createdAt: number;
  updatedAt: number;
}

export type MarkInput = Omit<Mark, "createdAt" | "updatedAt">;

export async function listAnnotations(chapterId: string): Promise<Mark[]> {
  return list(await invoke<unknown>("list_annotations", { chapterId }), "list_annotations").map(parseMark);
}

export async function saveAnnotation(chapterId: string, mark: MarkInput): Promise<Mark> {
  return parseMark(await invoke<unknown>("save_annotation", { chapterId, mark }));
}

export async function listSourceAnnotations(sourceName: string): Promise<Mark[]> {
  return list(await invoke<unknown>("list_source_annotations", { sourceName }), "list_source_annotations").map(
    parseMark,
  );
}

export async function saveSourceAnnotation(sourceName: string, mark: MarkInput): Promise<Mark> {
  return parseMark(await invoke<unknown>("save_source_annotation", { sourceName, mark }));
}

export async function deleteAnnotation(annotationId: string): Promise<boolean> {
  const removed = await invoke<unknown>("delete_annotation", { annotationId });
  if (typeof removed !== "boolean") throw new Error("delete_annotation: expected a boolean");
  return removed;
}

function parseMark(value: unknown): Mark {
  const r = record(value, "annotation");
  return {
    annotationId: str(r, "annotationId"),
    kind: oneOf(r, "kind", ["highlight", "note", "bookmark"] as const),
    color: oneOf(r, "color", COLORS),
    quote: str(r, "quote"),
    note: str(r, "note"),
    selectors: list(r.selectors, "selectors").map(parseSelector),
    resolution: oneOf(r, "resolution", ["resolved", "unresolved", "ambiguous"] as const),
    locator: num(r, "locator"),
    rects: list(r.rects, "rects").map(parseRect),
    createdAt: num(r, "createdAt"),
    updatedAt: num(r, "updatedAt"),
  };
}

function parseSelector(value: unknown): Selector {
  const r = record(value, "selector");
  switch (r.type) {
    case "TextQuoteSelector":
      return { type: r.type, exact: str(r, "exact"), prefix: str(r, "prefix"), suffix: str(r, "suffix") };
    case "TextPositionSelector":
      return { type: r.type, start: num(r, "start"), end: num(r, "end") };
    default:
      throw new Error(`unknown selector type ${JSON.stringify(r.type)}`);
  }
}

function oneOf<T extends string>(r: Record<string, unknown>, key: string, allowed: readonly T[]): T {
  const value = str(r, key);
  if (!(allowed as readonly string[]).includes(value)) throw new Error(`unexpected ${key} ${JSON.stringify(value)}`);
  return value as T;
}

function num(r: Record<string, unknown>, key: string): number {
  const value = r[key];
  if (typeof value !== "number") throw new Error(`expected "${key}" to be a number`);
  return value;
}
