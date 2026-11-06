# Hakobi Agent Harness

**Levels 1–4 only.** This is an orchestrator around coding workers, not a chatbot and not a multi-agent graph.

The harness supplies:

- an isolated place to work
- hard command/file walls
- independent checks that can fail
- corrective feedback
- a human PR gate

The worker does not decide whether it succeeded. Tests, command output, diff scope, and git state do.

## Implemented levels

### Level 1–2 — Harness

- Isolated workspace: clean git repositories use a git worktree; dirty or non-git sources use a full snapshot copy.
- The worker never edits the source tree directly. Sequential plans can apply a passing patch to the source working tree.
- External memory: `task.md`, `state.json`, prompts, feedback, worker logs, and commit evidence under `agent/runs/`.
- Git checkpoint: the harness commits only after checks pass.
- Restricted tool surface for workers: `read_file`, `edit_file`, `run_shell`, `run_tests`, `git_status`, `git_diff`, `git_commit`.
- Command allowlist with permanent blocks for recursive deletion, force push, shells, privilege escalation, and destructive filesystem commands.
- File fence: only declared files can be edited.
- Sensitive files excluded from snapshot workspaces.
- Sanitized worker environment.
- Worker timeout and token budget accounting.

### Level 3–4 — Loop

- `PRODUCE → CHECK → CORRECT → repeat/exit`
- The baseline test must fail before the fix; if it passes, the task is invalid.
- Checks run outside the worker:
  1. a previously failing test must now pass
  2. command output must be verifiable and match its marker
  3. diff must stay inside the file fence
  4. `git diff --check` must pass
  5. the workspace must contain a real change
- Feedback format is fixed:
  - `EXPECTED:`
  - `SAW:`
  - `FENCE:`
  - `DEAD_ENDS:`
- Exit conditions:
  - checks pass → commit and await human PR approval
  - maximum attempts
  - identical diff + checks twice in a row
  - worker timeout/crash or exhausted token budget
- Self-reported success is ignored.

Not implemented yet: Level 5–6 graph orchestration and Level 7 learning. There is no parallel multi-agent scheduler.

## Structure

```text
agent/
├── agentctl                     # CLI
├── hakobi_agent/
│   ├── task.py                 # task contract and fences
│   ├── policy.py               # command and path walls
│   ├── workspace.py            # worktree/snapshot isolation and git checkpoint
│   ├── tools.py                # restricted worker tool surface
│   ├── checks.py               # independent checks and feedback
│   ├── memory.py               # disk state and run artifacts
│   ├── loop.py                 # Level 3–4 loop
│   ├── plan.py                 # sequential plans and patch application
│   └── pr.py                   # human-gated PR creation
├── workers/
│   ├── reference_worker.py     # deterministic two-attempt demo worker
│   └── omp_worker.py           # OMP CLI adapter
├── tasks/
│   ├── fix-bug/
│   └── hakobi/                 # product backlog tasks
├── plans/                      # ordered task plans
├── examples/buggy_app/         # failing fixture repository
└── tests/
```

`agent/runs/` is runtime state and is gitignored.

## Run the example

```sh
python3 agent/agentctl run \
  --task agent/tasks/fix-bug/task.json
```

Expected result:

- attempt 1 applies an intentionally wrong fix
- unit tests fail with `AssertionError`
- attempt 2 receives `EXPECTED/SAW/FENCE/DEAD_ENDS`
- attempt 2 fixes `app.py`
- checks pass
- harness commits to branch `hakobi/fix-add-bug-*`
- status becomes `awaiting_approval`

Inspect:

```sh
python3 agent/agentctl inspect --run agent/runs/<run-id>
```

Artifacts:

```text
state.json
task.md
prompt-attempt-1.md
feedback-attempt-1.md
worker-attempt-1.log
PR.md
workspace/
```

## Run a sequential plan

A plan runs tasks in order. Each task gets its own isolated snapshot. If a task passes all checks, its commit diff is applied to the source working tree, then the next task starts from that updated tree. If any task fails, the plan stops before applying it.

```sh
python3 agent/agentctl run-plan \
  --plan agent/plans/hakobi-cp1-catalog.json
```

The plan writes `state.json` and numbered `applied-*.patch` files under `agent/runs/`. It never commits, pushes, or opens a PR; auto-apply only modifies the working tree for review.

