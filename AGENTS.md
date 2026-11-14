# Hakobi Working Agreement

## Product model

Hakobi is a self-hosted DevOps control plane: Rust/Axum SSR, SQLite, a native
host agent, and a Rust L7 edge. Apps bind a GitHub repository, branch, path,
Dockerfile, container port, environment, and auto-deploy policy. Primary UI:
Dashboard, Apps, Containers, Settings.

GitHub App registration and installation are separate states. Push-to-build,
manual deploy, rollback, app deletion, per-app subdomains, DNS/ingress cleanup,
and guarded domain CRUD must remain working. Use parameterized SQL only.

## Runtime invariants

- Production console: `https://console.glicc.id`, loopback port 3000.
- `hakobi-edge`: Pingora-based L7 proxy on `127.0.0.1:9000`.
- Pingora: pinned repository revision in `Cargo.toml`; 14 per-thread runtimes,
  work stealing disabled, shared upstream pool, streaming bodies.
- `max_retries = 1` means one total upstream attempt, not one retry. Zero
  skips proxying entirely. Never replay non-idempotent requests.
- Cloudflare Tunnel: explicit `llm.glicc.id` → 8081 and
  `llminferences.glicc.id` → 8082; wildcard `*.glicc.id` → edge 9000.
- Apps publish on loopback only. Do not expose raw host ports publicly.
- Tailscale is in active use. Do not stop or reconfigure Tailscale, the GPU
  workload, the LLM frontend, or the LLM API when changing the general edge.
- `ufw` is disabled by explicit user choice; do not enable it without asking.
- Secrets remain encrypted in the catalog vault and out of logs/rendering.
- Honest readiness only: no fabricated samples, success states, or silent
  error suppression. Polling uses last-good data where already implemented.

## Readability and design

- English-only code, comments, documentation, and commits.
- Every tracked source file: at most 500 lines. `scripts/check-source-limits.py`
  enforces this in CI; generated `Cargo.lock` is exempt.
- Every function: at most 50 lines, including tests and browser/Python code.
- Split by responsibility into focused submodules. Preserve public symbol
  paths with re-exports and existing module conventions.
- No OOP redesign. `ContainerBackend` and externally required traits such as
  Pingora `ProxyHttp`/`BackgroundService` are allowed.
- Prefer boring, direct code. No macros that merely hide function length.
- Do not duplicate validation or establish a second convention. Check LSP
  references before changing exported symbols.
- Do not commit `mockups/`, `examples_gen.rs`, or
  `docs/L7-PROXY-KNOWLEDGE.md`; the L7 guide is local-only.

## Required verification before pushing

1. `cargo fmt`
2. `cargo clippy --all-targets -- -D warnings`
3. Complete `cargo test`, never only one binary
4. `PYTHONPATH=agent python3 -m unittest discover -s agent/tests -v`
5. `python3 agent/agentctl run --task agent/tasks/fix-bug/task.json --runs-root /tmp/hakobi-agent-runs`
6. `PYTHONPATH=build-runner python3 -m unittest discover -s build-runner/tests -v`
7. `bun run build:assets` after template/CSS/JS changes; commit generated asset
   changes and keep `ASSET_VERSION` complete for new asset files
8. `python3 scripts/check-source-limits.py`
9. `python3 scripts/check-function-lengths.py` with the pinned tree-sitter
   packages from `.github/workflows/ci.yml` installed
10. Exercise the changed production path or a real local surface. UI changes
    require a real browser; performance claims require controlled repeated
    benchmarks with valid 200 responses, throughput, and p99 latency.

CI also runs the build-runner suite and source-limit guard. Do not narrow a
test, delete a behavioral assertion, or claim success from compilation alone.

## Testing conventions

- Preserve fake-backend HTTP tests and real-browser production QA.
- Refactors preserve public APIs, JSON wire formats, route behavior, SQL
  semantics, CSRF, token handling, and original test assertions.
- Test consumer-visible behavior and boundaries, not wiring, source strings,
  incidental defaults, or formatter output.
- Keep production rollback artifacts when replacing deployed binaries.

## Module map

- `src/catalog/`: SQLite schema, migrations, vault, apps, deployments, GitHub,
  hosts, and provider persistence.
- `src/providers/`: container backends, agent/Docker/Kubernetes integrations,
  command execution, validation, and wire conversion.
- `src/host_agent/`: native-agent wire models, collectors, and parsing.
- `src/bin/hakobi-agent*`: native-agent sampler, routes, handlers, installers.
- `src/buildrunner/`: HTTP build-runner client, payloads, validation, errors.
- `build-runner/build_runner/`: standalone Python build executor and server.
- `src/web/`: Axum routes, handlers, SSR templates, and behavior tests.
- `src/bin/hakobi-edge.rs`: Pingora request lifecycle and atomic catalog routes.
- `frontend/`: Tailwind sources; `static/assets/hakobi*.js`: browser behavior.

Update this map when moving responsibilities. Keep this file current whenever
architecture, build commands, production invariants, or user constraints change.
