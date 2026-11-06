import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from hakobi_agent.policy import CommandPolicy, PathPolicy, PolicyError, copy_ignore
from hakobi_agent.task import file_is_allowed


class CommandPolicyTests(unittest.TestCase):
    def setUp(self) -> None:
        self.policy = CommandPolicy(
            (
                ("python3", "-m", "unittest"),
                ("git", "status"),
                ("git", "diff"),
            )
        )

    def test_allowlisted_prefixes_pass(self) -> None:
        self.policy.validate(["python3", "-m", "unittest", "discover"])
        self.policy.validate(["git", "status", "--short"])

    def test_recursive_delete_is_always_blocked(self) -> None:
        with self.assertRaises(PolicyError):
            self.policy.validate(["rm", "-rf", "workspace"])

    def test_force_push_is_always_blocked(self) -> None:
        with self.assertRaises(PolicyError):
            self.policy.validate(["git", "push", "--force"])

    def test_shell_interpreters_are_blocked(self) -> None:
        with self.assertRaises(PolicyError):
            self.policy.validate(["bash", "-lc", "echo unsafe"])

    def test_unlisted_command_is_blocked(self) -> None:
        with self.assertRaises(PolicyError):
            self.policy.validate(["python3", "-c", "print(1)"])


class PathPolicyTests(unittest.TestCase):
    def setUp(self) -> None:
        self.policy = PathPolicy(("app.py", "src/**/*.rs"), (".env", "*.pem"))

    def test_file_fence_accepts_allowed_and_globbed_files(self) -> None:
        self.assertTrue(self.policy.can_edit("app.py"))
        self.assertTrue(self.policy.can_edit("src/web/mod.rs"))
        self.assertFalse(self.policy.can_edit("tests/test_app.py"))

    def test_copy_ignore_excludes_internal_runs(self) -> None:
        ignore = copy_ignore((), ())
        self.assertEqual(ignore("/repo/agent", ["runs", "src"]), {"runs"})
        self.assertEqual(ignore("/repo", ["agent", "src"]), set())

    def test_sensitive_files_are_not_readable(self) -> None:
        self.assertFalse(self.policy.can_read(".env"))
        self.assertFalse(self.policy.can_read("certs/server.pem"))

    def test_path_traversal_is_rejected(self) -> None:
        with TemporaryDirectory() as directory:
            with self.assertRaises(PolicyError):
                self.policy.relative_path(Path(directory), "../outside.py")
            with self.assertRaises(PolicyError):
                self.policy.relative_path(Path(directory), "/etc/passwd")

    def test_file_matching_is_explicit(self) -> None:
        self.assertTrue(file_is_allowed("app.py", ("app.py",)))
        self.assertFalse(file_is_allowed("other.py", ("app.py",)))


if __name__ == "__main__":
    unittest.main()
