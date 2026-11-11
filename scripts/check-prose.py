#!/usr/bin/env python3
"""Mechanical prose guardrail for TutorDocs markdown.

Catches two failures observed repeatedly in this project's sessions:
stray foreign-script fragments inside Thai prose, and em/en dashes banned
by the unslop style rule. Both are fixed character-set patterns, so they
belong in a check rather than in a reviewer's judgement.

Usage:
    python3 scripts/check-prose.py            # scan all markdown
    python3 scripts/check-prose.py FILE ...   # scan specific files
    python3 scripts/check-prose.py --strict   # dashes fail too

Exit status is 1 when a hard failure is found, 0 otherwise. Dashes are a
warning by default because fixing them needs sentence-level judgement;
--strict makes them a hard failure for final deliverables.
"""

from __future__ import annotations

import argparse
import sys
import unicodedata
from pathlib import Path

THAI_RANGE = range(0x0E00, 0x0E80)
ALLOWED_NON_ASCII = set("‘’“”…⅓≥≤→✗©×")
DASHES = {"\u2014": "em dash", "\u2013": "en dash"}
ROOT = Path(__file__).resolve().parent.parent
SKIP_DIRS = {".git", "node_modules", "target", "dist", "build", ".venv"}


def scan(text: str) -> tuple[dict[str, list[int]], dict[str, list[int]]]:
    """Return stray characters and dashes, each mapped to 1-based line numbers."""
    stray: dict[str, list[int]] = {}
    dashes: dict[str, list[int]] = {}
    for lineno, line in enumerate(text.splitlines(), start=1):
        for ch in line:
            code = ord(ch)
            if code < 128 or code in THAI_RANGE or ch in ALLOWED_NON_ASCII:
                continue
            if ch in DASHES:
                dashes.setdefault(ch, []).append(lineno)
            else:
                stray.setdefault(ch, []).append(lineno)
    return stray, dashes


def describe(ch: str) -> str:
    return f"U+{ord(ch):04X} {unicodedata.name(ch, 'UNNAMED')}"


def report(path: Path, stray: dict[str, list[int]], dashes: dict[str, list[int]]) -> bool:
    """Print findings for one file. Return True when a hard failure was found."""
    rel = path.relative_to(ROOT) if path.is_relative_to(ROOT) else path
    failed = False
    for ch, lines in sorted(stray.items()):
        failed = True
        shown = ", ".join(str(n) for n in lines[:8])
        more = f" (+{len(lines) - 8} more)" if len(lines) > 8 else ""
        print(f"FAIL {rel}: stray {describe(ch)} x{len(lines)} at line {shown}{more}")
    for ch, lines in sorted(dashes.items()):
        shown = ", ".join(str(n) for n in lines[:8])
        more = f" (+{len(lines) - 8} more)" if len(lines) > 8 else ""
        print(f"WARN {rel}: {DASHES[ch]} x{len(lines)} at line {shown}{more}")
    return failed


def skipped(path: Path) -> bool:
    """Ignore build and dependency dirs, but only for paths inside the repo."""
    try:
        parts = path.relative_to(ROOT).parts
    except ValueError:
        return False
    return bool(SKIP_DIRS.intersection(parts))


def collect(paths: list[str]) -> list[Path]:
    if not paths:
        return sorted(p for p in ROOT.rglob("*.md") if not skipped(p))
    found: list[Path] = []
    for raw in paths:
        path = Path(raw).resolve()
        if path.is_dir():
            found.extend(p for p in path.rglob("*.md") if not skipped(p))
        else:
            found.append(path)
    return sorted(set(found))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("files", nargs="*", help="markdown files or directories; default scans the repo")
    parser.add_argument("--strict", action="store_true", help="treat dashes as failures")
    args = parser.parse_args()

    files = collect(args.files)
    if not files:
        print("no markdown found")
        return 0

    hard = 0
    dash_total = 0
    for path in files:
        if not path.is_file():
            print(f"FAIL {path}: not a file")
            hard += 1
            continue
        stray, dashes = scan(path.read_text(encoding="utf-8"))
        if report(path, stray, dashes):
            hard += 1
        count = sum(len(v) for v in dashes.values())
        dash_total += count
        if args.strict and count:
            hard += 1

    print(f"\n{len(files)} files scanned, {hard} failing, {dash_total} dashes")
    return 1 if hard else 0


if __name__ == "__main__":
    sys.exit(main())