import { describe, expect, it } from "vitest";
import { parseGridTable } from "./grid-table";
import warehouseLayers from "./fixtures/tables/chapter-01-warehouse-layers.txt?raw";
import powerPath from "./fixtures/tables/chapter-02-power-path.txt?raw";
import arrowDiagram from "./fixtures/tables/chapter-03-arrow-diagram.txt?raw";
import pueBars from "./fixtures/tables/chapter-04-pue-bars.txt?raw";
import heatRejection from "./fixtures/tables/chapter-05-heat-rejection.txt?raw";
import liquidCooling from "./fixtures/tables/chapter-06-liquid-cooling.txt?raw";
import deepmindLoop from "./fixtures/tables/chapter-07-deepmind-loop.txt?raw";
import mediaPipeline from "./fixtures/tables/chapter-08-media-pipeline.txt?raw";
import lotoSteps from "./fixtures/tables/chapter-09-loto-steps.txt?raw";
import responsePower from "./fixtures/tables/chapter-10-response-structure-power.txt?raw";
import responseCooling from "./fixtures/tables/chapter-11-response-structure-cooling.txt?raw";
import responsePue from "./fixtures/tables/chapter-12-response-structure-pue.txt?raw";
import responseLoto from "./fixtures/tables/chapter-13-response-structure-loto.txt?raw";
import summaryBox from "./fixtures/tables/chapter-14-summary-box.txt?raw";

describe("parseGridTable on the power path grid from the user's chapter", () => {
  it("reads the title as the caption, one header row of 3 cells and 7 body rows", () => {
    const table = parseGridTable(powerPath);
    expect(table).not.toBeNull();
    expect(table!.caption).toBe("ELECTRICAL POWER TRANSFORMATION PATH");
    expect(table!.head).toEqual([["Stage", "Voltage Level", "Key Equipment & Function"]]);
    expect(table!.body).toHaveLength(7);
    expect(table!.body[0]).toEqual([
      "1. Grid Infeed",
      "115 kV – 230 kV AC",
      "Utility transmission lines, high-voltage disconnects",
    ]);
    expect(table!.body[6]).toEqual([
      "7. Server Motherboard",
      "12 V / 1 V DC",
      "Point-of-Load (PoL) VRMs stepping down to CPU/TPU core",
    ]);
  });
});

describe("parseGridTable on the one-column summary box from the user's chapter", () => {
  it("reads the title as the caption and each numbered item as one row, joining its wrapped lines", () => {
    const table = parseGridTable(summaryBox);
    expect(table).not.toBeNull();
    expect(table!.caption).toBe("CHAPTER 1 SUMMARY: THE HYPERSCALE FOUNDATION");
    expect(table!.head).toBeUndefined();
    expect(table!.body).toHaveLength(7);
    expect(table!.body[0]).toEqual([
      "1. Philosophy: The Datacenter as a Computer (Warehouse-Scale Computing). Heterogeneity and expensive hardware redundancy are replaced by software-managed fault tolerance, uniform modularity, and operational simplicity.",
    ]);
    expect(table!.body[6]).toEqual([
      "7. Culture: Blameless postmortems foster psychological safety and continuous system hardening without fear of retribution.",
    ]);
  });

  it("still parses when some rows end a character or two off the right border", () => {
    const widths = new Set(summaryBox.split("\n").filter(Boolean).map((line) => Array.from(line).length));
    expect(widths.size).toBeGreaterThan(1);
    expect(parseGridTable(summaryBox)).not.toBeNull();
  });
});

