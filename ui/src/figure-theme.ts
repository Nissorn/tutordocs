// A chapter figure is an SVG shown as an <img>, so it follows macOS and cannot
// see the app's theme. When the theme is forced, the figure asks the visual:
// scheme for its own palette with `?theme=light` or `?theme=dark`, and the host
// pins the SVG's prefers-color-scheme queries to it. The SVG is never inlined
// into the page. With the theme on System the figure loads with no query.

type Forced = "light" | "dark";

const FIGURE_SRC = 'img[src^="visual://localhost/"]';

/** The theme the app forces on the page, or undefined while it follows macOS. */
function forcedTheme(): Forced | undefined {
  const theme = document.documentElement.dataset.theme;
  return theme === "light" || theme === "dark" ? theme : undefined;
}

/** The address a figure should load for a forced theme. Only an SVG has a palette to pin. */
export function themedFigureSrc(src: string, forced: Forced | undefined): string {
  const url = new URL(src);
  if (!url.pathname.toLowerCase().endsWith(".svg")) return src;
  url.searchParams.delete("theme");
  if (forced) url.searchParams.set("theme", forced);
  return url.toString();
}

/** Points every figure under `root` at the palette that matches the page's forced theme. */
export function themeFigures(root: ParentNode = document): void {
  const forced = forcedTheme();
  for (const img of root.querySelectorAll<HTMLImageElement>(FIGURE_SRC)) {
    const next = themedFigureSrc(img.src, forced);
    if (next !== img.src) img.src = next;
  }
}
