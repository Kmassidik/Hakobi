# Hakobi CP3: multi-host agent registry

## Context

CP2 added a single local host provider through `HAKOBI_AGENT_URL` and
`HAKOBI_AGENT_TOKEN`. That works for one machine, but it cannot represent a VPS
fleet. The dashboard must manage multiple hosts without restarting the app
container or editing its environment.

The existing host agent and typed Docker protocol are in place. Preserve them.

## Goal

Add a catalog-backed host registry and let an authenticated operator add, test,
inspect, and remove remote hosts from the UI. Each enrolled host gets its own
provider row and its own encrypted agent credentials.

## Required behavior

### 1. Catalog

- Add a schema migration for a `host_agents` table:
  - id, name, agent URL, encrypted token, enabled, last_checked_at, last_error,
    created_at.
- Use the existing catalog encryption facilities for tokens.
- Never return, render, log, or commit a decrypted token.
- Provider endpoint mapping: `agent://{host_id}`.
- Creating a host atomically creates its provider row with kind `docker` and
  name derived from the host name.
- Deleting a host removes or disables its provider consistently.
- Migration must be idempotent and preserve existing CP2 provider rows.

### 2. Backend resolution

- Replace the process-wide agent endpoint with per-provider resolution.
- `agent://{host_id}` must resolve to the stored agent URL and decrypted token.
- Preserve `agent://local` for the existing local env-configured host as a
  backward-compatible alias, or migrate it cleanly.
- `RoutingBackend` must resolve each provider request independently.
- Keep direct `unix://` and `tcp://` Docker providers working.
- Do not hold a catalog connection across `.await` points.

### 3. HTTP UI

Add authenticated host-management routes and pages:

```text
GET    /hosts
POST   /hosts                    create
POST   /hosts/{id}/test          test agent + Docker
POST   /hosts/{id}/toggle        enable/disable
POST   /hosts/{id}/delete        delete
```

The page must show name, agent URL without token, enabled state, last check,
last error, and the mapped provider. CSRF and origin checks are mandatory.

- The create form takes name, agent URL, and token.
- Validate URL: `http` or `https`, host present, no userinfo, no query/fragment,
  reasonable length.
- Token minimum length follows `host_agent::MIN_TOKEN_LEN`.
- Reject duplicate host names and duplicate agent URLs.
- Test action must call `/v1/readiness` and `/v1/docker/version` and report only
  safe errors.

### 4. Overview integration

- Multi-host metrics aggregate all enabled providers.
- Service/stack keys already include provider IDs; keep that invariant.
- Provider failure state remains visible.
- Containers page must group by provider and host.

### 5. Tests

Do not require a live Docker daemon. Add tests for:

- migration and encrypted token storage;
- host create → provider create atomicity;
- duplicate host/name/URL rejection;
- URL and token validation;
- per-provider endpoint resolution;
- delete/toggle consistency;
- token never appears in rendered host HTML or errors;
- two fake agent servers can be listed and tested independently.

## Constraints

- English only.
- Do not deploy, push, commit, or modify the live host.
- No secrets in fixtures, logs, HTML, or commit text.
- No arbitrary command endpoint.
- Preserve all existing tests and the injected `router_with_backend` path.
- Do not mount the Docker socket into the app container.
- Keep the UI consistent with the existing Apple-style dashboard.

## Acceptance

- An operator can register two remote agents through `/hosts`; each appears as a
  provider and can be tested independently.
- Overview/containers aggregate data from both hosts.
- Disabling or deleting a host removes its data from active aggregation.
- Existing local provider behavior remains compatible.
- `cargo test`, `cargo clippy --all-targets -- -D warnings`, and
  `cargo fmt --check` pass.
