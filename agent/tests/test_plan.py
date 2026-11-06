import json
import shutil
import tempfile
import unittest
from pathlib import Path

from hakobi_agent.plan import run_plan


class PlanTests(unittest.TestCase):
    def test_plan_applies_passing_task_patch_to_live_repo(self) -> None:
        root = Path(__file__).parents[1]
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            repo = base / "repo"
            shutil.copytree(root / "examples" / "buggy_app", repo)
            task_dir = base / "task"
            task_dir.mkdir()
            (task_dir / "task.md").write_text(
                "Fix add() so the unit tests pass. Only edit app.py.\n",
                encoding="utf-8",
            )
            task = {
                "version": 1,
                "id": "fix-add-bug",
                "title": "Fix add() returning subtraction",
                "repo": "../repo",
                "prompt": "task.md",
                "worker": ["python3", "unused"],
                "workspace_mode": "snapshot",
                "max_attempts": 2,
                "token_budget": 20000,
                "worker_timeout": 90,
                "allowed_commands": [["python3", "-m", "unittest"], ["git", "status"], ["git", "diff"]],
                "allowed_files": ["app.py"],
                "sensitive_patterns": [".env"],
                "checks": [
                    {
                        "type": "command",
                        "id": "unit-tests",
                        "argv": ["python3", "-m", "unittest", "discover", "-s", "tests", "-v"],
                        "baseline": "fail",
                        "expect_output": "FAILED (failures=1)",
                    },
                    {"type": "diff_scope", "id": "file-fence", "allowed": ["app.py"]},
                    {"type": "diff_check", "id": "diff-clean"},
                ],
            }
            (task_dir / "task.json").write_text(json.dumps(task), encoding="utf-8")
            plan = base / "plan.json"
            plan.write_text(
                json.dumps({"version": 1, "name": "test", "tasks": ["task/task.json"]}),
                encoding="utf-8",
            )

            worker = root / "workers" / "reference_worker.py"
            state, _ = run_plan(
                plan,
                worker_override=f"python3 {worker}",
                runs_root=base / "runs",
            )

            self.assertEqual(state["status"], "completed", state.get("exit_reason"))
            self.assertTrue(state["tasks"][0]["applied"])
            self.assertIn("return left + right", (repo / "app.py").read_text(encoding="utf-8"))
            self.assertTrue(list((base / "runs").glob("*/applied-001.patch")))


if __name__ == "__main__":
    unittest.main()
