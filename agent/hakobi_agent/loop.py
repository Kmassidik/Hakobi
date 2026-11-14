from __future__ import annotations

import hashlib
import json
import shlex
import sys
from pathlib import Path

from .checks import CheckRunner, failure_summary, format_feedback
from .memory import RunStore, utc_now
from .policy import PolicyError
from .task import TaskSpec, load_task
from .tools import ToolError, run_worker
from .workspace import Workspace, WorkspaceError, prepare_workspace

AGENT_ROOT = Path(__file__).resolve().parents[1]


def _branch_name(task: TaskSpec, run_id: str) -> str:
    safe = "".join(character if character.isalnum() or character in "-_." else "-" for character in task.id)
    return f"hakobi/{safe}-{run_id.split('-')[-1]}"


def _usage_from_file(path: Path) -> int:
    if not path.is_file():
        return 0
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
        return int(data.get("input_tokens", 0)) + int(data.get("output_tokens", 0))
    except (OSError, ValueError, TypeError, json.JSONDecodeError):
        raise ToolError("worker usage file must contain numeric input_tokens/output_tokens")


def _attempt_prompt(task: TaskSpec, attempt: int, feedback: str | None) -> str:
    commands = "\n".join(" ".join(command) for command in task.allowed_commands)
    feedback_block = feedback or "None. This is the first attempt."
    return f"""# Hakobi agent task: {task.title}

{task.prompt}

## Attempt {attempt}

## Harness feedback

{feedback_block}

## Hard walls

- Work only inside the current workspace.
- Edit only files matching: {", ".join(task.allowed_files)}
- Allowed shell prefixes:
{commands}
- Do not access secrets, parent directories, the original repository, or the network.
- Do not open a PR. The harness commits only after independent checks pass.
- Your own claim of success is not evidence. The harness runs tests and diff checks.

When finished, leave the workspace changes in place and exit.
"""


def _write_attempt_artifacts(store: RunStore, attempt: int, prompt: str, feedback: str | None) -> tuple[Path, Path]:
    prompt_path = store.write_text(f"prompt-attempt-{attempt}.md", prompt)
    feedback_path = store.write_text(f"feedback-attempt-{attempt}.md", feedback or "None\n")
    return prompt_path, feedback_path


def _new_task_state(task: TaskSpec, store: RunStore) -> dict:
    return {
        "version": 1,
        "task_id": task.id,
        "title": task.title,
        "status": "preparing",
        "run_dir": str(store.run_dir),
        "workspace": None,
        "workspace_mode": None,
        "branch": None,
        "baseline_commit": None,
        "commit": None,
        "pr_base": task.pr_base,
        "attempts": [],
        "baseline_checks": [],
        "feedback": None,
        "dead_ends": [],
        "tokens_used": 0,
        "token_budget": task.token_budget,
        "exit_reason": None,
        "created_at": utc_now(),
    }


def _prepare_task_workspace(
    task: TaskSpec, store: RunStore, state: dict, branch: str
) -> Workspace | None:
    try:
        workspace = prepare_workspace(task, store.run_dir / "workspace", branch)
    except WorkspaceError as error:
        state["status"] = "error"
        state["exit_reason"] = str(error)
        store.save(state)
        return None
    state["workspace"] = str(workspace.path)
    state["workspace_mode"] = workspace.mode
    state["baseline_commit"] = workspace.baseline_commit
    store.save(state)
    return workspace


def _run_baseline(task: TaskSpec, workspace: Workspace, store: RunStore, state: dict) -> bool:
    baseline = CheckRunner(task, workspace).run("baseline")
    state["baseline_checks"] = [result.as_dict() for result in baseline]
    if all(result.passed for result in baseline):
        return True
    state["status"] = "invalid_task"
    state["exit_reason"] = "baseline checks did not match the declared pre-fix state"
    store.save(state)
    return False


