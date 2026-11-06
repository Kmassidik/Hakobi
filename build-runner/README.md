# Hakobi build runner

A standalone, rootless-capable image build service. It is intentionally **not**
integrated into the Rust control plane: Hakobi (or any other caller) talks to it
over HTTP, and it can be run, restarted, or revoked independently.

The service has no runtime dependencies beyond Python 3.12's standard library,
`git`, a Docker CLI/daemon, and the `nixpacks` binary for `/build/nixpacks`.
The HTTP layer is `http.server` on purpose: a
runner that can execute build workloads should not also need a pip dependency
tree, and the API surface is small enough that FastAPI would add supply-chain
surface without simplifying it. It clones a repository into a fresh temporary
directory and runs `docker build` or `nixpacks build`. There is no endpoint that
accepts a command, arguments, environment variables, or a Dockerfile body.

```
build-runner/
├── build_runner/       # HTTP service, validation, subprocess execution
├── tests/              # stdlib unittest suite (no network, no real builds)
├── Dockerfile          # runner image with git, docker CLI, nixpacks
├── .dockerignore
└── README.md
```

## API

All build endpoints require `Authorization: Bearer <BUILD_RUNNER_TOKEN>` when a
token is configured. `GET /healthz` is always unauthenticated and returns only
liveness information. Query parameters are rejected.

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/healthz` | `{"status":"ok","version":"0.1.0"}` |
| `POST` | `/build/dockerfile` | Clone and `docker build` |
| `POST` | `/build/nixpacks` | Clone and `nixpacks build` |
| `POST` | `/build/compose` | Returns `501 not_implemented` |

### `POST /build/dockerfile`

```json
{
  "git_url": "https://github.com/acme/app.git",
  "branch": "main",
  "dockerfile_path": "Dockerfile",
  "context_dir": ".",
  "image_tag": "acme/app:latest",
  "token": "optional-https-token"
}
```

`dockerfile_path` and `context_dir` are optional and default to `Dockerfile`
and `.`. `token` is optional and only valid for `https://` remotes. It is sent
as HTTP Basic auth with the username `x-access-token` (the GitHub convention).
Providers that require a different username should use an `ssh://` remote with
a mounted SSH agent/known_hosts instead.

```sh
curl --fail-with-body -sS \
  -X POST http://127.0.0.1:8787/build/dockerfile \
  -H "Authorization: Bearer $BUILD_RUNNER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "git_url": "https://github.com/acme/app.git",
    "branch": "main",
    "dockerfile_path": "Dockerfile",
    "context_dir": ".",
    "image_tag": "acme/app:latest"
  }'
```

### `POST /build/nixpacks`

```json
{
  "git_url": "https://github.com/acme/app.git",
  "branch": "main",
  "image_tag": "acme/app:nixpacks"
}
```

The repository root is the build context. `dockerfile_path` and `context_dir`
are rejected for this endpoint rather than silently ignored.

### Responses

Builds are synchronous. A request returns after the build finishes, so the
caller's HTTP timeout should exceed `BUILD_RUNNER_BUILD_TIMEOUT` (default 30
minutes). `200` means the build ran to completion; inspect `status`.

```json
{
  "status": "succeeded",
  "image_tag": "acme/app:latest",
  "exit_code": 0,
  "duration_ms": 48231,
  "truncated": false,
  "log_tail": "[git clone]\n...\n[build]\n...",
  "detail": null
}
```

`status` is one of `succeeded`, `failed`, `timed_out`, or `error`. `log_tail` is
the last `BUILD_RUNNER_RESPONSE_TAIL_BYTES` of clone/build output with ANSI
escapes removed and credentials redacted. `truncated` is true when output was
cut. `error` (for example, the `docker` binary is missing) maps to HTTP `500`;
a failed build still returns `200` so its log tail can be read.

Error bodies are uniform:

```json
{"error":{"code":"validation_error","message":"dockerfile_path: must stay inside the repository"}}
```

| Status | Meaning |
| --- | --- |
| `400` | Malformed JSON, missing/unknown field, or invalid URL/branch/path/tag |
| `401` | Missing or wrong bearer token |
| `404` | Unknown path |
| `405` | Wrong method (`Allow` header included) |
| `408` | Request body was not received before the socket timeout |
| `411` | Missing `Content-Length` / chunked body |
| `413` | Body larger than `BUILD_RUNNER_MAX_REQUEST_BYTES` |
| `415` | Body is not `application/json` |
| `429` | All build slots are busy (`Retry-After: 5`) |
| `501` | Compose builds are not implemented |

