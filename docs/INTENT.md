# Intent Map

What the user means vs what the code does. Update this file whenever a
feature's meaning diverges from its implementation. Read this file before
touching UI, navigation, or workflows.

## Format

Each entry:

```
### <area>
User intent: <what the user actually wants, in their words>
Code reality: <what the code currently does>
Gap: <difference, or "none">
```

## Entries

### Dashboard (sidebar: "Dashboard", route `/`)
User intent: Metrics only. Live server metrics (24h CPU/memory charts
aggregated across all hosts), total running services/containers (live count,
provider-neutral via ContainerBackend trait — Docker now, K8s/Nix later),
recent deployment activity. No hero banner, no greeting, no readiness
section, no instance cards.
Code reality: Metric cards (Active containers, Environments), 24h charts,
recent activity feed.
Gap: none (rebuilt 2026-11-10 per user direction).

### Deployment (sidebar: "Deployment", route `/hosts`)
User intent: The operator's main work surface. Where you register a remote
agent, see your hosts, and kick off deployments. Clicking "Deployment" in the
sidebar should land you where deployment happens.
Code reality: `/hosts` lists registered remote agents with the "Register a
remote agent" form. Deployment history lives at `/deployment` (no sidebar
entry — reached via environment pages).
Gap: none (nav retargeted 2026-11-10, commit 02926fa). Deployment history is
still orphaned from the sidebar; revisit if the user wants it surfaced.

### Host resources (route `/hosts/{id}/resources`)
User intent: Deep-dive metrics for one remote host — CPU, memory, disk,
network, plus history charts. Local host does NOT belong here; this page is
for remote agents only.
Code reality: Per-host metrics with 5m/1h/24h SVG charts (CP7). Local host
excluded from the Deployment tab.
Gap: none.

### Containers
User intent: See every container across all hosts. Click a container to see
metadata, live stats, logs, and lifecycle actions (start/stop/restart/delete).
Code reality: `/containers` grid + `/containers/{provider}/{name}` detail
with HTMX-polled stats/logs (CP8).
Gap: none.

### Environments
User intent: The unit of deployment config — like Cloudflare Workers-style
flat environments. Create one, get a webhook, deploy images to a host.
Code reality: Flat `environments` table (schema v5), CRUD UI at
`/environments`, webhook-driven pipeline (CP6).
Gap: none.
