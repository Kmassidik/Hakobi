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
import re
import shutil
import signal
import subprocess
import tempfile
import threading
import time
from dataclasses import dataclass, field, replace
from pathlib import Path
from typing import Callable, Iterable, Mapping, Sequence
from urllib.parse import urlsplit

from .config import Settings
from .validation import (
    ValidationError,
    resolve_within,
    validate_branch,
    validate_git_url,
    validate_image_tag,
    validate_repo_path,
    validate_token,
)

logger = logging.getLogger(__name__)

_ANSI_RE = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")
_READ_CHUNK = 65_536

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


def run_command(
    argv: Sequence[str],
    *,
    cwd: Path | None = None,
    env: Mapping[str, str] | None = None,
    timeout: float,
    max_output_bytes: int = 262_144,
) -> CommandResult:
    """Run a command in its own process group, keeping only the output tail."""

    if isinstance(argv, str) or not argv:
        raise ValueError("argv must be a non-empty sequence of strings")
    argv_list = list(argv)
    if not all(isinstance(arg, str) and arg for arg in argv_list):
        raise ValueError("argv entries must be non-empty strings")
    if timeout <= 0:
        raise ValueError("timeout must be positive")
    if max_output_bytes <= 0:
        raise ValueError("max_output_bytes must be positive")

    started = time.monotonic()
    process = subprocess.Popen(
        argv_list,
        cwd=str(cwd) if cwd is not None else None,
        env=dict(env) if env is not None else None,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        start_new_session=True,
        close_fds=True,
        bufsize=0,
    )

    buffer = bytearray()
    state = {"total": 0, "truncated": False}
    lock = threading.Lock()
    stream = process.stdout

    def read_output() -> None:
        if stream is None:  # pragma: no cover - Popen(stdout=PIPE) guarantees it
            return
        while True:
            try:
                chunk = stream.read(_READ_CHUNK)
            except (OSError, ValueError):
                break
            if not chunk:
                break
            with lock:
                state["total"] += len(chunk)
                buffer.extend(chunk)
                overflow = len(buffer) - max_output_bytes
                if overflow > 0:
                    del buffer[:overflow]
                    state["truncated"] = True

    reader = threading.Thread(target=read_output, name="build-runner-output", daemon=True)
    reader.start()

    timed_out = False
    exit_code: int | None
    try:
        exit_code = process.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        timed_out = True
        _terminate_process_tree(process)
        exit_code = process.poll()

    reader.join(timeout=15)
    if reader.is_alive():
        # A descendant still owns the pipe.  Stop waiting; the process group
        # was already signalled, so this is best-effort.
        try:
            if stream is not None:
                stream.close()
        except OSError:
            pass
        reader.join(timeout=2)
        with lock:
            state["truncated"] = True

    with lock:
        output = bytes(buffer)
        total = state["total"]
        truncated = bool(state["truncated"])

    return CommandResult(
        argv=tuple(argv_list),
        exit_code=exit_code,
        timed_out=timed_out,
        output=output,
        total_output_bytes=total,
        truncated=truncated,
        duration_ms=int((time.monotonic() - started) * 1_000),
    )


def _terminate_process_tree(process: subprocess.Popen[bytes]) -> None:
    """SIGTERM the process group, escalating to SIGKILL after a grace period."""

    _signal(process, signal.SIGTERM)
    try:
        process.wait(timeout=5)
        return
    except subprocess.TimeoutExpired:
        pass
    _signal(process, signal.SIGKILL)
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        logger.error("build process %s did not exit after SIGKILL", getattr(process, "pid", "?"))


def _signal(process: subprocess.Popen[bytes], sig: signal.Signals) -> None:
    pid = getattr(process, "pid", None)
    if pid:
        try:
            os.killpg(os.getpgid(pid), sig)
            return
        except (ProcessLookupError, PermissionError):
            pass
    try:
        if sig == signal.SIGKILL:
            process.kill()
        else:
            process.terminate()
    except (ProcessLookupError, OSError):
        pass


def redact(text: str, secrets: Iterable[str | None]) -> str:
    """Remove known credentials and credential-shaped strings from output."""

    for secret in secrets:
        if not secret:
            continue
        text = text.replace(secret, "[REDACTED]")
        encoded = base64.b64encode(secret.encode("utf-8", "replace")).decode("ascii")
        if encoded:
            text = text.replace(encoded, "[REDACTED]")
    text = re.sub(
        r"(?i)(authorization:\s*(?:bearer|basic)\s+)[^\s]+",
        r"\1[REDACTED]",
        text,
    )
    # Strip any URL userinfo, even when there is no password.  Some providers
    # accept a token as the user name.
    text = re.sub(
        r"(?i)(https?://)[^/\s@]+@",
        r"\1[REDACTED]@",
        text,
    )
    return text


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

        clone_argv = [
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
        clone = self._run(
            clone_argv,
            cwd=workspace,
            env=self._clone_env(request),
            timeout=self.settings.clone_timeout,
        )
        if clone.timed_out or clone.exit_code != 0:
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

        if not source.is_dir():
            raise ValidationError("git_url", "clone did not create a source directory")

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
        else:
            context = resolve_within(
                source, ".", field="context_dir", expect_directory=True
            )
            dockerfile = None

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
        work_dir = self.settings.work_dir
        try:
            # mkdtemp creates the per-build directory with mode 0700.  Do not
            # chmod an existing operator-provided directory (it may be shared
            # with the daemon or the host).
            work_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
            return Path(tempfile.mkdtemp(prefix="build-", dir=work_dir))
        except OSError as exc:
            reason = exc.strerror or type(exc).__name__
            raise BuildExecutionError(f"cannot prepare workspace: {reason}") from exc

    def _cleanup(self, workspace: Path | None) -> str | None:
        if workspace is None:
            return None

        def on_error(func, path: str, _exc: BaseException) -> None:
            try:
                if not os.path.islink(path):
                    os.chmod(path, 0o700)
                func(path)
            except OSError:
                pass

        try:
            shutil.rmtree(workspace, onexc=on_error)
        except Exception as exc:
            return f"workspace cleanup failed: {type(exc).__name__}"
        if workspace.exists():
            return "workspace cleanup incomplete"
        return None

    def _compose_log(
        self,
        clone: CommandResult | None,
        build: CommandResult | None,
        secrets: Iterable[str | None],
    ) -> tuple[str, bool]:
        parts: list[str] = []
        if clone is not None and clone.output:
            parts.append("[git clone]\n" + _decode(clone.output))
        if build is not None and build.output:
            parts.append("[build]\n" + _decode(build.output))
        text = redact("\n".join(parts), secrets).replace("\x00", "")
        text = _ANSI_RE.sub("", text)

        encoded = text.encode("utf-8", "replace")
        truncated = bool(
            (clone is not None and clone.truncated)
            or (build is not None and build.truncated)
        )
        limit = self.settings.response_tail_bytes
        if len(encoded) > limit:
            encoded = encoded[-limit:]
            text = encoded.decode("utf-8", "ignore")
            truncated = True
        return text, truncated


def _decode(data: bytes) -> str:
    return data.decode("utf-8", "replace")
