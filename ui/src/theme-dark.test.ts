// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";
import fs from "node:fs";
import path from "node:path";

describe("dark theme highlights and tokenized shadows", () => {
  const stylesCss = fs.readFileSync(path.resolve(__dirname, "styles.css"), "utf-8");
  const annotationsCss = fs.readFileSync(path.resolve(__dirname, "annotations.css"), "utf-8");
  const pdfReaderCss = fs.readFileSync(path.resolve(__dirname, "pdf-reader.css"), "utf-8");

  it("defines tokenized shadow variables on :root and dark overrides", () => {
    expect(stylesCss).toContain("--shadow-popover:");
    expect(stylesCss).toContain("--shadow-card:");
    expect(stylesCss).toContain("--pdf-caveat-bg:");
    expect(stylesCss).toContain("--swatch-border:");

    // Dark overrides in prefers-color-scheme: dark
    const darkBlock = stylesCss.match(/@media\s*\(prefers-color-scheme:\s*dark\)[\s\S]*?\n\}/)?.[0] ?? "";
    expect(darkBlock).toContain("--shadow-popover:");
    expect(darkBlock).toContain("--shadow-card:");
    expect(darkBlock).toContain("--pdf-caveat-bg:");
    expect(darkBlock).toContain("--swatch-border:");
  });

  it("defines highlight palette tokens on :root and dark overrides for all five colors", () => {
    const colors = ["yellow", "green", "blue", "pink", "purple"];
    for (const color of colors) {
      expect(stylesCss).toContain(`--mark-${color}:`);
      expect(stylesCss).toContain(`--mark-ink-${color}:`);
    }

    const darkBlock = stylesCss.match(/@media\s*\(prefers-color-scheme:\s*dark\)[\s\S]*?\n\}/)?.[0] ?? "";
    for (const color of colors) {
      expect(darkBlock).toContain(`--mark-${color}:`);
      expect(darkBlock).toContain(`--mark-ink-${color}:`);
    }
  });

  it("applies tokenized shadows and swatch border in annotations.css", () => {
    expect(annotationsCss).toContain("box-shadow: var(--shadow-popover)");
    expect(annotationsCss).toContain("border: 1px solid var(--swatch-border)");
  });

  it("applies tokenized caveat background and page shadow in pdf-reader.css", () => {
    expect(pdfReaderCss).toContain("background: var(--pdf-caveat-bg)");
    expect(pdfReaderCss).toContain("var(--shadow-card)");
  });

  it("uses the highlight color tokens in annotations.css instead of literal colors without dark overrides", () => {
    const colors = ["yellow", "green", "blue", "pink", "purple"];
    for (const color of colors) {
      expect(annotationsCss).toContain(`--mark: var(--mark-${color})`);
      expect(annotationsCss).toContain(`--mark-ink: var(--mark-ink-${color})`);
    }
  });

  it("provides data-theme attribute overrides for e2e theme forcing", () => {
    expect(stylesCss).toContain(':root[data-theme="dark"]');
    expect(stylesCss).toContain(':root[data-theme="light"]');
  });
});
