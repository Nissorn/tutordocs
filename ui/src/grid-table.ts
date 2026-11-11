/**
 * An ASCII grid an agent drew inside a code fence, read back as a table.
 * `head` rows come from the block under the title, `body` rows from the rest.
 * Every row has one cell per column.
 */
export type GridTable = {
  caption?: string;
  head?: string[][];
  body: string[][];
};

type Border = { kind: "border"; stops: number[] };
type Row = { kind: "row"; pipes: number[]; chars: string[] };
type Line = Border | Row;

/** A cell of one row line, and whether the line spans the whole table width. */
type Cells = { cells: string[]; spanning: boolean };

const BORDER = /^\+(?:-+\+)+$/;
// Three or more of these in a row is drawing (a bar, a rule), not prose.
const DRAWING = /={3,}|-{3,}/;

function classify(text: string): Line | null {
  const chars = Array.from(text);
  if (BORDER.test(text)) {
    const stops = chars.flatMap((c, i) => (c === "+" ? [i] : []));
    return { kind: "border", stops };
  }
  if (chars.length >= 2 && chars[0] === "|" && chars[chars.length - 1] === "|") {
    const pipes = chars.flatMap((c, i) => (c === "|" ? [i] : []));
    return { kind: "row", pipes, chars };
  }
  return null;
}

function sameStops(a: number[], b: number[]): boolean {
  return a.length === b.length && a.every((stop, i) => stop === b[i]);
}

function tidy(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

function joinText(parts: string[]): string {
  return tidy(parts.join(" "));
}

/**
 * The interior `|` of a row must sit on the grid's column boundaries. The
 * closing `|` and a border's closing `+` carry no column, so agents that
 * miscount trailing padding still parse.
 */
function cellsOf(row: Row, grid: number[]): Cells | null {
  const interior = row.pipes.slice(1, -1);
  const spanning = interior.length === 0;
  if (!spanning && !sameStops(interior, grid)) return null;
  const edges = [0, ...interior, row.chars.length - 1];
  const cells: string[] = [];
  for (let i = 0; i + 1 < edges.length; i++) {
    cells.push(tidy(row.chars.slice(edges[i] + 1, edges[i + 1]).join("")));
  }
  return { cells, spanning };
}

/**
 * One row block, the run of row lines between two borders, as rows.
 * One column: blank lines separate rows and the lines between them join.
 * Several columns: each line is a row, and a line whose first cell is empty
 * continues the row above it, as a wrapped cell does.
 */
function rowsOf(block: Cells[], columns: number): string[][] | null {
  const rows: string[][] = [];
  if (columns === 1) {
    let pending: string[] = [];
    const flush = () => {
      if (pending.length > 0) rows.push([joinText(pending)]);
      pending = [];
    };
    for (const line of block) {
      if (line.cells[0] === "") flush();
      else pending.push(line.cells[0]);
    }
    flush();
    return rows;
  }
  let open = false;
  for (const line of block) {
    if (line.cells.length !== columns) return null;
    if (line.cells.every((cell) => cell === "")) {
      open = false;
    } else if (open && line.cells[0] === "") {
      const last = rows[rows.length - 1];
      line.cells.forEach((cell, i) => {
        if (cell !== "") last[i] = joinText([last[i], cell]);
      });
    } else {
      rows.push([...line.cells]);
      open = true;
    }
  }
  return rows;
}

/**
 * Read a fence's text as an ASCII grid table. A grid is borders (`+---+`)
 * and rows (`|` ... `|`) and nothing else. Any line that is neither, an
 * interior `|` off the column boundaries, a drawing inside a cell or an
 * empty result gives null, and the fence stays the code block it was.
 */
export function parseGridTable(text: string): GridTable | null {
  const lines: Line[] = [];
  for (const raw of text.replace(/\n$/, "").split("\n")) {
    const line = classify(raw.trimEnd());
    if (!line) return null;
    lines.push(line);
  }
  if (lines.length < 3) return null;
  if (lines[0].kind !== "border" || lines[lines.length - 1].kind !== "border") return null;

  let grid: number[] | null = null;
  for (const line of lines) {
    if (line.kind !== "border") continue;
    const interior = line.stops.slice(1, -1);
    if (interior.length === 0) continue;
    if (grid === null) grid = interior;
    else if (!sameStops(grid, interior)) return null;
  }
  const columns = (grid?.length ?? 0) + 1;

  const blocks: Cells[][] = [];
  let current: Cells[] | null = null;
  for (const [i, line] of lines.entries()) {
    if (line.kind === "border") {
      if (lines[i - 1]?.kind === "border") return null;
      current = null;
      continue;
    }
    const cells = cellsOf(line, grid ?? []);
    if (!cells) return null;
    if (current === null) {
      current = [];
      blocks.push(current);
    }
    current.push(cells);
  }

  const titled = blocks.length >= 2 && blocks[0].every((line) => line.spanning);
  const caption = titled ? joinText(blocks[0].flatMap((line) => line.cells)) : undefined;
  if (titled && !caption) return null;
  // Without a title a one-column box is a callout or a list, not a table.
  if (columns === 1 && !titled) return null;
  const rest = titled ? blocks.slice(1) : blocks;

  const hasHead = columns > 1 && rest.length >= 2;
  const headRows = hasHead ? rowsOf(rest[0], columns) : [];
  const bodyRows: string[][] = [];
  for (const block of hasHead ? rest.slice(1) : rest) {
    const rows = rowsOf(block, columns);
    if (!rows) return null;
    bodyRows.push(...rows);
  }
  if (!headRows || bodyRows.length === 0) return null;

  const texts = [caption ?? "", ...headRows.flat(), ...bodyRows.flat()];
  if (texts.some((cell) => DRAWING.test(cell))) return null;

  return {
    ...(caption ? { caption } : {}),
    ...(hasHead ? { head: headRows } : {}),
    body: bodyRows,
  };
}
