from __future__ import annotations

import json
import os
import secrets
from datetime import datetime, timezone
from pathlib import Path


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def new_run_id(task_id: str) -> str:
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    return f"{stamp}-{task_id}-{secrets.token_hex(3)}"


class RunStore:
    def __init__(self, run_dir: Path):
        self.run_dir = run_dir
        self.state_path = run_dir / "state.json"

    @classmethod
    def create(cls, root: Path, task_id: str) -> "RunStore":
        root = root.expanduser().resolve()
        root.mkdir(parents=True, exist_ok=True)
        run_dir = root / new_run_id(task_id)
        run_dir.mkdir(mode=0o700, parents=False)
        return cls(run_dir)

    def write_text(self, name: str, content: str) -> Path:
        path = self.run_dir / name
        path.write_text(content, encoding="utf-8")
        path.chmod(0o600)
        return path

    def read_text(self, name: str) -> str:
        return (self.run_dir / name).read_text(encoding="utf-8")

    def save(self, state: dict) -> None:
        state["updated_at"] = utc_now()
        temporary = self.state_path.with_suffix(f".tmp-{os.getpid()}")
        temporary.write_text(json.dumps(state, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        temporary.chmod(0o600)
        os.replace(temporary, self.state_path)

    def load(self, run_dir: Path | None = None) -> dict:
        path = (run_dir or self.run_dir) / "state.json"
        return json.loads(path.read_text(encoding="utf-8"))
