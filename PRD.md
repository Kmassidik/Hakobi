# Hakobi Product Requirements Document

**Version:** 2.0
**Status:** Contract — every task implements against this document.
**Date:** 2026-11-08
**Author:** Repository owner
**Policy:** All output, comments, commits, and documentation MUST be English.

---

## Table of Contents

1. [Product Vision](#1-product-vision)
2. [Architecture](#2-architecture)
3. [Data Model](#3-data-model)
4. [Host Management](#4-host-management)
5. [Environment Model](#5-environment-model)
6. [Deployment Pipeline](#6-deployment-pipeline)
7. [Monitoring & Metrics](#7-monitoring--metrics)
8. [Container Management](#8-container-management)
9. [UI Specification](#9-ui-specification)
10. [Host Agent API](#10-host-agent-api)
11. [Security Requirements](#11-security-requirements)
12. [Build-Test-Fix Loop](#12-build-test-fix-loop)
13. [Milestone Checklists](#13-milestone-checklists)
14. [Bug Registry](#14-bug-registry)

---

## 1. Product Vision

Hakobi is a self-hosted DevOps control plane. One dashboard manages one or more
machines. Operators register environments, connect Git repositories, build
images, deploy stacks, monitor resources, and manage containers — all from a
single web UI.

### 1.1 What Hakobi replaces

| Legacy tool | Hakobi replaces |
|-------------|-----------------|
| Swarmpit | Container management dashboard |
| Jenkins | Build pipeline |
| Railway | Environment-based deployment |
| Portainer | Container ops |
| btop / htop | Host resource monitoring |
| Cloudflare Pages | Environment-based build/deploy |

### 1.2 What Hakobi does NOT replace

| Tool | Reason |
|------|--------|
| Kubernetes | Hakobi targets Docker hosts, not K8s clusters |
| Terraform | Infrastructure provisioning is out of scope |
| Prometheus | Hakobi has its own metrics pipeline |
| Grafana | Hakobi has its own dashboard UI |

### 1.3 Core principles

1. **No terminal after setup.** After the one-time agent install, every
   operation is done from the web UI.
2. **Typed APIs only.** No arbitrary shell endpoints. No raw Docker proxy.
   Every agent endpoint is a fixed, validated operation.
3. **Honest data.** Unknown states render as unknown, not zero. Provider
   failures are visible. No fake health.
4. **English-only.** All agent-authored output is English: code, comments,
   commit messages, docs, UI text, replies. No exceptions.
5. **Single admin.** One operator. One session. No multi-user auth in v1.
6. **Host agent as execution plane.** The dashboard never touches Docker
   directly. All host operations go through the native agent.
7. **Immutable deployments.** Each deployment is versioned. Rollback redeploys
   a previous version. No in-place mutation.

---

## 2. Architecture

### 2.1 Component diagram

```
┌─────────────────────────────────────────────────────────┐
│                    Browser (SSR + HTMX)                  │
│                                                         │
│  Overview │ Environments │ Containers │ Infrastructure  │
│  Providers │ Deployment │ Hosts │ Host Resources        │
└────────────────────────┬────────────────────────────────┘
                         │ HTTPS (Cloudflare Tunnel)
                         ▼
┌─────────────────────────────────────────────────────────┐
│                Hakobi App (Docker container)             │
│                                                         │
│  Axum 0.8 SSR │ SQLite Catalog │ SQLite Metrics         │
│  Session Auth │ CSRF │ Rate Limiter │ CSP Headers       │
│                                                         │
│  ContainerBackend trait                                 │
│    ├── DockerBackend (unix://, tcp://)                  │
│    └── AgentBackend (agent://{host_id})                 │
│                                                         │
│  Build Runner (external service)                        │
│    └── POST /build { source, type } → image             │
└────────────────────────┬────────────────────────────────┘
                         │
              ┌──────────┼──────────┐
              │          │          │
              ▼          ▼          ▼
     ┌──────────────┐ ┌─────────┐ ┌──────────┐
     │ hakobi-agent │ │ Docker  │ │ Registry │
     │ (native root)│ │ Engine  │ │ (option) │
     │              │ │         │ │          │
     │ readiness    │ │         │ │          │
     │ installers   │ │         │ │          │
     │ docker proxy │ │         │ │          │
     │ system stats │ │         │ │          │
     └──────────────┘ └─────────┘ └──────────┘
```

### 2.2 Communication

| From | To | Protocol | Auth |
|------|----|----------|------|
| Browser | Hakobi App | HTTPS (Cloudflare Tunnel) | Session cookie + CSRF |
| Hakobi App | hakobi-agent | HTTP localhost + Bearer token | Agent token |
| hakobi-agent | Docker Engine | Unix socket (bollard) | Socket permissions |
| Build Runner | Registry | HTTPS | Registry credentials |
| Git | Build Runner | HTTPS clone | Deploy key / token |

### 2.3 Crate structure

```
src/
├── main.rs                  # CLI entry point (setup, run, passwd)
├── lib.rs                   # Library root
├── config/                  # Configuration loading/saving
├── catalog.rs               # SQLite catalog (providers, hosts, envs, stacks)
├── metrics.rs               # SQLite metrics time-series (NEW)
├── host_agent.rs            # Shared protocol types (agent ↔ app)
├── install.rs               # Host tool detection
├── buildrunner.rs           # Build runner HTTP client
├── cloudflare/              # Cloudflare Tunnel management
├── providers/               # Container backends
│   ├── docker.rs            # Direct Docker API (bollard)
│   └── agent.rs             # Agent-backed Docker (HTTP to agent)
├── stacks/                  # Stack definitions and compose handling
├── utils/                   # Random tokens, URL validation
└── web/                     # Axum SSR application
    ├── mod.rs               # Router, middleware, layers
    ├── auth.rs              # Sessions, password hashing, rate limiting
    ├── handlers.rs          # All HTTP handlers
    ├── agent_client.rs      # Host agent HTTP client
    ├── stacks_admin.rs      # Stack CRUD handlers
    └── templates.rs         # Maud SSR templates
```

---

## 3. Data Model

### 3.1 Catalog database (`catalog.sqlite3`)

Schema version: **5**

```sql
-- Preserved from CP1 (legacy, may be removed in future)
CREATE TABLE IF NOT EXISTS organizations (
    id TEXT PRIMARY KEY NOT NULL,
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL CHECK (length(created_at) > 0)
);

CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY NOT NULL,
    organization_id TEXT NOT NULL REFERENCES organizations(id),
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL CHECK (length(created_at) > 0)
);

CREATE TABLE IF NOT EXISTS environments (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id),
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL CHECK (length(created_at) > 0)
);

CREATE TABLE IF NOT EXISTS services (
    id TEXT PRIMARY KEY NOT NULL,
    environment_id TEXT NOT NULL REFERENCES environments(id),
    slug TEXT NOT NULL,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL CHECK (length(created_at) > 0),
    UNIQUE (environment_id, slug)
);

CREATE TABLE IF NOT EXISTS audit_events (
    id TEXT PRIMARY KEY NOT NULL,
    action TEXT NOT NULL,
    entity_kind TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    actor TEXT NOT NULL,
    created_at TEXT NOT NULL CHECK (length(created_at) > 0)
);

-- CP2+ providers
CREATE TABLE IF NOT EXISTS provider_connections (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL CHECK (kind IN ('docker', 'kubernetes')),
    endpoint TEXT NOT NULL,
    encrypted_secret TEXT,
    capabilities TEXT NOT NULL DEFAULT '[]',
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    last_checked_at TEXT,
    last_error TEXT,
    created_at TEXT NOT NULL CHECK (length(created_at) > 0)
);

-- CP3 host agents
CREATE TABLE IF NOT EXISTS host_agents (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE,
    agent_url TEXT NOT NULL UNIQUE,
    encrypted_token TEXT,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    last_checked_at TEXT,
    last_error TEXT,
    created_at TEXT NOT NULL CHECK (length(created_at) > 0)
);

-- CP3 stack definitions
CREATE TABLE IF NOT EXISTS stack_definitions (
    id TEXT PRIMARY KEY NOT NULL,
    provider_id TEXT NOT NULL REFERENCES provider_connections(id),
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('compose', 'dockerfile', 'nixpacks')),
    source TEXT NOT NULL DEFAULT '.',
    yaml TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL CHECK (length(created_at) > 0),
    updated_at TEXT NOT NULL CHECK (length(updated_at) > 0)
);

-- CP5 NEW: environments (flat, not nested)
CREATE TABLE IF NOT EXISTS environments (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE,
    host_id TEXT NOT NULL REFERENCES host_agents(id),
    build_type TEXT NOT NULL CHECK (build_type IN ('compose', 'dockerfile', 'nixpacks')),
    source_type TEXT NOT NULL CHECK (source_type IN ('git', 'upload')),
    git_url TEXT,
    git_branch TEXT DEFAULT 'main',
    git_deploy_key TEXT,
    upload_path TEXT,
    registry_mode TEXT NOT NULL DEFAULT 'local' CHECK (registry_mode IN ('local', 'push')),
    registry_url TEXT,
    registry_credentials TEXT,
    webhook_secret TEXT,
    webhook_enabled INTEGER NOT NULL DEFAULT 0,
    auto_deploy INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'idle' CHECK (status IN ('idle', 'building', 'deploying', 'running', 'failed')),
    created_at TEXT NOT NULL CHECK (length(created_at) > 0),
    updated_at TEXT NOT NULL CHECK (length(updated_at) > 0)
);

-- CP5 NEW: deployments (versioned)
CREATE TABLE IF NOT EXISTS deployments (
    id TEXT PRIMARY KEY NOT NULL,
    environment_id TEXT NOT NULL REFERENCES environments(id),
    version INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'building', 'deploying', 'running', 'failed', 'rolled_back')),
    trigger_type TEXT NOT NULL CHECK (trigger_type IN ('manual', 'webhook')),
    git_commit_sha TEXT,
    git_commit_message TEXT,
    build_output TEXT,
    deploy_output TEXT,
    started_at TEXT NOT NULL,
    completed_at TEXT,
    created_at TEXT NOT NULL CHECK (length(created_at) > 0),
    UNIQUE (environment_id, version)
);

-- CP5 NEW: deployment logs
CREATE TABLE IF NOT EXISTS deployment_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    deployment_id TEXT NOT NULL REFERENCES deployments(id),
    timestamp TEXT NOT NULL,
    level TEXT NOT NULL CHECK (level IN ('info', 'warn', 'error')),
    message TEXT NOT NULL
);
```

### 3.2 Metrics database (`metrics.sqlite3`)

Separate from catalog. Created on first app startup.

```sql
-- Live 1-second samples (ring buffer, 5 minutes max)
CREATE TABLE IF NOT EXISTS metrics_live (
    host_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    cpu_percent REAL NOT NULL,
    memory_used_bytes INTEGER NOT NULL,
    memory_total_bytes INTEGER NOT NULL,
    disk_used_bytes INTEGER NOT NULL,
    disk_total_bytes INTEGER NOT NULL,
    network_rx_bps REAL NOT NULL,
    network_tx_bps REAL NOT NULL,
    load_1 REAL NOT NULL
);

-- 1-minute rollups (7 days retention)
CREATE TABLE IF NOT EXISTS metrics_1m (
    host_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    cpu_avg REAL NOT NULL,
    cpu_max REAL NOT NULL,
    memory_avg REAL NOT NULL,
    memory_max REAL NOT NULL,
    disk_used INTEGER NOT NULL,
    network_rx_avg REAL NOT NULL,
    network_tx_avg REAL NOT NULL
);

-- 5-minute rollups (30 days retention)
CREATE TABLE IF NOT EXISTS metrics_5m (
    host_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    cpu_avg REAL NOT NULL,
    cpu_max REAL NOT NULL,
    memory_avg REAL NOT NULL,
    disk_used INTEGER NOT NULL,
    network_rx_avg REAL NOT NULL,
    network_tx_avg REAL NOT NULL
);

-- 1-hour rollups (90 days retention)
CREATE TABLE IF NOT EXISTS metrics_1h (
    host_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    cpu_avg REAL NOT NULL,
    memory_avg REAL NOT NULL,
    disk_used INTEGER NOT NULL
);

CREATE INDEX idx_metrics_live_host_ts ON metrics_live (host_id, timestamp);
CREATE INDEX idx_metrics_1m_host_ts ON metrics_1m (host_id, timestamp);
CREATE INDEX idx_metrics_5m_host_ts ON metrics_5m (host_id, timestamp);
CREATE INDEX idx_metrics_1h_host_ts ON metrics_1h (host_id, timestamp);
```

Retention purge job:

```sql
DELETE FROM metrics_1m WHERE timestamp < (strftime('%s','now') - 7*86400) * 1000;
DELETE FROM metrics_5m WHERE timestamp < (strftime('%s','now') - 30*86400) * 1000;
DELETE FROM metrics_1h WHERE timestamp < (strftime('%s','now') - 90*86400) * 1000;
DELETE FROM metrics_live WHERE timestamp < (strftime('%s','now') - 300) * 1000;
```

---

## 4. Host Management

### 4.1 Host types

| Type | Access path | Provider endpoint | Auth |
|------|------------|-------------------|------|
| Local (agent) | `agent://local` via env token | `agent://local` | `HAKOBI_AGENT_TOKEN` env |
| Local (socket) | Direct bollard | `unix:///var/run/docker.sock` | Socket permissions |
| Remote (enrolled) | `agent://{host_id}` via catalog token | `agent://{host_id}` | Encrypted token in catalog |

### 4.2 Host agent protocol

All endpoints require `Authorization: Bearer <token>` header.
Token compared in constant time.
Agent binds `127.0.0.1:8787` by default.

| Method | Endpoint | Purpose |
|--------|----------|---------|
| GET | `/healthz` | Liveness (no auth) |
| GET | `/v1/readiness` | Host tool detection |
| GET | `/v1/system/metrics` | Full host resource snapshot |
| GET | `/v1/docker/metrics` | Aggregate Docker stats |
| GET | `/v1/docker/version` | Docker server + API version |
| GET | `/v1/docker/containers` | Container inventory |
| GET | `/v1/docker/snapshot` | Full Docker snapshot |
| POST | `/v1/docker/containers/run` | Create container |
| POST | `/v1/docker/containers/{name}/start` | Start container |
| POST | `/v1/docker/containers/{name}/stop` | Stop container |
| POST | `/v1/docker/containers/{name}/restart` | Restart container |
| POST | `/v1/docker/containers/{name}/delete` | Remove container |
| GET | `/v1/install/status` | Installer job status |
| POST | `/v1/install/{tool}` | Execute fixed installer |

### 4.3 Host agent install

```sh
curl -fsSL https://console.glicc.id/agent/install.sh | bash -s -- --token <TOKEN>
```

Install script:
1. Detects architecture (`uname -m`).
2. Downloads agent binary for architecture.
3. Creates `/etc/hakobi/agent.token` (0600).
4. Creates and starts systemd service.
5. Calls back to Hakobi server to confirm registration.

---

## 5. Environment Model

### 5.1 Environment creation flow

```
Dashboard → Environments → [New Environment]
                                │
                                ▼
                    ┌───────────────────────────┐
                    │ Step 1: Basics            │
                    │   Name (unique slug)      │
                    │   Host (select)           │
                    │   Build type              │
                    └───────────┬───────────────┘
                                ▼
                    ┌───────────────────────────┐
                    │ Step 2: Source            │
                    │   Git URL + branch        │
                    │   OR Upload tarball       │
                    └───────────┬───────────────┘
                                ▼
                    ┌───────────────────────────┐
                    │ Step 3: Registry          │
                    │   Local / Push            │
                    │   Registry URL + creds    │
                    └───────────┬───────────────┘
                                ▼
                    ┌───────────────────────────┐
                    │ Step 4: Confirm           │
                    │   Review and create       │
                    └───────────────────────────┘
```

### 5.2 Environment status transitions

```
idle → building → deploying → running
  ↑                    │
  └────── failed ←─────┘
```

### 5.3 Environment operations

| Operation | Method | Route |
|-----------|--------|-------|
| List all | GET | `/environments` |
| Create | POST | `/environments` |
| Detail | GET | `/environments/{id}` |
| Edit | GET + POST | `/environments/{id}/edit` |
| Delete | POST | `/environments/{id}/delete` |
| Deploy | POST | `/environments/{id}/deploy` |
| Rollback | POST | `/environments/{id}/rollback` |
| Deployments | GET | `/environments/{id}/deployments` |
| Logs | GET | `/environments/{id}/logs` |
| Webhook config | GET + POST | `/environments/{id}/webhook` |

---

## 6. Deployment Pipeline

### 6.1 Pipeline stages

```
pending
  │
  ▼
building
  │  Clone source (git) or extract upload
  │  Run build (compose build / docker build / nixpacks)
  │  Tag image: {env_name}:{version}
  │
  ▼
deploying
  │  Push to registry (if configured)
  │  Stop old containers
  │  Start new containers
  │
  ▼
running
  │  Health check (optional)
  │  Update environment status
  │
  ├──→ failed (at any stage)
  └──→ rolled_back (manual rollback)
```

### 6.2 Build runner integration

Hakobi sends a POST to the build runner service:

```json
{
  "type": "compose|dockerfile|nixpacks",
  "source_url": "git@github.com:user/repo.git",
  "branch": "main",
  "context_dir": ".",
  "dockerfile_path": "Dockerfile",
  "image_tag": "myapp:v3"
}
```

Response:

```json
{
  "status": "success",
  "image_id": "sha256:...",
  "image_tag": "myapp:v3",
  "build_duration_secs": 45
}
```

### 6.3 Deployment execution

After successful build, Hakobi instructs the target host agent to:

1. Pull image (if registry push was used).
2. Stop existing containers for this environment.
3. Start new containers with the built image.
4. Verify containers are running.
5. Record deployment as `running`.

Rollback: re-run deployment with previous version tag.

---

## 7. Monitoring & Metrics

### 7.1 Collection

| Source | Agent | App |
|--------|-------|-----|
| CPU total/per-core | `sysinfo::System::cpus()` | Polled from agent |
| Memory/swap | `sysinfo::System` | Polled from agent |
| Disk usage | `sysinfo::Disks` | Polled from agent |
| Disk I/O | `/proc/diskstats` | Polled from agent |
| Network RX/TX | `sysinfo::Networks` | Polled from agent |
| Processes | `sysinfo::System::processes()` | Polled from agent |
| Docker containers | bollard or `docker stats` | Polled from agent |
| Load average | `System::load_average()` | Polled from agent |
| Uptime | `System::uptime()` | Polled from agent |

### 7.2 Sampling cadence

| Cadence | Trigger | Storage |
|---------|---------|---------|
| 1s | Agent background thread | In-memory ring buffer (300 samples) |
| 5s | HTMX poll from browser | No storage |
| 15s | App polls agent | SQLite `metrics_live` (purged after 5m) |
| 60s | App rollup job | SQLite `metrics_1m` |
| 300s | App rollup job | SQLite `metrics_5m` |
| 3600s | App rollup job | SQLite `metrics_1h` |

### 7.3 UI refresh

| Page | Mechanism | Interval |
|------|-----------|----------|
| Overview | HTMX `every 15s` | `/partials/metrics` |
| Host readiness | HTMX `every 30s` | `/partials/readiness` |
| Host resources | HTMX `every 5s` | `/hosts/{id}/resources/partial` |
| Container detail | HTMX `every 10s` | `/containers/{provider}/{name}/partial` |

---

## 8. Container Management

### 8.1 Container inventory

Each provider contributes containers grouped by:

```rust
pub struct ContainerGroup {
    pub name: String,       // compose project or "Standalone"
    pub containers: Vec<ContainerRow>,
}

pub struct StackGroup {
    pub name: String,
    pub services: Vec<ServiceGroup>,
    pub container_count: usize,
    pub running_count: usize,
}
```

### 8.2 Container detail page (CP8)

```text
/containers/{provider_id}/{name}
```

Shows:

- Metadata: image, state, health, ports, labels
- CPU/memory usage (from Docker stats)
- Environment variables (from inspect)
- Logs (streaming tail via HTMX)
- Actions: start, stop, restart, delete

### 8.3 Container logs

```text
GET /containers/{provider}/{name}/logs?tail=100&follow=true
```

Returns HTML with log lines. HTMX poll every 2s for live tail. No WebSocket.

---

## 9. UI Specification

### 9.1 Design system

- Canvas: `#f5f5f7`
- Cards: `#ffffff`, `border-radius: 14px`, `box-shadow: 0 1px 3px rgba(0,0,0,0.04)`
- Font: Inter / system sans-serif
- Brand color: `#164f3b`
- Accent: `#d8f078`
- Sidebar: white, 256px, fixed left, no border
- Active nav: `bg-slate-100`, green indicator bar
- Icons: Font Awesome 7 solid (vendored)
- Charts: CSS bars + native `<progress>` + inline SVG for sparklines
- CSP: `default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:`
- No inline JavaScript. No inline CSS `style=""` attributes.

### 9.2 Page inventory

| Page | Route | Template function |
|------|-------|-------------------|
| Login | `/login` | `login()` |
| Overview | `/` | `dashboard()` |
| Environments | `/environments` | `environments()` |
| Environment detail | `/environments/{id}` | `environment_detail()` |
| Environment form | `/environments/new`, `/environments/{id}/edit` | `environment_form()` |
| Containers | `/containers` | `containers()` |
| Container detail | `/containers/{provider}/{name}` | `container_detail()` |
| Run container | `/containers/new` | `container_form()` |
| Stack list | `/stacks` | `stacks()` |
| Stack form | `/stacks/new`, `/stacks/{id}/edit` | `stack_form()` |
| Stack detail | `/stacks/{id}` | `stack_detail()` |
| Infrastructure | `/infrastructure` | `infrastructure()` |
| Providers | `/providers` | `providers()` |
| Deployment history | `/deployment` | `deployment_history()` |
| Hosts | `/hosts` | `hosts()` |
| Host resources | `/hosts/{id}/resources` | `host_resources()` |

### 9.3 Navigation

```text
Sidebar:
  Overview           →  /
  Environments       →  /environments
  Containers         →  /containers
  Infrastructure     →  /infrastructure
  Providers          →  /providers
  Deployment         →  /deployment
  Hosts              →  /hosts

Mobile bottom nav (6 items):
  Overview, Environments, Containers, Infra, Providers, Deploy

Hidden sub-pages (reachable from parent pages, not sidebar):
  /stacks/new
  /stacks/{id}/edit
  /stacks/{id}
  /containers/new
  /containers/{provider}/{name}
  /hosts/{id}/resources
  /environments/{id}
```

---

## 10. Host Agent API

### 10.1 Authentication

```text
Authorization: Bearer <token>
```

Token compared in constant time. Minimum 24 characters.

### 10.2 Endpoint specification

| Method | Endpoint | Auth | Request | Response |
|--------|----------|------|---------|----------|
| GET | `/healthz` | No | — | `200 OK` |
| GET | `/v1/readiness` | Yes | — | `Readiness` JSON |
| GET | `/v1/system/metrics` | Yes | — | `SystemMetrics` JSON |
| GET | `/v1/docker/metrics` | Yes | — | `DockerMetrics` JSON |
| GET | `/v1/docker/version` | Yes | — | `DockerVersion` JSON |
| GET | `/v1/docker/containers` | Yes | — | `DockerContainersResponse` JSON |
| GET | `/v1/docker/snapshot` | Yes | — | `DockerSnapshotResponse` JSON |
| POST | `/v1/docker/containers/run` | Yes | `RunContainerRequest` | 201 `RunContainerResponse` |
| POST | `/v1/docker/containers/{name}/start` | Yes | — | `204 No Content` |
| POST | `/v1/docker/containers/{name}/stop` | Yes | — | `204 No Content` |
| POST | `/v1/docker/containers/{name}/restart` | Yes | — | `204 No Content` |
| POST | `/v1/docker/containers/{name}/delete` | Yes | — | `204 No Content` |
| GET | `/v1/install/status` | Yes | — | `HashMap<String, InstallJob>` JSON |
| POST | `/v1/install/{tool}` | Yes | — | 202 `{"status": "started"}` |

### 10.3 Error format

```json
{
  "error": "human-readable message"
}
```

Status codes: 400 (validation), 401 (auth), 404 (unknown), 409 (conflict),
500 (server), 502 (bad gateway), 504 (timeout).

---

## 11. Security Requirements

### 11.1 Dashboard

- Session cookie: HttpOnly, SameSite=Strict, Secure (when HTTPS), 12h expiry
- CSRF: double-submit cookie, constant-time compare
- Origin validation on all POST
- Rate limiting on login: per-client semaphore
- Password: Argon2id
- CSP: no inline JS, no inline CSS style attributes
- Request body limit: 16 KB
- Global request timeout: 20 seconds

### 11.2 Host agent

- Bearer token: minimum 24 characters, constant-time compare
- Bind: `127.0.0.1:8787` only
- No arbitrary shell endpoint
- Fixed installers only: Docker, Compose, kubectl, Nix, Nixpacks
- Installers run as root (agent runs as root)
- Token is a host-root credential
- No request body limit (installers have no body)

### 11.3 Secrets at rest

- ChaCha20-Poly1305 encryption
- Per-record nonce
- Master key in separate file, `0600` permissions
- Tokens never rendered in HTML, logs, or API responses

---

## Code Quality Rules

These rules apply to every milestone and every file. They are not optional.

### File size limit

No single source file may exceed 500 lines. If a file approaches 500 lines
during implementation, split it by responsibility into a new module.

| Current file | Lines | Action if >500 |
|---|---|---|
| `src/web/handlers.rs` | ~1600 | Split into `handlers/dashboard.rs`, `handlers/hosts.rs`, `handlers/containers.rs`, `handlers/stacks.rs`, `handlers/infrastructure.rs`, `handlers/auth.rs` |
| `src/web/templates.rs` | ~2100 | Split into `templates/layout.rs`, `templates/dashboard.rs`, `templates/hosts.rs`, `templates/containers.rs`, `templates/stacks.rs`, `templates/infrastructure.rs`, `templates/providers.rs`, `templates/deployment.rs` |
| `src/bin/hakobi-agent.rs` | ~600 | Split into `agent_server.rs` (routes), `agent_docker.rs` (Docker handlers), `agent_system.rs` (system metrics), `agent_install.rs` (installers) |
| `src/catalog.rs` | ~900 | Split into `catalog/mod.rs`, `catalog/providers.rs`, `catalog/hosts.rs`, `catalog/stacks.rs`, `catalog/deployments.rs`, `catalog/migrations.rs` |
| `src/providers/docker.rs` | ~500 | Split into `providers/docker/mod.rs`, `providers/docker/backend.rs`, `providers/docker/inventory.rs`, `providers/docker/snapshot.rs` |
| `src/providers/agent.rs` | ~800 | Split into `providers/agent/mod.rs`, `providers/agent/backend.rs`, `providers/agent/routing.rs`, `providers/agent/registration.rs` |

### Function size limit

No single function may exceed 50 lines (excluding comments and blank lines).
If a function exceeds 50 lines, extract a helper function with a clear name.

### Architecture style

Use **plain functions and structs**. Do NOT introduce OOP patterns
(trait objects, inheritance, builder patterns) unless the PRD explicitly
requires a trait (e.g., `ContainerBackend`).

```
GOOD:  plain functions, plain structs, clear names, small modules
GOOD:  trait + implementations ONLY when polymorphism is required by PRD
BAD:   builder pattern chains, trait objects for single implementations
BAD:   nested enums, generic abstractions for one concrete type
BAD:   closure-heavy functional style that hides control flow
```

### Naming conventions

- Functions: `snake_case`, verb-first (`create_host`, `list_stacks`)
- Structs: `PascalCase`, noun (`HostAgent`, `DeploymentStatus`)
- Constants: `SCREAMING_SNAKE_CASE`
- SQL: parameterized, no string interpolation
- Error messages: lowercase, no trailing period, actionable

### Module organization

Each module has one responsibility. Module boundaries follow data boundaries:

```
catalog/          SQLite persistence layer
  mod.rs          Catalog struct, open/close, migrations
  providers.rs    Provider CRUD
  hosts.rs        Host agent CRUD
  environments.rs Environment CRUD
  deployments.rs  Deployment CRUD
  audit.rs        Audit event recording
metrics/          Time-series persistence layer
  mod.rs          MetricsDb struct, open/close
  live.rs         Live sample ring buffer
  rollups.rs      1m/5m/1h rollup queries
  retention.rs    Retention purge job
providers/        Container backends
  docker/         Direct Docker API (bollard)
  agent/          Host agent HTTP backend
web/              HTTP layer
  handlers/       HTTP handlers split by domain
  templates/      Maud SSR templates split by page
  auth.rs         Sessions, CSRF, rate limiting
agent/            Host agent binary modules
```

---

## 12. Build-Test-Fix Loop

### 12.1 Workflow

Every task follows this loop:

```
┌─────────────────────────────────────────────┐
│ 1. BUILD                                    │
│    Implement code per PRD specification     │
│    Add unit + integration tests             │
├─────────────────────────────────────────────┤
│ 2. TEST                                     │
│    Run: cargo fmt --check                   │
│    Run: cargo clippy --all-targets -D warn  │
│    Run: cargo test                          │
│    If any fail → go to FIX                  │
│    If all pass → go to UPDATE               │
├─────────────────────────────────────────────┤
│ 3. FIX                                      │
│    Read failure output                      │
│    Apply targeted fix                       │
│    Consult PRD section for correct spec     │
│    Return to TEST (step 2)                  │
├─────────────────────────────────────────────┤
│ 4. UPDATE                                   │
│    Mark checklist items as done             │
│    Update PRD.md checkboxes                 │
│    Report completion                        │
└─────────────────────────────────────────────┘
```

### 12.2 Rules

- A task is NOT complete until all checklist items are `[x]`.
- A task is NOT complete if any test fails.
- A task is NOT complete if clippy or fmt produce warnings/errors.
- Do not skip steps. Do not mark items done that are not verified.
- If a test cannot pass due to external dependency, mark it as
  `[ ] SKIPPED (reason)` and explain.
- If a PRD requirement cannot be implemented as specified, raise it before
  deviating. Do not silently change the spec.
- Every fix iteration MUST be driven by actual test output, not assumptions.
- The build-test-fix loop repeats until clean. There is no maximum iteration
  count, but token budget applies.

### 12.3 Commands

```sh
# Format check
cargo fmt --check

# Lint
cargo clippy --all-targets -- -D warnings

# Unit + integration tests
cargo test

# Individual test
cargo test web::tests::test_name

# Build release
cargo build --release
```

---

## 13. Milestone Checklists

### CP5: Environment Model + Registration UI

#### 13.1 CP5 Backend

- [x] Add `environments` table to catalog schema (v5)
- [x] Add `deployments` table to catalog schema (v5)
- [x] Add `deployment_logs` table to catalog schema (v5)
- [x] Implement `Catalog::create_environment(name, host_id, build_type, source_type, git_url, git_branch, registry_mode)` → `Result<String>`
- [x] Implement `Catalog::list_environments() -> Result<Vec<Environment>>`
- [x] Implement `Catalog::get_environment(id) -> Result<Option<Environment>>`
- [x] Implement `Catalog::update_environment(id, name, host_id, build_type, source_type, git_url, git_branch, registry_mode) -> Result<()>`
- [x] Implement `Catalog::delete_environment(id) -> Result<()>`
- [x] Implement `Catalog::create_deployment(environment_id, trigger_type, git_commit_sha) -> Result<String>`
- [x] Implement `Catalog::update_deployment_status(id, status, output) -> Result<()>`
- [x] Implement `Catalog::list_deployments(environment_id) -> Result<Vec<Deployment>>`
- [x] Implement `Catalog::get_latest_deployment(environment_id) -> Result<Option<Deployment>>`
- [x] Implement `Catalog::add_deployment_log(deployment_id, level, message) -> Result<()>`
- [x] Implement `Catalog::list_deployment_logs(deployment_id) -> Result<Vec<DeploymentLog>>`
- [x] Add audit events for environment create/update/delete
- [x] Add audit events for deployment create/status change
- [x] Validate environment name: unique, ASCII slug, 1–64 chars
- [x] Validate `host_id` references an existing enabled host
- [x] Validate `build_type` is one of `compose | dockerfile | nixpacks`
- [x] Validate `source_type` is one of `git | upload`
- [x] Validate `git_url` is non-empty when `source_type == "git"`
- [x] Validate `git_url` is `None` when `source_type == "upload"`

#### 13.2 CP5 Backend — Agent Docker endpoints

- [ ] Agent exposes `GET /v1/docker/containers` returning full `ContainerRow[]`
- [ ] Agent exposes `POST /v1/docker/containers/run` with `{image, name, container_port}`
- [ ] Agent exposes `POST /v1/docker/containers/{name}/start`
- [ ] Agent exposes `POST /v1/docker/containers/{name}/stop`
- [ ] Agent exposes `POST /v1/docker/containers/{name}/restart`
- [ ] Agent exposes `POST /v1/docker/containers/{name}/delete`
- [ ] Agent validates container names before Docker calls
- [ ] Agent validates image references before Docker calls
- [ ] Agent rejects port 0
- [ ] Agent returns 401 for missing/wrong bearer token
- [ ] Agent returns 400 for validation errors
- [ ] Agent returns 502 for Docker failures
- [ ] Agent returns 504 for Docker timeouts
- [ ] Agent caps response at 512 container rows

#### 13.3 CP5 App backend — AgentBackend

- [ ] Implement `AgentBackend` struct with `AgentEndpoint`
- [ ] Implement full `ContainerBackend` trait for `AgentBackend`
- [ ] Implement `RoutingBackend` dispatching `agent://{id}` to `AgentBackend`
- [ ] RoutingBackend resolves host credentials from catalog (decrypted token)
- [ ] RoutingBackend preserves direct Docker providers
- [ ] RoutingBackend does not hold catalog lock across `.await`
- [ ] Unit tests for `AgentBackend` with injectable HTTP client
- [ ] Unit tests for `RoutingBackend` endpoint dispatch
- [ ] Integration tests for provider create → container list flow

#### 13.4 CP5 App startup — Local provider registration

- [ ] On startup, if agent configured, ensure `agent://local` provider exists
- [ ] On startup, if Docker socket accessible, ensure `unix:///var/run/docker.sock` provider exists
- [ ] Idempotent: no duplicate providers
- [ ] Existing provider rows never overwritten or re-enabled
- [ ] Name collision prevented
- [ ] No provider created when neither path available
- [ ] Unit tests for registration idempotency and edge cases

#### 13.5 CP5 UI — Environments list

- [x] Sidebar shows `Environments` with `fa-layer-group` icon
- [x] `/environments` page lists all environments
- [x] Each environment shows: name, host, build type, source type, status
- [x] "New Environment" button links to `/environments/new`
- [x] Empty state with illustration and "Create your first environment" CTA
- [x] Provider failure state visible

#### 13.6 CP5 UI — Environment detail

- [x] `/environments/{id}` shows environment metadata
- [x] Shows latest deployment status and version
- [x] Shows deployment history table
- [x] "Deploy" button triggers POST to `/environments/{id}/deploy`
- [x] "Rollback" button available if previous deployment exists
- [x] "Edit" button links to `/environments/{id}/edit`
- [x] "Delete" button with typed-name confirmation dialog
- [x] Deployment logs view (last N lines)
- [x] HTMX auto-refresh deployment status every 5s

#### 13.7 CP5 UI — Environment form

- [x] `/environments/new` form: name, host, build type, source type, git URL, branch, registry mode
- [x] `/environments/{id}/edit` same form, pre-filled
- [x] Host select shows enabled hosts only
- [x] Registry mode toggle: local vs push with info badge
- [x] CSRF token hidden field
- [x] Origin header validation
- [x] Client-side form validation (required fields)
- [x] Server-side validation with error display

#### 13.8 CP5 UI — Webhook configuration

- [ ] SKIPPED (webhook receiver is CP6 scope; environments table already carries webhook_secret/webhook_enabled columns)
- [ ] SKIPPED (CP6: webhook receiver milestone)
- [ ] SKIPPED (CP6: webhook receiver milestone)
- [ ] SKIPPED (CP6: webhook receiver milestone)
- [ ] SKIPPED (CP6: webhook receiver milestone)

#### 13.9 CP5 Tests

- [x] Unit test: environment CRUD in catalog
- [x] Unit test: environment validation (name, host, build type, source type)
- [x] Unit test: deployment CRUD in catalog
- [x] Unit test: deployment status transitions
- [x] Unit test: deployment log storage
- [x] Integration test: create environment via HTTP POST
- [x] DONE IN CP6 (deploy pipeline executor; CP5 deploy endpoint was an explicit 501 placeholder)
- [x] DONE IN CP6 (rollback redeploys; CP5 rollback endpoint was an explicit 501 placeholder)
- [x] Integration test: unauthorized POST returns 403
- [x] Integration test: invalid host_id returns 400
- [x] All existing tests pass

#### 13.10 CP5 Checklist — Build-Test-Fix

- [x] BUILD: Implement all CP5 backend code
- [x] BUILD: Implement all CP5 UI code
- [x] BUILD: Add all CP5 tests
- [x] TEST: `cargo fmt --check`
- [x] TEST: `cargo clippy --all-targets -- -D warnings`
- [x] TEST: `cargo test`
- [ ] IF FAIL: fix and re-test
- [x] IF PASS: mark all CP5 checklist items as `[x]`
- [ ] UPDATE: commit and push — SKIPPED per task instruction ("Do not deploy or commit")

---

### CP6: Deployment Pipeline

#### 13.11 CP6 Backend — Build runner

- [x] Integrate with existing `buildrunner.rs` HTTP client
- [x] Build runner endpoint: configurable via `HAKOBI_BUILDRUNNER_URL`
- [x] Build runner auth: `HAKOBI_BUILDRUNNER_TOKEN`
- [x] Support compose build type
- [x] Support Dockerfile build type
- [x] Support nixpacks build type
- [x] Return build output and image tag on success
- [x] Return build error on failure
- [x] Build timeout configurable (default 15 minutes)

#### 13.12 CP6 Backend — Deployment executor

- [x] Implement deployment executor that:
  - Creates deployment record (status: pending)
  - Triggers build via build runner
  - Updates status to `building`
  - On build success, updates status to `deploying`
  - Instructs agent to stop old containers
  - Instructs agent to run new containers
  - Verifies containers are running
  - Updates status to `running`
  - On any failure, updates status to `failed`
- [x] Run deployment asynchronously (spawn tokio task)
- [x] Record deployment logs at each stage
- [x] Handle concurrent deployments (only one per environment at a time)
- [x] Support rollback to previous version

#### 13.13 CP6 Backend — Webhook receiver

- [x] POST `/webhook/{environment_id}/{secret}` endpoint
- [x] Verify secret matches environment's `webhook_secret`
- [x] Parse GitHub/GitLab webhook payload for commit SHA and message
- [x] Trigger deployment (trigger_type: webhook)
- [x] Return 202 Accepted immediately
- [x] Reject invalid secret with 403

#### 13.14 CP6 UI — Deployment page

- [x] `/deployment` page shows all deployments across environments
- [x] Each deployment shows: environment, version, status, trigger, started, completed
- [x] Status badges: pending (gray), building (blue), deploying (blue), running (green), failed (red), rolled_back (amber)
- [x] "Deploy" button per environment
- [x] "Rollback" button per environment with previous version
- [x] HTMX auto-refresh every 5s

#### 13.15 CP6 Tests

- [x] Unit test: build runner request serialization
- [x] Unit test: deployment executor state machine
- [x] Unit test: webhook signature verification
- [x] Integration test: manual deploy triggers full pipeline
- [x] Integration test: webhook deploy triggers pipeline
- [x] Integration test: build failure marks deployment as failed
- [x] Integration test: concurrent deploy rejected (409)
- [x] All existing tests pass

#### 13.16 CP6 Checklist — Build-Test-Fix

- [x] BUILD: Implement all CP6 backend code
- [x] BUILD: Implement all CP6 UI code
- [x] BUILD: Add all CP6 tests
- [x] TEST: `cargo fmt --check`
- [x] TEST: `cargo clippy --all-targets -- -D warnings`
- [x] TEST: `cargo test`
- [x] IF FAIL: fix and re-test
- [x] IF PASS: mark all CP6 checklist items as `[x]`
- [ ] UPDATE: commit and push — SKIPPED per task instruction ("Do not deploy or commit")

---

### CP7: Metrics History + Retention

#### 13.17 CP7 Backend — Metrics database

- [ ] Add `metrics.rs` module with SQLite metrics database
- [ ] Create `metrics_live`, `metrics_1m`, `metrics_5m`, `metrics_1h` tables
- [ ] Create indexes on `(host_id, timestamp)` for all tables
- [ ] Implement `MetricsDb::open(path) -> Result<Self>`
- [ ] Implement `MetricsDb::insert_live_sample(host_id, sample) -> Result<()>`
- [ ] Implement `MetricsDb::insert_1m_rollup(host_id, rollup) -> Result<()>`
- [ ] Implement `MetricsDb::insert_5m_rollup(host_id, rollup) -> Result<()>`
- [ ] Implement `MetricsDb::insert_1h_rollup(host_id, rollup) -> Result<()>`
- [ ] Implement `MetricsDb::query_1m(host_id, from, to) -> Result<Vec<...>>`
- [ ] Implement `MetricsDb::query_5m(host_id, from, to) -> Result<Vec<...>>`
- [ ] Implement `MetricsDb::query_1h(host_id, from, to) -> Result<Vec<...>>`
- [ ] Implement retention purge job (runs daily)
- [ ] Separate metrics database from catalog database

#### 13.18 CP7 Backend — Sampling job

- [ ] Spawn background tokio task that samples every 60 seconds
- [ ] Call agent `/v1/system/metrics` for each enabled host
- [ ] Write 1-minute rollup to `metrics_1m`
- [ ] Every 5 rollups, write 5-minute rollup to `metrics_5m`
- [ ] Every 60 rollups, write 1-hour rollup to `metrics_1h`
- [ ] Run retention purge daily
- [ ] Handle agent offline gracefully (skip, log warning)

#### 13.19 CP7 UI — History charts

- [ ] Host resources page gains time-window selector: 5m / 1h / 24h
- [ ] CPU history chart (line, server-rendered SVG)
- [ ] Memory history chart (line, server-rendered SVG)
- [ ] Disk usage history (bar, server-rendered SVG)
- [ ] Network throughput history (line, server-rendered SVG)
- [ ] Charts use CSS variables for colors
- [ ] No external chart library
- [ ] No inline styles

#### 13.20 CP7 Tests

- [ ] Unit test: metrics database open/close
- [ ] Unit test: insert/query round-trip for all tables
- [ ] Unit test: retention purge removes expired rows
- [ ] Unit test: rollup calculation correctness
- [ ] Integration test: sampling job writes to database
- [ ] Integration test: HTMX partial returns chart data
- [ ] All existing tests pass

#### 13.21 CP7 Checklist — Build-Test-Fix

- [ ] BUILD: Implement metrics database module
- [ ] BUILD: Implement sampling job
- [ ] BUILD: Implement history charts
- [ ] TEST: `cargo fmt --check`
- [ ] TEST: `cargo clippy --all-targets -- -D warnings`
- [ ] TEST: `cargo test`
- [ ] IF FAIL: fix and re-test
- [ ] IF PASS: mark all CP7 checklist items as `[x]`
- [ ] UPDATE: commit and push

---

### CP8: Container Detail + Logs

#### 13.22 CP8 Backend — Container detail

- [x] Agent exposes `GET /v1/docker/containers/{name}/inspect` returning full inspect data
- [x] Agent exposes `GET /v1/docker/containers/{name}/logs?tail=N` returning log lines
- [x] Agent validates container name before Docker calls
- [x] App `AgentBackend` implements `inspect_container()` and `container_logs()`

#### 13.23 CP8 Backend — Container stats per container

- [x] Agent exposes `GET /v1/docker/containers/{name}/stats` returning CPU%, memory, network I/O
- [ ] Uses `docker stats --no-stream --format json` filtered by container name
- [x] Or uses bollard stats API for real-time stats

#### 13.24 CP8 UI — Container detail page

- [x] Route: `/containers/{provider}/{name}`
- [x] Shows metadata: image, state, health, ports, labels, environment variables
- [x] Shows per-container CPU/memory usage
- [x] Shows streaming logs (HTMX poll every 2s, last 100 lines)
- [x] Shows start/stop/restart/delete actions
- [x] Delete requires typed name confirmation
- [x] Link from containers list to detail page

#### 13.25 CP8 Tests

- [x] Unit test: container detail serialization
- [x] Unit test: log line formatting
- [x] Integration test: container detail page renders
- [x] Integration test: log partial returns log lines
- [x] Integration test: container not found returns 404
- [x] All existing tests pass

#### 13.26 CP8 Checklist — Build-Test-Fix

- [x] BUILD: Implement container detail backend
- [x] BUILD: Implement container detail UI
- [x] BUILD: Implement log streaming
- [x] TEST: `cargo fmt --check`
- [x] TEST: `cargo clippy --all-targets -- -D warnings`
- [x] TEST: `cargo test`
- [x] IF FAIL: fix and re-test
- [x] IF PASS: mark all CP8 checklist items as `[x]`
- [ ] UPDATE: commit and push

---

---

### CP9: GitHub Integration — Sign-in, Repo Deploy Pipeline, PR Statuses

#### 14.1 Goal

Connect Hakobi to GitHub the way Cloudflare Workers Builds works:
one-time GitHub App installation by the admin, per-user GitHub sign-in
inside the dashboard (after Hakobi login), repo→environment deploy
pipeline with monorepo path filtering, and PR/commit status reporting
back to GitHub. Everything else (build runner, executor, environments,
webhook receiver) is reused from CP6.

#### 14.2 Non-goals (v1)

- No buildpacks/Nixpacks: Dockerfile builds only (`dockerfile_path`).
- No Kubernetes/Nix deploy targets: Docker host agents only.
- No GitHub check-run "annotations"; commit statuses + one check run
  per deploy is enough for v1.
- No multi-tenant: all environments belong to the single Hakobi admin
  user's GitHub connection; other users may authenticate later but v1
  authorizes environments from the admin's installation.

#### 14.3 Architecture decisions

- **GitHub App, not OAuth App**: manifest-driven install, fine-grained
  permissions (Contents: read, Commit statuses: read+write, Webhooks:
  read+write), installation tokens (1h TTL, auto-refresh), higher rate
  limits, per-repo installation scoping.
- **Stateless JWT sessions stay**: GitHub access tokens are stored
  encrypted in the existing vault (same envelope as provider
  credentials), keyed by Hakobi user.
- **Monorepo model**: an Environment points at `(repo, branch, path,
  dockerfile_path)`. Push events are matched against the path glob; one
  repo can serve many environments. Path detection is assisted by a
  scanner (see 14.6) but the stored value is explicit.
- **Concurrency**: a newer push for the same environment cancels the
  in-flight build (Workers-style); the cancelled deployment row is
  marked `cancelled`.

#### 14.4 Data model (schema v6)

New table `github_connections`:
- `id` TEXT PK
- `user_id` TEXT NOT NULL (Hakobi username)
- `github_login` TEXT NOT NULL
- `github_user_id` INTEGER NOT NULL
- `access_token` TEXT NOT NULL (encrypted, vault envelope)
- `refresh_token` TEXT (encrypted, nullable)
- `token_expires_at` TEXT NOT NULL (ISO-8601)
- `created_at` TEXT NOT NULL

New table `github_app` (single row, admin-level install):
- `id` TEXT PK (constant `app`)
- `app_id` INTEGER NOT NULL
- `client_id` TEXT NOT NULL
- `client_secret` TEXT NOT NULL (encrypted)
- `webhook_secret` TEXT NOT NULL (encrypted)
- `private_key_pem` TEXT NOT NULL (encrypted)
- `installation_id` INTEGER NOT NULL
- `installed_at` TEXT NOT NULL

New table `webhook_deliveries` (dedupe):
- `delivery_id` TEXT PK (X-GitHub-Delivery header)
- `received_at` TEXT NOT NULL

`environments` gains columns (ALTER TABLE, schema v6):
- `github_repo` TEXT (nullable, `owner/name`)
- `github_branch` TEXT (nullable)
- `github_path` TEXT (nullable, glob, empty = whole repo)
- `github_dockerfile` TEXT (nullable, default `Dockerfile` at path root)
- `github_auto_deploy` INTEGER NOT NULL DEFAULT 0
- `github_installation_id` INTEGER (nullable)

#### 14.5 GitHub App setup flow (admin, one-time)

1. Settings → GitHub → "Install GitHub App" renders a manifest
   (`name=hakobi-<random suffix>`, `url=base_url`,
   `hook_attributes.url=base_url+/webhooks/github`,
   `callback_urls=[base_url+/webhooks/github/callback]`,
   `public=false`,
   `request_oauth_on_install=true`,
   permissions: Contents R, Commit statuses RW, Metadata R,
   Pull requests R, Webhooks RW).
2. Admin reviews on `github.com/settings/apps/new?manifest=...`;
   GitHub redirects back to `/webhooks/github/callback?code=...`.
3. Hakobi POSTs the code to
   `POST /app-manifests/{code}` (no auth required by GitHub for this
   endpoint) and receives `id, client_id, client_secret, webhook_secret,
   pem`.
4. Hakobi derives the installation id via
   `GET /apps/{app_id}/installations`, stores all credentials encrypted
   in `github_app`, marks setup complete.
5. Connection health shown as a badge on the Settings page.

#### 14.6 Environment source config with auto-detection

The environment form's GitHub section automates what would otherwise be
hand-typed, using a script-driven recommendation flow:

1. User picks repo (dropdown from installation repos) and branch
   (dropdown from that repo's branches).
2. On selection, Hakobi fetches the repo tree (`GET /repos/{o}/{r}/git/
   trees/{branch}?recursive=1`, cached 5 min) and runs the detector:
   - finds files matching `(^|/)Dockerfile$` → candidate dockerfile
     paths
   - finds `docker-compose.yml` / `compose.yaml` as a hint
   - derives the set of top-level dirs that contain a Dockerfile
3. UI presents:
   - **Path recommendation**: if exactly one dir with a Dockerfile →
     preselected; if several → dropdown of dirs (monorepo), user picks;
     if zero → warning "no Dockerfile found at branch head".
   - **Dockerfile recommendation**: defaults to `<path>/Dockerfile`;
     dropdown lists all candidates under the chosen path.
   - Everything remains editable (detector only pre-fills).
4. On save, backend validates: path exists in tree; dockerfile path
   exists in tree. Stored explicitly per environment.

#### 14.7 Webhook pipeline

Route: `POST /webhooks/github` (public; auth via HMAC, not session).

1. Verify `X-Hub-Signature-256` = HMAC-SHA256(raw body,
   webhook_secret). Fail → 401.
2. Dedupe on `X-GitHub-Delivery` via `webhook_deliveries` (INSERT OR
   IGNORE; already seen → 200, skip).
3. Event routing:
   - `push` → pipeline below.
   - `installation` / `installation_repositories` → update stored
     installation id / repo cache.
   - `ping` → 200.
   - other → 200 ignore.
4. Parse push: `repository.full_name`, `ref` (strip `refs/heads/`),
   `commits[].modified/added/removed` (union = changed set; for
   forced pushes or >20 commits treat changed set as "all").
5. Match environments: enabled, `github_auto_deploy=1`,
   `github_repo` equal, `github_branch` equal, path glob matches any
   changed file (empty pattern = match all). No match → 200.
6. For each match, newest-wins: if the same environment has an
   in-flight (`building`) deployment from an earlier push, mark it
   `cancelled` and create a new deployment row (status `queued`).
7. Trigger the existing build runner with:
   - clone ref = branch head at `after` sha,
   - dockerfile = `github_dockerfile`,
   - build context root = `github_path`,
   - image tag = derived from environment name + version.
8. Status reporting timeline:
   - queued → `POST /repos/{o}/{r}/statuses/{after}`: state `pending`,
     context `hakobi/build`, target_url = deployment page.
   - build ok → status `success`; deploy starts, second status context
     `hakobi/deploy` pending.
   - deploy ok → `hakobi/deploy` success, description `deployed v{N}`.
   - any fail → matching context `failure` with error tail in
     description (max 140 chars).
   - PR pushes (`refs/pull/*/head` are not push targets; PRs are
     covered because GitHub sends push events for the PR branch when
     opened/synced from the same repo, and via `pull_request`
     opened/synchronize we post one check run `hakobi/deploy`).
9. Installation tokens: minted per API call from the JWT-signed app
   auth (`POST /app/installations/{id}/access_tokens`), 1h TTL, never
   stored; regenerated on demand. User access tokens (from OAuth) are
   only used for the repo/branch/tree pickers and identity, never for
   deploys.

#### 14.8 API surface (new, session-authenticated)

- `GET  /api/github/status` — app installed? user connected?
- `GET  /api/github/repos` — installation repos (cached 5 min)
- `GET  /api/github/repos/{owner}/{name}/branches` — branches (cached)
- `GET  /api/github/repos/{owner}/{name}/tree?ref=` — file tree
  (cached 5 min) used by the detector
- `POST /settings/github/install` — start manifest flow (admin)
- `GET  /webhooks/github/callback` — manifest code exchange (public,
  admin-only by GitHub install rights)
- `POST /settings/github/connect` — start user OAuth (session)
- `GET  /settings/github/callback` — user OAuth callback (session)
- `DELETE /settings/github` — disconnect user token (session)

#### 14.9 Security rules

- Webhook: constant-time HMAC compare; raw body (not re-serialized).
- GitHub secrets encrypted at rest with the existing vault key.
- `session.secret`-style file protection applies to nothing here:
  GitHub credentials live only in the catalog DB (encrypted).
- OAuth `state` parameter = session-bound CSRF token.
- Path globs validated: no absolute paths, no `..`.
- Installation tokens never logged, never rendered.
- Webhook endpoint exempt from session auth but rate-limited
  (60/min/IP) and HMAC-verified.

#### 14.10 UI

- Settings page gains a "GitHub" card: install state, connected user,
  install badge, disconnect button.
- Environment form gains "Source" section per 14.6 with the detector
  UX.
- Deployment history rows gain: commit SHA (short), branch, triggered
  by "push".
- Environment detail gains a "Retrigger" button (manual redeploy of
  latest pushed sha).

#### 14.11 Tests

- HMAC verification: valid, invalid, missing, replayed delivery.
- Path glob matching: exact dir, recursive glob, empty pattern,
  no-match.
- Monorepo match: two environments same repo/branch different paths,
  push touching one file deploys only one.
- Concurrency: second push cancels first in-flight deployment.
- Status client: pending/success/failure payloads hit the right
  endpoints (mock server).
- Detector: single Dockerfile, monorepo multi-Dockerfile, none found.
- Auth: OAuth state mismatch rejected; token refresh on expiry.
- Schema migration v5→v6 preserves environments rows.

#### 14.12 CP9 Checklist — Build-Test-Fix

- [ ] BUILD: schema v6 migration (github columns + tables)
- [ ] BUILD: GitHub App manifest setup flow + credential storage
- [ ] BUILD: user OAuth connect flow
- [ ] BUILD: installation token minting client
- [ ] BUILD: webhook HMAC verify + dedupe + event routing
- [ ] BUILD: push matcher (repo/branch/path glob, concurrency cancel)
- [ ] BUILD: commit status client (pending/success/failure)
- [ ] BUILD: repo/branch/tree picker endpoints with cache
- [ ] BUILD: environment form source section with detector
- [ ] BUILD: Settings GitHub card
- [ ] BUILD: retrigger button on environment detail
- [ ] TEST: unit tests per 14.11
- [ ] TEST: `cargo fmt --check`
- [ ] TEST: `cargo clippy --all-targets -- -D warnings`
- [ ] TEST: `cargo test`
- [ ] IF FAIL: fix and re-test
- [ ] IF PASS: mark all CP9 checklist items as `[x]`
- [ ] UPDATE: commit and push

## 14. Bug Registry

### 14.1 Known bugs at time of PRD creation

| # | Severity | Area | Description | Status |
|---|----------|------|-------------|--------|
| B-01 | Critical | Overview | Provider failure → silent zeros, no error shown | Open |
| B-02 | Critical | Deployment | `hosts_create()` accepts empty token, invalid URL | Open |
| B-03 | Critical | Catalog | `delete_host_agent()` + provider delete not in one transaction | Fixed in CP3 |
| B-04 | High | Overview | Service/stack keys missing provider ID → collision | Fixed |
| B-05 | High | Overview | `containers_healthy` treats no-healthcheck as healthy | Open |
| B-06 | High | Overview | `containers_restarting` misses Docker status wording | Open |
| B-07 | High | Overview | Memory/CPU from `/proc` inside container, not host | Fixed (agent metrics) |
| B-08 | High | Containers | No container detail page (CP8 scope) | Planned |
| B-09 | Medium | Overview | No historical metrics (CP7 scope) | Planned |
| B-10 | Medium | Stacks | Stack names globally unique, not per-provider | Open |
| B-11 | Medium | Containers | No confirmation dialog for restart (delete has one) | Open |
| B-12 | Medium | Infrastructure | Disk list shows duplicate APFS mounts | Open |
| B-13 | Medium | Infrastructure | Network table shows zero-traffic interfaces | Fixed (filter + take 5) |
| B-14 | Medium | Host Resources | No disk I/O rates | Open |
| B-15 | Medium | Host Resources | No GPU/temperature monitoring | Out of scope v1 |
| B-16 | Medium | Host Resources | Process `user` shows UID, not username | Open |
| B-17 | Low | Login | No password reset flow (CLI only) | Open |
| B-18 | Low | Login | No MFA/TOTP | Out of scope v1 |
| B-19 | Low | Login | No session invalidation on password change | Open |
| B-20 | Low | Overview | CPU is load average, not CPU utilization | Fixed (labeled as load) |
| B-21 | Low | Containers | No bulk operations | Out of scope v1 |
| B-22 | Low | Containers | No per-container stats endpoint (CP8 scope) | Planned |
| B-23 | Low | UI | Compose version string wraps in readiness card | Fixed (truncated) |
| B-24 | Low | UI | Stack count grammar (`1 stacks`) | Fixed |

### 14.2 Bug fix workflow

For each bug marked `Open`:

1. Create a failing test that reproduces the bug.
2. Fix the code.
3. Verify the test passes.
4. Verify all existing tests still pass.
5. Mark bug as `Fixed` in this registry.

---

## Appendix A: Glossary

| Term | Definition |
|------|-----------|
| Host | A physical or virtual machine running the Hakobi agent |
| Agent | Native binary running as root on a host, exposing typed APIs |
| Provider | A Docker engine connection (local socket, remote TCP, or agent) |
| Environment | A deployable unit with build config, source, and target host |
| Stack | A set of services defined by compose YAML |
| Deployment | A versioned instance of an environment build |
| Container | A running instance of a Docker image |
| Control plane | The Hakobi dashboard application |
| Build runner | External service that compiles source into images |

---

## Appendix B: File inventory

| Path | Purpose |
|------|---------|
| `src/main.rs` | CLI entry point |
| `src/lib.rs` | Library root, public modules |
| `src/config/` | Configuration loading/saving |
| `src/catalog.rs` | SQLite catalog |
| `src/metrics.rs` | SQLite metrics time-series (CP7) |
| `src/host_agent.rs` | Shared protocol types |
| `src/install.rs` | Host tool detection |
| `src/buildrunner.rs` | Build runner HTTP client |
| `src/cloudflare/` | Cloudflare Tunnel |
| `src/providers/docker.rs` | Direct Docker backend (bollard) |
| `src/providers/agent.rs` | Agent-backed backend |
| `src/stacks/` | Stack handling |
| `src/utils/` | Utilities |
| `src/web/` | Axum SSR application |
| `src/bin/hakobi-agent.rs` | Native host agent binary |
| `mockups/` | HTML UI prototypes |
| `agent/tasks/` | Agent harness task definitions |
| `packaging/` | systemd unit files |
| `docs/` | Documentation |
