from __future__ import annotations

import fnmatch
import os
from collections.abc import Callable
from pathlib import Path


class PolicyError(PermissionError):
    pass


SHELLS = {"sh", "bash", "zsh", "fish", "dash", "ksh", "csh", "tcsh", "pwsh", "powershell"}
DANGEROUS = {
    "sudo",
    "su",
    "doas",
    "mount",
    "umount",
    "mkfs",
    "shutdown",
    "reboot",
    "dd",
    "systemctl",
    "launchctl",
}


class CommandPolicy:
    def __init__(self, allowed: tuple[tuple[str, ...], ...]):
        self.allowed = allowed

    def validate(self, argv: tuple[str, ...] | list[str]) -> None:
        command = tuple(argv)
        if not command or any("\x00" in item for item in command):
            raise PolicyError("empty or invalid command")
        name = Path(command[0]).name
        if name in SHELLS:
            raise PolicyError("shell interpreters are not allowed")
        if name in DANGEROUS:
            raise PolicyError(f"command {name!r} is permanently blocked")
        if name == "rm" and any(item in {"-rf", "-fr", "-r", "--recursive"} for item in command[1:]):
            raise PolicyError("recursive deletion is permanently blocked")
        if command[:2] == ("git", "push") and any(
            item in {"--force", "-f", "--mirror", "--delete"} for item in command[2:]
        ):
            raise PolicyError("destructive git push is permanently blocked")
        if command[:2] == ("git", "reset") and "--hard" in command[2:]:
            raise PolicyError("git reset --hard is permanently blocked")
        if any(character in item for item in command for character in ("\n", "\r")):
            raise PolicyError("newlines are not allowed in commands")

        for prefix in self.allowed:
            if command[: len(prefix)] == prefix:
                return
        raise PolicyError(f"command is not allowlisted: {' '.join(command)}")


class PathPolicy:
    def __init__(self, allowed_files: tuple[str, ...], sensitive_patterns: tuple[str, ...]):
        self.allowed_files = allowed_files
        self.sensitive_patterns = sensitive_patterns

    def relative_path(self, workspace: Path, requested: str) -> str:
        raw = Path(requested)
        if raw.is_absolute() or any(part == ".." for part in raw.parts):
            raise PolicyError("path must stay inside the workspace")
        resolved = (workspace / raw).resolve()
        try:
            relative = resolved.relative_to(workspace.resolve())
        except ValueError as error:
            raise PolicyError("path escapes the workspace") from error
        normalized = relative.as_posix()
        if normalized == ".git" or normalized.startswith(".git/"):
            raise PolicyError("git internals are not readable or writable")
        return normalized

    def is_sensitive(self, relative: str) -> bool:
        name = Path(relative).name
        return any(
            fnmatch.fnmatch(relative, pattern) or fnmatch.fnmatch(name, pattern)
            for pattern in self.sensitive_patterns
        )

    def can_read(self, relative: str) -> bool:
        return not self.is_sensitive(relative)

    def can_edit(self, relative: str) -> bool:
        return not self.is_sensitive(relative) and any(
            fnmatch.fnmatch(relative, pattern) for pattern in self.allowed_files
        )


def copy_ignore(
    patterns: tuple[str, ...], base_ignores: tuple[str, ...]
) -> Callable[[str, list[str]], set[str]]:
    combined = base_ignores + patterns

    def ignore(directory: str, names: list[str]) -> set[str]:
        ignored: set[str] = set()
        for name in names:
            relative = (Path(directory) / name).as_posix()
            if name in {".git", "target", "node_modules", "result"}:
                ignored.add(name)
                continue
            if relative.rstrip("/").endswith("/agent/runs"):
                ignored.add(name)
                continue
            if any(
                fnmatch.fnmatch(name, pattern) or fnmatch.fnmatch(relative, pattern)
                for pattern in combined
            ):
                ignored.add(name)
        return ignored

    return ignore


def sanitized_environment(extra: dict[str, str]) -> dict[str, str]:
    keep = {
        "PATH",
        "HOME",
        "TMPDIR",
        "TEMP",
        "TMP",
        "LANG",
        "LC_ALL",
        "TERM",
        "USER",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_OAUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "OPENAI_API_KEY",
        "OMP_PROFILE",
    }
    environment = {key: value for key, value in os.environ.items() if key in keep}
    environment.update(
        {
            "GIT_TERMINAL_PROMPT": "0",
            "GIT_CONFIG_GLOBAL": os.devnull,
            "GIT_CONFIG_NOSYSTEM": "1",
            "PYTHONDONTWRITEBYTECODE": "1",
        }
    )
    environment.update(extra)
    return environment
