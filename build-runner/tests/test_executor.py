from __future__ import annotations

import base64
import tempfile
import unittest
from dataclasses import dataclass
from pathlib import Path

from build_runner.config import Settings
from build_runner.executor import (
    BuildExecutor,
    BuildRequest,
    BusyError,
    CommandResult,
    redact,
)
from build_runner.validation import ValidationError

TOKEN = "ghp_" + "s" * 40


@dataclass
class Call:
    argv: list[str]
    cwd: Path
    env: dict[str, str]
    timeout: float


class FakeRunner:
    def __init__(
        self,
        *,
        clone_exit: int = 0,
        build_exit: int = 0,
        clone_timed_out: bool = False,
        build_timed_out: bool = False,
        clone_output: bytes = b"",
        build_output: bytes = b"build output\n",
        create_dockerfile: bool = True,
        symlink_dockerfile_to: Path | None = None,
        raise_oserror: bool = False,
    ) -> None:
        self.calls: list[Call] = []
        self.clone_exit = clone_exit
        self.build_exit = build_exit
        self.clone_timed_out = clone_timed_out
        self.build_timed_out = build_timed_out
        self.clone_output = clone_output
        self.build_output = build_output
        self.create_dockerfile = create_dockerfile
        self.symlink_dockerfile_to = symlink_dockerfile_to
        self.raise_oserror = raise_oserror

    def __call__(self, argv, *, cwd, env, timeout, max_output_bytes):
        self.calls.append(Call(list(argv), Path(cwd), dict(env), timeout))
        if self.raise_oserror:
            raise FileNotFoundError(2, "No such file or directory", argv[0])

        if "clone" in argv:
            destination = Path(argv[-1])
            destination.mkdir(parents=True)
            if self.symlink_dockerfile_to is not None:
                (destination / "Dockerfile").symlink_to(self.symlink_dockerfile_to)
            elif self.create_dockerfile:
                (destination / "Dockerfile").write_text("FROM scratch\n", encoding="utf-8")
            return CommandResult(
                argv=tuple(argv),
                exit_code=None if self.clone_timed_out else self.clone_exit,
                timed_out=self.clone_timed_out,
                output=self.clone_output,
                total_output_bytes=len(self.clone_output),
                truncated=False,
                duration_ms=1,
            )

        return CommandResult(
            argv=tuple(argv),
            exit_code=None if self.build_timed_out else self.build_exit,
            timed_out=self.build_timed_out,
            output=self.build_output,
            total_output_bytes=len(self.build_output),
            truncated=False,
            duration_ms=1,
        )

    def clone_call(self) -> Call:
        for call in self.calls:
            if "clone" in call.argv:
                return call
        raise AssertionError("no clone call")

    def build_calls(self) -> list[Call]:
        return [call for call in self.calls if "clone" not in call.argv]


def request(**overrides) -> BuildRequest:
    values = {
        "git_url": "https://github.com/acme/app.git",
        "branch": "main",
        "image_tag": "acme/app:test",
        "dockerfile_path": "Dockerfile",
        "context_dir": ".",
        "token": None,
    }
    values.update(overrides)
    return BuildRequest(**values)


