from __future__ import annotations

import subprocess
from pathlib import Path

from .checks import CommandResult, run_command
from .policy import PolicyError, sanitized_environment
from .task import TaskSpec
from .workspace import Workspace


class ToolError(RuntimeError):
    pass


class ToolBox:
    """The restricted tool surface available to harness workers."""

    def __init__(self, task: TaskSpec, workspace: Workspace):
        self.task = task
        self.workspace = workspace
        self.policy = task.path_policy
        self.command_policy = task.command_policy

    def _relative(self, requested: str) -> str:
        return self.policy.relative_path(self.workspace.path, requested)

    def read_file(self, requested: str) -> str:
        relative = self._relative(requested)
        if not self.policy.can_read(relative):
            raise ToolError(f"reading {relative} is forbidden")
        path = self.workspace.path / relative
        if path.stat().st_size > 1_048_576:
            raise ToolError("file is larger than the 1 MiB read limit")
        return path.read_text(encoding="utf-8")

    def edit_file(self, requested: str, content: str) -> None:
        relative = self._relative(requested)
        if not self.policy.can_edit(relative):
            raise ToolError(f"editing {relative} is outside the file fence")
        path = self.workspace.path / relative
        if len(content.encode("utf-8")) > 1_048_576:
            raise ToolError("edit is larger than the 1 MiB limit")
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")

    def run_shell(self, argv: list[str], timeout: int | None = None) -> CommandResult:
        self.command_policy.validate(argv)
        return run_command(tuple(argv), self.workspace.path, timeout or 60)

    def run_tests(self) -> CommandResult:
        command = self.task.test_command
        if command is None:
            raise ToolError("task has no failing baseline test command")
        return self.run_shell(list(command), timeout=120)

    def git_status(self) -> str:
        return self.workspace.git("status", "--short").stdout

    def git_diff(self) -> str:
        return self.workspace.diff()

    def git_commit(self, message: str) -> str:
        return self.workspace.commit(message)


def run_worker(
    command: tuple[str, ...],
    workspace: Path,
    environment: dict[str, str],
    timeout: int,
    log_path: Path,
) -> subprocess.CompletedProcess[str]:
    try:
        result = subprocess.run(
            list(command),
            cwd=workspace,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=timeout,
            env=sanitized_environment(environment),
        )
    except subprocess.TimeoutExpired as error:
        output = error.stdout or ""
        if isinstance(output, bytes):
            output = output.decode(errors="replace")
        log_path.write_text((output or "")[-50_000:] + "\n[worker timeout]", encoding="utf-8")
        log_path.chmod(0o600)
        raise TimeoutError(f"worker exceeded {timeout}s") from error
    except OSError as error:
        log_path.write_text(f"worker failed to start: {error}\n", encoding="utf-8")
        raise ToolError(f"worker failed to start: {error}") from error
    log_path.write_text((result.stdout or "")[-50_000:], encoding="utf-8")
    log_path.chmod(0o600)
    return result
