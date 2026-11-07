# Hakobi host agent

The dashboard container is unprivileged. Host operations are performed by
`hakobi-agent`, a separate binary that runs natively on the machine and listens
only on loopback.

## Install

```sh
install -m 0755 hakobi-agent /usr/local/bin/hakobi-agent
install -d -m 0750 /etc/hakobi
openssl rand -hex 32 > /etc/hakobi/agent.token
chmod 0600 /etc/hakobi/agent.token
install -m 0644 packaging/hakobi-agent.service /etc/systemd/system/
systemctl daemon-reload
systemctl enable --now hakobi-agent
```

Configure the dashboard container with the same token:

```sh
HAKOBI_AGENT_URL=http://127.0.0.1:8787
HAKOBI_AGENT_TOKEN=<contents of /etc/hakobi/agent.token>
```

Keep both values in a root-owned env file rather than the shell history.

## Contract

- `GET /v1/readiness` returns the real host Docker, Compose, kubectl, Nix, and Nixpacks status.
- `POST /v1/install/{docker|compose|kubectl|nix|nixpacks}` runs a fixed installer.
- Authentication is `Authorization: Bearer <token>`, compared in constant time.
- The agent listens on `127.0.0.1:8787` by default.
- Request data is never interpolated into a shell command.
- Installers time out after 15 minutes and report bounded output.
- Jobs are asynchronous; `GET /v1/install/status` reports running/success/output.

## Security

The agent is root because package installation requires root. That also means a
compromised agent token is equivalent to host root for the fixed installer set.
Do not expose port 8787 publicly, do not add an arbitrary command endpoint, and
treat the token as a host root credential.

## Docker endpoints

The agent also exposes typed Docker reads and operations for the dashboard's
local provider:

```text
GET  /v1/docker/version
GET  /v1/docker/containers
GET  /v1/docker/snapshot
GET  /v1/docker/metrics
POST /v1/docker/containers/run
POST /v1/docker/containers/{name}/start
POST /v1/docker/containers/{name}/stop
POST /v1/docker/containers/{name}/restart
POST /v1/docker/containers/{name}/delete
```

All routes require the bearer token. The agent talks to Docker through the fixed
`unix:///var/run/docker.sock` endpoint using the API client, never through an
arbitrary shell or a user-supplied endpoint. At startup Hakobi registers a
local provider named `Local host` with endpoint `agent://local` when the agent
is configured. If no agent is configured but the native Docker socket is
present, it registers `unix:///var/run/docker.sock` instead.

## System resource endpoint

`GET /v1/system/metrics` reports host-level CPU (total and per-core), load,
uptime, memory/swap, mounted filesystems, interface throughput, and the top
processes by CPU. It is authenticated like the other endpoints and read-only.
The dashboard uses it for `/hosts/{id}/resources` and the overview CPU/memory
totals.
