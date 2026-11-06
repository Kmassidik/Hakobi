# Hakobi 運び

Self-hosted DevOps control plane.

Hakobi (運び, “to carry/deliver”) currently provides secure first-run setup, a Cloudflare Tunnel, an authenticated server-rendered dashboard, encrypted provider connections, and a read-only Docker container inventory. Image builds, deployments, telemetry, and logs are roadmap items, not implemented features.

## Features

- Interactive username, password, base URL, and Cloudflare setup
- Automatic Cloudflare Tunnel, ingress, and proxied CNAME creation
- Outbound-only connectivity; no public IP or open inbound ports required
- Rust Axum + Maud SSR interface
- Tailwind CSS, Font Awesome solid icons, and locally vendored HTMX
- Argon2id passwords, opaque sessions, CSRF protection, and login throttling
- Encrypted provider credentials with enable, test, and delete controls
- Read-only Docker inventory over Unix socket or TCP endpoints
- Host, container, image, volume, and network visibility
- Nix development environment and non-root Docker deployment
- Explicit loopback-only local mode

## Research

Product and architecture research lives in [`knowledge-base/`](./knowledge-base/README.md), including container platforms, CI activity monitoring, observability, bot mitigation, and Cloudflare Workers/Pages.

## Agent harness

Hakobi includes a Level 1–4 coding-agent harness in [`agent/`](./agent/README.md). It is not a chatbot:

- isolated snapshot/worktree workspace
- external `task.md` and `state.json` memory
- file and command walls
- independent tests, diff-scope checks, and failure feedback
- retry limits and git checkpoint commits
- human approval before pushing or opening a PR

Run the failing-bug example:

```sh
python3 agent/agentctl run --task agent/tasks/fix-bug/task.json
```

It intentionally fails once, receives specific `EXPECTED/SAW/FENCE/DEAD_ENDS` feedback, fixes itself, passes tests, and stops at the human PR gate. Level 5–7 graph/learning features are not implemented yet.

To run ordered Hakobi product tasks through the same loop and apply passing patches to the working tree:

```sh
python3 agent/agentctl run-plan --plan agent/plans/hakobi-cp1-catalog.json
```

Plan mode stops before applying any task that fails its checks. It never commits, pushes, or opens a PR.

## Requirements

Public setup requires:

1. A domain using Cloudflare DNS.
2. Its Cloudflare **account ID** and **zone ID**.
3. A scoped API token with:
   - **Account → Cloudflare Tunnel → Edit**
   - **Zone → DNS → Edit** for the target zone
4. Outbound access to Cloudflare. UDP port 7844 is preferred; HTTP/2 fallback is supported.

Use an HTTPS base URL such as `https://hakobi.example.com`, without a path or custom port. A first-level subdomain works with Cloudflare’s standard certificate; deeper hostnames may require an Advanced Certificate.

The API token is used once and is **not saved**. Setup stores the tunnel credential instead. It refuses to overwrite an existing configuration or DNS record.

## Quick start: Docker

From this repository:

```sh
# 1. Run the interactive setup
docker compose run --rm hakobi setup

# 2. Start Hakobi in the background
docker compose up -d

# 3. Follow startup logs if needed
docker compose logs -f
```

Open the configured HTTPS URL and sign in with the username and password created during setup.

No host port is published. The app and `cloudflared` run together in a non-root container. Settings persist in the `hakobi-data` volume.

## Connect Docker

After signing in, open **Providers → Add Docker provider** and use:

```text
Name:      local
Endpoint:  unix:///var/run/docker.sock
```

Click **Test** to verify the connection, then open **Containers** for the live read-only inventory. The same form accepts an explicitly reachable TCP endpoint such as `tcp://127.0.0.1:2375`.

The native binary reads the Docker socket directly. The default container does **not** mount the host Docker socket. Mounting `/var/run/docker.sock` grants host root-equivalent access, so enable it only deliberately:

```yaml
services:
  hakobi:
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock:ro
      - hakobi-data:/data
```

Docker socket permissions are not a security boundary against a compromised process; use a dedicated Docker context or TCP TLS endpoint when the trust boundary matters.

## Quick start: Nix

Run the complete first-run flow:

```sh
nix run path:.
```

The CLI provisions Cloudflare, starts the server and tunnel, and prints the public URL. Running the same command again skips setup and starts the existing configuration.

## CLI reference

```text
hakobi                     Set up if needed, then run
hakobi setup               Run public Cloudflare setup once
hakobi setup --local       Configure loopback-only local development
hakobi run                 Run an existing configuration
hakobi passwd              Change the dashboard password
hakobi --data-dir PATH …   Use a specific configuration directory
hakobi --help              Show all commands
hakobi --version           Show the installed version
```

