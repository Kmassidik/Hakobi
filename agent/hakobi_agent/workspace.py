from __future__ import annotations

import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path

from .policy import copy_ignore
from .task import TaskSpec


class WorkspaceError(RuntimeError):
    pass


def _run_git(cwd: Path, *arguments: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    command = [
        "git",
        "-c",
        "user.name=Hakobi Harness",
        "-c",
        "user.email=harness@hakobi.local",
        *arguments,
    ]
    result = subprocess.run(
        command,
        cwd=cwd,
        text=True,
        capture_output=True,
        timeout=60,
        env={
            "PATH": "/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/nix/var/nix/profiles/default/bin",
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_CONFIG_GLOBAL": "/dev/null",
            "HOME": str(cwd),
            "GIT_TERMINAL_PROMPT": "0",
        },
    )
    if check and result.returncode != 0:
        raise WorkspaceError(
            f"git {' '.join(arguments)} failed: {result.stderr.strip() or result.stdout.strip()}"
        )
    return result


@dataclass(frozen=True)
class Workspace:
    path: Path
    baseline_commit: str
    branch: str
    mode: str

    def git(self, *arguments: str, check: bool = True) -> subprocess.CompletedProcess[str]:
        return _run_git(self.path, *arguments, check=check)

    def add_intent_to_add(self) -> None:
        self.git("add", "-N", ".")

    def changed_files(self) -> list[str]:
        self.add_intent_to_add()
        output = self.git("diff", "--name-only", self.baseline_commit).stdout
        return sorted(line for line in output.splitlines() if line)

    def changed_outside(self, allowed: tuple[str, ...]) -> list[str]:
        from .task import file_is_allowed

        return [path for path in self.changed_files() if not file_is_allowed(path, allowed)]

    def diff(self) -> str:
        self.add_intent_to_add()
        return self.git("diff", self.baseline_commit).stdout

    def diff_digest(self) -> str:
        import hashlib

        return hashlib.sha256(self.diff().encode("utf-8")).hexdigest()

    def diff_check(self) -> tuple[bool, str]:
        result = self.git("diff", "--check", self.baseline_commit, check=False)
        return result.returncode == 0, (result.stdout + result.stderr).strip()

    def commit(self, message: str) -> str:
        status = self.git("status", "--porcelain").stdout
        if status.strip():
            self.git("add", "-A")
            result = self.git("commit", "-m", message, check=False)
            if result.returncode != 0:
                raise WorkspaceError(result.stderr.strip() or result.stdout.strip())
        return self.git("rev-parse", "HEAD").stdout.strip()


def _is_git_repo(path: Path) -> bool:
    result = subprocess.run(
        ["git", "-C", str(path), "rev-parse", "--show-toplevel"],
        text=True,
        capture_output=True,
    )
    return result.returncode == 0 and Path(result.stdout.strip()).resolve() == path.resolve()


def _worktree_is_clean(path: Path) -> bool:
    result = subprocess.run(
        ["git", "-C", str(path), "status", "--porcelain"],
        text=True,
        capture_output=True,
    )
    return result.returncode == 0 and not result.stdout.strip()


def prepare_workspace(
    task: TaskSpec,
    destination: Path,
    branch: str,
) -> Workspace:
    if destination.exists():
        shutil.rmtree(destination)
    mode = task.workspace_mode
    if mode == "auto":
        mode = "worktree" if _is_git_repo(task.repo) and _worktree_is_clean(task.repo) else "snapshot"

    if mode == "worktree":
        if not _is_git_repo(task.repo):
            raise WorkspaceError("worktree mode requires a git repository")
        if not _worktree_is_clean(task.repo):
            raise WorkspaceError("worktree mode requires a clean source repository")
        result = subprocess.run(
            ["git", "-C", str(task.repo), "worktree", "add", "-b", branch, str(destination), "HEAD"],
            text=True,
            capture_output=True,
        )
        if result.returncode != 0:
            raise WorkspaceError(result.stderr.strip() or result.stdout.strip())
        baseline = _run_git(destination, "rev-parse", "HEAD").stdout.strip()
        return Workspace(destination, baseline, branch, mode)

    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copytree(
        task.repo,
        destination,
        symlinks=True,
        ignore=copy_ignore(
            task.sensitive_patterns,
            (
                ".env",
                ".env.*",
                "*.pem",
                "*.key",
                "*.p12",
                ".npmrc",
                ".pypirc",
                ".netrc",
                "id_rsa*",
                ".ssh",
                ".aws",
                ".config/gcloud",
                "__pycache__",
                "*.pyc",
                ".pytest_cache",
                ".mypy_cache",
                ".ruff_cache",
            ),
        ),
    )
    _run_git(destination, "init", "-q")
    _run_git(destination, "checkout", "-q", "-b", branch)
    _run_git(destination, "add", "-A")
    _run_git(destination, "commit", "-q", "-m", "baseline: task workspace")
    baseline = _run_git(destination, "rev-parse", "HEAD").stdout.strip()
    return Workspace(destination, baseline, branch, mode)