## Security model

- **No shell.** `git`, `docker`, and `nixpacks` run through `subprocess.Popen`
  with argv arrays, `shell=False`, and `stdin=DEVNULL`. Each command gets its own
  process group and is `SIGTERM`/`SIGKILL`-ed on timeout.
- **Typed inputs only.** Git URLs must be `https://`, `ssh://`, or
  `user@host:path`; embedded passwords, query strings, fragments, `file://`,
  `git://`, and `ext::` are rejected. Branches follow `git check-ref-format`
  rules. Image references must be valid for `docker build -t`. Repository paths
  must be relative, `..`-free, and are re-resolved with symlinks after clone, so
  a repository cannot point `Dockerfile`/`context_dir` outside its checkout.
- **Credential handling.** An HTTPS token is supplied to Git through
  `GIT_CONFIG_KEY_0`/`GIT_CONFIG_VALUE_0` scoped to the remote's origin, never as
  an argument. The token and its Basic encoding are redacted from the returned
  log tail and all runner log lines. The build process does not receive the
  token. Tokens are rejected for SSH remotes; use an SSH agent/known_hosts.
- **Scrubbed environment.** Child processes inherit only a small allow-list
  (`PATH`, `HOME`, Docker client variables, `SSH_AUTH_SOCK`, TLS cert paths)
  plus fixed Git/Docker settings. Application secrets in the runner's
  environment are not visible to `git` or the build CLI.
- **Bounded work.** Clone and build have separate wall-clock timeouts, captured
  output is capped, and request bodies are capped. A semaphore limits concurrent
  builds (default one); excess requests get `429`.
- **Fresh workspaces.** Every request gets a new `0700` directory under
  `BUILD_RUNNER_WORK_DIR`; it is recursively removed in a `finally` block even
  when validation, cloning, or building fails.
- **No general command execution.** Paths and endpoints are fixed. There is no
  shell, exec, compose-up, push, or secret endpoint.

The runner is a build service, not a sandbox for untrusted code. Dockerfiles
and Nixpacks plans execute build steps inside the rootless Docker daemon and can
consume CPU, memory, disk, and network. Use a dedicated rootless daemon, keep
this service on a private network, and terminate TLS (or use mTLS) in front of
it if it is not loopback-only. The bearer token is the only built-in
authentication. `BUILD_RUNNER_ALLOW_UNAUTHENTICATED=1` is accepted only when
binding a loopback address.

## Configuration

| Variable | Default | Notes |
| --- | --- | --- |
| `BUILD_RUNNER_HOST` | `127.0.0.1` | Use `0.0.0.0` inside a container |
| `BUILD_RUNNER_PORT` | `8787` | `0` picks an ephemeral port (useful in tests) |
| `BUILD_RUNNER_TOKEN` | none | Required unless unauthenticated loopback mode; min 16 chars |
| `BUILD_RUNNER_ALLOW_UNAUTHENTICATED` | `false` | Only valid with a loopback bind |
| `BUILD_RUNNER_WORK_DIR` | `/tmp/build-runner` | Per-build directories are created with mode `0700` |
| `BUILD_RUNNER_MAX_CONCURRENT_BUILDS` | `1` | `1`–`64` |
| `BUILD_RUNNER_CLONE_TIMEOUT` | `300` | Seconds, `1`–`3600` |
| `BUILD_RUNNER_BUILD_TIMEOUT` | `1800` | Seconds, `1`–`86400` |
| `BUILD_RUNNER_MAX_OUTPUT_BYTES` | `262144` | Retained output tail per command |
| `BUILD_RUNNER_RESPONSE_TAIL_BYTES` | `65536` | Output returned in JSON |
| `BUILD_RUNNER_MAX_REQUEST_BYTES` | `1048576` | Request body cap |
| `BUILD_RUNNER_ALLOWED_GIT_HOSTS` | empty | Comma-separated hosts; `*.example.com` wildcard |
| `BUILD_RUNNER_DOCKER_BIN` | `docker` | Bare executable name |
| `BUILD_RUNNER_GIT_BIN` | `git` | Bare executable name |
| `BUILD_RUNNER_NIXPACKS_BIN` | `nixpacks` | Bare executable name |
| `BUILD_RUNNER_BUILDX_BUILDER` | none | Use `docker buildx build --builder <name> --load` |
| `BUILD_RUNNER_LOG_LEVEL` | `INFO` | Standard logging level |

The runner never reads the token from the request URL, and it does not log
request headers or bodies.

