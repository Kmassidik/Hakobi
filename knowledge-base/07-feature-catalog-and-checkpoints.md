# Hakobi Feature Catalog and Build Checkpoints

Last updated: **2026-11-05**

This is the complete intended feature map. Statuses prevent shipped, next, and speculative features from being confused.

> **Roadmap note:** the canonical checkpoint order and product requirements are now in [PRD v0.3](./08-prd-v0.3-merged.md). This file remains the shipped-feature inventory and long-range capability catalog. Where the two documents differ on roadmap order, the merged PRD wins.

## Status legend

| Status | Meaning |
|---|---|
| **Shipped** | Implemented and verified |
| **Next** | Approved for the next build checkpoint |
| **Planned** | Intended, not scheduled |
| **Candidate** | Worth evaluating after foundations |
| **Excluded** | Deliberate non-goal |

## Shipped baseline: Hakobi v0.1

- [x] First-run CLI setup
- [x] Username and Argon2id password
- [x] Optional loopback-only local mode
- [x] Public HTTPS base URL validation and canonicalization
- [x] Cloudflare account/zone setup
- [x] Dedicated Cloudflare Tunnel creation
- [x] Tunnel ingress configuration
- [x] Proxied DNS CNAME creation
- [x] Transactional rollback for failed Cloudflare provisioning
- [x] Cloudflare API token used once and not persisted
- [x] Supervised `cloudflared` process inside the deployment
- [x] Non-root Docker image with read-only root filesystem policy
- [x] Nix development and package outputs
- [x] Server-rendered login
- [x] Server-rendered authenticated dashboard
- [x] Opaque in-memory sessions
- [x] Session revocation on logout and restart
- [x] Double-submit CSRF protection
- [x] Per-client login throttling
- [x] Bounded concurrent password hashing
- [x] Secure cookies (`HttpOnly`, `SameSite=Strict`, `Secure`, `__Host-`)
- [x] CSP and security headers
- [x] Dynamic `PORT` configuration
- [x] Password reset CLI without recreating the tunnel
- [x] Maud typed SSR components
- [x] Tailwind CSS build pipeline
- [x] Font Awesome solid icon subset
- [x] Locally vendored HTMX
- [x] Embedded and content-hashed static assets
- [x] HMAC-free opaque session token storage (SHA-256 token digests)
- [x] Health endpoint
- [x] Docker Compose deployment
- [x] Public deployment verified over Cloudflare Tunnel
- [x] Nix/Docker builds and 11 regression tests

## Checkpoint 1 — Platform persistence and provider foundation

**Goal:** stop being a single-configuration application; prepare for multiple provider connections.

### Core data

- [ ] Persistent catalog store (SQLite first; migration path to PostgreSQL)
- [ ] Schema migrations and versioned startup checks
- [ ] Organization, project, environment, and service entities
- [ ] Workload identity and provider resource mapping
- [ ] Ownership and labels/annotations
- [ ] Encrypted provider credential storage
- [ ] Credential rotation and revocation
- [ ] Per-provider capability flags (`read`, `operate`, `admin`)
- [ ] Health/freshness fields for every external reading
- [ ] Immutable audit events

### Provider framework

- [ ] Typed provider connection interface
- [ ] Connection test and capability probe
- [ ] Egress allowlist and SSRF protection
- [ ] Poll scheduling with jitter and rate limits
- [ ] Webhook receiver framework
- [ ] Connector error isolation
- [ ] No arbitrary shell execution

**Gate:** an administrator can add, test, disable, and delete a provider connection; credentials never appear in logs or browser payloads; all reads have timestamps.

## Checkpoint 2 — Docker, Podman, and Compose visibility

**Goal:** one read-only operations view for the most common container environments.

- [ ] Docker Engine connection: Unix socket, TCP+TLS, SSH context
- [ ] Podman connection through REST API/SSH
- [ ] OrbStack through the Docker endpoint model
- [ ] Host inventory
- [ ] Container inventory and status
- [ ] Compose project and stack grouping
- [ ] Images and tags
- [ ] Volumes
- [ ] Networks
- [ ] Container resource snapshot
- [ ] Port mappings
- [ ] Health checks
- [ ] Restart counts and exit codes
- [ ] Docker Swarm services, tasks, and nodes
- [ ] Discovery from labels and Compose metadata
- [ ] Service ownership confirmation
- [ ] Container logs, bounded and paginated
- [ ] Read-only service topology graph
- [ ] Evidence-backed network and route edges
- [ ] Topology list fallback
- [ ] External dashboard links

**Gate:** user can see every container/stack, its owner, host, image, health, ports, and recent activity without write access.

## Checkpoint 3 — Jenkins and GitLab Runner activity

**Goal:** connect CI runners to the same service/activity model.

### Jenkins

