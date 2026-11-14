"""Strict input validation for the build runner.

Nothing in this module touches the network, the filesystem (except
:func:`resolve_within`, which only inspects an existing checkout), or a
subprocess.  Rejecting bad input here means it never reaches ``git`` or
``docker``.
"""

from __future__ import annotations

import ipaddress
import posixpath
import re
from pathlib import Path
from urllib.parse import urlsplit

MAX_GIT_URL_LENGTH = 2_048
MAX_BRANCH_LENGTH = 255
MAX_IMAGE_TAG_LENGTH = 255
MAX_PATH_LENGTH = 1_024
MAX_TOKEN_LENGTH = 8_192

_CONTROL_OR_SPACE_RE = re.compile(r"[\x00-\x20\x7f]")
_FORBIDDEN_REF_CHARS = frozenset(' ~^:?*[\\\x7f')
_SCP_RE = re.compile(
    r"^(?P<user>[A-Za-z0-9._-]+)@(?P<host>[A-Za-z0-9._-]+):(?P<path>[^\s]+)$"
)
_HOST_RE = re.compile(r"^[A-Za-z0-9_](?:[A-Za-z0-9._-]*[A-Za-z0-9_])?$")
_WINDOWS_DRIVE_RE = re.compile(r"^[A-Za-z]:")

_DOMAIN_LABEL = r"[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?"
_DOMAIN = (
    rf"(?:\[[0-9A-Fa-f:]+\]|{_DOMAIN_LABEL}(?:\.{_DOMAIN_LABEL})*)"
    r"(?::[0-9]{1,5})?"
)
_PATH_COMPONENT = r"[a-z0-9]+(?:(?:[._]|__|-+)[a-z0-9]+)*"
_IMAGE_NAME = rf"(?:{_DOMAIN}/)?{_PATH_COMPONENT}(?:/{_PATH_COMPONENT})*"
_IMAGE_TAG = r"[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}"
_IMAGE_RE = re.compile(rf"^{_IMAGE_NAME}(?::{_IMAGE_TAG})?$")


class ValidationError(ValueError):
    """A request field is malformed or unsafe."""

    def __init__(self, field: str, message: str) -> None:
        super().__init__(f"{field}: {message}")
        self.field = field
        self.message = message

    def as_message(self) -> str:
        return f"{self.field}: {self.message}"


def validate_git_url(value: object, *, allowed_hosts: tuple[str, ...] = ()) -> str:
    """Validate an https or ssh Git remote and return it unchanged.

    Rejected: embedded passwords, query strings, fragments, control characters,
    option-like values, and every other transport (``file://``, ``git://``,
    ``ext::``, ``http://``).
    """

    value = _validate_git_url_string(value)
    if "://" not in value:
        host, path = _validate_scp_remote(value)
        _check_host_allowed(host, allowed_hosts)
        return value

    host = _validate_url_remote(value)
    _check_host_allowed(host, allowed_hosts)
    return value


def _validate_git_url_string(value: object) -> str:
    if not isinstance(value, str):
        raise ValidationError("git_url", "must be a string")
    if not value:
        raise ValidationError("git_url", "must not be empty")
    if value != value.strip():
        raise ValidationError("git_url", "must not have leading or trailing whitespace")
    if len(value) > MAX_GIT_URL_LENGTH:
        raise ValidationError("git_url", "is too long")
    if _CONTROL_OR_SPACE_RE.search(value):
        raise ValidationError("git_url", "must not contain whitespace or control characters")
    if value.startswith("-"):
        raise ValidationError("git_url", "must not start with '-'")
    return value


