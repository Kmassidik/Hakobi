# Intent Map

What the user means versus what the code does. Update this file whenever a
feature's meaning diverges from its implementation. Read it before touching
UI, navigation, deployment, routing, or project conventions.

## Format

Each entry records user intent, code reality, and any remaining gap. Do not
keep historical implementation claims here after a clean cutover.

## Entries

### Dashboard
User intent: Live host/container metrics, 24-hour charts, deployment activity,
and honest empty/stale states. Polling must never append duplicate panels or
empty-state messages.
Code reality: Dashboard, Apps, Containers, Settings navigation. Metrics and
charts poll partials with last-good behavior. The metrics empty state lives
inside the single replaced root and is browser-verified across repeated polls.
Gap: none.

### Apps
User intent: Railway-shaped applications, not overlapping environment/stack
pages. An app binds a GitHub repository, branch, path, Dockerfile, port,
environment variables, and auto-deploy policy. Create, edit, deploy, roll back,
view logs/history, and delete with full cleanup.
Code reality: `apps` and `deployments(app_id)` in SQLite. The host agent builds
and runs containers; deployments publish `app.container_port` on loopback.
Gap: commit statuses and PR check runs remain separate CP9 work.

### Containers
User intent: See every container, inspect metadata, live stats and logs, and
perform guarded lifecycle actions.
Code reality: `/containers` plus provider/name detail pages with HTMX polling.
The control plane uses the `ContainerBackend` contract across Docker and the
native host agent.
Gap: none.

### GitHub integration
User intent: GitHub App repository selection, push-to-build, auto-deploy,
rollback, and deletion. Registration, installation, and user connection must
be honest distinct states. Monorepos and single repositories both work.
Code reality: Manifest registration, installation webhooks/callback/recheck,
installation tokens, push matching, and a repository picker. Session cookies
are SameSite=Lax for callbacks; cross-site POST CSRF protection remains.
Gap: commit statuses and PR checks are not complete.

### Cloudflare and subdomains
User intent: Each app receives a public subdomain with DNS/ingress lifecycle
cleanup. Settings can list and guard domain CRUD. Existing LLM frontend/API
and Tailscale access must remain untouched by general edge changes.
Code reality: Cloudflare API manages zone records and tunnel ingress. Explicit
LLM routes remain on ports 8081/8082; wildcard apps route through the edge.
Only records marked `Managed by Hakobi` can be deleted through Hakobi.
Gap: none.

### Edge proxy
User intent: A human-readable Rust L7 proxy built from proven Pingora source
patterns and tuned with real benchmarks, not unsupported performance claims.
Code reality: Pingora at the pinned revision in `Cargo.toml`, 14 per-thread
runtimes, work stealing disabled, shared hot-queue/LRU pooling, atomic catalog
reloads, and one total upstream attempt. Wildcard traffic uses loopback 9000.
Gap: nginx remains a useful benchmark reference; no universal performance
claim is made.

### Readability and build environment
User intent: Every tracked source file is at most 500 lines and every function
at most 50 lines. Code stays understandable by humans and future work retains
these rules.
Code reality: `AGENTS.md` is the persistent working agreement. CI runs source
file and syntax-based function length guards, complete Rust/Python tests,
frontend asset checks, security audit, and container builds.
Gap: none once the current refactor and verification are complete.