- [ ] Connection through base URL and scoped API token
- [ ] Job and folder inventory
- [ ] Build queue
- [ ] Executor capacity
- [ ] Online/offline agents
- [ ] Current builds
- [ ] Last build result and duration
- [ ] Failed-build trend
- [ ] Build log link or bounded retrieval
- [ ] Prometheus plugin detection
- [ ] Collector freshness

### GitLab Runner

- [ ] GitLab.com/self-managed connection
- [ ] Runner inventory
- [ ] Runner status and last contact
- [ ] Running job count
- [ ] Concurrency and queue indicators
- [ ] Autoscaler machine state
- [ ] Job/pipeline status
- [ ] Private metrics endpoint verification
- [ ] Project/group ownership mapping

### Shared

- [ ] Unified runner list
- [ ] Unified job activity timeline
- [ ] Stuck/offline detection
- [ ] Alert when a collector is stale
- [ ] Links to source systems

**Gate:** operators can answer “what is running, what is queued, what is failing, and which runner is unhealthy?” without opening two dashboards.

## Checkpoint 4 — Prometheus and Grafana context

**Goal:** attach existing observability data to the operating model.

- [ ] Prometheus connection with read-only access
- [ ] Target discovery and scrape health
- [ ] Firing/pending/resolved alerts
- [ ] Alert-to-service mapping
- [ ] Query current and range metrics
- [ ] Grafana connection with a scoped service account
- [ ] Dashboard discovery and deep links
- [ ] Grafana alert state
- [ ] Dashboard-to-service mapping
- [ ] Alert owner and runbook link
- [ ] Collectors health
- [ ] No duplicate time-series storage
- [ ] No embedded dashboards by default

**Gate:** every service has health evidence from at least one source, and every alert can be traced to its source URL and owner.

## Checkpoint 5 — Kubernetes visibility

**Goal:** add Kubernetes without pretending it behaves like Docker.

- [ ] Cluster connection with read-only ServiceAccount
- [ ] Namespace inventory
- [ ] Deployments, StatefulSets, DaemonSets
- [ ] Pods, containers, restarts, readiness
- [ ] Jobs and CronJobs
- [ ] Services and ingress
- [ ] Nodes and pressure conditions
- [ ] Events and warning aggregation
- [ ] `kube-state-metrics` integration
- [ ] `metrics-server` integration
- [ ] Rollout status
- [ ] Replica drift
- [ ] Read-only manifest view
- [ ] Workload identity from labels and owner references
- [ ] No raw `kubectl` proxying

**Gate:** cluster state, workload health, rollout progress, and events appear in the same service view without granting write permissions.

## Checkpoint 6 — Cloudflare Workers, Pages, and Wrangler

**Goal:** add edge workloads as a distinct workload type.

### Inventory

- [ ] Workers scripts
- [ ] Pages projects
- [ ] Environments
- [ ] Routes and custom domains
- [ ] Deployment history
- [ ] Current production version
- [ ] Bindings and cron status
- [ ] Plan and usage summary
- [ ] Worker error rate and request metrics
- [ ] Recent issues

### Activity

- [ ] Workers Logs link or bounded query
- [ ] Tail session for active investigation
- [ ] Logpush/OpenTelemetry destination status
- [ ] Deployment correlation with errors
- [ ] Pages build/deployment status
- [ ] Rollback availability

### Tooling

- [ ] API-first management adapter
- [ ] Wrangler detection in CI projects
- [ ] Wrangler command preview, never arbitrary shell execution
- [ ] Workers for Platforms inventory as a separate kind
- [ ] No assumption that edge workloads are containers

**Gate:** an operator can see every edge project, its routes, deployment, usage, and recent errors, with links to Cloudflare.

## Checkpoint 7 — Safe deployments and operations

**Goal:** allow changes only after the read-only graph is trusted.

- [ ] Build container images
- [ ] Build logs and status
- [ ] Tag and push to selected registry
- [ ] Compose stack deploy
- [ ] Docker Swarm service update
- [ ] Kubernetes rollout/restart/scale
- [ ] Cloudflare Worker/Pages deploy
- [ ] Rollback
- [ ] Pre-deploy checks
- [ ] Diff/preview before action
- [ ] Approval policy
- [ ] Re-authentication for destructive actions
- [ ] Per-action timeout
- [ ] Provider-side verification
- [ ] Audit before/after state
- [ ] Explicit idempotency keys
- [ ] Automatic rollback on failed verification (configurable)

**Gate:** every mutation is typed, authorized, auditable, idempotent, and reversible or explicitly marked irreversible.

## Checkpoint 8 — Abuse detection and response

**Goal:** reduce abuse without becoming a false-positive factory.

### Detection

- [ ] Cloudflare bot/rate-limit/WAF signal import
- [ ] CrowdSec decision import
- [ ] Authentication anomaly signals
- [ ] Normalized abuse decision model
- [ ] ASN/NAT/verified-bot context
- [ ] Provider capability detection
- [ ] Shadow evaluation

### Enforcement