def _validate_url_remote(value: str) -> str:
    try:
        parsed = urlsplit(value)
    except ValueError as exc:
        raise ValidationError("git_url", "is not a valid URL") from exc

    scheme = parsed.scheme.lower()
    if scheme not in {"https", "ssh"}:
        raise ValidationError("git_url", "scheme must be https or ssh")
    if parsed.query or parsed.fragment:
        raise ValidationError("git_url", "must not contain a query string or fragment")
    if parsed.password is not None:
        raise ValidationError("git_url", "must not embed a password or token")
    if "@" in parsed.netloc and not parsed.username:
        raise ValidationError("git_url", "has an empty user name")
    if parsed.username and (
        parsed.username.startswith("-")
        or not re.fullmatch(r"[A-Za-z0-9._-]+", parsed.username)
    ):
        raise ValidationError("git_url", "has an invalid user name")

    host = parsed.hostname
    if not host:
        raise ValidationError("git_url", "must include a host")
    _validate_host(host)
    try:
        port = parsed.port
    except ValueError as exc:
        raise ValidationError("git_url", "has an invalid port") from exc
    if port is not None and not 1 <= port <= 65_535:
        raise ValidationError("git_url", "has an invalid port")
    if not parsed.path or parsed.path == "/":
        raise ValidationError("git_url", "must include a repository path")
    if ".." in parsed.path.split("/"):
        raise ValidationError("git_url", "must not contain '..' path segments")
    return host


def _validate_scp_remote(value: str) -> tuple[str, str]:
    match = _SCP_RE.match(value)
    if not match:
        raise ValidationError("git_url", "must be an https, ssh://, or user@host:path remote")
    host = match.group("host")
    if host.startswith("-") or host.endswith(".") or ".." in host:
        raise ValidationError("git_url", "has an invalid host")
    if match.group("user").startswith("-"):
        raise ValidationError("git_url", "has an invalid user name")
    path = match.group("path")
    if ".." in path.split("/"):
        raise ValidationError("git_url", "must not contain '..' path segments")
    if "?" in path or "#" in path:
        raise ValidationError("git_url", "must not contain a query string or fragment")
    return host, path


def _validate_host(host: str) -> None:
    # urlsplit strips IPv6 brackets; accept literal addresses and DNS names.
    try:
        ipaddress.ip_address(host)
        return
    except ValueError:
        pass
    if not _HOST_RE.match(host) or host.startswith(("-", ".")) or host.endswith((".", "-")):
        raise ValidationError("git_url", "has an invalid host")
    if ".." in host:
        raise ValidationError("git_url", "has an invalid host")


def _check_host_allowed(host: str, allowed_hosts: tuple[str, ...]) -> None:
    if not allowed_hosts:
        return
    normalized = host.lower().rstrip(".")
    for pattern in allowed_hosts:
        candidate = pattern.lower().rstrip(".")
        if candidate.startswith("*."):
            suffix = candidate[1:]
            if normalized.endswith(suffix) and normalized != suffix[1:]:
                return
        elif normalized == candidate:
            return
    raise ValidationError("git_url", f"host {host!r} is not in BUILD_RUNNER_ALLOWED_GIT_HOSTS")


def validate_branch(value: object) -> str:
    """Validate a Git branch/ref name using ``git check-ref-format`` rules."""

    if not isinstance(value, str):
        raise ValidationError("branch", "must be a string")
    if not value:
        raise ValidationError("branch", "must not be empty")
    if len(value) > MAX_BRANCH_LENGTH:
        raise ValidationError("branch", "is too long")
    if value.startswith("-"):
        raise ValidationError("branch", "must not start with '-'")
    if value == "@":
        raise ValidationError("branch", "must not be '@'")
    if value.startswith("/") or value.endswith("/") or "//" in value:
        raise ValidationError("branch", "must not start or end with '/', or contain '//'")
    if value.startswith(".") or value.endswith("."):
        raise ValidationError("branch", "must not start or end with '.'")
    if ".." in value:
        raise ValidationError("branch", "must not contain '..'")
    if "@{" in value:
        raise ValidationError("branch", "must not contain '@{'")
    if any(ch in _FORBIDDEN_REF_CHARS or ord(ch) < 0x20 or ord(ch) == 0x7F for ch in value):
        raise ValidationError("branch", "contains a character Git refs cannot use")
    for component in value.split("/"):
        if not component:
            raise ValidationError("branch", "contains an empty path component")
        if component.startswith("."):
            raise ValidationError("branch", "has a path component starting with '.'")
        if component.endswith(".lock"):
            raise ValidationError("branch", "has a path component ending with '.lock'")
    return value


