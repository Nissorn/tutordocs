import "katex/dist/katex.min.css";
import katex, { type KatexOptions } from "katex";
import "./math.css";
import type { MarkdownIt } from "markdown-it";
import { tex } from "@mdit/plugin-tex";

// The agent writes the chapter, so a formula is input from outside the reader.
// KaTeX gets no trust (no \href, \url, \htmlClass, \includegraphics), a bad
// formula renders as its source instead of throwing, and sizes and macro
// expansion are capped so a formula cannot grow without bound.
const OPTIONS: KatexOptions = {
  trust: false,
  throwOnError: false,
  // Its "LaTeX-incompatible input" warnings would only reach a console nobody reads.
  strict: "ignore",
  maxSize: 10,
  maxExpand: 1000,
};

/** `$...$` and `$$...$$` render through KaTeX. A dollar amount such as "$5 and $10" stays text. */
export function math(md: MarkdownIt): void {
  md.use(tex, {
    render(source, displayMode) {
      let html: string;
      try {
        html = katex.renderToString(source, { ...OPTIONS, displayMode });
      } catch {
        // throwOnError: false covers KaTeX's ParseError only. A formula nested
        // deeper than the stack throws RangeError, which would take the whole chapter down.
        html = `<span class="katex-error">${md.utils.escapeHtml(source)}</span>`;
      }
      return displayMode ? `<div class="math-block">${html}</div>\n` : html;
    },
  });
}