Plan format:

```json
{
  "version": 1,
  "name": "hakobi-cp1-catalog",
  "tasks": ["../tasks/hakobi/cp1-catalog/task.json"]
}
```

Task paths are relative to the plan file. `--max-tasks N` limits a run; `--worker` overrides the worker command for all tasks.

## Use a real coding worker

The default task uses the deterministic reference worker so the harness can be tested offline and in CI. To use the local OMP CLI:

```sh
python3 agent/agentctl run \
  --task agent/tasks/fix-bug/task.json \
  --worker "python3 agent/workers/omp_worker.py"
```

The adapter limits OMP tools to read/write/edit/bash/glob/grep and applies time and cost limits. Any worker command can be supplied; the harness only trusts the final independent checks.

Workers receive:

```text
HAKOBI_WORKSPACE
HAKOBI_PROMPT_FILE
HAKOBI_FEEDBACK_FILE
HAKOBI_STATE_FILE
HAKOBI_TASK_FILE
HAKOBI_BASELINE_COMMIT
HAKOBI_BRANCH
HAKOBI_USAGE_FILE
HAKOBI_ATTEMPT
HAKOBI_TOKEN_BUDGET_REMAINING
PYTHONPATH
```

Workers may write usage JSON:

```json
{"input_tokens": 120, "output_tokens": 30}
```

The harness sums reported usage and stops when the budget is exceeded.

## Human PR gate

The loop never pushes or opens a PR.

```sh
python3 agent/agentctl approve-pr \
  --run agent/runs/<run-id>
```

Typing `OPEN` (or `--yes`) authorizes:

1. checking that the workspace is clean
2. pushing the task branch without force
3. running `gh pr create`
4. recording the PR URL in `state.json`

A git worktree against a clean clone keeps the origin remote and can open a real PR. A snapshot workspace has no origin and intentionally refuses PR creation. The bundled example uses a snapshot, so it stops at `awaiting_approval`.

## Task contract

```json
{
  "version": 1,
  "id": "fix-add-bug",
  "title": "Fix add() returning subtraction",
  "repo": "../../examples/buggy_app",
  "prompt": "task.md",
  "worker": ["python3", "../../workers/reference_worker.py"],
  "workspace_mode": "snapshot",
  "max_attempts": 3,
  "token_budget": 20000,
  "worker_timeout": 90,
  "allowed_commands": [
    ["python3", "-m", "unittest"],
    ["git", "status"],
    ["git", "diff"]
  ],
  "allowed_files": ["app.py"],
  "sensitive_patterns": [".env", "*.pem", "*.key"],
  "checks": [
    {
      "type": "command",
      "id": "unit-tests",
      "argv": ["python3", "-m", "unittest", "discover", "-s", "tests", "-v"],
      "baseline": "fail",
      "expect_output": "FAILED (failures=1)",
      "timeout": 30
    },
    {"type": "diff_scope", "id": "file-fence", "allowed": ["app.py"]},
    {"type": "diff_check", "id": "diff-clean"}
  ]
}
```

`workspace_mode`:

- `snapshot` — copy into an isolated repo; safe for dirty/non-git sources; no PR remote.
- `worktree` — clean git repo; preserves origin/history; supports a real PR.
- `auto` — worktree when safe, snapshot otherwise.

## Safety boundaries

- The command allowlist and file fence are enforced by the harness tool surface and every check command.
- The worker runs as an ordinary local process, not a VM. A malicious external worker with shell access could attempt to escape the workspace. For untrusted repositories or workers, run the entire harness inside a disposable container/VM with network and filesystem isolation.
- Snapshot mode excludes common sensitive files. Worktree mode inherits tracked files only; do not track secrets.
- The PR gate is deliberately human-controlled.
- There is no generic remote shell, browser tool, or network fetch exposed by `ToolBox`.

## Verify

```sh
PYTHONPATH=agent python3 -m unittest discover -s agent/tests -v
python3 agent/agentctl run --task agent/tasks/fix-bug/task.json --runs-root /tmp/hakobi-agent-runs
python3 agent/agentctl run-plan --plan agent/plans/hakobi-cp1-catalog.json --max-tasks 1 --runs-root /tmp/hakobi-plan-runs
```
