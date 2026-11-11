// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";
import { renderMarkdown } from "./markdown";
import powerPath from "./fixtures/tables/chapter-02-power-path.txt?raw";
import arrowDiagram from "./fixtures/tables/chapter-03-arrow-diagram.txt?raw";
import summaryBox from "./fixtures/tables/chapter-14-summary-box.txt?raw";

const fence = (text: string, info = "") => "```" + info + "\n" + text + "```\n";

function dom(markdown: string): HTMLElement {
  const box = document.createElement("div");
  box.innerHTML = renderMarkdown(markdown, "c");
  return box;
}

describe("a text grid in a fence", () => {
  it("renders the power path grid as a table with a caption, a 3-cell header and 7 body rows", () => {
    const box = dom(`Before.\n\n${fence(powerPath)}\nAfter.\n`);
    expect(box.querySelectorAll("pre")).toHaveLength(0);
    expect(box.querySelector("table > caption")?.textContent).toBe("ELECTRICAL POWER TRANSFORMATION PATH");
    expect(Array.from(box.querySelectorAll("thead th"), (th) => th.textContent)).toEqual([
      "Stage",
      "Voltage Level",
      "Key Equipment & Function",
    ]);
    expect(box.querySelectorAll("tbody tr")).toHaveLength(7);
    expect(box.querySelectorAll("tbody tr")[3].querySelectorAll("td")[1].textContent).toBe("415 V / 480 V AC");
    expect(box.textContent).toContain("Before.");
    expect(box.textContent).toContain("After.");
  });

  it("renders the one-column summary box as a table with 7 rows and no header", () => {
    const box = dom(fence(summaryBox));
    expect(box.querySelector("caption")?.textContent).toBe("CHAPTER 1 SUMMARY: THE HYPERSCALE FOUNDATION");
    expect(box.querySelector("thead")).toBeNull();
    expect(box.querySelectorAll("tbody tr")).toHaveLength(7);
    expect(box.querySelectorAll("pre")).toHaveLength(0);
  });

  it("leaves the box-and-arrow diagram as a code block with its text unchanged", () => {
    const box = dom(fence(arrowDiagram));
    expect(box.querySelector("table")).toBeNull();
    expect(box.querySelector("pre code")?.textContent).toBe(arrowDiagram);
  });

  it("leaves a ragged grid as a code block", () => {
    const ragged = [
      "+-------+-------+",
      "| Stage | Level |",
      "+-------+-------+",
      "| One  | 13 kV |",
      "+-------+-------+",
      "",
    ].join("\n");
    const box = dom(fence(ragged));
    expect(box.querySelector("table")).toBeNull();
    expect(box.querySelector("pre code")?.textContent).toBe(ragged);
  });

  it("leaves a fence with an info string as code, even when the text is a grid", () => {
    const box = dom(fence(powerPath, "text"));
    expect(box.querySelector("table")).toBeNull();
    expect(box.querySelector("pre")).not.toBeNull();
  });

  it("renders inline markdown in a cell and nothing raw", () => {
    const grid = [
      "+------+--------------------------+",
      "| Name | Note                     |",
      "+------+--------------------------+",
      "| a    | **bold** and `code`      |",
      "| b    | <img src=x onerror=1>    |",
      "| c    | [go](javascript:alert(1))|",
      "+------+--------------------------+",
      "",
    ].join("\n");
    const box = dom(fence(grid));
    const cells = Array.from(box.querySelectorAll("tbody td:nth-child(2)"));
    expect(cells[0].innerHTML).toBe("<strong>bold</strong> and <code>code</code>");
    expect(box.querySelector("img")).toBeNull();
    expect(box.querySelector("a")).toBeNull();
    expect(cells[1].textContent).toBe("<img src=x onerror=1>");
  });

  it("escapes a caption", () => {
    const grid = ["+-------+", "| <b>T  |", "+-------+", "| x     |", "+-------+", ""].join("\n");
    const box = dom(fence(grid));
    expect(box.querySelector("b")).toBeNull();
    expect(box.querySelector("caption")?.textContent).toBe("<b>T");
  });

  it("keeps a visual fence working beside a grid", () => {
    const box = dom(`${fence(powerPath)}\n\`\`\`visual cube\nA cube.\n\`\`\`\n`);
    expect(box.querySelector("table")).not.toBeNull();
    expect(box.querySelector('figure.visual[data-visual-id="cube"]')?.textContent).toContain("A cube.");
  });
});

describe("numeric columns", () => {
  const pipe = ["| Stage | Level | Note |", "| --- | --- | --- |", "| One | 13.8 | high |", "| Two | 1,200 | low |", ""].join("\n");

  it("right-aligns a pipe table column whose body cells are all numbers, header included", () => {
    const box = dom(pipe);
    const classes = (selector: string) => Array.from(box.querySelectorAll(selector), (cell) => cell.className);
    expect(classes("thead th")).toEqual(["", "num", ""]);
    expect(classes("tbody tr:nth-child(1) td")).toEqual(["", "num", ""]);
    expect(classes("tbody tr:nth-child(2) td")).toEqual(["", "num", ""]);
  });

  it("does not right-align a column with one cell that is not a number, a unit or a range", () => {
    for (const odd of ["13.8 kV", "1.1 - 2.0", "n/a", ""]) {
      const box = dom(["| A | B |", "| --- | --- |", "| x | 5 |", `| y | ${odd} |`, ""].join("\n"));
      expect(box.querySelectorAll(".num"), odd).toHaveLength(0);
    }
  });

  it("keeps an alignment the author asked for", () => {
    const box = dom(["| A | B |", "| --- | :---: |", "| x | 5 |", ""].join("\n"));
    expect(box.querySelector("td:nth-child(2)")?.getAttribute("style")).toBe("text-align:center");
    expect(box.querySelectorAll(".num")).toHaveLength(0);
  });

  it("right-aligns the same columns of a grid table", () => {
    const grid = [
      "+-------+--------+",
      "| Stage | Level  |",
      "+-------+--------+",
      "| One   | 13.8   |",
      "| Two   | 1,200  |",
      "+-------+--------+",
      "",
    ].join("\n");
    const box = dom(fence(grid));
    expect(Array.from(box.querySelectorAll("th, td"), (cell) => cell.className)).toEqual(["", "num", "", "num", "", "num"]);
  });
});

describe("the table wrapper", () => {
  it("puts a pipe table and a grid table in the same scroll wrapper", () => {
    const pipe = dom("| A | B |\n| --- | --- |\n| 1 | 2 |\n");
    const grid = dom(fence(powerPath));
    for (const box of [pipe, grid]) {
      expect(box.querySelectorAll(".table-wrap > table")).toHaveLength(1);
      expect(box.querySelectorAll("table table")).toHaveLength(0);
    }
  });
});
