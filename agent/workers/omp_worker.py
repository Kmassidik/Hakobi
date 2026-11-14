#!/usr/bin/env python3
"""Adapter that lets an OMP coding agent operate on the isolated workspace."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path


def _worker_command(executable: str, workspace: Path, prompt: str) -> list[str]:
    command = [
        executable,
        "-p",
        "--mode=json",
        "--no-session",
        "--no-context-files",
        "--no-skills",
        "--no-rules",
        "--auto-approve",
        "--cwd",
        str(workspace),
        "--tools=read,write,edit,bash,glob,grep",
        "--max-time=300",
        "--max-cost=0.50",
    ]
    model = os.environ.get("HAKOBI_OMP_MODEL")
    if model:
        command.extend(["--model", model])
    command.append(prompt)
    return command


def main() -> int:
    executable = shutil.which("omp")
    if executable is None:
        print("omp is not installed", file=sys.stderr)
        return 127
    workspace = Path(os.environ["HAKOBI_WORKSPACE"])
    prompt = Path(os.environ["HAKOBI_PROMPT_FILE"]).read_text(encoding="utf-8")
    command = _worker_command(executable, workspace, prompt)
    result = subprocess.run(command, cwd=workspace, text=True, capture_output=True)

    input_tokens = 0
    output_tokens = 0
    for line in result.stdout.splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if event.get("type") == "turn_end":
            usage = (event.get("message") or {}).get("usage") or {}
            input_tokens += int(usage.get("input", 0))
            output_tokens += int(usage.get("output", 0))
        if event.get("type") == "message_end":
            message = event.get("message") or {}
            if message.get("role") == "assistant":
                for part in message.get("content", []):
                    if part.get("type") == "text":
                        print(part.get("text", ""))
    if result.stderr:
        print(result.stderr, file=sys.stderr)
    usage_file = os.environ.get("HAKOBI_USAGE_FILE")
    if usage_file:
        Path(usage_file).write_text(
            json.dumps({"input_tokens": input_tokens, "output_tokens": output_tokens}),
            encoding="utf-8",
        )
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
