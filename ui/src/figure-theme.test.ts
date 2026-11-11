// @vitest-environment happy-dom
import { afterEach, describe, expect, it } from "vitest";
import { themeFigures, themedFigureSrc } from "./figure-theme";

const SVG = "visual://localhost/c1/power-path.svg";

describe("choosing a figure's palette", () => {
  it("adds the forced theme to an SVG address and takes it off again", () => {
    expect(themedFigureSrc(SVG, "dark")).toBe(`${SVG}?theme=dark`);
    expect(themedFigureSrc(`${SVG}?theme=dark`, "light")).toBe(`${SVG}?theme=light`);
    expect(themedFigureSrc(`${SVG}?theme=dark`, undefined)).toBe(SVG);
    expect(themedFigureSrc(SVG, undefined)).toBe(SVG);
  });

  it("leaves a picture alone, since a PNG has no palette to pin", () => {
    const png = "visual://localhost/c1/photo.png";
    expect(themedFigureSrc(png, "dark")).toBe(png);
  });
});

describe("theming the figures on the page", () => {
  afterEach(() => {
    delete document.documentElement.dataset.theme;
    document.body.replaceChildren();
  });

  function page(): HTMLElement {
    const root = document.createElement("div");
    root.innerHTML = `<img src="${SVG}" alt="a"><img src="visual://localhost/c1/photo.png" alt="b"><img src="https://example.test/x.svg" alt="c">`;
    document.body.append(root);
    return root;
  }

  it("re-points chapter SVGs when the page forces a theme, and back when it follows macOS", () => {
    const root = page();
    const [svg, png, outside] = Array.from(root.querySelectorAll("img"));
    document.documentElement.dataset.theme = "dark";
    themeFigures(root);
    expect(svg.getAttribute("src")).toBe(`${SVG}?theme=dark`);
    expect(png.getAttribute("src")).toBe("visual://localhost/c1/photo.png");
    expect(outside.getAttribute("src")).toBe("https://example.test/x.svg");
    delete document.documentElement.dataset.theme;
    themeFigures(root);
    expect(svg.getAttribute("src")).toBe(SVG);
  });
});
