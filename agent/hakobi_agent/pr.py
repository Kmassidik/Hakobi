from __future__ import annotations

import subprocess
from pathlib import Path

from .memory import RunStore, utc_now


class PRGateError(RuntimeError):
    pass


def _run(argv: list[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(argv, cwd=cwd, text=True, capture_output=True, timeout=120)


def approve_pr(run_dir: str | Path, yes: bool = False) -> str:
    store = RunStore(Path(run_dir))
    state = store.load()
    if state.get("status") != "awaiting_approval":
        raise PRGateError("run is not awaiting approval")
    workspace = Path(state["workspace"])
    branch = state["branch"]
    if not (workspace / ".git").exists():
        raise PRGateError("workspace is not a git checkout")
    remote = _run(["git", "remote", "get-url", "origin"], workspace)
    if remote.returncode != 0:
        raise PRGateError(
            "workspace has no origin remote; snapshot mode cannot open a real PR. "
            "Use worktree mode against a clean GitHub checkout."
        )
    status = _run(["git", "status", "--porcelain"], workspace)
    if status.stdout.strip():
        raise PRGateError("workspace is dirty after verification; refusing to open PR")
    if not yes:
        answer = input(f"Type OPEN to push {branch} and create the PR: ")
        if answer != "OPEN":
            raise PRGateError("PR approval cancelled")
    push = _run(["git", "push", "-u", "origin", branch], workspace)
    if push.returncode != 0:
        raise PRGateError(push.stderr.strip() or push.stdout.strip())
    body = Path(run_dir) / "PR.md"
    if not body.is_file():
        raise PRGateError("PR.md is missing")
    args = [
        "gh",
        "pr",
        "create",
        "--base",
        state.get("pr_base", "main"),
        "--head",
        branch,
        "--title",
        state["title"],
        "--body-file",
        str(body),
    ]
    created = _run(args, workspace)
    if created.returncode != 0:
        raise PRGateError(created.stderr.strip() or created.stdout.strip())
    url = created.stdout.strip()
    state["status"] = "pr_opened"
    state["pr_url"] = url
    state["updated_at"] = utc_now()
    store.save(state)
    return url
