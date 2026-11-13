import json
import os
import tempfile
import unittest
from pathlib import Path

from hakobi_agent.loop import run_task


class LoopTests(unittest.TestCase):
    def test_reference_worker_fails_once_is_corrected_and_commits(self) -> None:
        task = Path(__file__).parents[1] / "tasks" / "fix-bug" / "task.json"
        with tempfile.TemporaryDirectory() as directory:
            state, run_dir = run_task(task, runs_root=directory)
            self.assertEqual(state["status"], "awaiting_approval", state.get("exit_reason"))
            self.assertEqual(len(state["attempts"]), 2)
            self.assertIsNotNone(state["commit"])
            self.assertTrue(all(check["passed"] for check in state["baseline_checks"]))
            self.assertFalse(state["attempts"][0]["checks"][1]["passed"])
            self.assertTrue(all(check["passed"] for check in state["attempts"][1]["checks"]))
            self.assertEqual(state["dead_ends"], ["unit-tests: exit=1 timeout=False"])

            feedback = (run_dir / "feedback-attempt-2.md").read_text(encoding="utf-8")
            for field in ("EXPECTED:", "SAW:", "FENCE:", "DEAD_ENDS:"):
                self.assertIn(field, feedback)
            self.assertIn("AssertionError", feedback)
            self.assertIn("app.py", feedback)

            workspace = Path(state["workspace"])
            files = state["attempts"][1]["checks"][0]["saw"]
            self.assertIn("app.py", files)
            git_source = (workspace / "app.py").read_text(encoding="utf-8")
            self.assertIn("return left + right", git_source)
            self.assertFalse((workspace / "__pycache__").exists())
            self.assertTrue((run_dir / "PR.md").is_file())
            persisted = json.loads((run_dir / "state.json").read_text(encoding="utf-8"))
            self.assertEqual(persisted["commit"], state["commit"])

    def test_relative_runs_root_uses_absolute_worker_paths(self) -> None:
        task = Path(__file__).parents[1] / "tasks" / "fix-bug" / "task.json"
        with tempfile.TemporaryDirectory() as directory:
            relative_root = Path(os.path.relpath(directory))
            self.assertFalse(relative_root.is_absolute())
            state, run_dir = run_task(task, runs_root=relative_root)
            self.assertEqual(state["status"], "awaiting_approval", state.get("exit_reason"))
            self.assertTrue(run_dir.is_absolute())
            self.assertTrue(Path(state["workspace"]).is_absolute())
            self.assertTrue(all(check["passed"] for check in state["attempts"][1]["checks"]))

    def test_identical_failure_stops_for_human_review(self) -> None:
        task = Path(__file__).parents[1] / "tasks" / "fix-bug" / "task.json"
        with tempfile.TemporaryDirectory() as directory:
            worker = Path(directory) / "stuck_worker.py"
            worker.write_text(
                "from pathlib import Path\n"
                "import os\n"
                "path = Path(os.environ['HAKOBI_WORKSPACE']) / 'app.py'\n"
                "text = path.read_text()\n"
                "if 'return left - right' in text:\n"
                "    path.write_text(text.replace('return left - right', 'return left * right'))\n",
                encoding="utf-8",
            )
            state, _ = run_task(
                task,
                runs_root=directory,
                worker_override=f"python3 {worker}",
            )
            self.assertEqual(state["status"], "needs_human")
            self.assertIn("identical", state["exit_reason"])
            self.assertEqual(len(state["attempts"]), 2)
            self.assertIsNone(state["commit"])
            self.assertTrue(state["attempts"][0]["checks"][1]["passed"] is False)
            self.assertTrue(state["attempts"][1]["checks"][1]["passed"] is False)

    def test_snapshot_cannot_open_a_real_pr(self) -> None:
        from hakobi_agent.pr import PRGateError, approve_pr

        task = Path(__file__).parents[1] / "tasks" / "fix-bug" / "task.json"
        with tempfile.TemporaryDirectory() as directory:
            state, run_dir = run_task(task, runs_root=directory)
            self.assertEqual(state["status"], "awaiting_approval")
            with self.assertRaises(PRGateError):
                approve_pr(run_dir, yes=True)


if __name__ == "__main__":
    unittest.main()
