from __future__ import annotations

import fnmatch
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .policy import CommandPolicy, PathPolicy


class TaskError(ValueError):
    pass


@dataclass(frozen=True)
class CheckSpec:
    kind: str
    id: str
    argv: tuple[str, ...] = ()
    baseline: str = "pass"
    timeout: int = 60
    allowed: tuple[str, ...] = ()
    expect_output: str | None = None


@dataclass(frozen=True)
class TaskSpec:
    id: str
    title: str
    task_file: Path
    task_dir: Path
    repo: Path
    prompt_file: Path
    prompt: str
    worker: tuple[str, ...]
    allowed_commands: tuple[tuple[str, ...], ...]
    allowed_files: tuple[str, ...]
    sensitive_patterns: tuple[str, ...]
    checks: tuple[CheckSpec, ...]
    max_attempts: int
    token_budget: int
    worker_timeout: int
    workspace_mode: str
    pr_base: str

    @property
    def command_policy(self) -> CommandPolicy:
        return CommandPolicy(self.allowed_commands)

    @property
    def path_policy(self) -> PathPolicy:
        return PathPolicy(self.allowed_files, self.sensitive_patterns)

    @property
    def test_command(self) -> tuple[str, ...] | None:
        for check in self.checks:
            if check.kind == "command" and check.baseline == "fail":
                return check.argv
        return None

    def resolve_worker(self, task_dir: Path | None = None) -> tuple[str, ...]:
        base = task_dir or self.task_dir
        resolved: list[str] = []
        for argument in self.worker:
            if argument.startswith(("./", "../")):
                candidate = (base / argument).resolve()
                resolved.append(str(candidate))
            else:
                resolved.append(argument)
        return tuple(resolved)


def _require(data: dict[str, Any], key: str, expected: type) -> Any:
    value = data.get(key)
    if not isinstance(value, expected):
        raise TaskError(f"task field {key!r} must be {expected.__name__}")
    return value


def load_task(path: str | Path) -> TaskSpec:
    task_file = Path(path).resolve()
    try:
        data = json.loads(task_file.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise TaskError(f"cannot read task {task_file}: {error}") from error
    if not isinstance(data, dict):
        raise TaskError("task must be a JSON object")
    if data.get("version") != 1:
        raise TaskError("unsupported task version")

    task_dir = task_file.parent
    prompt_file = (task_dir / _require(data, "prompt", str)).resolve()
    try:
        prompt = prompt_file.read_text(encoding="utf-8")
    except OSError as error:
        raise TaskError(f"cannot read prompt {prompt_file}: {error}") from error

    repo = (task_dir / _require(data, "repo", str)).resolve()
    if not repo.is_dir():
        raise TaskError(f"task repository does not exist: {repo}")

    raw_commands = data.get("allowed_commands", [])
    if not isinstance(raw_commands, list) or not raw_commands:
        raise TaskError("allowed_commands must be a non-empty list")
    allowed_commands: list[tuple[str, ...]] = []
    for command in raw_commands:
        if not isinstance(command, list) or not command or not all(isinstance(item, str) for item in command):
            raise TaskError("each allowed command must be a non-empty string list")
        allowed_commands.append(tuple(command))

    raw_checks = data.get("checks", [])
    if not isinstance(raw_checks, list) or not raw_checks:
        raise TaskError("checks must be a non-empty list")
    checks: list[CheckSpec] = []
    for raw in raw_checks:
        if not isinstance(raw, dict) or not isinstance(raw.get("id"), str):
            raise TaskError("each check needs an id")
        kind = raw.get("type")
        if kind == "command":
            argv = raw.get("argv")
            if not isinstance(argv, list) or not argv or not all(isinstance(item, str) for item in argv):
                raise TaskError(f"check {raw['id']} needs a command argv")
            baseline = raw.get("baseline", "pass")
            if baseline not in {"pass", "fail"}:
                raise TaskError(f"check {raw['id']} baseline must be pass or fail")
            expect_output = raw.get("expect_output")
            if expect_output is not None and not isinstance(expect_output, str):
                raise TaskError(f"check {raw['id']} expect_output must be a string")
            checks.append(
                CheckSpec(
                    kind="command",
                    id=raw["id"],
                    argv=tuple(argv),
                    baseline=baseline,
                    timeout=int(raw.get("timeout", 60)),
                    expect_output=raw.get("expect_output"),
                )
            )
        elif kind == "diff_scope":
            allowed = raw.get("allowed", data.get("allowed_files", []))
            if not isinstance(allowed, list) or not allowed:
                raise TaskError(f"check {raw['id']} needs allowed files")
            checks.append(CheckSpec(kind="diff_scope", id=raw["id"], allowed=tuple(allowed)))
        elif kind == "diff_check":
            checks.append(CheckSpec(kind="diff_check", id=raw["id"]))
        else:
            raise TaskError(f"unknown check type {kind!r}")

    command_policy = CommandPolicy(tuple(allowed_commands))
    for check in checks:
        if check.kind == "command":
            command_policy.validate(check.argv)

    allowed_files = data.get("allowed_files", [])
    if not isinstance(allowed_files, list) or not allowed_files or not all(
        isinstance(item, str) for item in allowed_files
    ):
        raise TaskError("allowed_files must be a non-empty string list")
    sensitive = data.get("sensitive_patterns", [])
    if not isinstance(sensitive, list) or not all(isinstance(item, str) for item in sensitive):
        raise TaskError("sensitive_patterns must be a string list")

    worker = data.get("worker", [])
    if not isinstance(worker, list) or not worker or not all(isinstance(item, str) for item in worker):
        raise TaskError("worker must be a non-empty string list")

    max_attempts = int(data.get("max_attempts", 3))
    token_budget = int(data.get("token_budget", 20000))
    worker_timeout = int(data.get("worker_timeout", 180))
    if max_attempts < 1 or token_budget < 1 or worker_timeout < 1:
        raise TaskError("max_attempts, token_budget, and worker_timeout must be positive")

    workspace_mode = data.get("workspace_mode", "auto")
    if workspace_mode not in {"auto", "snapshot", "worktree"}:
        raise TaskError("workspace_mode must be auto, snapshot, or worktree")

    if not any(check.kind == "command" and check.baseline == "fail" for check in checks):
        raise TaskError("at least one command check must fail before the fix")

    return TaskSpec(
        id=_require(data, "id", str),
        title=_require(data, "title", str),
        task_file=task_file,
        task_dir=task_dir,
        repo=repo,
        prompt_file=prompt_file,
        prompt=prompt,
        worker=tuple(worker),
        allowed_commands=tuple(allowed_commands),
        allowed_files=tuple(allowed_files),
        sensitive_patterns=tuple(sensitive),
        checks=tuple(checks),
        max_attempts=max_attempts,
        token_budget=token_budget,
        worker_timeout=worker_timeout,
        workspace_mode=workspace_mode,
        pr_base=str(data.get("pr_base", "main")),
    )


def file_is_allowed(path: str, patterns: tuple[str, ...]) -> bool:
    normalized = path.replace("\\", "/")
    return any(fnmatch.fnmatch(normalized, pattern) for pattern in patterns)
