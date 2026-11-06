from __future__ import annotations

import unittest
from pathlib import Path

from build_runner.config import ConfigError, Settings

TOKEN = "config-token-0123456789"


class SettingsTests(unittest.TestCase):
    def test_defaults(self) -> None:
        settings = Settings.from_env({})
        self.assertEqual(settings.host, "127.0.0.1")
        self.assertEqual(settings.port, 8787)
        self.assertIsNone(settings.api_token)
        self.assertEqual(settings.work_dir, Path("/tmp/build-runner"))
        self.assertEqual(settings.max_concurrent_builds, 1)
        self.assertEqual(settings.build_timeout, 1800.0)
        self.assertEqual(settings.allowed_git_hosts, ())

    def test_parses_environment(self) -> None:
        settings = Settings.from_env(
            {
                "BUILD_RUNNER_HOST": "localhost",
                "BUILD_RUNNER_PORT": "9000",
                "BUILD_RUNNER_TOKEN": TOKEN,
                "BUILD_RUNNER_WORK_DIR": "/srv/work",
                "BUILD_RUNNER_MAX_CONCURRENT_BUILDS": "3",
                "BUILD_RUNNER_CLONE_TIMEOUT": "42.5",
                "BUILD_RUNNER_BUILD_TIMEOUT": "600",
                "BUILD_RUNNER_MAX_OUTPUT_BYTES": "8192",
                "BUILD_RUNNER_RESPONSE_TAIL_BYTES": "1024",
                "BUILD_RUNNER_MAX_REQUEST_BYTES": "2048",
                "BUILD_RUNNER_ALLOWED_GIT_HOSTS": "GitHub.com, *.example.com",
                "BUILD_RUNNER_BUILDX_BUILDER": "rootless",
            }
        )
        self.assertEqual(settings.host, "localhost")
        self.assertEqual(settings.port, 9000)
        self.assertEqual(settings.api_token, TOKEN)
        self.assertEqual(settings.work_dir, Path("/srv/work"))
        self.assertEqual(settings.max_concurrent_builds, 3)
        self.assertEqual(settings.clone_timeout, 42.5)
        self.assertEqual(settings.build_timeout, 600.0)
        self.assertEqual(settings.max_output_bytes, 8192)
        self.assertEqual(settings.response_tail_bytes, 1024)
        self.assertEqual(settings.max_request_bytes, 2048)
        self.assertEqual(settings.allowed_git_hosts, ("github.com", "*.example.com"))
        self.assertEqual(settings.buildx_builder, "rootless")
        settings.validate_runtime()

    def test_token_is_not_in_repr(self) -> None:
        settings = Settings(api_token=TOKEN)
        self.assertNotIn(TOKEN, repr(settings))

    def test_rejects_invalid_values(self) -> None:
        invalid = [
            {"BUILD_RUNNER_PORT": "not-a-number"},
            {"BUILD_RUNNER_PORT": "70000"},
            {"BUILD_RUNNER_TOKEN": "short"},
            {"BUILD_RUNNER_ALLOW_UNAUTHENTICATED": "sometimes"},
            {"BUILD_RUNNER_CLONE_TIMEOUT": "0"},
            {"BUILD_RUNNER_BUILD_TIMEOUT": "999999"},
            {"BUILD_RUNNER_MAX_OUTPUT_BYTES": "100"},
            {"BUILD_RUNNER_RESPONSE_TAIL_BYTES": "999999999"},
            {"BUILD_RUNNER_MAX_REQUEST_BYTES": "10"},
            {"BUILD_RUNNER_MAX_CONCURRENT_BUILDS": "0"},
            {"BUILD_RUNNER_BUILDX_BUILDER": "-bad"},
            {"BUILD_RUNNER_GIT_BIN": "/usr/bin/git"},
            {"BUILD_RUNNER_HOST": "bad host"},
        ]
        for values in invalid:
            with self.subTest(values=values):
                with self.assertRaises(ConfigError):
                    Settings.from_env(values)

    def test_authentication_policy(self) -> None:
        with self.assertRaises(ConfigError):
            Settings.from_env({}).validate_runtime()
        with self.assertRaises(ConfigError):
            Settings.from_env(
                {
                    "BUILD_RUNNER_ALLOW_UNAUTHENTICATED": "1",
                    "BUILD_RUNNER_HOST": "0.0.0.0",
                }
            ).validate_runtime()
        Settings.from_env(
            {
                "BUILD_RUNNER_ALLOW_UNAUTHENTICATED": "1",
                "BUILD_RUNNER_HOST": "127.0.0.1",
            }
        ).validate_runtime()
        Settings.from_env({"BUILD_RUNNER_TOKEN": TOKEN}).validate_runtime()

    def test_token_control_characters_rejected(self) -> None:
        with self.assertRaises(ConfigError):
            Settings(api_token="valid-token-1234\ninjection")


if __name__ == "__main__":
    unittest.main()
