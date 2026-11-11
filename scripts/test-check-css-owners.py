#!/usr/bin/env python3
"""Tests for check-css-owners.py. Each case writes stylesheets to a temp folder
and runs the script on it, so the test sees what the gate sees."""

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent / "check-css-owners.py"


def run(files: dict[str, str]) -> subprocess.CompletedProcess:
    with tempfile.TemporaryDirectory() as tmp:
        for name, text in files.items():
            Path(tmp, name).write_text(text)
        return subprocess.run(
            [sys.executable, str(SCRIPT), tmp], capture_output=True, text=True
        )


class CheckCssOwners(unittest.TestCase):
    def test_one_owner_per_pair_passes(self):
        result = run({"a.css": ".x { color: red; }", "b.css": ".x { margin: 0; }"})
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_same_pair_in_two_files_fails_and_names_both(self):
        result = run({"a.css": ".x { color: red; }", "b.css": ".x { color: blue; }"})
        self.assertEqual(result.returncode, 1)
        self.assertIn(".x  color  set in a.css and b.css", result.stdout)

    def test_selector_lists_are_split(self):
        result = run({"a.css": ".x, .y { color: red; }", "b.css": ".y { color: blue; }"})
        self.assertEqual(result.returncode, 1)
        self.assertIn(".y  color", result.stdout)

    def test_media_blocks_are_flattened(self):
        result = run(
            {
                "a.css": "@media (max-width: 600px) { .x { display: none; } }",
                "b.css": ".x { display: flex; }",
            }
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn(".x  display", result.stdout)

    def test_keyframe_steps_are_skipped(self):
        result = run(
            {
                "a.css": "@keyframes a { 50% { opacity: 0.4; } }",
                "b.css": "@keyframes b { 50% { opacity: 0.2; } }",
            }
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_same_file_twice_is_fine(self):
        result = run({"a.css": ".x { color: red; } .x:hover { color: blue; } .x { color: green; }"})
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_comments_and_braces_in_strings_do_not_confuse_the_parser(self):
        result = run(
            {
                "a.css": '/* .x { color: red; } */ .y::before { content: "{"; }',
                "b.css": ".x { color: blue; }",
            }
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
