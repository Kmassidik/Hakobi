# Container Platform Landscape

Last verified: **2026-11-05**

## Scope

This note covers products commonly proposed as “container platforms” and what their vendors actually sell. It separates runtimes, local development tools, management UIs, and orchestrators because Hakobi must integrate with each at a different layer.

## Market map

| Product | Category | What it sells | Hakobi relationship |
|---|---|---|---|
| **Swarmpit** | Self-hosted Docker Swarm management UI | Open source under EPL-1.0; donations via Open Collective; no paid SaaS tier advertised | Feature benchmark for simple fleet management; integrate through Docker APIs, not Swarmpit |
| **Podman** | OCI container engine | Open source; rootless/daemonless engine and Podman Desktop; vendors monetize support and Kubernetes platforms | Integration target through Podman REST API / compatible socket |
| **OrbStack** | macOS container and Linux VM product | Free personal use; Pro at **$8/user/month billed annually**; Enterprise includes SSO and sales-led terms | Treat as a local Docker context, not a provider-specific backend |
| **Docker** | Developer platform and container ecosystem | Personal free; Pro/Team/Business subscriptions; Docker Hub, Scout, Testcontainers, hardened images | Primary API integration surface for containers and Compose |
| **Portainer** | Commercial container management platform | Free Community Edition; Starter from **$1,045/year**; Scale from **$2,095/year**; Enterprise custom; priced by nodes/vCPUs | Closest commercial benchmark; differentiate on cross-provider operations |
| **Kubernetes** | Open-source orchestration | No license fee; cloud vendors sell managed clusters and support | Integrate with the API and metrics ecosystem; do not rebuild Kubernetes |

Pricing is a snapshot and excludes taxes, negotiated terms, and add-ons. Verify before publishing a price comparison.

## Product-by-product notes

### Swarmpit

Swarmpit provides a lightweight, self-hosted interface for Docker Swarm clusters:

- Stack, service, secret, volume, and network management
- Service scaling, rollback, and image redeploy
- Real-time CPU, memory, and disk statistics
- REST API and an MCP server
- CouchDB for application data and InfluxDB for cluster statistics
- Deployment through an installer that mounts the Docker socket

It is **not** a commercial control plane and does not advertise a paid product. Its privacy/self-hosted positioning is relevant to Hakobi’s audience.

**Important risk:** mounting `/var/run/docker.sock` into Swarmpit gives it root-equivalent control of the host. Hakobi must not expose such a socket directly to a web process without a broker and strict authorization.

### Podman

Podman is daemonless, Linux-native, and supports rootless containers. It provides a Docker-compatible CLI, OCI images, pods, and a REST API. The REST service runs on Linux; remote clients exist for macOS and Windows.

Podman is a runtime, not a full orchestration product. It does not replace Kubernetes or Docker Swarm conceptually. Podman Desktop adds a GUI and local Kubernetes integration, but production management still needs an orchestrator or a control plane.

**Hakobi integration:** support a Podman connection type backed by the Podman REST API, preferably over SSH or a local Unix socket. Avoid assuming Docker-specific objects always exist.

### OrbStack

OrbStack is a macOS-focused alternative to Docker Desktop for containers and Linux machines. Its commercial model is seat-based, while personal use is free.

OrbStack exposes Docker-compatible behavior. Hakobi should therefore connect to it as a **Docker endpoint**, not build OrbStack-specific features. This avoids turning a local developer convenience into a permanent architecture dependency.

### Docker

Docker’s business is broader than “run a container”:

- Docker Desktop, Engine, Compose, and Kubernetes
- Docker Hub registries and pull limits
- Docker Scout image analysis
- Testcontainers Cloud
- Hardened Images, SBOM/provenance, and enterprise controls
- Audit logs, SSO/SCIM, RBAC, and enhanced isolation on higher tiers

Docker Engine itself is open source; Docker sells productivity, governance, registry, and supply-chain services around it.

**Hakobi integration:** Docker Engine API and Compose are the first practical backend. Support TLS Docker endpoints, local sockets, and SSH contexts. Never expose unrestricted Docker sockets.

### Portainer

Portainer is the closest commercial comparison to Hakobi’s proposed container-management scope. It charges by node/vCPU, bundles Docker/Swarm/Kubernetes under one license, and sells:

- Fleet and multi-cluster management
- RBAC and identity integration
- GitOps and internal developer portal features
- Edge/IIoT management
- Vendor support, onboarding, compliance evidence, and managed services

It demonstrates that buyers pay for **governance and support**, not merely a UI that starts containers.

### Kubernetes

Kubernetes is a portable, extensible open-source platform. It provides desired-state reconciliation, service discovery, rollouts, self-healing, secret/config distribution, batch execution, and horizontal scaling.

Kubernetes explicitly does not provide CI/CD, logging, monitoring, application middleware, or a complete PaaS. Those are integration concerns—exactly where Hakobi can add value.

## Competitive conclusion

The market is crowded at the runtime/UI level. Hakobi should not compete on:

- Number of container runtimes supported
- Reimplementing a metrics database
- Reimplementing Kubernetes dashboards
- A prettier Docker Compose editor

The open space is **cross-provider operational context**:

- One service catalog across Docker, Podman, Kubernetes, CI runners, and edge workloads
- Who owns what, where it runs, what changed, and whether it is healthy
- Approval and audit workflows for destructive actions
- Policy-based incident response across providers

## Integration surfaces

| Provider | Preferred API | Required caution |
|---|---|---|
| Docker Engine | `/var/run/docker.sock`, TCP+TLS, SSH context | Socket is root-equivalent |
| Podman | REST API over Unix socket/SSH | Rootless behavior and API differences |
| Kubernetes | Kubernetes HTTP API + metrics add-ons | Read-only RBAC first; do not store kubeconfigs unchanged |
| OrbStack | Docker-compatible endpoint | macOS/local only; no server-side assumption |
| Cloudflare | REST API/GraphQL + Workers APIs | Scoped tokens, per-zone permissions |
| CI systems | Jenkins/GitLab HTTP APIs + Prometheus endpoints | Tokens are high-value; metrics endpoints may be unauthenticated |

## Sources

- Swarmpit site: <https://swarmpit.io/>
- Swarmpit repository: <https://github.com/swarmpit/swarmpit>
- Podman documentation: <https://docs.podman.io/en/stable/index.html>
- Podman Desktop: <https://podman-desktop.io/>
- OrbStack pricing: <https://orbstack.dev/pricing>
- Docker pricing: <https://www.docker.com/pricing/>
- Portainer pricing: <https://www.portainer.io/pricing>
- Kubernetes overview: <https://kubernetes.io/docs/concepts/overview/>
