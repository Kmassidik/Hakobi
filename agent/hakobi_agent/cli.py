from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from .loop import run_task
from .memory import RunStore
from .plan import PlanError, run_plan
from .pr import PRGateError, approve_pr
from .task import TaskError, load_task


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="hakobi-agent", description="Hakobi Level 1-4 agent harness")
    subparsers = parser.add_subparsers(dest="command", required=True)

    run_parser = subparsers.add_parser("run", help="run a task through the harness and check loop")
    run_parser.add_argument("--task", required=True, help="path to task.json")
    run_parser.add_argument("--worker", help="override worker command")
    run_parser.add_argument("--runs-root", default="agent/runs", help="external memory root")
    run_parser.add_argument("--max-attempts", type=int, help="override task attempt limit")

    plan_parser = subparsers.add_parser("run-plan", help="run a sequential task plan and apply passing patches")
    plan_parser.add_argument("--plan", required=True, help="path to plan.json")
    plan_parser.add_argument("--worker", help="override worker command")
    plan_parser.add_argument("--runs-root", default="agent/runs", help="external memory root")
    plan_parser.add_argument("--max-tasks", type=int, help="limit how many tasks run")

    inspect_parser = subparsers.add_parser("inspect", help="print a run state")
    inspect_parser.add_argument("--run", required=True)

    pr_parser = subparsers.add_parser("approve-pr", help="human gate: push branch and open a PR")
    pr_parser.add_argument("--run", required=True)
    pr_parser.add_argument("--yes", action="store_true", help="non-interactive approval")

    args = parser.parse_args(argv)
    try:
        if args.command == "run":
            state, run_dir = run_task(
                args.task,
                worker_override=args.worker,
                runs_root=args.runs_root,
                max_attempts=args.max_attempts,
            )
            print(json.dumps({"run": str(run_dir), "status": state["status"], "reason": state["exit_reason"]}, indent=2))
            print(f"state: {run_dir / 'state.json'}")
            print(f"workspace: {state.get('workspace')}")
            if state.get("commit"):
                print(f"commit: {state['commit']}")
            if state["status"] == "awaiting_approval":
                print(f"PR preview: {run_dir / 'PR.md'}")
                print(f"Human gate: hakobi-agent approve-pr --run {run_dir}")
                return 0
            return 2
        if args.command == "run-plan":
            state, run_dir = run_plan(
                args.plan,
                worker_override=args.worker,
                runs_root=args.runs_root,
                max_tasks=args.max_tasks,
            )
            print(json.dumps({"run": str(run_dir), "status": state["status"], "reason": state["exit_reason"]}, indent=2))
            print(f"state: {run_dir / 'state.json'}")
            for task in state["tasks"]:
                suffix = f" -> {task.get('patch')}" if task.get("patch") else ""
                print(f"task {task['number']}: {task['status']}{suffix}")
            return 0 if state["status"] == "completed" else 2
        if args.command == "inspect":
            state = RunStore(Path(args.run)).load()
            print(json.dumps(state, indent=2))
            return 0
        if args.command == "approve-pr":
            url = approve_pr(args.run, yes=args.yes)
            print(url)
            return 0
    except (TaskError, PlanError, PRGateError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 1
