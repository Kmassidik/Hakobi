#!/usr/bin/env python3
"""Deterministic fixture worker used to exercise the harness and feedback loop."""

from __future__ import annotations

import json
import os
from pathlib import Path

from hakobi_agent.task import load_task
from hakobi_agent.tools import ToolBox
from hakobi_agent.workspace import Workspace


def main() -> int:
    task = load_task(os.environ["HAKOBI_TASK_FILE"])
    workspace = Workspace(
        path=Path(os.environ["HAKOBI_WORKSPACE"]),
        baseline_commit=os.environ["HAKOBI_BASELINE_COMMIT"],
        branch=os.environ["HAKOBI_BRANCH"],
        mode="snapshot",
    )
    tools = ToolBox(task, workspace)
    attempt = int(os.environ.get("HAKOBI_ATTEMPT", "1"))

    baseline = tools.run_tests()
    print("baseline tests:")
    print(baseline.output)

    content = tools.read_file("app.py")
    if attempt == 1 and "return left - right" in content:
        content = content.replace("return left - right", "return left * right")
        tools.edit_file("app.py", content)
        print("attempt 1 applied an intentionally incorrect fix")
    elif "return left * right" in content:
        content = content.replace("return left * right", "return left + right")
        tools.edit_file("app.py", content)
        print("attempt 2 corrected the fix")
    elif "return left - right" in content:
        content = content.replace("return left - right", "return left + right")
        tools.edit_file("app.py", content)
        print("applied the correct fix")

    after = tools.run_tests()
    print("tests after edit:")
    print(after.output)

    usage_file = os.environ.get("HAKOBI_USAGE_FILE")
    if usage_file:
        Path(usage_file).write_text(
            json.dumps({"input_tokens": 120, "output_tokens": 30}),
            encoding="utf-8",
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