describe("parseGridTable refuses what is not a grid", () => {
  it("leaves the box-and-arrow diagram as code", () => {
    expect(parseGridTable(arrowDiagram)).toBeNull();
  });

  it("leaves a bulleted or numbered list as code", () => {
    for (const list of [responsePower, responseCooling, responsePue, responseLoto]) {
      expect(parseGridTable(list)).toBeNull();
    }
  });

  it("leaves a grid whose interior pipe is a column off as code", () => {
    const ragged = [
      "+-------+-------+",
      "| Stage | Level |",
      "+-------+-------+",
      "| One   | 13 kV |",
      "| Two   |  5 kV |",
      "+-------+-------+",
    ].join("\n");
    expect(parseGridTable(ragged.replace("| One   |", "| One  |"))).toBeNull();
    expect(parseGridTable(ragged.replace("|  5 kV |", "|  5 kV  |"))).not.toBeNull();
    expect(parseGridTable(ragged)).not.toBeNull();
  });

  it("leaves a grid with a column more in one row than in its borders as code", () => {
    const extra = ["+-----+-----+", "| a   | b   |", "+-----+-----+", "| 1 | 2 | 3 |", "+-----+-----+"].join("\n");
    expect(parseGridTable(extra)).toBeNull();
  });

  it("leaves a block with a line outside the grid as code", () => {
    const note = ["+-----+-----+", "| a   | b   |", "+-----+-----+", "| 1   | 2   |", "+-----+-----+", "Source: survey"].join("\n");
    expect(parseGridTable(note)).toBeNull();
    expect(parseGridTable("")).toBeNull();
    expect(parseGridTable("+---+\n")).toBeNull();
  });

  it("leaves a grid with no border below it as code", () => {
    expect(parseGridTable(["+-----+-----+", "| a   | b   |", "+-----+-----+", "| 1   | 2   |"].join("\n"))).toBeNull();
  });

  it("leaves a one-column box with one row, a callout, as code", () => {
    expect(parseGridTable(["+--------------+", "| Note: do it  |", "+--------------+"].join("\n"))).toBeNull();
    expect(parseGridTable(["+------+", "| Todo |", "+------+", "| one  |", "+------+"].join("\n"))).toEqual({
      caption: "Todo",
      body: [["one"]],
    });
  });

  it("leaves a bar or a pipe drawn inside the cells as code", () => {
    expect(parseGridTable(pueBars)).toBeNull();
    for (const diagram of [heatRejection, liquidCooling, deepmindLoop, mediaPipeline]) {
      expect(parseGridTable(diagram)).toBeNull();
    }
  });
});

describe("parseGridTable on every code fence in the user's chapter", () => {
  const verdicts: Array<[string, string, boolean]> = [
    ["01 warehouse layers", warehouseLayers, true],
    ["02 power path", powerPath, true],
    ["03 arrow diagram", arrowDiagram, false],
    ["04 PUE bars", pueBars, false],
    ["05 heat rejection", heatRejection, false],
    ["06 liquid cooling", liquidCooling, false],
    ["07 deepmind loop", deepmindLoop, false],
    ["08 media pipeline", mediaPipeline, false],
    ["09 LOTO steps", lotoSteps, false],
    ["10 response power", responsePower, false],
    ["11 response cooling", responseCooling, false],
    ["12 response PUE", responsePue, false],
    ["13 response LOTO", responseLoto, false],
    ["14 summary box", summaryBox, true],
  ];
  for (const [name, text, isTable] of verdicts) {
    it(`${name} ${isTable ? "becomes a table" : "stays code"}`, () => {
      expect(parseGridTable(text) !== null).toBe(isTable);
    });
  }

  it("reads the layer stack as a caption over five one-cell rows", () => {
    const table = parseGridTable(warehouseLayers);
    expect(table!.caption).toBe("WAREHOUSE-SCALE COMPUTER (WSC)");
    expect(table!.body).toHaveLength(5);
    expect(table!.body[0][0]).toMatch(/^Software Orchestration Layer \(Borg/);
  });
});

describe("parseGridTable on grids a hand-drawn table could take", () => {
  it("joins a wrapped cell, whose next line has an empty first cell, into the row above", () => {
    const wrapped = [
      "+-----------+-------------------+",
      "| Stage     | Notes             |",
      "+-----------+-------------------+",
      "| Grid      | Long text wraps   |",
      "|           | onto a second line|",
      "+-----------+-------------------+",
      "| Substation| Short             |",
      "+-----------+-------------------+",
    ].join("\n");
    expect(parseGridTable(wrapped)).toEqual({
      head: [["Stage", "Notes"]],
      body: [
        ["Grid", "Long text wraps onto a second line"],
        ["Substation", "Short"],
      ],
    });
  });

  it("keeps an empty corner cell in the header", () => {
    const cross = [
      "+-----+-----+-----+",
      "|     | Q1  | Q2  |",
      "+-----+-----+-----+",
      "| Ann | 4   | 7   |",
      "+-----+-----+-----+",
    ].join("\n");
    expect(parseGridTable(cross)).toEqual({ head: [["", "Q1", "Q2"]], body: [["Ann", "4", "7"]] });
  });

  it("reads a grid with no title and no header as body rows only", () => {
    const plain = ["+-----+-----+", "| a   | b   |", "| c   | d   |", "+-----+-----+"].join("\n");
    expect(parseGridTable(plain)).toEqual({ body: [["a", "b"], ["c", "d"]] });
  });
});