def validate_image_tag(value: object) -> str:
    """Validate a Docker image reference suitable for ``docker build -t``."""

    if not isinstance(value, str):
        raise ValidationError("image_tag", "must be a string")
    if not value:
        raise ValidationError("image_tag", "must not be empty")
    if value != value.strip():
        raise ValidationError("image_tag", "must not have leading or trailing whitespace")
    if len(value) > MAX_IMAGE_TAG_LENGTH:
        raise ValidationError("image_tag", "is too long")
    if "@" in value:
        raise ValidationError("image_tag", "must not contain a digest")
    if not _IMAGE_RE.match(value):
        raise ValidationError(
            "image_tag",
            "must be a lowercase repository name with an optional tag, e.g. "
            "registry.example.com/team/app:v1",
        )
    return value


def validate_repo_path(value: object, field: str, *, allow_root: bool = False) -> str:
    """Validate a repository-relative POSIX path.

    Absolute paths, drive letters, backslashes, control characters, and ``..``
    segments are rejected before a checkout exists.
    """

    if not isinstance(value, str):
        raise ValidationError(field, "must be a string")
    if not value:
        raise ValidationError(field, "must not be empty")
    if len(value) > MAX_PATH_LENGTH:
        raise ValidationError(field, "is too long")
    if _CONTROL_OR_SPACE_RE.search(value):
        raise ValidationError(field, "must not contain whitespace or control characters")
    if "\\" in value:
        raise ValidationError(field, "must use '/' separators")
    if value.startswith(("/", "~")):
        raise ValidationError(field, "must be relative to the repository root")
    if _WINDOWS_DRIVE_RE.match(value):
        raise ValidationError(field, "must not be an absolute Windows path")
    if "//" in value:
        raise ValidationError(field, "must not contain empty path components")

    components = value.split("/")
    if any(component == ".." for component in components):
        raise ValidationError(field, "must not contain '..' path segments")

    normalized = posixpath.normpath(value)
    if normalized == ".." or normalized.startswith("../"):
        raise ValidationError(field, "must stay inside the repository")
    if normalized == "." and not allow_root:
        raise ValidationError(field, "must point to a file, not the repository root")
    return normalized


def resolve_within(
    root: Path,
    relative: str,
    *,
    field: str,
    must_exist: bool = True,
    expect_directory: bool = False,
) -> Path:
    """Resolve ``relative`` inside ``root``, following symlinks safely.

    The real path (after symlink resolution) must remain inside the checkout.
    This blocks a repository that symlinks ``Dockerfile`` or a context
    directory to a host path.
    """

    try:
        root_resolved = Path(root).resolve(strict=True)
    except (OSError, RuntimeError) as exc:
        raise ValidationError(field, "checkout is not readable") from exc
    if not root_resolved.is_dir():
        raise ValidationError(field, "checkout is not a directory")

    candidate = root_resolved / relative
    try:
        resolved = candidate.resolve(strict=must_exist)
    except (OSError, RuntimeError) as exc:
        raise ValidationError(field, f"{relative!r} is not accessible") from exc

    if resolved != root_resolved and root_resolved not in resolved.parents:
        raise ValidationError(field, f"{relative!r} resolves outside the repository")

    if must_exist:
        if expect_directory and not resolved.is_dir():
            raise ValidationError(field, f"{relative!r} is not a directory")
        if not expect_directory and not resolved.is_file():
            raise ValidationError(field, f"{relative!r} is not a file")
    return resolved


def validate_token(value: object, *, git_url: str) -> str | None:
    """Validate the optional HTTPS access token.

    The token is delivered to Git through environment-scoped config, never as
    an argument.  SSH remotes use the runner's SSH agent/known_hosts instead.
    """

    if value is None:
        return None
    if not isinstance(value, str):
        raise ValidationError("token", "must be a string or null")
    if not value:
        raise ValidationError("token", "must not be empty when provided")
    if len(value) > MAX_TOKEN_LENGTH:
        raise ValidationError("token", "is too long")
    if _CONTROL_OR_SPACE_RE.search(value):
        raise ValidationError("token", "must not contain whitespace or control characters")
    if not value.isascii():
        raise ValidationError("token", "must be ASCII")
    if not git_url.lower().startswith("https://"):
        raise ValidationError("token", "is only supported for https remotes")
    return value
