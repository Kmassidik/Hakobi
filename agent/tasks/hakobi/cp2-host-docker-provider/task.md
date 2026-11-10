# Hakobi CP2: host-backed local Docker provider

## Context

Hakobi runs as an unprivileged container. A native `hakobi-agent` already exposes
host readiness, host installers, Docker aggregate metrics, and install job
status over a localhost bearer-token API.

The dashboard still reads container/service inventory from catalog providers.
The catalog currently has no provider row, and the dashboard container has no
Docker socket. Consequently the overview reports unknown service data even when
Docker is running workloads on the host.

Implement the missing **agent-backed local Docker provider** so Hakobi can see
and operate the host Docker engine without mounting `/var/run/docker.sock` into
the dashboard container.

## Goal

When the host agent is configured, Hakobi must automatically have a local Docker
provider that is backed by typed host-agent endpoints. With a working Docker
daemon on the host, the overview, Containers page, and provider test must show
real data without a manual provider setup.

## Required design

### 1. Shared protocol types

Add one shared Rust module for host-agent Docker requests and responses. Reuse
it from both binaries. Do not duplicate DTOs in the agent binary and the web
crate.

Minimum DTOs:

- container row: id, name, image, state, status, health, compose project,
  compose service, ports
- port: host IP, host port, container port, protocol
- Docker snapshot: host info, images, volumes, networks
- run request/result

Use serde. Keep unknown fields forward-compatible where practical.

### 2. Typed agent Docker endpoints

Add authenticated endpoints to `hakobi-agent`:

```text
GET  /v1/docker/version
GET  /v1/docker/containers
GET  /v1/docker/snapshot
POST /v1/docker/containers/{name}/start
POST /v1/docker/containers/{name}/stop
POST /v1/docker/containers/{name}/restart
POST /v1/docker/containers/{name}/delete
POST /v1/docker/containers/run
```

The agent must call Docker through the existing `DockerBackend` (bollard/API),
not through a shell. Fixed endpoint: `unix:///var/run/docker.sock`.

Security requirements:

- Existing bearer-token authentication and constant-time comparison stay.
- Validate container names with the existing validation helper.
- Accept only the fixed Docker endpoint; no user-supplied endpoint.
- `run` accepts only image, name, and container port; validate image and name.
- No raw Docker API proxy and no arbitrary command endpoint.
- Bound response sizes and use existing timeout patterns.
- Map Docker errors to safe error JSON; do not leak secrets.

### 3. Agent-backed `ContainerBackend`

Add an app-side backend that implements the existing `ContainerBackend` trait
and calls the typed agent endpoints. It must support the full trait surface:

```text
test
containers
snapshot
start_container
stop_container
restart_container
remove_container
run_container
```

Add a routing backend that selects:

```text
agent://local       -> agent backend
everything else     -> existing DockerBackend
```

Do not change the behavior of `router_with_backend` tests that inject a fake
backend.

### 4. Automatic local provider registration

At application startup, before serving:

1. If `HAKOBI_AGENT_URL` and a valid token are present, ensure a provider exists
   with endpoint `agent://local`, name `Local host`, kind `docker`, enabled.
2. Else, if the native Docker socket is accessible, ensure a provider exists
   with endpoint `unix:///var/run/docker.sock`.
3. Do not duplicate providers, overwrite existing providers, re-enable a
   deliberately disabled provider, or create a provider when neither path is
   available.
4. Keep startup idempotent.

Use the catalog API. Do not write provider rows from request handlers.

### 5. Tests

Add tests that do not require a live host Docker daemon:

- protocol round-trip or serialization tests;
- routing backend dispatch by endpoint scheme;
- local-provider registration is idempotent;
- existing provider rows are preserved;
- no provider is registered when neither agent nor socket is available;
- agent auth remains required for all Docker endpoints.

Use a fake agent HTTP server or injectable client if needed. Do not shell out to
Docker in tests.

## Constraints

- English only for code, comments, docs, and commit text.
- Do not mount the Docker socket into the dashboard container.
- Do not add an arbitrary command endpoint.
- Do not modify `AGENTS.md`, `docs/WORKING_AGREEMENT.md`, or the server host.
- Keep the existing SSR UI and tests working.
- No network calls except tests against a local fake server.
- Do not deploy, commit, or push.

## Required files / likely touch points

- `src/bin/hakobi-agent.rs`
- `src/providers/agent.rs` or equivalent
- `src/providers/mod.rs`
- `src/host_agent.rs` or equivalent shared protocol module
- `src/web/agent_client.rs` (move/share client as needed)
- `src/web/mod.rs`
- `src/cli/mod.rs`
- `src/catalog.rs`
- `Cargo.toml` / `Cargo.lock` only if required

## Acceptance criteria

- `cargo test` passes.
- `cargo clippy --all-targets -- -D warnings` passes.
- `cargo fmt --check` passes.
- With `HAKOBI_AGENT_*` configured and host Docker available, a local provider is
  registered automatically and the overview can receive non-empty container
  data.
- Provider test action succeeds through the agent backend.
- Without agent/socket, registration does nothing and existing behavior remains.
- No Docker socket is mounted into the app container.
- No secrets, arbitrary shell execution, or public agent binding are introduced.
