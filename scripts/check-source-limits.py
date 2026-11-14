#!/usr/bin/env python3
"""Reject tracked source files that exceed the project readability limit."""

from __future__ import annotations

import subprocess
from pathlib import Path

MAX_LINES = 500
SOURCE_SUFFIXES = {
    ".css",
    ".html",
    ".js",
    ".mjs",
    ".py",
    ".rs",
    ".sh",
    ".sql",
    ".toml",
    ".ts",
    ".tsx",
}


def tracked_sources() -> list[str]:
    output = subprocess.check_output(["git", "ls-files"], text=True)
    return [
        name
        for name in output.splitlines()
        if Path(name).suffix in SOURCE_SUFFIXES and name != "Cargo.lock"
    ]


def main() -> int:
    oversized = []
    for name in tracked_sources():
        lines = len(Path(name).read_text(encoding="utf-8").splitlines())
        if lines > MAX_LINES:
            oversized.append((name, lines))
    if oversized:
        for name, lines in sorted(oversized):
            print(f"{name}: {lines} lines (maximum {MAX_LINES})")
        return 1
    print(f"All tracked source files are at most {MAX_LINES} lines.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
