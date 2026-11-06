from __future__ import annotations

import contextlib
import json
import tempfile
import threading
import unittest
from http.client import HTTPConnection
from pathlib import Path
from typing import Iterator

from build_runner.config import ConfigError, Settings
from build_runner.executor import BuildExecutor, BuildResult, BusyError
from build_runner.server import create_server
from build_runner.validation import ValidationError

TOKEN = "server-token-0123456789"


class StubExecutor:
    def __init__(self) -> None:
        self.requests: list[tuple[str, object]] = []
        self.error: Exception | None = None
        self.status = "succeeded"

    def _respond(self, kind: str, request):
        self.requests.append((kind, request))
        if self.error is not None:
            raise self.error
        return BuildResult(
            status=self.status,
            image_tag=request.image_tag,
            exit_code=0,
            duration_ms=5,
            log_tail="hello from stub",
            truncated=False,
        )

    def build_dockerfile(self, request):
        return self._respond("dockerfile", request)

    def build_nixpacks(self, request):
        return self._respond("nixpacks", request)


def settings(work_dir: Path | None = None, **overrides) -> Settings:
    values = {
        "host": "127.0.0.1",
        "port": 0,
        "api_token": TOKEN,
        "work_dir": work_dir or Path(tempfile.gettempdir()) / "build-runner-tests",
        "clone_timeout": 5.0,
        "build_timeout": 5.0,
        "max_output_bytes": 4096,
        "response_tail_bytes": 2048,
        "max_request_bytes": 1_048_576,
        "max_concurrent_builds": 1,
    }
    values.update(overrides)
    return Settings(**values)


@contextlib.contextmanager
def running_server(
    config: Settings, executor: StubExecutor | BuildExecutor
) -> Iterator[tuple[str, int]]:
    server = create_server(config, executor)
    thread = threading.Thread(
        target=server.serve_forever, kwargs={"poll_interval": 0.05}, daemon=True
    )
    thread.start()
    host, port = server.server_address[0], server.server_address[1]
    try:
        yield host, port
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


def call(
    host: str,
    port: int,
    method: str,
    path: str,
    *,
    body: object | bytes | None = None,
    token: str | None = None,
    headers: dict[str, str] | None = None,
) -> tuple[int, dict | None, dict[str, str]]:
    conn = HTTPConnection(host, port, timeout=5)
    request_headers = dict(headers or {})
    if token is not None:
        request_headers["Authorization"] = f"Bearer {token}"
    payload: bytes | None
    if body is None:
        payload = None
    elif isinstance(body, bytes):
        payload = body
    else:
        payload = json.dumps(body).encode("utf-8")
    if payload is not None:
        request_headers.setdefault("Content-Type", "application/json")
        request_headers["Content-Length"] = str(len(payload))
    conn.request(method, path, body=payload, headers=request_headers)
    response = conn.getresponse()
    raw = response.read()
    response_headers = {name.lower(): value for name, value in response.getheaders()}
    conn.close()
    parsed = json.loads(raw) if raw else None
    return response.status, parsed, response_headers


