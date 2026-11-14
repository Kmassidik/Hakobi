"""Creation and best-effort removal of isolated build workspaces."""

from __future__ import annotations

import os
import shutil
import tempfile
from pathlib import Path

from .execution_models import BuildExecutionError


def create_workspace(work_dir: Path) -> Path:
    try:
        # mkdtemp creates the per-build directory with mode 0700.  Do not
        # chmod an existing operator-provided directory (it may be shared
        # with the daemon or the host).
        work_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
        return Path(tempfile.mkdtemp(prefix="build-", dir=work_dir))
    except OSError as exc:
        reason = exc.strerror or type(exc).__name__
        raise BuildExecutionError(f"cannot prepare workspace: {reason}") from exc


def cleanup_workspace(workspace: Path | None) -> str | None:
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
