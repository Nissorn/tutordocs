#!/usr/bin/env python3
"""Fails when a text pair in ui/src/styles.css drops below WCAG AA.

Reads the four theme blocks in styles.css (light, dark by media query, and the
two forced-theme selectors), resolves each OKLCH token, and checks the pairs the
app draws text and focus rings with. Hover and selected overlays carry alpha and
are composited over the surface they sit on. Exit 1 on any failure.
"""
from __future__ import annotations

import math
import re
import sys
from pathlib import Path

STYLES = Path(__file__).resolve().parent.parent / "ui" / "src" / "styles.css"

# (foreground, background, minimum ratio, what it is)
PAIRS = [
    ("--ink", "--paper", 7.0, "body text on paper"),
    ("--ink", "--sidebar-bg", 7.0, "text on the sidebar"),
    ("--ink-muted", "--paper", 4.5, "secondary text on paper"),
    ("--ink-muted", "--sidebar-bg", 4.5, "secondary text on the sidebar"),
    ("--ink-quiet", "--paper", 4.5, "helper and placeholder text on paper"),
    ("--ink-quiet", "--sidebar-bg", 4.5, "helper text on the sidebar"),
    ("--ink-quiet", "--code-bg", 4.5, "helper text on a code ground"),
    ("--ink-faint", "--paper", 3.0, "decorative marks and control borders"),
    ("--accent", "--paper", 4.5, "links on paper"),
    ("--accent-ink", "--accent-soft", 4.5, "the current chapter in the sidebar"),
    ("--focus", "--paper", 3.0, "focus ring on paper"),
    ("--focus", "--sidebar-bg", 3.0, "focus ring on the sidebar"),
    ("--danger", "--paper", 4.5, "error text"),
    ("--danger", "--bad-bg", 4.5, "error text on its tint"),
    ("--busy-ink", "--busy-bg", 4.5, "busy pill"),
    ("--ok-ink", "--ok-bg", 4.5, "done pill"),
    ("--warn-ink", "--paper", 4.5, "warning text"),
]


def oklch_to_srgb(lightness: float, chroma: float, hue: float) -> tuple[float, float, float]:
    a = chroma * math.cos(math.radians(hue))
    b = chroma * math.sin(math.radians(hue))
    l_ = lightness + 0.3963377774 * a + 0.2158037573 * b
    m_ = lightness - 0.1055613458 * a - 0.0638541728 * b
    s_ = lightness - 0.0894841775 * a - 1.2914855480 * b
    l, m, s = l_**3, m_**3, s_**3
    rgb = (
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    )

    def encode(value: float) -> float:
        value = max(0.0, min(1.0, value))
        return 12.92 * value if value <= 0.0031308 else 1.055 * value ** (1 / 2.4) - 0.055

    return tuple(encode(v) for v in rgb)  # type: ignore[return-value]


OKLCH = re.compile(r"oklch\(\s*([\d.]+)%\s+([\d.]+)\s+([\d.]+)(?:\s*/\s*([\d.]+))?\s*\)")


def parse(value: str) -> tuple[tuple[float, float, float], float]:
    match = OKLCH.fullmatch(value.strip())
    if not match:
        raise ValueError(f"not an oklch() colour: {value!r}")
    lightness, chroma, hue, alpha = match.groups()
    return oklch_to_srgb(float(lightness) / 100, float(chroma), float(hue)), float(alpha or 1)


def luminance(rgb: tuple[float, float, float]) -> float:
    def linear(v: float) -> float:
        return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4

    r, g, b = (linear(v) for v in rgb)
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def ratio(foreground: str, background: str) -> float:
    (fg, alpha), (bg, _) = parse(foreground), parse(background)
    mixed = tuple(alpha * f + (1 - alpha) * b for f, b in zip(fg, bg))
    hi, lo = sorted((luminance(mixed), luminance(bg)), reverse=True)
    return (hi + 0.05) / (lo + 0.05)


def theme_blocks(css: str) -> dict[str, dict[str, str]]:
    """The token maps of the four theme blocks, keyed by a readable name."""
    names = {
        ":root {": "light (:root)",
        ':root:not([data-theme="light"]) {': "dark (prefers-color-scheme)",
        ':root[data-theme="dark"] {': "dark (data-theme)",
        ':root[data-theme="light"] {': "light (data-theme)",
    }
    blocks: dict[str, dict[str, str]] = {}
    for opener, name in names.items():
        start = css.index(opener)
        end = css.index("\n  }" if "not(" in opener else "\n}", start)
        body = css[start:end]
        blocks[name] = dict(re.findall(r"(--[\w-]+):\s*([^;]+);", body))
    return blocks


def main() -> int:
    css = STYLES.read_text()
    failures = 0
    for name, tokens in theme_blocks(css).items():
        for fg, bg, minimum, what in PAIRS:
            value = ratio(tokens[fg], tokens[bg])
            if value < minimum:
                failures += 1
                print(f"FAIL {name}: {what}: {fg} on {bg} is {value:.2f}:1, needs {minimum}:1")
    if failures:
        print(f"check-contrast: {failures} pair(s) below WCAG AA")
        return 1
    print("check-contrast: every text and focus pair meets WCAG AA in all four theme blocks")
    return 0


if __name__ == "__main__":
    sys.exit(main())
