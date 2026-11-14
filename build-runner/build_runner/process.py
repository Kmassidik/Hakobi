"""Process execution, bounded output collection, and process-tree cleanup."""

from __future__ import annotations

import logging
import os
import signal
import subprocess
import threading
import time
from pathlib import Path
from typing import BinaryIO, Mapping, Sequence

from .execution_models import CommandResult

# Keep the original logger name for compatibility with existing log handlers.
logger = logging.getLogger("build_runner.executor")

_READ_CHUNK = 65_536


def _validate_command(
    argv: Sequence[str], timeout: float, max_output_bytes: int
) -> list[str]:
    if isinstance(argv, str) or not argv:
        raise ValueError("argv must be a non-empty sequence of strings")
    argv_list = list(argv)
    if not all(isinstance(arg, str) and arg for arg in argv_list):
        raise ValueError("argv entries must be non-empty strings")
    if timeout <= 0:
        raise ValueError("timeout must be positive")
    if max_output_bytes <= 0:
        raise ValueError("max_output_bytes must be positive")
    return argv_list


def _spawn_process(
    argv: list[str], cwd: Path | None, env: Mapping[str, str] | None
) -> subprocess.Popen[bytes]:
    return subprocess.Popen(
        argv,
        cwd=str(cwd) if cwd is not None else None,
        env=dict(env) if env is not None else None,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        start_new_session=True,
        close_fds=True,
        bufsize=0,
    )


def _read_output(
    stream: BinaryIO | None,
    buffer: bytearray,
    state: dict[str, int | bool],
    lock: threading.Lock,
    max_output_bytes: int,
) -> None:
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


def _wait_for_process(
    process: subprocess.Popen[bytes], timeout: float
) -> tuple[int | None, bool]:
    try:
        return process.wait(timeout=timeout), False
    except subprocess.TimeoutExpired:
        _terminate_process_tree(process)
        return process.poll(), True


def _finish_reader(
    reader: threading.Thread,
    stream: BinaryIO | None,
    state: dict[str, int | bool],
    lock: threading.Lock,
) -> None:
    reader.join(timeout=15)
    if not reader.is_alive():
        return
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


def run_command(
    argv: Sequence[str],
    *,
    cwd: Path | None = None,
    env: Mapping[str, str] | None = None,
    timeout: float,
    max_output_bytes: int = 262_144,
) -> CommandResult:
    """Run a command in its own process group, keeping only the output tail."""

    argv_list = _validate_command(argv, timeout, max_output_bytes)
    started = time.monotonic()
    process = _spawn_process(argv_list, cwd, env)
    buffer = bytearray()
    state: dict[str, int | bool] = {"total": 0, "truncated": False}
    lock = threading.Lock()
    stream = process.stdout
    reader = threading.Thread(
        target=_read_output,
        args=(stream, buffer, state, lock, max_output_bytes),
        name="build-runner-output",
        daemon=True,
    )
    reader.start()
    exit_code, timed_out = _wait_for_process(process, timeout)
    _finish_reader(reader, stream, state, lock)

    with lock:
        output = bytes(buffer)
        total = int(state["total"])
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
