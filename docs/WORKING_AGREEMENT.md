# Working Agreement — Git + Local Memory

## Language policy (mandatory)

All agent-authored text MUST be English, including replies, plans, commit
messages, PR descriptions, documentation, comments, and issue text. This rule
overrides the language used by the user's prompt. If the prompt is Indonesian or
any other language, the agent MUST continue in English. Non-English output is a
protocol violation and must be rewritten before delivery.

This repository's agent-facing files stay in English. Only the repository owner
may change this policy, by explicitly instructing a change to the policy itself.

This repository is the durable record of Hakobi. Every meaningful change must be
recoverable from two places:

1. **Git memory** — branch name + commit message.
2. **Local memory** — the checked-out working tree and this document.

## Branch names

Use a descriptive `type/scope` branch for non-trivial work:

```text
feat/hakobi-v0.1-core
feat/hakobi-assets
fix/container-runtime
docs/deployment
chore/repo-hygiene
```

Do not use vague names such as `dev`, `update`, `wip`, or `test`.

## Commit messages

Every commit must explain:

- what changed
- where it changed
- why it changed
- security or operational implications, if any
- verification performed

Recommended shape:

```text
type(scope): short imperative summary

What:
- ...

Where:
- ...

Why:
- ...

Verification:
- ...

Risk / notes:
- ...
```

## Dual-memory rule

- Git is the source of truth for code, documentation, history, branch names, and
  commit messages.
- The local checkout is the working memory for active context, temporary files,
  and uncommitted work.
- Before ending a significant work session: commit, push, and update this file or
  the relevant local notes.
- Never commit secrets, credentials, generated run directories, build output, or
  environment files.

## Current layout

```text
src/           Rust Axum SSR app
static/        compiled CSS, HTMX, icons
tests/         integration tests
build-runner/  isolated image-build service
agent/         task/loop tooling
knowledge-base/ design and research notes
docs/          working agreements and operational notes
```