When using Nix development, prefix commands with `cargo run --`, for example:

```sh
cargo run -- setup --local
cargo run -- run
```

## Routine Docker operations

```sh
docker compose up -d          # Start
docker compose up -d --build  # Rebuild and start after source changes
docker compose restart        # Restart
docker compose logs -f        # Follow logs
docker compose ps             # Show container status
docker compose down           # Stop and remove the container; keep data
```

**Do not run `docker compose down -v` unless you intend to delete Hakobi’s stored configuration and tunnel credential.**

Sessions last 12 hours and are invalidated by logout or server restart. After repeated failed sign-ins, wait five minutes before trying again.

## Configuration and backups

Hakobi reads `config.toml` from its data directory. Set `HAKOBI_DATA_DIR` or use `--data-dir PATH` to override the platform default:

```sh
hakobi --data-dir /var/lib/hakobi setup
hakobi --data-dir /var/lib/hakobi run
```

On Unix, the data directory is mode `0700` and the configuration is written atomically with mode `0600`. Startup rejects configuration files accessible by group or other users.

Back up the data directory securely. It contains the Argon2id password hash and tunnel token, not the plaintext password or Cloudflare API token. Anyone with the tunnel token can operate that tunnel.

The HTTP listener is always loopback-only and uses `PORT` (default `3000`). Keep `PORT` stable after public setup because Cloudflare’s ingress is configured with that port; only `cloudflared` connects to it in public mode.

For Docker, set the same `PORT` for setup and runtime:

```sh
PORT=3100 docker compose run --rm hakobi setup
PORT=3100 docker compose up -d
```

### Changing the password

Use the reset command without recreating the tunnel or losing configuration:

```sh
docker compose run --rm hakobi passwd
docker compose restart
```

Restarting applies the new password and invalidates active sessions.

### Resetting an installation

Setup is intentionally one-time and does not overwrite credentials or DNS settings. To start over:

1. Stop Hakobi.
2. Back up anything you need.
3. Remove the Hakobi-created DNS record and tunnel through Cloudflare.
4. Remove the installation’s data directory or Docker volume.
5. Run setup again.

Deleting only the local configuration does not delete Cloudflare resources.

## Local development

```sh
nix develop path:.
cargo run -- setup --local
cargo run -- run
```

Open <http://127.0.0.1:3000>. To use another port, keep `PORT` consistent between setup and run:

```sh
PORT=3100 cargo run -- setup --local
PORT=3100 cargo run -- run
```

Local mode does not create Cloudflare resources and uses a loopback-only HTTP listener.

### Frontend assets

Frontend development requires Node.js 22 and npm. Browser assets are vendored locally—the app uses no CDN:

```sh
npm ci
npm run build:assets
npm run watch:css
```

`build:assets` compiles Tailwind and copies the pinned HTMX runtime, Font Awesome solid font, CSS, and licenses into `static/assets/`. Generated assets are committed and embedded into the Rust binary. After changing Maud classes or frontend sources, run `npm run build:assets` again.

Font Awesome is intentionally limited to the solid icon set; no external font or script host is allowed by the CSP.

Development checks:

```sh
rustfmt --edition 2021 --check src/main.rs
cargo-clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo audit
cargo deny check
```

## Source layout

```text
src/
├── main.rs               # process entry point
├── cli/                  # interactive setup and server lifecycle
├── config/               # validated models and atomic storage
├── cloudflare/           # API provisioning and tunnel process
├── web/
│   ├── auth/             # passwords, sessions, CSRF, throttling
│   ├── handlers.rs       # HTTP behavior and embedded assets
│   ├── templates.rs      # typed Maud SSR components
│   └── tests.rs          # web security regressions
└── utils/                # small, genuinely shared helpers

frontend/app.css          # Tailwind source and theme
scripts/build-assets.mjs  # vendored HTMX/Font Awesome pipeline
static/assets/            # generated, committed, embedded browser assets
```

Keep each source file below 500 lines and domain logic with its domain. `utils` is only for code shared by multiple domains.

## Security notes

- Session cookies contain random opaque tokens; only SHA-256 token digests remain in memory.
- Cookies are `HttpOnly` and `SameSite=Strict`; HTTPS deployments use `Secure` and `__Host-` names.
- Login and logout use double-submit CSRF tokens and validate non-null Origin headers.
- Responses include CSP, HSTS on HTTPS, anti-framing, MIME-sniffing, referrer, and permissions policies.
- Password hashing has bounded concurrency, and login throttling is applied per client.
- The tunnel token is passed through `TUNNEL_TOKEN`, not process arguments.
- Provisioning failures reconcile created DNS records before tunnel cleanup.
- Dependencies are locked; CI runs tests, Clippy, RustSec advisory checks, and the container build.