def _resolve_worker(task: TaskSpec, worker_override: str | None) -> tuple[str, ...]:
    if not worker_override:
        return task.resolve_worker()
    worker_args = shlex.split(worker_override)
    return tuple(
        str(Path(argument).resolve()) if Path(argument).is_file() else argument
        for argument in worker_args
    )


def _attempt_environment(
    task: TaskSpec,
    workspace: Workspace,
    store: RunStore,
    state: dict,
    branch: str,
    attempt: int,
    prompt_path: Path,
    feedback_path: Path,
    usage_path: Path,
) -> dict[str, str]:
    return {
        "HAKOBI_WORKSPACE": str(workspace.path),
        "HAKOBI_PROMPT_FILE": str(prompt_path),
        "HAKOBI_FEEDBACK_FILE": str(feedback_path),
        "HAKOBI_STATE_FILE": str(store.state_path),
        "HAKOBI_TASK_FILE": str(task.task_file),
        "HAKOBI_BASELINE_COMMIT": workspace.baseline_commit,
        "HAKOBI_BRANCH": branch,
        "HAKOBI_USAGE_FILE": str(usage_path),
        "HAKOBI_ATTEMPT": str(attempt),
        "HAKOBI_TOKEN_BUDGET_REMAINING": str(max(0, task.token_budget - state["tokens_used"])),
        "PYTHONPATH": str(AGENT_ROOT),
    }


def _invoke_worker(
    worker_command: tuple[str, ...],
    workspace: Workspace,
    environment: dict[str, str],
    timeout: int,
    log_path: Path,
    usage_path: Path,
    state: dict,
) -> tuple[int | None, str | None]:
    worker_exit: int | None = None
    worker_error: str | None = None
    try:
        result = run_worker(worker_command, workspace.path, environment, timeout, log_path)
        worker_exit = result.returncode
    except TimeoutError as error:
        worker_error = str(error)
    except ToolError as error:
        worker_error = str(error)
    if usage_path.is_file():
        try:
            state["tokens_used"] += _usage_from_file(usage_path)
        except ToolError as error:
            worker_error = str(error)
    return worker_exit, worker_error


def _execute_attempt_worker(
    task: TaskSpec,
    workspace: Workspace,
    store: RunStore,
    state: dict,
    worker_command: tuple[str, ...],
    branch: str,
    attempt: int,
) -> tuple[int | None, str | None, Path]:
    feedback = state["feedback"]
    prompt = _attempt_prompt(task, attempt, feedback)
    prompt_path, feedback_path = _write_attempt_artifacts(store, attempt, prompt, feedback)
    usage_path = store.run_dir / f"usage-attempt-{attempt}.json"
    log_path = store.run_dir / f"worker-attempt-{attempt}.log"
    environment = _attempt_environment(
        task, workspace, store, state, branch, attempt, prompt_path, feedback_path, usage_path
    )
    print(f"attempt {attempt}: worker start", file=sys.stderr)
    worker_exit, worker_error = _invoke_worker(
        worker_command, workspace, environment, task.worker_timeout, log_path, usage_path, state
    )
    return worker_exit, worker_error, log_path


def _record_worker_error(
    state: dict, attempt: int, worker_exit: int | None, worker_error: str
) -> None:
    state["attempts"].append(
        {
            "number": attempt,
            "worker_exit": worker_exit,
            "worker_error": worker_error,
            "checks": [],
        }
    )
    state["status"] = "needs_human"
    state["exit_reason"] = worker_error


def _record_check_attempt(
    state: dict, attempt: int, worker_exit: int | None, log_path: Path, results: list
) -> None:
    state["attempts"].append(
        {
            "number": attempt,
            "worker_exit": worker_exit,
            "worker_log": log_path.name,
            "checks": [result.as_dict() for result in results],
        }
    )


def _stop_attempt(state: dict, reason: str) -> None:
    state["status"] = "needs_human"
    state["exit_reason"] = reason


