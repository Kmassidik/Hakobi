from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from build_runner.validation import (
    ValidationError,
    resolve_within,
    validate_branch,
    validate_git_url,
    validate_image_tag,
    validate_repo_path,
    validate_token,
)


class GitUrlTests(unittest.TestCase):
    def test_accepts_https_ssh_and_scp_forms(self) -> None:
        valid = [
            "https://github.com/acme/app.git",
            "https://git.example.com:8443/team/app",
            "ssh://git@github.com/acme/app.git",
            "ssh://git@git.example.com:2222/team/app",
            "git@github.com:acme/app.git",
            "deploy@git.internal:team/app",
        ]
        for value in valid:
            with self.subTest(value=value):
                self.assertEqual(validate_git_url(value), value)

    def test_rejects_unsafe_or_unsupported_remotes(self) -> None:
        invalid = [
            "http://github.com/acme/app.git",
            "file:///etc/passwd",
            "git://github.com/acme/app.git",
            "ext::sh -c id",
            "https://user:secret@github.com/acme/app.git",
            "https://github.com/acme/app.git?token=abc",
            "https://github.com/acme/app.git#main",
            "https://github.com",
            "https://github.com/",
            "--upload-pack=evil",
            "https://github.com/acme/app.git\nmalicious",
            " ssh://git@github.com/acme/app.git",
            "https://github.com/acme/../app.git",
            "git@github.com:acme/../../app.git",
        ]
        for value in invalid:
            with self.subTest(value=value):
                with self.assertRaises(ValidationError):
                    validate_git_url(value)

    def test_allowed_hosts_exact_and_wildcard(self) -> None:
        validate_git_url(
            "https://github.com/acme/app.git", allowed_hosts=("github.com",)
        )
        validate_git_url(
            "https://git.example.com/acme/app.git", allowed_hosts=("*.example.com",)
        )
        with self.assertRaises(ValidationError):
            validate_git_url(
                "https://gitlab.com/acme/app.git", allowed_hosts=("github.com",)
            )
        with self.assertRaises(ValidationError):
            validate_git_url(
                "https://example.com.evil.test/acme/app.git",
                allowed_hosts=("*.example.com",),
            )

    def test_scp_requires_user_host_and_path(self) -> None:
        for value in ["git@github.com", "github.com:acme/app", "@github.com:app"]:
            with self.subTest(value=value):
                with self.assertRaises(ValidationError):
                    validate_git_url(value)


class BranchTests(unittest.TestCase):
    def test_valid_branches(self) -> None:
        for value in ["main", "feature/build-runner", "v1.2.3", "release-1.0", "a+b"]:
            with self.subTest(value=value):
                self.assertEqual(validate_branch(value), value)

    def test_invalid_branches(self) -> None:
        invalid = [
            "",
            "-main",
            "@",
            "..",
            "a..b",
            "a b",
            "a~b",
            "a^b",
            "a:b",
            "a?b",
            "a*b",
            "a[b",
            "a\\b",
            "a@{b",
            "a/",
            "/a",
            "a//b",
            ".hidden",
            "a/.hidden",
            "a.lock",
            "a/b.lock",
            "a.",
            "a\x00b",
            "a\nb",
        ]
        for value in invalid:
            with self.subTest(value=value):
                with self.assertRaises(ValidationError):
                    validate_branch(value)


class ImageTagTests(unittest.TestCase):
    def test_valid_image_tags(self) -> None:
        for value in [
            "app",
            "app:v1",
            "app:1.0.0-rc.1",
            "registry.example.com:5000/team/app",
            "registry.example.com:5000/team/app:v2",
            "localhost:5000/app",
            "ghcr.io/acme/app:sha-abc123",
            "team/app",
        ]:
            with self.subTest(value=value):
                self.assertEqual(validate_image_tag(value), value)

    def test_invalid_image_tags(self) -> None:
        invalid = [
            "",
            "APP",
            "app:",
            "app:-tag",
            "app@sha256:abcdef",
            "../app",
            "app/../other",
            "app v1",
            "-app",
            "registry.example.com/App",
            "app:" + "x" * 129,
            "a" * 256,
        ]
        for value in invalid:
            with self.subTest(value=value):
                with self.assertRaises(ValidationError):
                    validate_image_tag(value)


