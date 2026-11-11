// How wide the sidebar and the study panel may be. One pure module, so the
// separator, the settings parser and the tests all agree on the limits.

export type Side = "left" | "right";

/** Pixels one arrow key moves a separator. */
export const STEP = 16;
/** The reading column never gets narrower than this, whatever the panels ask for. */
export const MIN_DOCUMENT = 480;

export const BOUNDS: Record<Side, { min: number; max: number }> = {
  left: { min: 180, max: 420 },
  right: { min: 224, max: 560 },
};

const LEFT_DEFAULT = 240;
const RIGHT_DEFAULT = { min: 256, share: 0.22, max: 352 };

/** What the stylesheet draws when no width is saved: 15rem, and a study panel that grows with the window. */
export function defaultWidth(side: Side, viewport: number): number {
  if (side === "left") return LEFT_DEFAULT;
  return Math.round(Math.min(RIGHT_DEFAULT.max, Math.max(RIGHT_DEFAULT.min, viewport * RIGHT_DEFAULT.share)));
}

export interface Fit {
  /** The reader's width in pixels. */
  container: number;
  /** The width the other panel's column takes now, 0 when it has none. */
  other: number;
}

export function widthRange(side: Side, fit: Fit): { min: number; max: number } {
  const { min, max } = BOUNDS[side];
  return { min, max: Math.max(min, Math.min(max, fit.container - fit.other - MIN_DOCUMENT)) };
}

export function clampWidth(side: Side, requested: number, fit: Fit): number {
  const { min, max } = widthRange(side, fit);
  return Math.round(Math.min(max, Math.max(min, requested)));
}

/** The column widths in a computed `grid-template-columns` value. */
export function parseTracks(gridTemplateColumns: string): number[] {
  return gridTemplateColumns
    .split(/\s+/)
    .map((token) => (token.endsWith("px") ? Number.parseFloat(token) : Number.NaN))
    .filter((width) => Number.isFinite(width));
}
