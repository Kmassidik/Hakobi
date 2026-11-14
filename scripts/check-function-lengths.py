#!/usr/bin/env python3
"""Reject functions over 50 lines using syntax trees, not text guesses."""

from pathlib import Path
import subprocess

import tree_sitter_javascript as javascript
import tree_sitter_python as python
import tree_sitter_rust as rust
from tree_sitter import Language, Parser

MAX_LINES = 50
LANGUAGES = {
    ".rs": (rust, {"function_item"}),
    ".py": (python, {"function_definition"}),
    ".js": (javascript, {
        "function_declaration",
        "method_definition",
        "function_expression",
        "arrow_function",
    }),
    ".mjs": (javascript, {
        "function_declaration",
        "method_definition",
        "function_expression",
        "arrow_function",
    }),
}


def check_node(node, kinds, source, path, violations):
    if node.type in kinds:
        lines = node.end_point[0] - node.start_point[0] + 1
        if lines > MAX_LINES:
            name_node = node.child_by_field_name("name")
            name = node.type
            if name_node:
                name = source[name_node.start_byte:name_node.end_byte].decode()
            violations.append((path, node.start_point[0] + 1, name, lines))
    for child in node.children:
        check_node(child, kinds, source, path, violations)


def check_file(path, module, kinds):
    source = Path(path).read_bytes()
    parser = Parser(Language(module.language()))
    violations = []
    check_node(parser.parse(source).root_node, kinds, source, path, violations)
    return violations


def main():
    violations = []
    files = subprocess.check_output(["git", "ls-files"], text=True).splitlines()
    for path in files:
        if Path(path).suffix in LANGUAGES:
            module, kinds = LANGUAGES[Path(path).suffix]
            violations.extend(check_file(path, module, kinds))
    for path, line, name, lines in sorted(violations):
        print(f"{path}:{line}: {name} spans {lines} lines (maximum {MAX_LINES})")
    print(f"Functions over {MAX_LINES} lines: {len(violations)}")
    return int(bool(violations))


if __name__ == "__main__":
    raise SystemExit(main())