class RepoPathTests(unittest.TestCase):
    def test_normalizes_relative_paths(self) -> None:
        self.assertEqual(validate_repo_path("Dockerfile", "dockerfile_path"), "Dockerfile")
        self.assertEqual(
            validate_repo_path("docker/Dockerfile", "dockerfile_path"),
            "docker/Dockerfile",
        )
        self.assertEqual(
            validate_repo_path("./Dockerfile", "dockerfile_path"), "Dockerfile"
        )
        self.assertEqual(validate_repo_path(".", "context_dir", allow_root=True), ".")

    def test_rejects_escape_attempts(self) -> None:
        invalid = [
            "",
            "../Dockerfile",
            "docker/../../etc/passwd",
            "/etc/passwd",
            "~/.ssh/id_rsa",
            "C:/Windows/System32",
            "..\\..\\secret",
            "docker\\Dockerfile",
            "a//b",
            ".",
            "a b",
            "a\x00b",
        ]
        for value in invalid:
            with self.subTest(value=value):
                with self.assertRaises(ValidationError):
                    validate_repo_path(value, "dockerfile_path")

    def test_rejects_unsupported_types(self) -> None:
        for value in [None, 1, ["Dockerfile"]]:
            with self.subTest(value=value):
                with self.assertRaises(ValidationError):
                    validate_repo_path(value, "context_dir")


class TokenTests(unittest.TestCase):
    def test_token_only_for_https(self) -> None:
        token = "ghp_" + "a" * 32
        self.assertEqual(
            validate_token(token, git_url="https://github.com/acme/app.git"), token
        )
        with self.assertRaises(ValidationError):
            validate_token(token, git_url="ssh://git@github.com/acme/app.git")
        with self.assertRaises(ValidationError):
            validate_token("has space", git_url="https://github.com/acme/app.git")
        with self.assertRaises(ValidationError):
            validate_token("", git_url="https://github.com/acme/app.git")
        self.assertIsNone(validate_token(None, git_url="https://github.com/acme/app.git"))


class ResolveWithinTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name) / "checkout"
        self.root.mkdir()
        self.outside = Path(self._tmp.name) / "outside.txt"
        self.outside.write_text("secret", encoding="utf-8")

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def test_accepts_files_inside_checkout(self) -> None:
        (self.root / "Dockerfile").write_text("FROM scratch\n", encoding="utf-8")
        sub = self.root / "docker"
        sub.mkdir()
        (sub / "Dockerfile").write_text("FROM scratch\n", encoding="utf-8")

        self.assertEqual(
            resolve_within(self.root, "Dockerfile", field="dockerfile_path"),
            (self.root / "Dockerfile").resolve(),
        )
        self.assertEqual(
            resolve_within(self.root, "docker", field="context_dir", expect_directory=True),
            sub.resolve(),
        )

    def test_rejects_symlink_escape_file(self) -> None:
        (self.root / "Dockerfile").symlink_to(self.outside)
        with self.assertRaises(ValidationError):
            resolve_within(self.root, "Dockerfile", field="dockerfile_path")

    def test_rejects_symlink_escape_directory(self) -> None:
        (self.root / "context").symlink_to(self.outside.parent)
        with self.assertRaises(ValidationError):
            resolve_within(self.root, "context", field="context_dir", expect_directory=True)

    def test_missing_and_wrong_type(self) -> None:
        with self.assertRaises(ValidationError):
            resolve_within(self.root, "Dockerfile", field="dockerfile_path")
        (self.root / "dir").mkdir()
        with self.assertRaises(ValidationError):
            resolve_within(self.root, "dir", field="dockerfile_path")


if __name__ == "__main__":
    unittest.main()
