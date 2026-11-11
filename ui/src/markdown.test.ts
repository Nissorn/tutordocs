// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";
import { renderMarkdown } from "./markdown";

describe("renderMarkdown", () => {
  it("renders headings, emphasis and lists", () => {
    const html = renderMarkdown("# Cells\n\nThe cell is **basic**.\n\n- Nucleus\n- Membrane\n");
    expect(html).toContain("<h1>Cells</h1>");
    expect(html).toContain("<strong>basic</strong>");
    expect(html).toContain("<li>Nucleus</li>");
  });

  it("escapes raw HTML from the agent instead of passing it through", () => {
    const html = renderMarkdown("Hi <script>alert(1)</script> <img src=x onerror=alert(1)>");
    expect(html).not.toContain("<script");
    expect(html).not.toContain("<img");
  });

  it("does not turn javascript: urls into links", () => {
    const html = renderMarkdown("[click](javascript:alert(1))");
    expect(html).not.toContain("<a ");
  });

  it("renders inline math between dollar signs as a formula, not literal text", () => {
    const html = renderMarkdown("Let $x^2$ be a square.");
    expect(html).toContain('<span class="katex">');
    expect(html).not.toContain("$x^2$");
  });

  it("renders a $$ block as a display formula, on one line or across several", () => {
    const oneLine = renderMarkdown("$$6\\,CO_2 + 6\\,H_2O \\rightarrow C_6H_{12}O_6 + 6\\,O_2$$");
    expect(oneLine).toContain('class="katex-display"');
    expect(oneLine).not.toContain("$$");
    const spread = renderMarkdown("Energy:\n\n$$\n\\int_0^1 x\\,dx\n$$\n\nDone.");
    expect(spread).toContain('class="katex-display"');
    expect(spread).toContain("<p>Done.</p>");
  });

  it("leaves a dollar sign used as money as text", () => {
    for (const sentence of [
      "It costs $5 and $10 for the pair.",
      "Prices run $5-$10 a week.",
      "Pay $5, then $10, then $20.",
      "Escaped \\$5 and \\$10 stay as typed.",
    ]) {
      const html = renderMarkdown(sentence);
      expect(html, sentence).not.toContain("katex");
      expect(html, sentence).toContain("$");
    }
    expect(renderMarkdown("It costs $5 and $10 for the pair.")).toBe("<p>It costs $5 and $10 for the pair.</p>\n");
  });

  it("keeps dollar signs literal inside code", () => {
    const html = renderMarkdown("Use `$x$` or\n\n```\n$$y$$\n```\n");
    expect(html).not.toContain("katex");
    expect(html).toContain("<code>$x$</code>");
  });

  it("renders formula markup an agent could abuse inert: no links, images, scripts or class and style hooks", () => {
    const attempts = [
      String.raw`$\href{javascript:alert(1)}{click}$`,
      String.raw`$\url{https://example.com}$`,
      String.raw`$\htmlClass{evil}{x}$`,
      String.raw`$\htmlId{evil}{x}$`,
      String.raw`$\htmlStyle{position:fixed;top:0}{x}$`,
      String.raw`$\htmlData{k=v}{x}$`,
      String.raw`$\includegraphics{https://example.com/a.png}$`,
      "$<img src=x onerror=alert(1)>$",
      "$$<script>alert(1)</script>$$",
    ];
    for (const attempt of attempts) {
      const box = document.createElement("div");
      box.innerHTML = renderMarkdown(attempt);
      const hooks = box.querySelectorAll(
        'a, img, script, iframe, [href], [src], [onerror], [class~="evil"], [id="evil"], [data-k], [style*="position"]',
      );
      expect(hooks.length, attempt).toBe(0);
    }
  });

  it("shows the source of a formula KaTeX cannot parse and keeps the rest of the chapter", () => {
    const html = renderMarkdown("Before $\\frac{1$ and after.");
    expect(html).toContain('class="katex-error" title="ParseError: KaTeX parse error');
    expect(html).toContain("\\frac{1");
    expect(html).toContain("and after.");
  });

  it("stops a macro that expands without end", () => {
    const html = renderMarkdown(String.raw`Loop $\def\a{\a\a}\a$ done.`);
    expect(html).toContain('class="katex-error"');
    expect(html).toContain("done.");
  });

  it("caps the size a formula can ask for", () => {
    const box = document.createElement("div");
    box.innerHTML = renderMarkdown(String.raw`$\rule{99999em}{99999em}$`);
    const attributes = Array.from(box.querySelectorAll("*")).flatMap((node) => Array.from(node.attributes).map((a) => a.value));
    expect(attributes.some((value) => value.includes("99999"))).toBe(false);
    expect(attributes.some((value) => value.includes("10em"))).toBe(true);
  });

  it("shows a chapter figure as an image on the visual scheme", () => {
    const html = renderMarkdown("![a](visuals/x.svg)", "c");
    expect(html).toContain('<img src="visual://localhost/c/x.svg" alt="a">');
    expect(html).not.toContain("chapter-figure");
  });

  it("captions a titled chapter image and leaves an untitled or refused image without a figure", () => {
    const titled = renderMarkdown('![a](visuals/x.svg "A <b> claim")', "c");
    expect(titled).toContain(
      '<figure class="chapter-figure"><img src="visual://localhost/c/x.svg" alt="a"><figcaption>A &lt;b&gt; claim</figcaption></figure>',
    );

    const untitled = renderMarkdown("![a](visuals/x.svg)", "c");
    expect(untitled).toContain('<img src="visual://localhost/c/x.svg" alt="a">');
    expect(untitled).not.toContain("<figure");

    const refused = renderMarkdown('![secret](https://e.com/x.png "The claim")', "c");
    expect(refused).not.toContain("<img");
    expect(refused).not.toContain("<figure");
    expect(refused).not.toContain("The claim");
    expect(refused).not.toContain("https://e.com");
    expect(refused).toContain(">secret<");
  });

  it("shows only the alt text when an image is not a chapter figure", () => {
    for (const src of ["https://e.com/x.png", "../x.svg", "visuals/x.exe"]) {
      const html = renderMarkdown(`![a](${src})`, "c");
      expect(html).not.toContain("<img");
      expect(html).not.toContain(src);
      expect(html).toContain(">a<");
    }
  });

  it("keeps an interactive visual fence beside a chapter figure", () => {
    const html = renderMarkdown("![a](visuals/x.svg)\n\n```visual cube\nA cube.\n```\n", "c");
    expect(html).toContain("visual://localhost/c/x.svg");
    expect(html).toContain('data-visual-id="cube"');
  });

  it("survives a formula nested deeper than KaTeX can parse", () => {
    const deep = "{".repeat(20000) + "x" + "}".repeat(20000);
    const html = renderMarkdown(`Before $${deep}$ after.`);
    expect(html).toContain("katex-error");
    expect(html).toContain("after.");
  });
});
