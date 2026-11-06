# Hakobi R&D Knowledge Base

Last updated: **2026-11-05**

This folder records product and architecture research before implementation. Vendor pricing and plan limits change; treat figures as a snapshot with links to the primary sources.

## Documents

1. [Container platform landscape](./01-container-platform-landscape.md) — Swarmpit, Podman, OrbStack, Docker, Portainer, Kubernetes.
2. [CI and orchestration activity](./02-ci-and-orchestration-activity.md) — Jenkins, GitLab Runner, Kubernetes.
3. [Observability stack](./03-observability-stack.md) — Prometheus, Grafana, alerts, integration boundaries.
4. [Bot detection and automatic banning](./04-bot-detection-and-auto-ban.md) — Cloudflare, CrowdSec, safe enforcement design.
5. [Cloudflare Workers, Pages, and Wrangler](./05-cloudflare-workers-pages-wrangler.md) — edge workloads, observability, management APIs.
6. [Hakobi integration strategy and opinion](./06-hakobi-integration-strategy.md) — product thesis, architecture, roadmap, risks.
7. [Feature catalog and build checkpoints](./07-feature-catalog-and-checkpoints.md) — shipped features, planned capabilities, phase gates, exclusions.
8. [Merged PRD v0.3](./08-prd-v0.3-merged.md) — canonical product requirements, reconciled with research.
9. [Rust observability decision](./09-rust-observability-decision.md) — GreptimeDB vs OpenObserve vs building our own.
10. [Service topology UI](./10-service-topology-ui.md) — node graph, evidence-backed edges, interaction and phases.

## Executive conclusions

- Hakobi should be a **provider-agnostic operations and policy layer**, not another container engine, CI system, time-series database, or Grafana clone.
- The first useful product is **read-only inventory, activity, health, and deployment history** across providers.
- Mutating actions must arrive later with typed APIs, scoped credentials, approvals, audit logs, and rollback.
- **Docker API first**, then Podman through its compatible/REST surface, then Kubernetes. OrbStack is a Docker endpoint, not a separate integration category.
- **Swarmpit is a benchmark, not a target API**. Docker Swarm is a niche inside the Docker platform.
- Jenkins, GitLab Runner, and Kubernetes all expose monitoring data, but through different models. Normalize activity, do not proxy their entire UIs.
- Native CI is bounded to Hakobi application pipelines; it is not a general-purpose build farm or Jenkins replacement.
- Integrate existing **Prometheus/Grafana** rather than storing another metrics history.
- Bot enforcement should use provider decisions and **shadow mode → challenge → short ban → reviewed long ban**. Never start with permanent automatic bans.
- Cloudflare Workers/Pages belong in the service catalog, but after container/CI/observability foundations.
- The commercial opening is governance: RBAC, approvals, audit, policy, fleet visibility, and support—not per-container hosting markup.
