from __future__ import annotations

import json
import subprocess
from pathlib import Path

from .loop import run_task
from .memory import RunStore, utc_now


class PlanError(RuntimeError):
    pass


def load_plan(path: str | Path) -> dict:
    plan_file = Path(path).resolve()
    try:
        data = json.loads(plan_file.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise PlanError(f"cannot read plan {plan_file}: {error}") from error
    if not isinstance(data, dict):
        raise PlanError("plan must be a JSON object")
    if data.get("version") != 1:
        raise PlanError("unsupported plan version")
    name = data.get("name")
    tasks = data.get("tasks")
    if not isinstance(name, str) or not name.strip():
        raise PlanError("plan name must be a non-empty string")
    if not isinstance(tasks, list) or not tasks or not all(isinstance(item, str) for item in tasks):
        raise PlanError("plan tasks must be a non-empty string list")
    resolved = []
    for item in tasks:
        task_path = (plan_file.parent / item).resolve()
        if not task_path.is_file():
            raise PlanError(f"task does not exist: {task_path}")
        resolved.append(task_path)
    return {"name": name, "file": plan_file, "tasks": resolved}


def _apply_patch(repo: Path, patch: str, patch_path: Path) -> None:
    if not patch.strip():
        raise PlanError("task produced an empty patch")
    patch_path.write_text(patch, encoding="utf-8")
    patch_path.chmod(0o600)
    check = subprocess.run(
        ["git", "-C", str(repo), "apply", "--check", "--whitespace=nowarn", "-"],
        input=patch,
        text=True,
        capture_output=True,
    )
    if check.returncode != 0:
        raise PlanError(f"patch does not apply to {repo}: {check.stderr.strip()}")
    applied = subprocess.run(
        ["git", "-C", str(repo), "apply", "--whitespace=nowarn", "-"],
        input=patch,
        text=True,
        capture_output=True,
    )
    if applied.returncode != 0:
        raise PlanError(f"patch apply failed for {repo}: {applied.stderr.strip()}")


def run_plan(
    plan_path: str | Path,
    worker_override: str | None = None,
    runs_root: str | Path = "agent/runs",
    max_tasks: int | None = None,
) -> tuple[dict, Path]:
    plan = load_plan(plan_path)
    store = RunStore.create(Path(runs_root), f"plan-{plan['name']}")
    state = {
        "version": 1,
        "plan": plan["name"],
        "plan_file": str(plan["file"]),
        "status": "running",
        "tasks": [],
        "created_at": utc_now(),
        "exit_reason": None,
    }
    store.save(state)
    store.write_text("plan.md", "# Plan: " + plan["name"] + "\n\n" + "\n".join(
        f"- `{task}`" for task in plan["tasks"]
    ) + "\n")
    limit = max_tasks if max_tasks is not None else len(plan["tasks"])
    if limit < 1:
        raise PlanError("max_tasks must be positive")
    total = len(plan["tasks"])
    state["planned_tasks"] = total

    for index, task_path in enumerate(plan["tasks"][:limit], start=1):
        task_state, task_run_dir = run_task(
            task_path,
            worker_override=worker_override,
            runs_root=store.run_dir / "tasks",
        )
        record = {
            "number": index,
            "task": str(task_path),
            "run": str(task_run_dir),
            "status": task_state["status"],
            "reason": task_state.get("exit_reason"),
        }
        if task_state["status"] != "awaiting_approval":
            state["tasks"].append(record)
            state["status"] = "failed"
            state["exit_reason"] = f"task {index} stopped with {task_state['status']}"
            store.save(state)
            return state, store.run_dir

        from .task import load_task

        task = load_task(task_path)
        workspace_path = Path(task_state["workspace"])
        patch = subprocess.run(
            [
                "git",
                "-C",
                str(workspace_path),
                "diff",
                str(task_state["baseline_commit"]),
                str(task_state["commit"]),
            ],
            text=True,
            capture_output=True,
        )
        if patch.returncode != 0:
            record["status"] = "patch_failed"
            record["reason"] = patch.stderr.strip()
            state["tasks"].append(record)
            state["status"] = "failed"
            state["exit_reason"] = f"could not extract patch for task {index}"
            store.save(state)
            return state, store.run_dir

        patch_path = store.run_dir / f"applied-{index:03d}.patch"
        try:
            _apply_patch(task.repo, patch.stdout, patch_path)
        except PlanError as error:
            record["status"] = "apply_failed"
            record["reason"] = str(error)
            state["tasks"].append(record)
            state["status"] = "failed"
            state["exit_reason"] = f"task {index} patch was not applied"
            store.save(state)
            return state, store.run_dir

        record["patch"] = patch_path.name
        record["applied"] = True
        state["tasks"].append(record)
        store.save(state)

    state["status"] = "completed" if limit >= total else "partial"
    state["exit_reason"] = (
        "all planned tasks applied"
        if limit >= total
        else f"applied {limit} of {total} planned tasks"
    )
    store.save(state)
    return state, store.run_dir
