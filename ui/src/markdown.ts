import MarkdownIt from "markdown-it";
import type { MarkdownIt as MarkdownItInstance } from "markdown-it";
import { math } from "./math";
import { tables } from "./tables";
import { visualFence } from "./visuals/fence";

// The agent writes the chapter, so its markdown is input from outside the
// reader: raw HTML stays off (it is escaped, not passed through) and
// markdown-it's link validator drops javascript:, vbscript: and file: urls.
const parser = new MarkdownIt({ html: false }).use(visualFence).use(tables).use(math).use(chapterFigures);

const IMAGE_EXTENSIONS = ["svg", "png", "jpg", "jpeg", "gif", "webp"] as const;

export type ImageExtension = (typeof IMAGE_EXTENSIONS)[number];

/**
 * A chapter figure, parsed at the renderer boundary from a markdown image
 * source. `name` is one plain file name. `extension` is from the closed list.
 * Anything else is not a figure, and the renderer shows the alt text instead.
 */
export type ChapterImage = {
  name: string;
  extension: ImageExtension;
};

const EXTENSIONS: ReadonlySet<string> = new Set(IMAGE_EXTENSIONS);

function isPlainComponent(value: string): boolean {
  return (
    value.length > 0 &&
    value !== "." &&
    value !== ".." &&
    !value.includes("/") &&
    !value.includes("\\") &&
    !value.includes("\0")
  );
}

/** `visuals/<name>.<extension>` only. A URL, a `..`, a slash or any other extension is refused. */
export function parseChapterImage(src: string): ChapterImage | null {
  const prefix = "visuals/";
  if (!src.startsWith(prefix)) return null;
  const fileName = src.slice(prefix.length);
  if (!isPlainComponent(fileName)) return null;
  const dot = fileName.lastIndexOf(".");
  if (dot <= 0) return null;
  const name = fileName.slice(0, dot);
  const extension = fileName.slice(dot + 1);
  if (!isPlainComponent(name) || !EXTENSIONS.has(extension)) return null;
  return { name, extension: extension as ImageExtension };
}

function figureUrl(chapterId: string, image: ChapterImage): string | null {
  if (!isPlainComponent(chapterId)) return null;
  const fileName = `${image.name}.${image.extension}`;
  if (!isPlainComponent(fileName)) return null;
  return `visual://localhost/${encodeURIComponent(chapterId)}/${encodeURIComponent(fileName)}`;
}

/** Chapter figures become `<img>` loads on `visual:`. Every other image source is dropped. */
function chapterFigures(md: MarkdownItInstance): void {
  md.renderer.rules.image = (tokens, idx, _options, env) => {
    const token = tokens[idx];
    const alt = md.utils.escapeHtml(token.content);
    const rawSrc = token.attrGet("src");
    const image = parseChapterImage(typeof rawSrc === "string" ? rawSrc : "");
    const chapterId = env && typeof env.chapterId === "string" ? env.chapterId : "";
    const url = image ? figureUrl(chapterId, image) : null;
    if (!url) return alt;
    const img = `<img src="${md.utils.escapeHtml(url)}" alt="${alt}">`;
    const rawTitle = token.attrGet("title");
    const title = typeof rawTitle === "string" ? rawTitle.trim() : "";
    if (!title) return img;
    return `<figure class="chapter-figure">${img}<figcaption>${md.utils.escapeHtml(title)}</figcaption></figure>`;
  };
}

/** Render one chapter. `chapterId` is the chapter on screen; a figure resolves only against it. */
export function renderMarkdown(source: string, chapterId?: string): string {
  return parser.render(source, { chapterId: chapterId ?? "" });
}
