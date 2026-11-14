"""Build execution: clone, validate checkout paths, build an image, clean up.

All commands run with argv arrays (never ``shell=True``), a scrubbed
environment, a wall-clock timeout, and a bounded output tail.  Workspaces live
under ``BUILD_RUNNER_WORK_DIR`` and are removed even when validation or the
build fails.
"""

from __future__ import annotations

import base64
import logging
import os
import threading
import time
from dataclasses import replace
from pathlib import Path
from typing import Iterable, Mapping, Sequence
from urllib.parse import urlsplit

from . import process as _process
from .config import Settings
from .execution_models import (
    BuildExecutionError,
    BuildRequest,
    BuildResult,
    BusyError,
    CommandResult,
    CommandRunner,
)
from .logs import compose_log, redact
from .process import run_command
from .validation import (
    ValidationError,
    resolve_within,
    validate_branch,
    validate_git_url,
    validate_image_tag,
    validate_repo_path,
    validate_token,
)
from .workspace import cleanup_workspace, create_workspace

# Keep these module attributes available for existing subprocess patch targets.
subprocess = _process.subprocess

logger = logging.getLogger(__name__)

# Only these variables are inherited by git/docker/nixpacks.  Everything else
# (application secrets, cloud credentials, proxies) stays in the parent.
_CHILD_ENV_ALLOWLIST = frozenset(
    {
        "PATH",
        "HOME",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "TMPDIR",
        "DOCKER_HOST",
        "DOCKER_CONTEXT",
        "DOCKER_CONFIG",
        "DOCKER_TLS_VERIFY",
        "DOCKER_CERT_PATH",
        "XDG_RUNTIME_DIR",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
        "BUILDKIT_HOST",
    }
)

_BASE_CHILD_ENV = {
    "GIT_TERMINAL_PROMPT": "0",
    "GIT_ASKPASS": "/bin/false",
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_CONFIG_GLOBAL": "/dev/null",
    "GIT_ALLOW_PROTOCOL": "https:ssh",
    "GIT_SSH_COMMAND": "ssh -o BatchMode=yes -o StrictHostKeyChecking=yes",
    "DOCKER_BUILDKIT": "1",
}