class ExecutorTestCase(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.work_dir = Path(self._tmp.name) / "work"

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def settings(self, **overrides) -> Settings:
        values = {
            "host": "127.0.0.1",
            "port": 0,
            "api_token": "test-token-0123456789",
            "work_dir": self.work_dir,
            "clone_timeout": 5.0,
            "build_timeout": 5.0,
            "max_output_bytes": 4096,
            "response_tail_bytes": 2048,
            "max_request_bytes": 4096,
            "max_concurrent_builds": 1,
        }
        values.update(overrides)
        return Settings(**values)

    def executor(self, runner: FakeRunner, **overrides) -> BuildExecutor:
        return BuildExecutor(self.settings(**overrides), runner=runner)


class DockerfileBuildTests(ExecutorTestCase):
    def test_success_uses_argv_arrays_and_cleans_workspace(self) -> None:
        runner = FakeRunner(build_output=b"Step 1/2\nSuccessfully built\n")
        executor = self.executor(runner)

        result = executor.build_dockerfile(request())

        self.assertEqual(result.status, "succeeded")
        self.assertEqual(result.exit_code, 0)
        self.assertIn("Successfully built", result.log_tail)
        self.assertFalse(result.truncated)

        clone_call = runner.clone_call()
        self.assertEqual(clone_call.argv[0], "git")
        self.assertIn("--branch", clone_call.argv)
        self.assertEqual(
            clone_call.argv[clone_call.argv.index("--branch") + 1], "main"
        )
        self.assertIn("--single-branch", clone_call.argv)
        self.assertIn("--no-recurse-submodules", clone_call.argv)
        self.assertEqual(clone_call.argv[-1], str(clone_call.cwd / "src"))

        build_call = runner.build_calls()[0]
        self.assertEqual(build_call.argv[0], "docker")
        self.assertEqual(build_call.argv[1], "build")
        self.assertIn("--file", build_call.argv)
        self.assertIn("--tag", build_call.argv)
        self.assertEqual(
            build_call.argv[build_call.argv.index("--tag") + 1], "acme/app:test"
        )
        self.assertEqual(build_call.argv[-1], str((clone_call.cwd / "src").resolve()))

        workspace = clone_call.cwd
        self.assertFalse(workspace.exists())

    def test_build_failure_reports_exit_and_cleans_workspace(self) -> None:
        runner = FakeRunner(build_exit=17, build_output=b"error: boom\n")
        executor = self.executor(runner)

        result = executor.build_dockerfile(request())

        self.assertEqual(result.status, "failed")
        self.assertEqual(result.exit_code, 17)
        self.assertIn("boom", result.log_tail)
        self.assertIsNotNone(result.detail)
        self.assertFalse(runner.clone_call().cwd.exists())

    def test_build_timeout_cleans_workspace(self) -> None:
        runner = FakeRunner(build_timed_out=True)
        executor = self.executor(runner)

        result = executor.build_dockerfile(request())

        self.assertEqual(result.status, "timed_out")
        self.assertEqual(result.exit_code, None)
        self.assertIn("timed out", result.detail or "")
        self.assertFalse(runner.clone_call().cwd.exists())

    def test_clone_failure_skips_build_and_cleans_workspace(self) -> None:
        runner = FakeRunner(clone_exit=128, clone_output=b"fatal: repository not found\n")
        executor = self.executor(runner)

        result = executor.build_dockerfile(request())

        self.assertEqual(result.status, "failed")
        self.assertEqual(result.exit_code, 128)
        self.assertEqual(runner.build_calls(), [])
        self.assertIn("repository not found", result.log_tail)
        self.assertFalse(runner.clone_call().cwd.exists())

    def test_clone_timeout_cleans_workspace(self) -> None:
        runner = FakeRunner(clone_timed_out=True)
        executor = self.executor(runner)

        result = executor.build_dockerfile(request())

        self.assertEqual(result.status, "timed_out")
        self.assertEqual(runner.build_calls(), [])
        self.assertFalse(runner.clone_call().cwd.exists())

    def test_missing_dockerfile_is_rejected_and_cleaned(self) -> None:
        runner = FakeRunner(create_dockerfile=False)
        executor = self.executor(runner)

        with self.assertRaises(ValidationError):
            executor.build_dockerfile(request())

        self.assertEqual(runner.build_calls(), [])
        self.assertFalse(runner.clone_call().cwd.exists())

    def test_symlinked_dockerfile_escape_is_rejected_and_cleaned(self) -> None:
        outside = Path(self._tmp.name) / "outside.txt"
        outside.write_text("FROM scratch\n", encoding="utf-8")
        runner = FakeRunner(symlink_dockerfile_to=outside)
        executor = self.executor(runner)

        with self.assertRaises(ValidationError):
            executor.build_dockerfile(request())

        self.assertEqual(runner.build_calls(), [])
        self.assertFalse(runner.clone_call().cwd.exists())

    def test_context_symlink_escape_is_rejected(self) -> None:
        outside = Path(self._tmp.name) / "outside-dir"
        outside.mkdir()

        class ContextSymlinkRunner(FakeRunner):
            def __call__(self, argv, *, cwd, env, timeout, max_output_bytes):
                call = super().__call__(
                    argv, cwd=cwd, env=env, timeout=timeout, max_output_bytes=max_output_bytes
                )
                if "clone" in argv:
                    Path(argv[-1], "context").symlink_to(outside)
                return call

        runner = ContextSymlinkRunner()
        executor = BuildExecutor(self.settings(), runner=runner)

        with self.assertRaises(ValidationError):
            executor.build_dockerfile(request(context_dir="context"))
        self.assertEqual(runner.build_calls(), [])
        self.assertFalse(runner.clone_call().cwd.exists())

    def test_dockerfile_outside_context_still_builds(self) -> None:
        runner = FakeRunner()
        executor = self.executor(runner)

        def create_nested(argv, *, cwd, env, timeout, max_output_bytes):
            call = runner(
                argv, cwd=cwd, env=env, timeout=timeout, max_output_bytes=max_output_bytes
            )
            if "clone" in argv:
                nested = Path(argv[-1], "docker")
                nested.mkdir()
                (nested / "Dockerfile").write_text("FROM scratch\n", encoding="utf-8")
            return call

        executor = BuildExecutor(self.settings(), runner=create_nested)
        result = executor.build_dockerfile(
            request(dockerfile_path="docker/Dockerfile", context_dir=".")
        )
        self.assertEqual(result.status, "succeeded")
        self.assertTrue(Path(runner.build_calls()[0].argv[-1]).name == "src")

    def test_missing_binary_returns_error_and_cleans_workspace(self) -> None:
        runner = FakeRunner(raise_oserror=True)
        executor = self.executor(runner)

        result = executor.build_dockerfile(request())

        self.assertEqual(result.status, "error")
        self.assertIn("could not execute", result.detail or "")
        self.assertFalse(runner.clone_call().cwd.exists())

    def test_unsafe_paths_are_rejected_before_clone(self) -> None:
        for overrides in (
            {"dockerfile_path": "../Dockerfile"},
            {"context_dir": "/etc"},
            {"dockerfile_path": "/etc/passwd"},
        ):
            with self.subTest(overrides=overrides):
                runner = FakeRunner()
                executor = self.executor(runner)
                with self.assertRaises(ValidationError):
                    executor.build_dockerfile(request(**overrides))
                self.assertEqual(runner.calls, [])


class TokenHandlingTests(ExecutorTestCase):
    def test_token_is_passed_via_environment_not_argv(self) -> None:
        runner = FakeRunner(build_output=b"building with token " + TOKEN.encode() + b"\n")
        executor = self.executor(runner)

        result = executor.build_dockerfile(request(token=TOKEN))

        clone_call = runner.clone_call()
        joined_argv = "\n".join(clone_call.argv)
        self.assertNotIn(TOKEN, joined_argv)
        self.assertNotIn(TOKEN, "\n".join(runner.build_calls()[0].argv))

        encoded = base64.b64encode(f"x-access-token:{TOKEN}".encode()).decode()
        self.assertEqual(clone_call.env["GIT_CONFIG_COUNT"], "1")
        self.assertTrue(clone_call.env["GIT_CONFIG_KEY_0"].startswith("http.https://"))
        self.assertTrue(clone_call.env["GIT_CONFIG_KEY_0"].endswith("/.extraHeader"))
        self.assertIn(encoded, clone_call.env["GIT_CONFIG_VALUE_0"])

        build_env = runner.build_calls()[0].env
        self.assertNotIn("GIT_CONFIG_VALUE_0", build_env)
        self.assertNotIn("GIT_CONFIG_COUNT", build_env)

        self.assertNotIn(TOKEN, result.log_tail)
        self.assertIn("[REDACTED]", result.log_tail)

    def test_token_with_ssh_remote_is_rejected_before_clone(self) -> None:
        runner = FakeRunner()
        executor = self.executor(runner)
        with self.assertRaises(ValidationError):
            executor.build_dockerfile(
                request(git_url="ssh://git@github.com/acme/app.git", token=TOKEN)
            )
        self.assertEqual(runner.calls, [])

    def test_scrubbed_child_environment(self) -> None:
        import os

        runner = FakeRunner()
        executor = self.executor(runner)
        os.environ["BUILD_RUNNER_SHOULD_NOT_LEAK"] = "secret"
        try:
            executor.build_dockerfile(request())
        finally:
            del os.environ["BUILD_RUNNER_SHOULD_NOT_LEAK"]

        env = runner.clone_call().env
        self.assertNotIn("BUILD_RUNNER_SHOULD_NOT_LEAK", env)
        self.assertEqual(env["GIT_TERMINAL_PROMPT"], "0")
        self.assertEqual(env["GIT_CONFIG_GLOBAL"], "/dev/null")
        self.assertEqual(env["GIT_ALLOW_PROTOCOL"], "https:ssh")
        self.assertEqual(env["DOCKER_BUILDKIT"], "1")

    def test_redact_removes_headers_and_url_userinfo(self) -> None:
        text = (
            "Authorization: Bearer secret-token\n"
            "fatal: https://user:password@example.com/acme/app.git\n"
            "fatal: https://a-token@example.com/acme/app.git\n"
        )
        cleaned = redact(text, ["secret-token"])
        self.assertNotIn("secret-token", cleaned)
        self.assertNotIn("password", cleaned)
        self.assertNotIn("a-token", cleaned)
        self.assertIn("[REDACTED]@example.com", cleaned)


class NixpacksTests(ExecutorTestCase):
    def test_nixpacks_argv_and_cleanup(self) -> None:
        runner = FakeRunner(build_output=b"nixpacks output\n")
        executor = self.executor(runner)

        result = executor.build_nixpacks(
            request(dockerfile_path="ignored", context_dir="ignored")
        )

        self.assertEqual(result.status, "succeeded")
        build_call = runner.build_calls()[0]
        self.assertEqual(build_call.argv[0], "nixpacks")
        self.assertEqual(build_call.argv[1], "build")
        self.assertEqual(
            build_call.argv[2], str((runner.clone_call().cwd / "src").resolve())
        )
        self.assertEqual(
            build_call.argv[build_call.argv.index("--name") + 1], "acme/app:test"
        )
        self.assertFalse(runner.clone_call().cwd.exists())


class BuildxTests(ExecutorTestCase):
    def test_buildx_builder_is_used_when_configured(self) -> None:
        runner = FakeRunner()
        executor = self.executor(runner, buildx_builder="rootless")
        result = executor.build_dockerfile(request())

        self.assertEqual(result.status, "succeeded")
        argv = runner.build_calls()[0].argv
        self.assertEqual(argv[:4], ["docker", "buildx", "build", "--builder"])
        self.assertEqual(argv[4], "rootless")
        self.assertIn("--load", argv)


class ConcurrencyAndOutputTests(ExecutorTestCase):
    def test_busy_executor_rejects_concurrent_build(self) -> None:
        runner = FakeRunner()
        executor = self.executor(runner)
        executor._slots.acquire()
        try:
            with self.assertRaises(BusyError):
                executor.build_dockerfile(request())
        finally:
            executor._slots.release()
        self.assertEqual(runner.calls, [])

    def test_response_tail_is_bounded_and_redacted(self) -> None:
        output = (b"x" * 10_000) + b" token=" + TOKEN.encode()
        runner = FakeRunner(build_output=output)
        executor = self.executor(runner, response_tail_bytes=1024)

        result = executor.build_dockerfile(request(token=TOKEN))

        self.assertTrue(result.truncated)
        self.assertLessEqual(len(result.log_tail.encode("utf-8")), 1024)
        self.assertNotIn(TOKEN, result.log_tail)

    def test_ansi_sequences_are_stripped(self) -> None:
        runner = FakeRunner(build_output=b"\x1b[31mred\x1b[0m plain\n")
        executor = self.executor(runner)
        result = executor.build_dockerfile(request())
        self.assertNotIn("\x1b[", result.log_tail)
        self.assertIn("red plain", result.log_tail)


if __name__ == "__main__":
    unittest.main()
