# Service Topology UI

Last updated: **2026-11-05**  
Status: **Planned after Checkpoint 2 inventory**

## Goal

Show services as connected nodes so an operator can immediately understand:

- what exists
- which service belongs to which project/environment
- which services share a network or route
- which service depends on which database, runner, or domain
- what is healthy, stale, or disconnected

This is a **service topology map**, not a Canva editor. It should feel spatial and direct without allowing arbitrary diagramming.

## Visual concept

```text
┌──────────────────────────────────────────────────────────┐
│ Services / Topology            [Environment] [Search]    │
├──────────────────────────────────────────────────────────┤
│                                                          │
│   ┌────────────┐       ┌────────────┐                    │
│   │ API        │──────▶│ PostgreSQL │                    │
│   │ healthy    │       │ healthy    │                    │
│   └─────┬──────┘       └────────────┘                    │
│         │                                                │
│         ▼                                                │
│   ┌────────────┐       ┌────────────┐                    │
│   │ Worker A   │──────▶│ Redis      │                    │
│   │ degraded   │       │ unknown    │                    │
│   └────────────┘       └────────────┘                    │
│                                                          │
│   [Fit] [Zoom -] [Zoom +] [List view]                    │
└──────────────────────────────────────────────────────────┘
```

## Node types

- Application service
- Compose stack
- Container
- Database
- Cache
- Job/worker
- CI runner
- Domain/route
- External provider endpoint

Each node displays:

- Name and service type
- Environment/project
- Health status
- Source freshness
- Optional icon
- Provider badge

No node is rendered without a catalog record and source.

## Edge types

Edges must show **why** they exist:

| Edge | Meaning | Source |
|---|---|---|
| Network | Shared Docker/Kubernetes network | Provider metadata |
| Route | Domain or reverse proxy points to service | Cloudflare/proxy |
| Deploy | Service came from a build/release | CI/CD records |
| Observed | Request traffic observed between services | Traces later |
| Manual | Operator-declared dependency | Catalog |

Never infer a runtime dependency from network adjacency without labeling it as network adjacency.

## Layout

- Default: automatic deterministic layout grouped by project/environment.
- Manual node positions are persisted per user/project.
- Pan, zoom, and fit-to-screen.
- Search highlights a node and its direct neighbors.
- Filter by environment, provider, health, and workload type.
- Minimap after the graph grows beyond the viewport.
- Collapse groups when a project contains many services.
- A list/table view remains available for accessibility and large environments.

## Interaction

Initial read-only:

- Click a node to open a details panel.
- Hover highlights direct edges.
- Click an edge to see its evidence and source.
- Open provider/resource deep link.
- Copy shareable view URL with filters and selected node.
- Empty state when no provider is connected.

After the action layer exists:

- Drag node positions.
- Pin/unpin automatic layout.
- Create manual dependency edges with approval.
- Add notes/ownership labels.
- Export PNG/SVG.
- Full-screen mode.

Do not allow arbitrary text, unescaped HTML, or user-defined node rendering.

## Recommended implementation

Use a small isolated browser module, not a full SPA:

- Server/Maud renders the topology page, node data, and initial graph.
- Rust computes filtered graph data and authorization scope.
- HTML/SVG edges render on the page.
- One focused vanilla JS module handles pan, zoom, selection, and drag.
- Node positions are sent back through an authenticated, CSRF-protected endpoint.
- Tailwind styles the nodes/panels; Font Awesome supplies icons.

For very large graphs, evaluate Cytoscape.js or a canvas renderer. Do not add it before the service count warrants it.

## Security and correctness

- Filter nodes and edges on the server by authorization.
- Never send hidden service data to the browser.
- Escape all labels; no raw HTML from provider metadata.
- Do not expose secrets, environment values, or connection strings.
- Edge evidence must identify its source and freshness.
- Distinguish unknown from healthy.
- Graph positions are presentation data, not source of truth.
- Manual edges require audit entries.
- Allowlisted provider URLs only.

## Accessibility

- Keyboard move/select through nodes.
- Visible focus.
- List view for screen readers.
- Status communicated by text as well as color.
- Reduced-motion support.
- Zoom controls in addition to wheel/pinch.

## Delivery phases

### Topology A — read-only graph

After Checkpoint 2:

- Nodes from Docker/Podman/Compose inventory
- Network edges from provider metadata
- Route edges from domain configuration
- Automatic layout
- Click-to-inspect
- List fallback

### Topology B — interactive canvas

After Checkpoint 5:

- Pan/zoom/drag
- Persisted positions
- Search and filters
- Minimap
- Observed trace edges
- Health updates

### Topology C — editing

After safe operations:

- Manual dependencies
- Grouping
- Annotations
- Export and sharing

## PRD placement

- Main navigation: add **Topology**.
- Checkpoint 2: read-only graph from real inventory.
- Checkpoint 5: observed traffic edges from traces.
- Checkpoint 7: manual topology editing only if it affects operations safely.

## Opinion

Yes, this is worth building because it explains the system faster than tables. But it must be a **generated operational graph with evidence**, not a decorative node board. Fake A/B/C/D nodes would undermine trust.

Start with a read-only graph after Docker/Podman inventory. Add drag/zoom as an isolated interaction module, not a frontend rewrite.
