"""Dependency-free HTTP API for the standalone build runner."""

from __future__ import annotations

import hmac
import json
import logging
import re
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any, Mapping
from urllib.parse import urlsplit

from . import __version__
from .config import Settings
from .executor import BuildExecutor, BuildRequest, BusyError
from .validation import (
    MAX_BRANCH_LENGTH,
    MAX_GIT_URL_LENGTH,
    MAX_IMAGE_TAG_LENGTH,
    MAX_PATH_LENGTH,
    MAX_TOKEN_LENGTH,
    ValidationError,
)

logger = logging.getLogger("build_runner.http")

_BODY_FIELD_LIMITS = {
    "git_url": MAX_GIT_URL_LENGTH,
    "branch": MAX_BRANCH_LENGTH,
    "image_tag": MAX_IMAGE_TAG_LENGTH,
    "dockerfile_path": MAX_PATH_LENGTH,
    "context_dir": MAX_PATH_LENGTH,
    "token": MAX_TOKEN_LENGTH,
}

_FIELDS = {
    "dockerfile": {
        "git_url",
        "branch",
        "dockerfile_path",
        "context_dir",
        "image_tag",
        "token",
    },
    "nixpacks": {"git_url", "branch", "image_tag", "token"},
}

_LOG_UNSAFE_RE = re.compile(r"[\x00-\x1f\x7f]")


class ApiError(Exception):
    def __init__(
        self,
        status: int,
        code: str,
        message: str,
        *,
        headers: tuple[tuple[str, str], ...] = (),
    ) -> None:
        super().__init__(message)
        self.status = status
        self.code = code
        self.message = message
        self.headers = headers


