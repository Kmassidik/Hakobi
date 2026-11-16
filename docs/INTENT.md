# Intent Map

What the user means versus what the code does. Update this file whenever a
feature's meaning diverges from its implementation. Read it before touching
UI, navigation, deployment, routing, or project conventions.

## Format

Each entry records user intent, code reality, and any remaining gap. Do not
keep historical implementation claims here after a clean cutover.

## Entries

### Self-hosted CI direction
User intent: Hakobi must not permanently depend on GitHub-hosted compute for
its own verification. GitHub remains the repository and event source.
Code reality: application builds run on the configured host; Hakobi's own CI
currently uses GitHub-hosted runners.
Gap: migrate CI to an isolated self-hosted runner on a separate VM or machine,
then dogfood Hakobi CI orchestration when available. Never run untrusted public
PR code directly on the production host or expose its Docker socket/secrets.

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
environment variables, optional CPU/memory limits, and auto-deploy policy. Create, edit, deploy, roll back,
view logs/history, start/stop/restart/pause/resume, inspect live CPU/memory
charts, and delete with full cleanup.
Code reality: `apps` and `deployments(app_id)` in SQLite. The host agent builds
and runs containers; deployments publish `app.container_port` on loopback. Optional
CPU-core and MiB limits are stored as bytes and enforced through Docker `HostConfig`
for both Dockerfile and Nixpacks builds; blank fields remain unlimited. Runtime
actions use the app's managed container and persist stopped/paused state apart
from deployment state. Usage charts accumulate up to 60 real samples over five
minutes in the browser; failed polls never insert zero samples.
Gap: commit statuses and PR check runs remain separate CP9 work.

### Containers
User intent: See every container, inspect metadata, live stats and logs, and
perform guarded lifecycle actions.
Code reality: `/containers` is a Docker workbench with Containers, Images,
Networks, and Volumes tabs backed by real provider snapshots. It includes
guarded image/network/volume creation and deletion, exact-name confirmation,
and built-in network protection. `/containers/{provider}/{name}` retains live
stats/log polling. Hakobi-managed containers display the app name, with the
real `hakobi-<app-id>` Docker name retained as secondary metadata and in routes.
Server-side pagination caps Docker resources at 25 rows per page, Apps at 20,
deployment history at 10, and Settings domains at 25; direct page URLs preserve
resource tabs and unrelated query state.
Kubernetes remains visible in Settings as a planned future deployment target,
not as a currently supported runtime.
Gap: none.

### GitHub integration
User intent: GitHub App repository selection, push-to-build, auto-deploy,
rollback, and deletion. Registration, installation, and user connection must
be honest distinct states. Monorepos and single repositories both work.
New-app creation uses one fully visible form with detected source defaults,
port 8080, and recommended 1-core/512-MiB limits (custom values remain visible).
Creating an app immediately queues its first build and container start. Apps
remain private until a running app is explicitly assigned a subdomain; only
then are managed DNS and tunnel ingress created and public health checked.
Managed existing DNS/ingress that exactly matches the app's loopback target can
be adopted; foreign or wrong-target routes remain protected. Hostname
availability is checked before submission and again authoritatively on publish.
Unpublished deletion never guesses a hostname, and Settings cannot delete a
route assigned to an app. Deployment cancellation blocks later pipeline stages,
and host installation uses the authenticated native-agent API.
Future deployments update that stored route. Public traffic still enters only
through the existing tunnel/L7 edge; raw application host ports stay private.
Code reality: Manifest registration, installation webhooks/callback/recheck,
installation tokens, push matching, and a repository picker. Repository lists
use a shared five-minute cache and durable last-good snapshots; stale snapshots
render immediately while one background refresh updates them. Selecting a scanned
app reads only .env.example or .env.sample and prefills blank JSON values for
valid variable names; real .env files and example values are never imported.
Session cookies
are SameSite=Lax for callbacks; cross-site POST CSRF protection remains.
Gap: commit statuses and PR checks are not complete.

### Cloudflare and subdomains
User intent: Each app receives a public subdomain with DNS/ingress lifecycle
cleanup. Settings can list and guard domain CRUD. Existing LLM frontend/API
and Tailscale access must remain untouched by general edge changes.
Code reality: first setup stores the API token in the encrypted catalog vault
for later DNS/ingress management; the separate connector token only runs
cloudflared. Settings reuses that vault entry and allows blank-token updates.
Cloudflare API manages zone records and tunnel ingress. Explicit LLM routes
remain on ports 8081/8082; wildcard apps route through the edge. Only records
marked `Managed by Hakobi` can be deleted through Hakobi.
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