class HttpApiTests(unittest.TestCase):
    def test_healthz_is_unauthenticated(self) -> None:
        executor = StubExecutor()
        with running_server(settings(), executor) as (host, port):
            status, body, _ = call(host, port, "GET", "/healthz")
        self.assertEqual(status, 200)
        self.assertEqual(body["status"], "ok")
        self.assertIn("version", body)

    def test_build_requires_bearer_token(self) -> None:
        executor = StubExecutor()
        with running_server(settings(), executor) as (host, port):
            status, body, headers = call(
                host,
                port,
                "POST",
                "/build/dockerfile",
                body={
                    "git_url": "https://github.com/acme/app.git",
                    "branch": "main",
                    "dockerfile_path": "Dockerfile",
                    "context_dir": ".",
                    "image_tag": "acme/app:test",
                },
            )
        self.assertEqual(status, 401)
        self.assertEqual(body["error"]["code"], "unauthorized")
        self.assertIn("bearer", headers["www-authenticate"].lower())
        self.assertEqual(executor.requests, [])

    def test_wrong_token_and_wrong_scheme_are_rejected(self) -> None:
        executor = StubExecutor()
        with running_server(settings(), executor) as (host, port):
            status, _, _ = call(
                host, port, "POST", "/build/dockerfile", body={}, token="wrong"
            )
            self.assertEqual(status, 401)
            status, _, _ = call(
                host,
                port,
                "POST",
                "/build/dockerfile",
                body={},
                headers={"Authorization": "Basic abc"},
            )
            self.assertEqual(status, 401)
        self.assertEqual(executor.requests, [])

    def test_dockerfile_build_returns_log_tail(self) -> None:
        executor = StubExecutor()
        with running_server(settings(), executor) as (host, port):
            status, body, _ = call(
                host,
                port,
                "POST",
                "/build/dockerfile",
                token=TOKEN,
                body={
                    "git_url": "https://github.com/acme/app.git",
                    "branch": "main",
                    "dockerfile_path": "docker/Dockerfile",
                    "context_dir": ".",
                    "image_tag": "acme/app:test",
                },
            )
        self.assertEqual(status, 200)
        self.assertEqual(body["status"], "succeeded")
        self.assertEqual(body["log_tail"], "hello from stub")
        self.assertEqual(executor.requests[0][0], "dockerfile")
        request = executor.requests[0][1]
        self.assertEqual(request.dockerfile_path, "docker/Dockerfile")
        self.assertEqual(request.image_tag, "acme/app:test")
        self.assertIsNone(request.token)

    def test_dockerfile_defaults(self) -> None:
        executor = StubExecutor()
        with running_server(settings(), executor) as (host, port):
            status, _, _ = call(
                host,
                port,
                "POST",
                "/build/dockerfile",
                token=TOKEN,
                body={
                    "git_url": "https://github.com/acme/app.git",
                    "branch": "main",
                    "image_tag": "acme/app:test",
                },
            )
        self.assertEqual(status, 200)
        request = executor.requests[0][1]
        self.assertEqual(request.dockerfile_path, "Dockerfile")
        self.assertEqual(request.context_dir, ".")

    def test_nixpacks_build(self) -> None:
        executor = StubExecutor()
        with running_server(settings(), executor) as (host, port):
            status, body, _ = call(
                host,
                port,
                "POST",
                "/build/nixpacks",
                token=TOKEN,
                body={
                    "git_url": "ssh://git@github.com/acme/app.git",
                    "branch": "main",
                    "image_tag": "acme/app:nixpacks",
                },
            )
        self.assertEqual(status, 200)
        self.assertEqual(body["image_tag"], "acme/app:nixpacks")
        self.assertEqual(executor.requests[0][0], "nixpacks")

    def test_nixpacks_rejects_dockerfile_fields(self) -> None:
        executor = StubExecutor()
        with running_server(settings(), executor) as (host, port):
            status, body, _ = call(
                host,
                port,
                "POST",
                "/build/nixpacks",
                token=TOKEN,
                body={
                    "git_url": "https://github.com/acme/app.git",
                    "branch": "main",
                    "image_tag": "acme/app:test",
                    "dockerfile_path": "Dockerfile",
                },
            )
        self.assertEqual(status, 400)
        self.assertIn("unknown", body["error"]["message"])
        self.assertEqual(executor.requests, [])

    def test_compose_is_not_implemented(self) -> None:
        executor = StubExecutor()
        with running_server(settings(), executor) as (host, port):
            status, body, _ = call(
                host,
                port,
                "POST",
                "/build/compose",
                token=TOKEN,
                body={
                    "git_url": "https://github.com/acme/app.git",
                    "branch": "main",
                    "image_tag": "acme/app:test",
                },
            )
        self.assertEqual(status, 501)
        self.assertEqual(body["error"]["code"], "not_implemented")
        self.assertEqual(executor.requests, [])

    def test_unknown_route(self) -> None:
        with running_server(settings(), StubExecutor()) as (host, port):
            status, body, _ = call(host, port, "GET", "/nope")
        self.assertEqual(status, 404)
        self.assertEqual(body["error"]["code"], "not_found")

    def test_query_parameters_are_rejected(self) -> None:
        with running_server(settings(), StubExecutor()) as (host, port):
            status, body, _ = call(host, port, "GET", "/healthz?verbose=1")
        self.assertEqual(status, 400)
        self.assertEqual(body["error"]["code"], "invalid_request")

    def test_get_build_is_method_not_allowed(self) -> None:
        with running_server(settings(), StubExecutor()) as (host, port):
            status, body, headers = call(host, port, "GET", "/build/dockerfile")
        self.assertEqual(status, 405)
        self.assertEqual(headers["allow"], "POST")
        self.assertEqual(body["error"]["code"], "method_not_allowed")

    def test_invalid_json_and_non_object_bodies(self) -> None:
        executor = StubExecutor()
        with running_server(settings(), executor) as (host, port):
            status, body, _ = call(
                host,
                port,
                "POST",
                "/build/dockerfile",
                token=TOKEN,
                body=b"{not json",
            )
            self.assertEqual(status, 400)
            self.assertEqual(body["error"]["code"], "invalid_json")

            status, body, _ = call(
                host, port, "POST", "/build/dockerfile", token=TOKEN, body=b"[]"
            )
            self.assertEqual(status, 400)
            self.assertEqual(body["error"]["code"], "invalid_request")
        self.assertEqual(executor.requests, [])

    def test_wrong_content_type(self) -> None:
        with running_server(settings(), StubExecutor()) as (host, port):
            status, body, _ = call(
                host,
                port,
                "POST",
                "/build/dockerfile",
                token=TOKEN,
                body=b"{}",
                headers={"Content-Type": "text/plain"},
            )
        self.assertEqual(status, 415)
        self.assertEqual(body["error"]["code"], "unsupported_media_type")

    def test_missing_field(self) -> None:
        with running_server(settings(), StubExecutor()) as (host, port):
            status, body, _ = call(
                host,
                port,
                "POST",
                "/build/nixpacks",
                token=TOKEN,
                body={"git_url": "https://github.com/acme/app.git", "branch": "main"},
            )
        self.assertEqual(status, 400)
        self.assertEqual(body["error"]["code"], "missing_field")
        self.assertIn("image_tag", body["error"]["message"])

    def test_oversized_body(self) -> None:
        with running_server(
            settings(max_request_bytes=1024), StubExecutor()
        ) as (host, port):
            status, body, _ = call(
                host,
                port,
                "POST",
                "/build/dockerfile",
                token=TOKEN,
                body=b"x" * 2048,
            )
        self.assertEqual(status, 413)
        self.assertEqual(body["error"]["code"], "payload_too_large")

    def test_validation_error_returns_400(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            executor = BuildExecutor(settings(work_dir=Path(tmp)))
            with running_server(settings(work_dir=Path(tmp)), executor) as (host, port):
                status, body, _ = call(
                    host,
                    port,
                    "POST",
                    "/build/dockerfile",
                    token=TOKEN,
                    body={
                        "git_url": "https://github.com/acme/app.git",
                        "branch": "main",
                        "dockerfile_path": "../Dockerfile",
                        "context_dir": ".",
                        "image_tag": "acme/app:test",
                    },
                )
        self.assertEqual(status, 400)
        self.assertEqual(body["error"]["code"], "validation_error")
        self.assertIn("dockerfile_path", body["error"]["message"])

    def test_busy_returns_429_with_retry_after(self) -> None:
        executor = StubExecutor()
        executor.error = BusyError("busy")
        with running_server(settings(), executor) as (host, port):
            status, body, headers = call(
                host,
                port,
                "POST",
                "/build/dockerfile",
                token=TOKEN,
                body={
                    "git_url": "https://github.com/acme/app.git",
                    "branch": "main",
                    "image_tag": "acme/app:test",
                },
            )
        self.assertEqual(status, 429)
        self.assertEqual(body["error"]["code"], "busy")
        self.assertEqual(headers["retry-after"], "5")

    def test_executor_validation_error_returns_400(self) -> None:
        executor = StubExecutor()
        executor.error = ValidationError("git_url", "should be handled as 400")
        with running_server(settings(), executor) as (host, port):
            status, _, _ = call(
                host,
                port,
                "POST",
                "/build/dockerfile",
                token=TOKEN,
                body={
                    "git_url": "https://github.com/acme/app.git",
                    "branch": "main",
                    "image_tag": "acme/app:test",
                },
            )
        self.assertEqual(status, 400)

    def test_unexpected_executor_error_returns_500(self) -> None:
        executor = StubExecutor()
        executor.error = RuntimeError("boom")
        with self.assertLogs("build_runner.http", level="ERROR"):
            with running_server(settings(), executor) as (host, port):
                status, body, _ = call(
                    host,
                    port,
                    "POST",
                    "/build/dockerfile",
                    token=TOKEN,
                    body={
                        "git_url": "https://github.com/acme/app.git",
                        "branch": "main",
                        "image_tag": "acme/app:test",
                    },
                )
        self.assertEqual(status, 500)
        self.assertEqual(body["error"]["code"], "internal_error")

    def test_executor_error_status_maps_to_500(self) -> None:
        executor = StubExecutor()
        executor.status = "error"
        with running_server(settings(), executor) as (host, port):
            status, body, _ = call(
                host,
                port,
                "POST",
                "/build/dockerfile",
                token=TOKEN,
                body={
                    "git_url": "https://github.com/acme/app.git",
                    "branch": "main",
                    "image_tag": "acme/app:test",
                },
            )
        self.assertEqual(status, 500)
        self.assertEqual(body["status"], "error")

    def test_query_secrets_are_not_logged(self) -> None:
        with self.assertLogs("build_runner.http", level="INFO") as captured:
            with running_server(settings(), StubExecutor()) as (host, port):
                status, _, _ = call(
                    host, port, "GET", f"/healthz?token={TOKEN}"
                )
        self.assertEqual(status, 400)
        self.assertNotIn(TOKEN, "\n".join(captured.output))

    def test_unauth_mode_only_loopback(self) -> None:
        with self.assertRaises(ConfigError):
            settings(
                api_token=None,
                allow_unauthenticated_http=True,
                host="0.0.0.0",
            ).validate_runtime()
        config = settings(api_token=None, allow_unauthenticated_http=True)
        with running_server(config, StubExecutor()) as (host, port):
            status, _, _ = call(
                host,
                port,
                "POST",
                "/build/nixpacks",
                body={
                    "git_url": "https://github.com/acme/app.git",
                    "branch": "main",
                    "image_tag": "acme/app:test",
                },
            )
        self.assertEqual(status, 200)


if __name__ == "__main__":
    unittest.main()
