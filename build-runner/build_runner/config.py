"""Runtime configuration for the standalone build runner.

Every knob is read from the environment exactly once at process start.  The
settings object is intentionally immutable so request handlers cannot widen
timeouts, output limits, or the allow-lists while serving traffic.
"""

from __future__ import annotations

import ipaddress
import logging
import os
import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Mapping

logger = logging.getLogger(__name__)

MIN_API_TOKEN_LENGTH = 16
DEFAULT_PORT = 8787
DEFAULT_WORK_DIR = Path("/tmp/build-runner")

_BUILDER_NAME_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$")
_CONTROL_RE = re.compile(r"[\x00-\x1f\x7f]")


class ConfigError(ValueError):
    """Raised when the environment does not describe a safe configuration."""


@dataclass(frozen=True)
class Settings:
    """Validated, immutable build-runner settings."""

    host: str = "127.0.0.1"
    port: int = DEFAULT_PORT
    api_token: str | None = field(default=None, repr=False)
    allow_unauthenticated_http: bool = False
    work_dir: Path = DEFAULT_WORK_DIR
    max_concurrent_builds: int = 1
    clone_timeout: float = 300.0
    build_timeout: float = 1800.0
    max_output_bytes: int = 262_144
    response_tail_bytes: int = 65_536
    max_request_bytes: int = 1_048_576
    allowed_git_hosts: tuple[str, ...] = field(default_factory=tuple)
    docker_bin: str = "docker"
    git_bin: str = "git"
    nixpacks_bin: str = "nixpacks"
    buildx_builder: str | None = None

    def __post_init__(self) -> None:
        self._normalize_fields()
        self._validate_host_and_port()
        self._validate_api_token()
        self._validate_limits()
        self._validate_executables()

    def _normalize_fields(self) -> None:
        if isinstance(self.work_dir, str) and not self.work_dir.strip():
            raise ConfigError("BUILD_RUNNER_WORK_DIR must not be empty")
        object.__setattr__(self, "work_dir", Path(self.work_dir))
        raw_hosts = (
            (self.allowed_git_hosts,)
            if isinstance(self.allowed_git_hosts, str)
            else self.allowed_git_hosts
        )
        object.__setattr__(
            self,
            "allowed_git_hosts",
            tuple(host.strip().lower() for host in raw_hosts if host.strip()),
        )

    def _validate_host_and_port(self) -> None:
        if not self.host or not self.host.strip():
            raise ConfigError("BUILD_RUNNER_HOST must not be empty")
        if _CONTROL_RE.search(self.host) or any(ch.isspace() for ch in self.host):
            raise ConfigError("BUILD_RUNNER_HOST contains invalid characters")
        if not 0 <= self.port <= 65_535:
            raise ConfigError("BUILD_RUNNER_PORT must be between 0 and 65535")

    def _validate_api_token(self) -> None:
        if self.api_token is None:
            return
        if not isinstance(self.api_token, str):
            raise ConfigError("BUILD_RUNNER_TOKEN must be a string")
        if len(self.api_token) < MIN_API_TOKEN_LENGTH:
            raise ConfigError(
                f"BUILD_RUNNER_TOKEN must be at least {MIN_API_TOKEN_LENGTH} characters"
            )
        if len(self.api_token) > 8_192:
            raise ConfigError("BUILD_RUNNER_TOKEN is too long")
        if _CONTROL_RE.search(self.api_token):
            raise ConfigError("BUILD_RUNNER_TOKEN must not contain control characters")
        if any(ch.isspace() for ch in self.api_token) or not self.api_token.isascii():
            raise ConfigError("BUILD_RUNNER_TOKEN must be printable ASCII without spaces")

    def _validate_limits(self) -> None:
        if not 1 <= self.max_concurrent_builds <= 64:
            raise ConfigError("BUILD_RUNNER_MAX_CONCURRENT_BUILDS must be between 1 and 64")
        if not 1.0 <= self.clone_timeout <= 3_600.0:
            raise ConfigError("BUILD_RUNNER_CLONE_TIMEOUT must be between 1 and 3600 seconds")
        if not 1.0 <= self.build_timeout <= 86_400.0:
            raise ConfigError("BUILD_RUNNER_BUILD_TIMEOUT must be between 1 and 86400 seconds")
        if not 1_024 <= self.max_output_bytes <= 16_777_216:
            raise ConfigError("BUILD_RUNNER_MAX_OUTPUT_BYTES must be between 1024 and 16777216")
        if not 256 <= self.response_tail_bytes <= self.max_output_bytes:
            raise ConfigError(
                "BUILD_RUNNER_RESPONSE_TAIL_BYTES must be between 256 and "
                "BUILD_RUNNER_MAX_OUTPUT_BYTES"
            )
        if not 1_024 <= self.max_request_bytes <= 16_777_216:
            raise ConfigError("BUILD_RUNNER_MAX_REQUEST_BYTES must be between 1024 and 16777216")

    def _validate_executables(self) -> None:
        for name, value in (
            ("BUILD_RUNNER_DOCKER_BIN", self.docker_bin),
            ("BUILD_RUNNER_GIT_BIN", self.git_bin),
            ("BUILD_RUNNER_NIXPACKS_BIN", self.nixpacks_bin),
        ):
            if not value or "/" in value or _CONTROL_RE.search(value):
                raise ConfigError(f"{name} must be a bare executable name")
        if self.buildx_builder is not None and not _BUILDER_NAME_RE.match(self.buildx_builder):
            raise ConfigError("BUILD_RUNNER_BUILDX_BUILDER contains invalid characters")

    def validate_runtime(self) -> None:
        """Enforce authentication policy before binding a socket."""

        if self.api_token is None:
            if not self.allow_unauthenticated_http:
                raise ConfigError(
                    "BUILD_RUNNER_TOKEN is required; set BUILD_RUNNER_ALLOW_UNAUTHENTICATED=1 "
                    "only for loopback-only development"
                )
            if not _is_loopback(self.host):
                raise ConfigError(
                    "unauthenticated mode is only allowed when BUILD_RUNNER_HOST is loopback"
                )

    @classmethod
    def from_env(cls, environ: Mapping[str, str] | None = None) -> "Settings":
        env = os.environ if environ is None else environ

        work_dir = Path(
            env.get("BUILD_RUNNER_WORK_DIR") or str(DEFAULT_WORK_DIR)
        )
        return cls(
            host=env.get("BUILD_RUNNER_HOST", "127.0.0.1"),
            port=_env_int(env, "BUILD_RUNNER_PORT", DEFAULT_PORT),
            api_token=env.get("BUILD_RUNNER_TOKEN") or None,
            allow_unauthenticated_http=_env_bool(
                env, "BUILD_RUNNER_ALLOW_UNAUTHENTICATED", False
            ),
            work_dir=work_dir,
            max_concurrent_builds=_env_int(env, "BUILD_RUNNER_MAX_CONCURRENT_BUILDS", 1),
            clone_timeout=_env_float(env, "BUILD_RUNNER_CLONE_TIMEOUT", 300.0),
            build_timeout=_env_float(env, "BUILD_RUNNER_BUILD_TIMEOUT", 1_800.0),
            max_output_bytes=_env_int(env, "BUILD_RUNNER_MAX_OUTPUT_BYTES", 262_144),
            response_tail_bytes=_env_int(env, "BUILD_RUNNER_RESPONSE_TAIL_BYTES", 65_536),
            max_request_bytes=_env_int(env, "BUILD_RUNNER_MAX_REQUEST_BYTES", 1_048_576),
            allowed_git_hosts=_env_list(env, "BUILD_RUNNER_ALLOWED_GIT_HOSTS"),
            docker_bin=env.get("BUILD_RUNNER_DOCKER_BIN", "docker"),
            git_bin=env.get("BUILD_RUNNER_GIT_BIN", "git"),
            nixpacks_bin=env.get("BUILD_RUNNER_NIXPACKS_BIN", "nixpacks"),
            buildx_builder=env.get("BUILD_RUNNER_BUILDX_BUILDER") or None,
        )