- [ ] Recommend-only mode
- [ ] Cloudflare managed challenge action
- [ ] Cloudflare temporary block action
- [ ] CrowdSec bouncer integration
- [ ] Local firewall action where supported
- [ ] Allowlists for admins, CI, monitoring, and verified bots
- [ ] TTL and escalation rules
- [ ] Ban caps and kill switch
- [ ] One-click reversal
- [ ] Complete audit
- [ ] Never permanent-auto-ban by default

**Gate:** a shadow run produces evidence, zero unexpected blocks, and a reversible enforcement path before enabling any automatic ban.

## Checkpoint 9 — Teams, tenancy, and enterprise controls

- [ ] Multiple users
- [ ] Organization/project/environment RBAC
- [ ] Roles: viewer, operator, admin, approver
- [ ] OIDC SSO
- [ ] SAML SSO
- [ ] SCIM provisioning
- [ ] API tokens with scopes and expiry
- [ ] Approval separation
- [ ] Audit export
- [ ] Per-tenant provider credentials
- [ ] Tenant isolation tests
- [ ] Backup and restore
- [ ] Retention policy
- [ ] Usage/quotas for hosted mode
- [ ] High-availability deployment
- [ ] External PostgreSQL
- [ ] Encryption key rotation

**Gate:** two tenants cannot observe or operate each other’s resources through any UI, API, webhook, cache, or error path.

## Checkpoint 10 — Scale and ecosystem

**Candidate features:**

- [ ] GitHub Actions runner visibility
- [ ] Azure DevOps and Bitbucket adapters
- [ ] Loki/OpenSearch log search
- [ ] Alertmanager and PagerDuty integration
- [ ] Slack, Teams, Discord, email notifications
- [ ] Incident timeline
- [ ] Runbook execution with approvals
- [ ] Service templates
- [ ] Scheduled tasks and maintenance windows
- [ ] Cost and resource-rightsizing insights
- [ ] Connector SDK/plugin model
- [ ] Public API and CLI
- [ ] Embedded agent for private networks
- [ ] Offline/air-gapped deployment
- [ ] Multi-region control plane
- [ ] Workers for Platforms tenant provisioning

## Provider capability matrix

| Capability | Docker | Podman | Kubernetes | Jenkins | GitLab | Prometheus | Grafana | Cloudflare |
|---|---|---|---|---|---|---|---|---|
| Discovery | Planned | Planned | Planned | Planned | Planned | Planned | Planned | Planned |
| Inventory | Planned | Planned | Planned | Planned | Planned | Planned | Planned | Planned |
| Health | Planned | Planned | Planned | Planned | Planned | Planned | Planned | Planned |
| Metrics | Planned | Planned | Planned | Planned | Planned | Planned | Planned | Planned |
| Logs | Planned | Planned | Planned | Planned | Planned | Candidate | Candidate | Planned |
| Deploy | Planned | Planned | Planned | Candidate | Candidate | — | — | Planned |
| Rollback | Planned | Planned | Planned | Candidate | Candidate | — | — | Planned |
| Approved action | Planned | Planned | Planned | Planned | Planned | Candidate | Candidate | Planned |
| Abuse enforcement | — | — | — | — | — | — | — | Planned |
| Read-only mode | Planned | Planned | Planned | Planned | Planned | Planned | Planned | Planned |

## Cross-cutting requirements

- [ ] Every external value has a source and freshness timestamp
- [ ] Every action has an actor, capability check, and audit record
- [ ] Credentials are encrypted at rest and absent from logs
- [ ] Provider endpoints are allowlisted
- [ ] TLS verification is mandatory outside explicit private-CA configuration
- [ ] Web UI works without client-side JavaScript for core flows
- [ ] Server-side authorization on every route and HTMX fragment
- [ ] No raw provider credentials in the browser
- [ ] No arbitrary command execution
- [ ] Pagination and bounded payload sizes
- [ ] Retry with jitter and no thundering herd
- [ ] Capability degradation is visible, not silent
- [ ] Migration and backup paths for persistent data
- [ ] Tests for authorization boundaries and destructive actions

## Excluded non-goals

- Container engine implementation
- Container image registry hosting
- Prometheus-compatible time-series database
- Grafana replacement
- General-purpose CI scheduler or build farm
- Jenkins replacement as a standalone automation product
- Kubernetes replacement
- Proprietary bot fingerprinting
- Permanent automatic bans by default
- Raw shell/`docker exec`/`kubectl exec` interface
- Arbitrary URL proxy or SSRF-capable fetch tool
- Universal YAML that hides provider-specific behavior
- Per-container hosting resale as the core business

## Recommended next checkpoint

Build **Checkpoint 1 + Checkpoint 2 read-only** together:

1. Persistent service/provider catalog
2. Encrypted provider credentials
3. Docker/Podman/Compose read-only inventory
4. Freshness and collector health
5. Service ownership
6. No mutation yet

That creates a useful product skeleton without taking on Kubernetes, Cloudflare, or bot enforcement prematurely.
