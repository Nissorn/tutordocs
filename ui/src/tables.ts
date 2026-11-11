import type { MarkdownIt } from "markdown-it";
import { parseGridTable, type GridTable } from "./grid-table";

type Env = Parameters<MarkdownIt["renderInline"]>[1];
type Token = ReturnType<MarkdownIt["parse"]>[number];

// A figure with a sign, a currency mark, thousands commas, a decimal part or a
// percent sign. A unit ("13.8 kV") or a range makes a cell text, not a number.
const NUMERIC = /^[-+−]?[$€£¥]?(?:\d[\d,]*(?:\.\d+)?|\.\d+)%?$/;

export function isNumeric(text: string): boolean {
  return NUMERIC.test(text.trim());
}

/** A column is numeric when every body cell in it is a number. */
export function numericColumns(body: string[][]): boolean[] {
  const width = body.reduce((most, row) => Math.max(most, row.length), 0);
  return Array.from({ length: width }, (_, col) => body.every((row) => isNumeric(row[col] ?? "")));
}

function renderGridTable(md: MarkdownIt, table: GridTable, env: Env): string {
  const numeric = numericColumns(table.body);
  const row = (tag: "th" | "td", cells: string[]) =>
    "<tr>" +
    cells
      .map((cell, col) => `<${tag}${numeric[col] ? ' class="num"' : ""}>${md.renderInline(cell, env)}</${tag}>`)
      .join("") +
    "</tr>\n";
  const caption = table.caption ? `<caption>${md.renderInline(table.caption, env)}</caption>\n` : "";
  const head = table.head ? `<thead>\n${table.head.map((cells) => row("th", cells)).join("")}</thead>\n` : "";
  const body = `<tbody>\n${table.body.map((cells) => row("td", cells)).join("")}</tbody>\n`;
  return `<div class="table-wrap"><table>\n${caption}${head}${body}</table></div>\n`;
}

/**
 * Every table in a chapter shares one look. A fence with no info string whose
 * text is an ASCII grid renders as a table, and a markdown pipe table gets the
 * same wrapper (so a wide one scrolls inside itself) and the same right-aligned
 * numeric columns. Any other fence is untouched.
 */
export function tables(md: MarkdownIt): void {
  const code = md.renderer.rules.fence!;
  md.renderer.rules.fence = (tokens, idx, options, env, self) => {
    const token = tokens[idx];
    const table = token.info.trim() === "" ? parseGridTable(token.content) : null;
    return table ? renderGridTable(md, table, env) : code(tokens, idx, options, env, self);
  };

  md.renderer.rules.table_open = (tokens, idx, options, _env, self) =>
    `<div class="table-wrap">${self.renderToken(tokens, idx, options)}`;
  md.renderer.rules.table_close = (tokens, idx, options, _env, self) =>
    `${self.renderToken(tokens, idx, options)}</div>\n`;

  md.core.ruler.push("numeric_columns", (state) => {
    const { tokens } = state;
    for (let i = 0; i < tokens.length; i++) {
      if (tokens[i].type !== "table_open") continue;
      const cells: Array<{ token: Token; col: number }> = [];
      const bodyRows: string[][] = [];
      let col = 0;
      let inBody = false;
      for (i++; tokens[i].type !== "table_close"; i++) {
        const { type } = tokens[i];
        if (type === "tbody_open") inBody = true;
        else if (type === "tr_open") {
          col = 0;
          if (inBody) bodyRows.push([]);
        } else if (type === "th_open" || type === "td_open") {
          cells.push({ token: tokens[i], col });
          if (inBody) bodyRows[bodyRows.length - 1].push(tokens[i + 1].content);
          col++;
        }
      }
      const numeric = numericColumns(bodyRows);
      for (const { token, col: column } of cells) {
        if (numeric[column] && !token.attrGet("style")) token.attrJoin("class", "num");
      }
    }
  });
}