def _env_int(env: Mapping[str, str], name: str, default: int) -> int:
    raw = env.get(name)
    if raw is None or raw == "":
        return default
    try:
        return int(raw)
    except ValueError as exc:
        raise ConfigError(f"{name} must be an integer") from exc


def _env_float(env: Mapping[str, str], name: str, default: float) -> float:
    raw = env.get(name)
    if raw is None or raw == "":
        return default
    try:
        return float(raw)
    except ValueError as exc:
        raise ConfigError(f"{name} must be a number") from exc


def _env_bool(env: Mapping[str, str], name: str, default: bool) -> bool:
    raw = env.get(name)
    if raw is None or raw == "":
        return default
    normalized = raw.strip().lower()
    if normalized in {"1", "true", "yes", "on"}:
        return True
    if normalized in {"0", "false", "no", "off"}:
        return False
    raise ConfigError(f"{name} must be a boolean")


def _env_list(env: Mapping[str, str], name: str) -> tuple[str, ...]:
    raw = env.get(name, "")
    return tuple(part.strip() for part in raw.split(",") if part.strip())


def _is_loopback(host: str) -> bool:
    normalized = host.strip().lower()
    if normalized == "localhost":
        return True
    try:
        return ipaddress.ip_address(normalized).is_loopback
    except ValueError:
        return False


def log_settings(settings: Settings) -> None:
    """Log non-secret, operationally useful settings."""

    logger.info(
        "config host=%s port=%d auth=%s work_dir=%s concurrency=%d "
        "clone_timeout=%.0fs build_timeout=%.0fs output_cap=%d",
        settings.host,
        settings.port,
        "token" if settings.api_token else "disabled",
        settings.work_dir,
        settings.max_concurrent_builds,
        settings.clone_timeout,
        settings.build_timeout,
        settings.max_output_bytes,
    )
    if settings.api_token is None:
        logger.warning("HTTP authentication is disabled; loopback-only development mode")
    if settings.buildx_builder:
        logger.info("using buildx builder %s", settings.buildx_builder)
