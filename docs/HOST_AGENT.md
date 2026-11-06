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