## Run locally

```sh
cd build-runner
BUILD_RUNNER_TOKEN="$(openssl rand -hex 32)" python3 -m build_runner
```

Then point the Docker CLI at a rootless daemon (below). For development only,
`BUILD_RUNNER_ALLOW_UNAUTHENTICATED=1 BUILD_RUNNER_HOST=127.0.0.1` removes the
bearer requirement.

Run the tests (standard library only; subprocess calls are mocked):

```sh
cd build-runner
python3 -m unittest discover -s tests -t .
```

## Rootless Docker / BuildKit setup

Run the runner against a rootless daemon running as an unprivileged host user.
The runner container only needs the Docker socket; it never starts a daemon.

On a Debian/Ubuntu host:

```sh
sudo apt-get install -y docker-ce-rootless-extras uidmap dbus-user-session \
  slirp4netns fuse-overlayfs
dockerd-rootless-setuptool.sh install
systemctl --user enable --now docker
loginctl enable-linger "$USER"   # keep the rootless daemon available after logout
export DOCKER_HOST="unix:///run/user/$(id -u)/docker.sock"
docker info   # SecurityOptions should include "rootless"
```

`docker build` uses BuildKit through the rootless daemon (`DOCKER_BUILDKIT=1` is
set for children). If you need an explicit BuildKit builder, create one in the
rootless daemon and set `BUILD_RUNNER_BUILDX_BUILDER`:

```sh
docker buildx create --name hakobi --driver docker-container
```

The runner then invokes `docker buildx build --builder hakobi --load ...`.
Builder definitions live under `$HOME/.docker/buildx`, so when using this option
mount that directory into the runner container and set `DOCKER_CONFIG` (or run
the runner directly on the host).

### Build and run the runner container

```sh
cd build-runner
docker build -t hakobi-build-runner .

export RUNNER_TOKEN="$(openssl rand -hex 32)"
export RUNNER_UID="$(id -u)"
mkdir -p /srv/build-runner-work
sudo chown "$(id -u):$(id -g)" /srv/build-runner-work
mkdir -p "$HOME/.ssh"
touch "$HOME/.ssh/known_hosts"   # mount source must exist

docker run -d --name build-runner \
  --restart unless-stopped \
  --user 0:0 \
  -p 127.0.0.1:8787:8787 \
  -e BUILD_RUNNER_TOKEN="$RUNNER_TOKEN" \
  -e DOCKER_HOST="unix:///run/user/$RUNNER_UID/docker.sock" \
  -v "/run/user/$RUNNER_UID/docker.sock:/run/user/$RUNNER_UID/docker.sock" \
  -v /srv/build-runner-work:/work \
  -v "$HOME/.ssh/known_hosts:/home/runner/.ssh/known_hosts:ro" \
  --security-opt no-new-privileges \
  hakobi-build-runner
```

In a rootless daemon, the unprivileged host user is mapped to container UID 0;
that is why the runner is started with `--user 0:0`. It is still the same
unprivileged host user, and `--security-opt no-new-privileges` stays in effect.
The socket and `/work` must be accessible to that mapped user; a host directory
owned by your user works. If you use a **rootful** daemon instead, do not run the
runner as root: use a socket proxy that only permits the build API, and run the
image with its normal `10001` user and the socket proxy's group.

Verify:

```sh
curl -sS http://127.0.0.1:8787/healthz
docker exec build-runner docker info --format '{{.SecurityOptions}}'
```

For SSH remotes, host-key checking is strict (`StrictHostKeyChecking=yes`), so
mount a known_hosts file as above. To use an agent, also mount the socket and
set `SSH_AUTH_SOCK` to the in-container path.

## Operational notes

- Images are left in the rootless daemon after a build. Registry pushes are not
  implemented yet; prune or copy images with your normal daemon tooling.
- Compose is a deliberate `501`; Dockerfile and Nixpacks are the supported
  kinds.
- Clones are shallow (`--depth 1`) and do not recurse submodules. Repositories
  whose Dockerfile needs full history or submodule contents will fail to build;
  commit the needed files or use Nixpacks.
- Workspace cleanup is best-effort. Failures are logged (without credentials)
  and reported in `detail`; inspect `/work` and remove leftovers.
- The HTTP server is single-process with a thread per request. Run one runner
  per trust boundary rather than sharing a token between tenants.
- Build logs are repository-controlled content. They are redacted for the
  configured token and credential-shaped strings, but may still contain
  sensitive values printed by the build itself; protect access to the API and
  its logs.
