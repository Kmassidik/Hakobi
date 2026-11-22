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
from deployment state. Deployment output streams incrementally from the native
agent through the console as redacted, bounded catalog events. An authenticated
SSE endpoint replays event IDs for reconnecting build terminals; status and
duration come from the deployment record. Active output opens inline above
history, while completed deployments retain the same terminal behind Open.
Deletion removes networks labeled `id.hakobi.app` with
the exact app UUID. Apps may define a custom web start command, a release
command, private workers, and named volume mounts. Release commands run after
the image build and before replacing production; non-zero exits fail the
deployment without replacing the previous web container. Workers use the same
image, environment, limits, and volumes but publish no ports. Lifecycle and
deletion operations include every configured worker. Persistent volume data
survives rebuilds and mount edits. Plain variables and encrypted write-only
secrets share runtime injection; secret values are redacted from release logs
and never rerendered. Connected repositories support push auto-deploy, while
public HTTPS Git sources use tokenless manual Deploy latest builds. Authenticated
public-source scanning imports names only from `.env.example`/`.env.sample` on
GitHub, GitLab, and Codeberg, with blank values, no redirects, bounded reads, and
existing-value preservation. Matching volumes are preserved unless the user separately
confirms permanent app-data deletion. Unlabeled, shared, external, and other-app
resources are never selected by cleanup.
Usage charts accumulate up to 60 real samples over five
minutes in the browser; failed polls never insert zero samples.
Gap: commit statuses and PR check runs remain separate CP9 work.

### Console sessions
User intent: signing in once should survive browser and console restarts without
repeated daily authentication.
Code reality: signed JWT cookies use HttpOnly, SameSite=Lax, Secure in
production, and a 30-day Max-Age/expiry. The signing secret persists in the
catalog data directory; credential rotation can rotate it to invalidate all
existing sessions. Logout revokes the current session ID.
Gap: none.

### Containers
User intent: See every container, inspect metadata, live stats and logs, and
perform guarded lifecycle actions.
Code reality: `/containers` is a Docker workbench with Containers, Images,
Networks, and Volumes tabs backed by real provider snapshots. It includes
guarded image/network/volume creation and deletion, exact-name confirmation,
and built-in network protection. Network and volume creation can optionally
assign an app on the same host, persisted as an `id.hakobi.app` Docker label.
The workbench displays that ownership without making unlabeled resources
eligible for app cleanup. `/containers/{provider}/{name}` retains live
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
New-app creation uses validated Source, Identity, Build, and Review steps with
the canonical connected progress card, an eight-column task panel, and a sticky
four-column live recipe summary from `docs/UI_STYLE_PROMPT.md`. Detection fills
source defaults, port 8080, and recommended 1-core/512-MiB limits (custom values
remain editable). Port mapping exposes an optional host port and the detected
container port: blank host port uses automatic allocation, while a custom value
requests that exact loopback binding and fails honestly on conflicts. The target
machine is auto-selected and hidden for a single enabled host, with a selector
shown when multiple hosts are available. Environment variables use add/remove name-value
rows; example-file discovery imports names only and preserves manual edits. The
server wire format remains a JSON object, with native-form fallback when browser
scripting is unavailable. Editing an existing app remains a fully visible form.
Creating an app immediately queues its first build and container start. The
wizard is creation-only; editing uses deep-linkable General, Source, Build,
Processes, and Environment & data tabs with a shared sticky save footer and
native query-link fallback. Switching tabs preserves unsaved values. Apps
remain private until a running app is explicitly assigned a subdomain; only
then are managed DNS and tunnel ingress created and public health checked.
Managed existing DNS/ingress that exactly matches the app's loopback target can
be adopted; foreign or wrong-target routes remain protected. Hostname
availability is checked before submission and again authoritatively on publish.
Unpublished deletion never guesses a hostname, and Settings cannot delete a
route assigned to an app. Deployment cancellation blocks later pipeline stages,
and host installation uses the authenticated native-agent API.
Future deployments update that stored route. Pingora watches both the main
catalog and SQLite WAL before atomically swapping routes, and its bounded
shutdown prevents long-lived build terminals from blocking restart. Intentional
binary restarts use systemd SIGINT fast shutdown with a ten-second stop budget;
SSE clients reconnect while ordinary catalog changes remain graceful ArcSwap
reloads. Publication
uses bounded external health checks and only presents success after a final 2xx;
failed verification clears the displayed assignment but preserves managed
DNS/ingress for safe retry. Public traffic still enters only
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
