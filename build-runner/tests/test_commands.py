from __future__ import annotations

import io
import signal
import subprocess
import unittest
from unittest import mock

from build_runner.executor import CommandResult, run_command


class FakeProcess:
    payload = b""
    exit_code = 0
    hang = False
    pid_value = None
    instances: list["FakeProcess"] = []

    def __init__(self, argv, **kwargs):
        self.argv = argv
        self.kwargs = kwargs
        self.pid = self.pid_value
        self.stdout = io.BytesIO(self.payload)
        self.returncode = None
        self.terminated = False
        self.killed = False
        FakeProcess.instances.append(self)

    def wait(self, timeout=None):
        if self.hang and not self.terminated:
            raise subprocess.TimeoutExpired(self.argv, timeout or 0)
        if self.terminated:
            self.returncode = -9 if self.killed else -15
        else:
            self.returncode = self.exit_code
        return self.returncode

    def poll(self):
        return self.returncode

    def terminate(self):
        self.terminated = True
        self.returncode = -15

    def kill(self):
        self.killed = True
        self.terminated = True
        self.returncode = -9


class RunCommandTests(unittest.TestCase):
    def setUp(self) -> None:
        FakeProcess.instances = []

    def test_runs_without_a_shell_and_captures_output(self) -> None:
        class Process(FakeProcess):
            payload = b"hello\n"

        with mock.patch("build_runner.executor.subprocess.Popen", Process):
            result = run_command(["git", "--version"], timeout=5, max_output_bytes=1024)

        self.assertIsInstance(result, CommandResult)
        self.assertEqual(result.exit_code, 0)
        self.assertEqual(result.output, b"hello\n")
        self.assertEqual(result.total_output_bytes, 6)
        self.assertFalse(result.timed_out)

        process = Process.instances[0]
        self.assertEqual(process.argv, ["git", "--version"])
        self.assertNotIn("shell", process.kwargs)
        self.assertTrue(process.kwargs["start_new_session"])
        self.assertTrue(process.kwargs["close_fds"])
        self.assertIs(process.kwargs["stdin"], subprocess.DEVNULL)
        self.assertIs(process.kwargs["stdout"], subprocess.PIPE)
        self.assertIs(process.kwargs["stderr"], subprocess.STDOUT)

    def test_output_is_bounded_to_the_tail(self) -> None:
        class Process(FakeProcess):
            payload = b"a" * 10_000

        with mock.patch("build_runner.executor.subprocess.Popen", Process):
            result = run_command(["build"], timeout=5, max_output_bytes=1024)

        self.assertEqual(len(result.output), 1024)
        self.assertEqual(result.output, b"a" * 1024)
        self.assertEqual(result.total_output_bytes, 10_000)
        self.assertTrue(result.truncated)

    def test_nonzero_exit_is_reported(self) -> None:
        class Process(FakeProcess):
            exit_code = 3

        with mock.patch("build_runner.executor.subprocess.Popen", Process):
            result = run_command(["build"], timeout=5, max_output_bytes=1024)

        self.assertEqual(result.exit_code, 3)
        self.assertFalse(result.timed_out)

    def test_timeout_terminates_the_child(self) -> None:
        class Process(FakeProcess):
            hang = True

        with mock.patch("build_runner.executor.subprocess.Popen", Process):
            result = run_command(["sleep"], timeout=0.01, max_output_bytes=1024)

        self.assertTrue(result.timed_out)
        self.assertEqual(result.exit_code, -15)
        self.assertTrue(Process.instances[0].terminated)

    def test_timeout_kills_the_process_group(self) -> None:
        class Process(FakeProcess):
            hang = True
            pid_value = 4242

        with (
            mock.patch("build_runner.executor.subprocess.Popen", Process),
            mock.patch("build_runner.executor.os.getpgid", return_value=4242) as getpgid,
            mock.patch("build_runner.executor.os.killpg") as killpg,
            self.assertLogs("build_runner.executor", level="ERROR"),
        ):
            result = run_command(["sleep"], timeout=0.01, max_output_bytes=1024)

        self.assertTrue(result.timed_out)
        getpgid.assert_called_with(4242)
        signals = [call.args[1] for call in killpg.call_args_list]
        self.assertIn(signal.SIGTERM, signals)
        self.assertIn(signal.SIGKILL, signals)
        self.assertEqual(Process.instances[0].kwargs["start_new_session"], True)

    def test_missing_binary_raises_oserror(self) -> None:
        with mock.patch(
            "build_runner.executor.subprocess.Popen",
            side_effect=FileNotFoundError(2, "No such file", "docker"),
        ):
            with self.assertRaises(FileNotFoundError):
                run_command(["docker", "build"], timeout=5, max_output_bytes=1024)

    def test_rejects_string_argv(self) -> None:
        with self.assertRaises(ValueError):
            run_command("echo hi", timeout=5)  # type: ignore[arg-type]

    def test_rejects_empty_argv_and_bad_limits(self) -> None:
        with self.assertRaises(ValueError):
            run_command([], timeout=5)
        with self.assertRaises(ValueError):
            run_command([""], timeout=5)
        with self.assertRaises(ValueError):
            run_command(["true"], timeout=0)
        with self.assertRaises(ValueError):
            run_command(["true"], timeout=1, max_output_bytes=0)


if __name__ == "__main__":
    unittest.main()
