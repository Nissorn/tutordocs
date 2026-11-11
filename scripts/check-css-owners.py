#!/usr/bin/env python3
"""Fail when one selector sets the same property in two stylesheets.

Component stylesheets load after styles.css, so two files that set the same
property on the same selector are settled by load order and win or lose
silently. Each selector and property pair gets one owner file.

@media, @supports and @layer blocks are flattened, so a pair set in one file
at one width and in another file at another width still counts. @keyframes
and @font-face blocks are skipped, since their steps are not selectors.

Usage:
    python3 scripts/check-css-owners.py          # scan ui/src
    python3 scripts/check-css-owners.py DIR      # scan another folder

Exit status is 1 when a pair is set in more than one file.
"""

from __future__ import annotations

import re
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SKIPPED_AT_RULES = ("@keyframes", "@-webkit-keyframes", "@font-face")


def strip_comments(text: str) -> str:
    return re.sub(r"/\*.*?\*/", "", text, flags=re.S)


def split_top_level(text: str, sep: str) -> list[str]:
    parts: list[str] = []
    depth = 0
    quote = ""
    start = 0
    for i, ch in enumerate(text):
        if quote:
            if ch == quote:
                quote = ""
        elif ch in "\"'":
            quote = ch
        elif ch in "([":
            depth += 1
        elif ch in ")]":
            depth -= 1
        elif ch == sep and depth == 0:
            parts.append(text[start:i])
            start = i + 1
    parts.append(text[start:])
    return parts


def matching_brace(text: str, open_at: int) -> int:
    depth = 0
    quote = ""
    for i in range(open_at, len(text)):
        ch = text[i]
        if quote:
            if ch == quote:
                quote = ""
        elif ch in "\"'":
            quote = ch
        elif ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return i
    raise ValueError("unbalanced braces")


def declared_properties(body: str) -> list[str]:
    props = []
    for decl in split_top_level(body, ";"):
        if ":" not in decl:
            continue
        name = decl.split(":", 1)[0].strip().lower()
        if name:
            props.append(name)
    return props


def rules(text: str) -> list[tuple[str, str]]:
    """Return (selector, property) pairs, with at-rule blocks flattened."""
    found: list[tuple[str, str]] = []
    pos = 0
    while pos < len(text):
        brace = text.find("{", pos)
        semi = text.find(";", pos)
        if brace == -1:
            break
        if 0 <= semi < brace:
            pos = semi + 1
            continue
        prelude = text[pos:brace].strip()
        end = matching_brace(text, brace)
        body = text[brace + 1 : end]
        pos = end + 1
        if prelude.startswith("@"):
            if prelude.lower().startswith(SKIPPED_AT_RULES):
                continue
            found.extend(rules(body))
            continue
        if "{" in re.sub(r"\"[^\"]*\"|'[^']*'", "", body):
            raise ValueError(f"nested rule inside '{prelude}' is not supported")
        props = declared_properties(body)
        for selector in split_top_level(prelude, ","):
            selector = " ".join(selector.split())
            found.extend((selector, prop) for prop in props)
    return found


def find_conflicts(folder: Path) -> list[tuple[str, str, list[str]]]:
    owners: dict[tuple[str, str], set[str]] = defaultdict(set)
    for path in sorted(folder.glob("*.css")):
        for pair in rules(strip_comments(path.read_text())):
            owners[pair].add(path.name)
    return [
        (selector, prop, sorted(files))
        for (selector, prop), files in sorted(owners.items())
        if len(files) > 1
    ]


def main(argv: list[str]) -> int:
    folder = Path(argv[1]) if len(argv) > 1 else ROOT / "ui" / "src"
    conflicts = find_conflicts(folder)
    for selector, prop, files in conflicts:
        print(f"{selector}  {prop}  set in {' and '.join(files)}")
    if conflicts:
        print(
            f"check-css-owners: {len(conflicts)} selector and property pair(s) "
            "set in more than one file. Keep one owner file for each.",
            file=sys.stderr,
        )
        return 1
    print("check-css-owners: every selector and property pair has one owner file")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