def _complete_attempt(
    task: TaskSpec, workspace: Workspace, store: RunStore, state: dict, results: list
) -> None:
    state["commit"] = workspace.commit(f"agent: {task.title}")
    state["status"] = "awaiting_approval"
    state["feedback"] = None
    state["exit_reason"] = "checks passed; waiting for human PR approval"
    store.save(state)
    _write_pr_preview(store, task, state, results, workspace)


def _record_feedback(task: TaskSpec, state: dict, results: list) -> None:
    state["feedback"] = format_feedback(task, results, state["dead_ends"])
    summary = failure_summary(results)
    if summary and summary not in state["dead_ends"]:
        state["dead_ends"].append(summary)


def _run_attempt(
    task: TaskSpec,
    workspace: Workspace,
    store: RunStore,
    state: dict,
    worker_command: tuple[str, ...],
    branch: str,
    attempt: int,
    attempt_limit: int,
    previous_signature: str,
) -> tuple[bool, str]:
    worker_exit, worker_error, log_path = _execute_attempt_worker(
        task, workspace, store, state, worker_command, branch, attempt
    )
    if worker_error:
        _record_worker_error(state, attempt, worker_exit, worker_error)
        return False, previous_signature
    results = CheckRunner(task, workspace).run("after")
    _record_check_attempt(state, attempt, worker_exit, log_path, results)
    signature = workspace.diff_digest()
    if state["tokens_used"] > task.token_budget:
        state["feedback"] = "Token budget exceeded."
        _stop_attempt(state, "token budget exceeded")
        return False, previous_signature
    if all(result.passed for result in results):
        _complete_attempt(task, workspace, store, state, results)
        return False, previous_signature
    _record_feedback(task, state, results)
    if signature == previous_signature:
        _stop_attempt(state, "identical diff and check result repeated twice")
        return False, previous_signature
    if attempt >= attempt_limit:
        _stop_attempt(state, "maximum attempts reached")
        return False, previous_signature
    store.save(state)
    return True, signature


def run_task(
    task_path: str | Path,
    worker_override: str | None = None,
    runs_root: str | Path = "agent/runs",
    max_attempts: int | None = None,
) -> tuple[dict, Path]:
    task = load_task(task_path)
    store = RunStore.create(Path(runs_root), task.id)
    state = _new_task_state(task, store)
    store.save(state)
    store.write_text("task.md", task.prompt)
    branch = _branch_name(task, store.run_dir.name)
    state["branch"] = branch
    workspace = _prepare_task_workspace(task, store, state, branch)
    if workspace is None:
        return state, store.run_dir
    if not _run_baseline(task, workspace, store, state):
        return state, store.run_dir
    worker_command = _resolve_worker(task, worker_override)
    attempt_limit = max_attempts or task.max_attempts
    previous_signature = ""
    state["status"] = "running"
    store.save(state)

    for attempt in range(1, attempt_limit + 1):
        if state["tokens_used"] >= task.token_budget:
            _stop_attempt(state, "token budget exhausted before the next attempt")
            break
        should_continue, previous_signature = _run_attempt(
            task, workspace, store, state, worker_command, branch, attempt, attempt_limit, previous_signature
        )
        if state["status"] == "awaiting_approval":
            return state, store.run_dir
        if not should_continue:
            break
    store.save(state)
    return state, store.run_dir


def _write_pr_preview(store: RunStore, task: TaskSpec, state: dict, results, workspace) -> None:
    changed = workspace.changed_files()
    checks = "\n".join(
        f"- [{'x' if result.passed else ' '}] `{result.id}` — {result.expected}"
        for result in results
    )
    body = (
        f"# {task.title}\n\n"
        f"Task: `{task.id}`\n\n"
        f"## What changed\n\n"
        + "\n".join(f"- `{path}`" for path in changed)
        + "\n\n## Verification\n\n"
        + checks
        + f"\n\nCommit: `{state.get('commit')}`\n\n"
        "Generated by the Hakobi Level 1–4 agent harness. The PR remains gated until human approval.\n"
    )
    store.write_text("PR.md", body)
