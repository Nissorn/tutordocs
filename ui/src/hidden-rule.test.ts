import { describe, expect, it } from "vitest";
import fs from "node:fs";
import path from "node:path";

const sheets = fs
  .readdirSync(__dirname)
  .filter((name) => name.endsWith(".css"))
  .map((name) => ({ name, css: fs.readFileSync(path.resolve(__dirname, name), "utf-8") }));

describe("the hidden attribute has one rule", () => {
  it("styles.css hides every [hidden] element, even one with its own display rule", () => {
    const styles = sheets.find((sheet) => sheet.name === "styles.css")!;
    const rule = styles.css.match(/(?:^|\n)\[hidden\]\s*\{([^}]*)\}/)?.[1] ?? "";
    expect(rule.replace(/\s+/g, " ").trim()).toBe("display: none !important;");
  });

  it("no component stylesheet writes its own [hidden] display rule", () => {
    const rules = sheets
      .filter((sheet) => sheet.name !== "styles.css")
      .flatMap((sheet) =>
        [...sheet.css.matchAll(/([^{}]*\[hidden\][^{}]*)\{([^}]*)\}/g)]
          .filter((match) => /\bdisplay\s*:/.test(match[2]))
          .map((match) => `${sheet.name}: ${match[1].trim()}`),
      );
    expect(rules).toEqual([]);
  });
});
