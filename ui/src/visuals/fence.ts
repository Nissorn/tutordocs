import type { MarkdownIt } from "markdown-it";

const VISUAL_INFO = /^visual\s+([A-Za-z0-9][A-Za-z0-9._-]*)$/;

/**
 * A fenced block whose info string is `visual <id>` embeds that artifact from
 * the chapter's `visuals/<id>/`. Its body is markdown: the text version of the
 * same concept, shown when the artifact cannot start. Any other fence renders
 * as code.
 */
export function visualFence(md: MarkdownIt): void {
  const code = md.renderer.rules.fence!;
  md.renderer.rules.fence = (tokens, idx, options, env, self) => {
    const match = VISUAL_INFO.exec(tokens[idx].info.trim());
    if (!match) return code(tokens, idx, options, env, self);
    const id = md.utils.escapeHtml(match[1]);
    const text = md.render(tokens[idx].content, env);
    return `<figure class="visual" data-visual-id="${id}"><div class="visual-fallback">${text}</div></figure>\n`;
  };
}