class BuildExecutor:
    """Serializes/concurrency-limits builds and owns workspace cleanup."""

    def __init__(self, settings: Settings, runner: CommandRunner = run_command) -> None:
        self.settings = settings
        self._runner = runner
        self._slots = threading.BoundedSemaphore(settings.max_concurrent_builds)

    def build_dockerfile(self, request: BuildRequest) -> BuildResult:
        return self._build(request, kind="dockerfile")

    def build_nixpacks(self, request: BuildRequest) -> BuildResult:
        return self._build(request, kind="nixpacks")

    # ------------------------------------------------------------------
    # orchestration
    # ------------------------------------------------------------------
    def _build(self, request: BuildRequest, *, kind: str) -> BuildResult:
        normalized = self._normalize(request, kind=kind)

        if not self._slots.acquire(blocking=False):
            raise BusyError("all build slots are busy; retry later")

        workspace: Path | None = None
        cleanup_error: str | None = None
        started = time.monotonic()
        try:
            try:
                workspace = self._create_workspace()
                result = self._execute(normalized, kind=kind, workspace=workspace)
            except BuildExecutionError as exc:
                result = BuildResult(
                    status="error",
                    image_tag=normalized.image_tag,
                    duration_ms=int((time.monotonic() - started) * 1_000),
                    detail=str(exc),
                )
        finally:
            cleanup_error = self._cleanup(workspace)
            self._slots.release()

        if cleanup_error:
            logger.warning("%s", cleanup_error)
            detail = result.detail
            result = replace(
                result,
                detail=f"{detail}; {cleanup_error}" if detail else cleanup_error,
            )
        return result

    def _normalize(self, request: BuildRequest, *, kind: str) -> BuildRequest:
        git_url = validate_git_url(
            request.git_url, allowed_hosts=self.settings.allowed_git_hosts
        )
        branch = validate_branch(request.branch)
        image_tag = validate_image_tag(request.image_tag)
        token = validate_token(request.token, git_url=git_url)

        if kind == "dockerfile":
            dockerfile_path = validate_repo_path(request.dockerfile_path, "dockerfile_path")
            context_dir = validate_repo_path(
                request.context_dir, "context_dir", allow_root=True
            )
        elif kind == "nixpacks":
            dockerfile_path = "Dockerfile"
            context_dir = "."
        else:  # pragma: no cover - internal misuse
            raise ValueError(f"unknown build kind {kind!r}")

        return BuildRequest(
            git_url=git_url,
            branch=branch,
            image_tag=image_tag,
            dockerfile_path=dockerfile_path,
            context_dir=context_dir,
            token=token,
        )

    def _execute(self, request: BuildRequest, *, kind: str, workspace: Path) -> BuildResult:
        secrets = (request.token,)
        source = workspace / "src"
        started = time.monotonic()
        clone = self._run(
            self._clone_argv(request, source),
            cwd=workspace,
            env=self._clone_env(request),
            timeout=self.settings.clone_timeout,
        )
        failure = self._clone_failure(request, clone, started, secrets)
        if failure is not None:
            return failure

        if not source.is_dir():
            raise ValidationError("git_url", "clone did not create a source directory")
        context, dockerfile = self._resolve_build_paths(request, kind=kind, source=source)
        build_argv = self._build_argv(
            kind=kind,
            dockerfile=dockerfile,
            context=context,
            image_tag=request.image_tag,
        )
        build = self._run(
            build_argv,
            cwd=source,
            env=self._base_env(),
            timeout=self.settings.build_timeout,
        )
        return self._build_result(request, clone, build, started, secrets)

    def _clone_argv(self, request: BuildRequest, source: Path) -> list[str]:
        return [
            self.settings.git_bin,
            "-c",
            "credential.helper=",
            "-c",
            "protocol.file.allow=never",
            "-c",
            "protocol.ext.allow=never",
            "clone",
            "--depth",
            "1",
            "--single-branch",
            "--no-recurse-submodules",
            "--branch",
            request.branch,
            "--",
            request.git_url,
            str(source),
        ]

    def _clone_failure(
        self,
        request: BuildRequest,
        clone: CommandResult,
        started: float,
        secrets: Iterable[str | None],
    ) -> BuildResult | None:
        if not clone.timed_out and clone.exit_code == 0:
            return None
        log_tail, truncated = self._compose_log(clone, None, secrets)
        detail = (
            f"git clone timed out after {self.settings.clone_timeout:.0f}s"
            if clone.timed_out
            else f"git clone exited with code {clone.exit_code}"
        )
        return BuildResult(
            status="timed_out" if clone.timed_out else "failed",
            image_tag=request.image_tag,
            exit_code=clone.exit_code,
            duration_ms=int((time.monotonic() - started) * 1_000),
            log_tail=log_tail,
            truncated=truncated,
            detail=detail,
        )

    def _resolve_build_paths(
        self, request: BuildRequest, *, kind: str, source: Path
    ) -> tuple[Path, Path | None]:
        if kind == "dockerfile":
            context = resolve_within(
                source,
                request.context_dir,
                field="context_dir",
                expect_directory=True,
            )
            dockerfile = resolve_within(
                source,
                request.dockerfile_path,
                field="dockerfile_path",
            )
            return context, dockerfile
        context = resolve_within(source, ".", field="context_dir", expect_directory=True)
        return context, None

    def _build_result(
        self,
        request: BuildRequest,
        clone: CommandResult,
        build: CommandResult,
        started: float,
        secrets: Iterable[str | None],
    ) -> BuildResult:
        log_tail, truncated = self._compose_log(clone, build, secrets)
        if build.timed_out:
            status = "timed_out"
            detail: str | None = f"build timed out after {self.settings.build_timeout:.0f}s"
        elif build.exit_code != 0:
            status = "failed"
            detail = f"{Path(build.argv[0]).name} build exited with code {build.exit_code}"
        else:
            status = "succeeded"
            detail = None
        return BuildResult(
            status=status,
            image_tag=request.image_tag,
            exit_code=build.exit_code,
            duration_ms=int((time.monotonic() - started) * 1_000),
            log_tail=log_tail,
            truncated=truncated,
            detail=detail,
        )

    # ------------------------------------------------------------------
    # command construction
    # ------------------------------------------------------------------
    def _build_argv(
        self,
        *,
        kind: str,
        dockerfile: Path | None,
        context: Path,
        image_tag: str,
    ) -> list[str]:
        if kind == "dockerfile":
            assert dockerfile is not None
            if self.settings.buildx_builder:
                return [
                    self.settings.docker_bin,
                    "buildx",
                    "build",
                    "--builder",
                    self.settings.buildx_builder,
                    "--progress=plain",
                    "--load",
                    "--file",
                    str(dockerfile),
                    "--tag",
                    image_tag,
                    str(context),
                ]
            return [
                self.settings.docker_bin,
                "build",
                "--progress=plain",
                "--file",
                str(dockerfile),
                "--tag",
                image_tag,
                str(context),
            ]
        return [
            self.settings.nixpacks_bin,
            "build",
            str(context),
            "--name",
            image_tag,
        ]

    def _run(
        self,
        argv: Sequence[str],
        *,
        cwd: Path,
        env: Mapping[str, str],
        timeout: float,
    ) -> CommandResult:
        try:
            return self._runner(
                list(argv),
                cwd=cwd,
                env=env,
                timeout=timeout,
                max_output_bytes=self.settings.max_output_bytes,
            )
        except OSError as exc:
            binary = Path(argv[0]).name
            reason = exc.strerror or type(exc).__name__
            raise BuildExecutionError(f"could not execute {binary}: {reason}") from exc

    def _base_env(self) -> dict[str, str]:
        env = {key: os.environ[key] for key in _CHILD_ENV_ALLOWLIST if key in os.environ}
        env.setdefault("PATH", "/usr/local/bin:/usr/bin:/bin")
        env.update(_BASE_CHILD_ENV)
        return env

    def _clone_env(self, request: BuildRequest) -> dict[str, str]:
        env = self._base_env()
        # The agent is only useful for cloning; do not expose it to the build
        # CLI process.
        if "SSH_AUTH_SOCK" in os.environ:
            env["SSH_AUTH_SOCK"] = os.environ["SSH_AUTH_SOCK"]
        if request.token:
            parsed = urlsplit(request.git_url)
            host = parsed.hostname or ""
            if ":" in host and not host.startswith("["):
                host = f"[{host}]"
            netloc = f"{host}:{parsed.port}" if parsed.port else host
            scope = f"http.https://{netloc}/"
            encoded = base64.b64encode(
                f"x-access-token:{request.token}".encode("utf-8")
            ).decode("ascii")
            env["GIT_CONFIG_COUNT"] = "1"
            env["GIT_CONFIG_KEY_0"] = f"{scope}.extraHeader"
            env["GIT_CONFIG_VALUE_0"] = f"AUTHORIZATION: Basic {encoded}"
        return env

    # ------------------------------------------------------------------
    # workspace + output
    # ------------------------------------------------------------------
    def _create_workspace(self) -> Path:
        return create_workspace(self.settings.work_dir)

    def _cleanup(self, workspace: Path | None) -> str | None:
        return cleanup_workspace(workspace)

    def _compose_log(
        self,
        clone: CommandResult | None,
        build: CommandResult | None,
        secrets: Iterable[str | None],
    ) -> tuple[str, bool]:
        return compose_log(clone, build, secrets, self.settings.response_tail_bytes)
