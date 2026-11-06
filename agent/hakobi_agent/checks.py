from __future__ import annotations

import subprocess
from dataclasses import dataclass

from .policy import PolicyError, sanitized_environment
from .task import CheckSpec, TaskSpec, file_is_allowed
from .workspace import Workspace

OUTPUT_LIMIT = 8000


@dataclass
class CheckResult:
    id: str
    kind: str
    passed: bool
    expected: str
    saw: str

    def as_dict(self) -> dict[str, object]:
        return {
            "id": self.id,
            "kind": self.kind,
            "passed": self.passed,
            "expected": self.expected,
            "saw": self.saw,
        }


@dataclass
class CommandResult:
    returncode: int
    output: str
    timed_out: bool


def run_command(
    argv: tuple[str, ...],
    cwd,
    timeout: int,
    extra_env: dict[str, str] | None = None,
) -> CommandResult:
    try:
        result = subprocess.run(
            list(argv),
            cwd=cwd,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=timeout,
            env=sanitized_environment(extra_env or {}),
        )
        return CommandResult(result.returncode, (result.stdout or "")[-OUTPUT_LIMIT:], False)
    except subprocess.TimeoutExpired as error:
        output = error.stdout or ""
        if isinstance(output, bytes):
            output = output.decode(errors="replace")
        return CommandResult(124, (output or "")[-OUTPUT_LIMIT:] + "\n[timeout]", True)


class CheckRunner:
    def __init__(self, task: TaskSpec, workspace: Workspace):
        self.task = task
        self.workspace = workspace
        self.policy = task.command_policy

    def run(self, phase: str) -> list[CheckResult]:
        if phase not in {"baseline", "after"}:
            raise ValueError("phase must be baseline or after")
        results: list[CheckResult] = []
        for check in self.task.checks:
            if check.kind == "command":
                results.append(self._command(check, phase))
            elif check.kind == "diff_scope":
                results.append(self._diff_scope(check, phase))
            elif check.kind == "diff_check":
                results.append(self._diff_check(check))
            else:
                raise ValueError(f"unsupported check {check.kind}")
        if phase == "after":
            changed = self.workspace.changed_files()
            results.insert(
                0,
                CheckResult(
                    id="changed-files",
                    kind="changed",
                    passed=bool(changed),
                    expected="at least one file changes",
                    saw=f"changed={changed}",
                ),
            )
        return results

    def _command(self, check: CheckSpec, phase: str) -> CheckResult:
        try:
            self.policy.validate(check.argv)
        except PolicyError as error:
            return CheckResult(check.id, "command", False, "allowlisted command", str(error))
        result = run_command(check.argv, self.workspace.path, check.timeout)
        output = result.output
        if check.expect_output is None:
            marker_ok = True
        elif phase == "baseline":
            marker_ok = check.expect_output in output
        else:
            marker_ok = check.expect_output not in output
        if phase == "baseline":
            passed = result.returncode != 0 and marker_ok
            expected = (
                f"command fails with {check.expect_output!r}"
                if check.expect_output
                else "command fails"
            )
        else:
            passed = result.returncode == 0 and marker_ok
            expected = (
                "command exits 0 without the baseline failure"
                if check.expect_output
                else "command exits 0"
            )
        saw = f"exit={result.returncode} timeout={result.timed_out}\n{output}"
        return CheckResult(check.id, "command", passed, expected, saw)

    def _diff_scope(self, check: CheckSpec, phase: str) -> CheckResult:
        changed = self.workspace.changed_files()
        outside = [path for path in changed if not file_is_allowed(path, check.allowed)]
        if phase == "baseline":
            passed = not outside
            expected = "no file changes before the worker runs"
        else:
            passed = not outside
            expected = f"only files matching {list(check.allowed)}"
        return CheckResult(
            check.id,
            "diff_scope",
            passed,
            expected,
            f"changed={changed} outside={outside}",
        )

    def _diff_check(self, check: CheckSpec) -> CheckResult:
        passed, detail = self.workspace.diff_check()
        return CheckResult(
            check.id,
            "diff_check",
            passed,
            "git diff --check passes",
            detail or "clean",
        )


def format_feedback(task: TaskSpec, results: list[CheckResult], dead_ends: list[str]) -> str:
    failures = [result for result in results if not result.passed]
    expected = " | ".join(f"{item.id}: {item.expected}" for item in failures)
    saw = "\n".join(f"{item.id}: {item.saw}" for item in failures)
    fence = ", ".join(task.allowed_files)
    dead = "; ".join(dead_ends[-8:]) or "none"
    return (
        f"EXPECTED: {expected}\n"
        f"SAW:\n{saw}\n"
        f"FENCE: only modify files matching {fence}\n"
        f"DEAD_ENDS: {dead}\n"
    )


def failure_summary(results: list[CheckResult]) -> str:
    failures = [result for result in results if not result.passed]
    return "; ".join(f"{item.id}: {item.saw.splitlines()[0]}" for item in failures)
