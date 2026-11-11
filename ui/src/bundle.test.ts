import { fileURLToPath } from "node:url";
import { beforeAll, describe, expect, it } from "vitest";
import { build } from "vite";

// The CSP is `default-src 'self'` with no font-src, so a font must be a file the
// app serves itself: not inlined as a data: URI and not fetched from elsewhere.
describe("the production bundle", () => {
  let css: string;
  let files: string[];

  beforeAll(async () => {
    const result = await build({
      root: fileURLToPath(new URL("..", import.meta.url)),
      logLevel: "silent",
      build: { write: false },
    });
    const outputs = (Array.isArray(result) ? result : [result]).flatMap((r) => ("output" in r ? r.output : []));
    files = outputs.map((o) => o.fileName);
    css = outputs
      .filter((o) => o.type === "asset" && o.fileName.endsWith(".css"))
      .map((o) => String((o as { source: string | Uint8Array }).source))
      .join("\n");
  }, 60_000);

  it("serves every font as a same-origin file, none inlined and none remote", () => {
    const fontFaces = css.match(/@font-face\{[^}]*\}/g) ?? [];
    const sources = fontFaces.flatMap((rule) => Array.from(rule.matchAll(/url\(([^)]+)\)/g), (m) => m[1].replace(/["']/g, "")));
    expect(sources.length).toBeGreaterThan(0);
    for (const source of sources) {
      expect(source, source).toMatch(/^\/assets\/(?:KaTeX_|newsreader-|atkinson-hyperlegible-next-|jetbrains-mono-)[\w-]+\.woff2$/);
      expect(files).toContain(source.slice(1));
    }
  });

  it("bundles the reading serif, the reading sans and the code face", () => {
    for (const family of ["newsreader", "atkinson-hyperlegible-next", "jetbrains-mono"]) {
      expect(files.some((file) => new RegExp(`^assets/${family}-latin-.+\\.woff2$`).test(file)), family).toBe(true);
    }
  });

  it("keeps the smallest KaTeX font, which Vite would otherwise inline, as a file", () => {
    expect(files.some((file) => /^assets\/KaTeX_Size3-Regular-.+\.woff2$/.test(file))).toBe(true);
    expect(css).not.toContain("data:font");
    expect(css).not.toContain("data:application/font");
  });

  it("ships only the woff2 copy of each font", () => {
    expect(files.filter((file) => /KaTeX_.+\.(woff|ttf)$/.test(file))).toEqual([]);
  });
});
