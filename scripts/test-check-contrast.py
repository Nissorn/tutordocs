#!/usr/bin/env python3
"""Tests for scripts/check-contrast.py."""
import importlib.util
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location("check_contrast", Path(__file__).with_name("check-contrast.py"))
check = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check)


class RatioTests(unittest.TestCase):
    def test_black_on_white_is_twenty_one(self):
        self.assertAlmostEqual(check.ratio("oklch(0% 0 0)", "oklch(100% 0 0)"), 21.0, places=1)

    def test_same_colour_is_one(self):
        self.assertAlmostEqual(check.ratio("oklch(50% 0.1 80)", "oklch(50% 0.1 80)"), 1.0, places=3)

    def test_alpha_is_composited_over_the_background(self):
        solid = check.ratio("oklch(24% 0.012 65)", "oklch(98% 0.006 85)")
        faded = check.ratio("oklch(24% 0.012 65 / 0.4)", "oklch(98% 0.006 85)")
        self.assertLess(faded, solid)

    def test_a_light_grey_on_paper_fails_body_text(self):
        self.assertLess(check.ratio("oklch(62% 0.012 75)", "oklch(98.2% 0.006 85)"), 4.5)

    def test_rejects_a_colour_it_cannot_read(self):
        with self.assertRaises(ValueError):
            check.parse("#ffffff")


class ShippedTokensTests(unittest.TestCase):
    def test_the_stylesheet_has_all_four_theme_blocks(self):
        blocks = check.theme_blocks(check.STYLES.read_text())
        self.assertEqual(len(blocks), 4)
        for tokens in blocks.values():
            for fg, bg, _, _ in check.PAIRS:
                self.assertIn(fg, tokens)
                self.assertIn(bg, tokens)

    def test_the_shipped_tokens_pass(self):
        self.assertEqual(check.main(), 0)


if __name__ == "__main__":
    unittest.main()
