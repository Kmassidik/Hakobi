"""Build log composition, credential redaction, and response tail limiting."""

from __future__ import annotations

import base64
import re
from typing import Iterable

from .execution_models import CommandResult

_ANSI_RE = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")


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


def compose_log(
    clone: CommandResult | None,
    build: CommandResult | None,
    secrets: Iterable[str | None],
    response_tail_bytes: int,
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
    if len(encoded) > response_tail_bytes:
        encoded = encoded[-response_tail_bytes:]
        text = encoded.decode("utf-8", "ignore")
        truncated = True
    return text, truncated


def _decode(data: bytes) -> str:
    return data.decode("utf-8", "replace")
