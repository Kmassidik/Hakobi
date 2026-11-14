"""Data types and errors shared by build execution components."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Callable


class BusyError(RuntimeError):
    """All configured build slots are in use."""


class BuildExecutionError(RuntimeError):
    """A build could not be started (missing binary, unusable work dir)."""


@dataclass(frozen=True)
class BuildRequest:
    git_url: str
    branch: str
    image_tag: str
    dockerfile_path: str = "Dockerfile"
    context_dir: str = "."
    token: str | None = field(default=None, repr=False)


@dataclass(frozen=True)
class BuildResult:
    status: str
    image_tag: str
    exit_code: int | None = None
    duration_ms: int = 0
    log_tail: str = ""
    truncated: bool = False
    detail: str | None = None

    def to_dict(self) -> dict[str, object]:
        return {
            "status": self.status,
            "image_tag": self.image_tag,
            "exit_code": self.exit_code,
            "duration_ms": self.duration_ms,
            "truncated": self.truncated,
            "log_tail": self.log_tail,
            "detail": self.detail,
        }


@dataclass(frozen=True)
class CommandResult:
    argv: tuple[str, ...]
    exit_code: int | None
    timed_out: bool
    output: bytes
    total_output_bytes: int
    truncated: bool
    duration_ms: int


CommandRunner = Callable[..., CommandResult]