def make_handler(settings: Settings, executor: BuildExecutor):
    """Create a request handler bound to immutable settings and an executor."""

    class BuildRunnerHandler(BaseHTTPRequestHandler):
        server_version = "build-runner"
        sys_version = ""
        protocol_version = "HTTP/1.1"
        timeout = 60

        # -- lifecycle -------------------------------------------------
        def version_string(self) -> str:
            return self.server_version

        def log_request(self, code: object = "-", size: object = "-") -> None:
            # Never log the query string (a caller may mistakenly put a token
            # there) or control characters from a malformed request line.
            path = "/"
            if isinstance(self.path, str):
                try:
                    path = urlsplit(self.path).path or "/"
                except ValueError:
                    path = "/"
            if settings.api_token:
                path = path.replace(settings.api_token, "[REDACTED]")
            path = _LOG_UNSAFE_RE.sub("?", path)[:512]
            command = _LOG_UNSAFE_RE.sub("?", str(self.command))[:32]
            logger.info(
                '%s "%s %s" %s %s',
                self.address_string(),
                command,
                path,
                code,
                size,
            )

        def log_message(self, format: str, *args: object) -> None:
            try:
                message = format % args
            except (TypeError, ValueError):  # pragma: no cover - defensive
                message = str(format)
            if settings.api_token:
                message = message.replace(settings.api_token, "[REDACTED]")
            message = _LOG_UNSAFE_RE.sub("?", message)[:512]
            logger.info("%s %s", self.address_string(), message)

        def _route(self) -> str:
            parsed = urlsplit(self.path)
            if parsed.query:
                raise ApiError(400, "invalid_request", "query parameters are not supported")
            if parsed.path == "/healthz":
                return "healthz"
            if parsed.path == "/build/dockerfile":
                return "dockerfile"
            if parsed.path == "/build/nixpacks":
                return "nixpacks"
            if parsed.path == "/build/compose":
                return "compose"
            raise ApiError(404, "not_found", "no such endpoint")

        def do_GET(self) -> None:
            self._dispatch("GET")

        def do_HEAD(self) -> None:
            self._dispatch("HEAD")

        def do_POST(self) -> None:
            self._dispatch("POST")

        def do_PUT(self) -> None:
            self._dispatch("PUT")

        def do_PATCH(self) -> None:
            self._dispatch("PATCH")

        def do_DELETE(self) -> None:
            self._dispatch("DELETE")

        def do_OPTIONS(self) -> None:
            self._dispatch("OPTIONS")

        def do_TRACE(self) -> None:
            self._dispatch("TRACE")

        def _dispatch(self, method: str) -> None:
            try:
                route = self._route()
                allowed = "GET, HEAD" if route == "healthz" else "POST"
                if method not in {part.strip() for part in allowed.split(",")}:
                    raise ApiError(
                        405,
                        "method_not_allowed",
                        f"{route} does not support {method}",
                        headers=(("Allow", allowed),),
                    )
                if route == "healthz":
                    self._send_json(200, {"status": "ok", "version": __version__})
                    return

                self._authenticate()
                if route == "compose":
                    raise ApiError(
                        501,
                        "not_implemented",
                        "compose builds are not implemented by this runner",
                    )
                self._handle_build(route)
            except ApiError as exc:
                self._send_error(exc)
            except ValidationError as exc:
                self._send_error(
                    ApiError(400, "validation_error", exc.as_message())
                )
            except BusyError as exc:
                self._send_error(
                    ApiError(
                        429,
                        "busy",
                        str(exc),
                        headers=(("Retry-After", "5"),),
                    )
                )
            except (BrokenPipeError, ConnectionResetError):
                self.close_connection = True
            except TimeoutError:
                self._send_error(ApiError(408, "request_timeout", "request timed out"))
            except Exception as exc:
                # Do not log exception text: a future code path could include
                # request data.  The type is enough to triage.
                logger.error(
                    "unhandled error while serving %s (%s)",
                    self.command,
                    type(exc).__name__,
                )
                self._send_error(ApiError(500, "internal_error", "internal server error"))

        # -- auth + body ----------------------------------------------
        def _authenticate(self) -> None:
            expected = settings.api_token
            if expected is None:
                return

            values = self.headers.get_all("Authorization", [])
            if len(values) != 1:
                raise self._unauthorized()
            scheme, separator, provided = values[0].partition(" ")
            if not separator or scheme.lower() != "bearer":
                raise self._unauthorized()
            provided = provided.strip()
            if not provided or not hmac.compare_digest(
                provided.encode("utf-8"), expected.encode("utf-8")
            ):
                raise self._unauthorized()

        @staticmethod
        def _unauthorized() -> ApiError:
            return ApiError(
                401,
                "unauthorized",
                "a valid bearer token is required",
                headers=(("WWW-Authenticate", 'Bearer realm="build-runner"'),),
            )

        def _read_json(self) -> dict[str, Any]:
            transfer_encoding = self.headers.get("Transfer-Encoding")
            if transfer_encoding and transfer_encoding.strip().lower() != "identity":
                raise ApiError(411, "length_required", "chunked request bodies are not supported")

            lengths = self.headers.get_all("Content-Length", [])
            if len(lengths) != 1:
                raise ApiError(411, "length_required", "exactly one Content-Length is required")
            try:
                length = int(lengths[0])
            except (TypeError, ValueError):
                raise ApiError(400, "invalid_request", "Content-Length must be an integer") from None
            if length <= 0:
                raise ApiError(400, "invalid_request", "request body must not be empty")
            if length > settings.max_request_bytes:
                raise ApiError(413, "payload_too_large", "request body is too large")

            content_type = self.headers.get("Content-Type", "")
            media_type = content_type.split(";", 1)[0].strip().lower()
            if media_type != "application/json":
                raise ApiError(415, "unsupported_media_type", "Content-Type must be application/json")

            body = self.rfile.read(length)
            if len(body) != length:
                raise ApiError(400, "invalid_request", "incomplete request body")

            try:
                payload = json.loads(body)
            except (json.JSONDecodeError, UnicodeDecodeError, RecursionError):
                raise ApiError(400, "invalid_json", "request body is not valid JSON") from None
            if not isinstance(payload, dict):
                raise ApiError(400, "invalid_request", "request body must be a JSON object")
            return payload

        def _handle_build(self, kind: str) -> None:
            payload = self._read_json()
            allowed = _FIELDS[kind]
            unknown = sorted(set(payload) - allowed)
            if unknown:
                raise ApiError(
                    400,
                    "invalid_request",
                    f"unknown field(s): {', '.join(unknown)}",
                )

            git_url = self._string(payload, "git_url", required=True)
            branch = self._string(payload, "branch", required=True)
            image_tag = self._string(payload, "image_tag", required=True)

            token_value = payload.get("token")
            if token_value is not None and not isinstance(token_value, str):
                raise ApiError(400, "validation_error", "token: must be a string or null")
            if isinstance(token_value, str) and len(token_value) > MAX_TOKEN_LENGTH:
                raise ApiError(400, "validation_error", "token: is too long")

            if kind == "dockerfile":
                dockerfile_path = self._string(
                    payload, "dockerfile_path", default="Dockerfile"
                )
                context_dir = self._string(payload, "context_dir", default=".")
            else:
                dockerfile_path = "Dockerfile"
                context_dir = "."

            request = BuildRequest(
                git_url=git_url,
                branch=branch,
                image_tag=image_tag,
                dockerfile_path=dockerfile_path,
                context_dir=context_dir,
                token=token_value,
            )

            if kind == "dockerfile":
                result = executor.build_dockerfile(request)
            else:
                result = executor.build_nixpacks(request)

            status_code = 500 if result.status == "error" else 200
            logger.info(
                "build kind=%s status=%s image_tag=%s duration_ms=%d",
                kind,
                result.status,
                result.image_tag,
                result.duration_ms,
            )
            self._send_json(status_code, result.to_dict())

        def _string(
            self,
            payload: Mapping[str, Any],
            name: str,
            *,
            required: bool = False,
            default: str | None = None,
        ) -> str:
            if name not in payload:
                if required:
                    raise ApiError(400, "missing_field", f"{name} is required")
                assert default is not None
                return default
            value = payload[name]
            if not isinstance(value, str):
                raise ApiError(400, "validation_error", f"{name}: must be a string")
            if len(value) > _BODY_FIELD_LIMITS[name]:
                raise ApiError(400, "validation_error", f"{name}: is too long")
            return value

        # -- responses -------------------------------------------------
        def _send_error(self, error: ApiError) -> None:
            headers = list(error.headers)
            if error.status == 429 and not any(h == "Retry-After" for h, _ in headers):
                headers.append(("Retry-After", "5"))
            self._send_json(
                error.status,
                {"error": {"code": error.code, "message": error.message}},
                headers=headers,
            )

        def _send_json(
            self,
            status: int,
            payload: Mapping[str, Any],
            *,
            headers: list[tuple[str, str]] | tuple[tuple[str, str], ...] = (),
        ) -> None:
            body = json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode(
                "utf-8"
            )
            try:
                self.send_response(status)
                self.send_header("Content-Type", "application/json; charset=utf-8")
                self.send_header("Content-Length", str(len(body)))
                self.send_header("Cache-Control", "no-store")
                self.send_header("X-Content-Type-Options", "nosniff")
                self.send_header("Connection", "close")
                for name, value in headers:
                    self.send_header(name, value)
                self.end_headers()
                if self.command != "HEAD":
                    self.wfile.write(body)
            except (BrokenPipeError, ConnectionResetError):
                pass
            finally:
                self.close_connection = True

    return BuildRunnerHandler


class BuildRunnerServer(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True
    request_queue_size = 32


def create_server(settings: Settings, executor: BuildExecutor | None = None) -> BuildRunnerServer:
    settings.validate_runtime()
    if executor is None:
        executor = BuildExecutor(settings)
    handler = make_handler(settings, executor)
    return BuildRunnerServer((settings.host, settings.port), handler)
