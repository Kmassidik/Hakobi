# syntax=docker/dockerfile:1.7

# Pin both images by digest so a rebuild does not silently change the runner.
ARG PYTHON_IMAGE=python:3.12-slim-bookworm@sha256:da54eb86849fe19be7584d8b3f291fcf0c53c196760ab3d70dc6382f681ef9c0
FROM ${PYTHON_IMAGE} AS python-base

# The official CLI image supplies a static docker binary plus the buildx and
# compose plugins.  Only the CLI is copied; the daemon stays on the host.
FROM docker:27-cli@sha256:851f91d241214e7c6db86513b270d58776379aacc5eb9c4a87e5b47115e3065c AS dockercli

FROM python-base AS runner

ARG NIXPACKS_VERSION=v1.41.0
ARG TARGETARCH

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    PIP_NO_CACHE_DIR=1 \
    BUILD_RUNNER_HOST=0.0.0.0 \
    BUILD_RUNNER_PORT=8787 \
    BUILD_RUNNER_WORK_DIR=/work \
    HOME=/home/runner

RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        ca-certificates \
        curl \
        git \
        openssh-client \
        tini \
    && rm -rf /var/lib/apt/lists/*

COPY --from=dockercli /usr/local/bin/docker /usr/local/bin/docker
COPY --from=dockercli /usr/local/libexec/docker/cli-plugins /usr/local/libexec/docker/cli-plugins

# Nixpacks is fetched from its pinned release and verified against a checksum.
RUN set -eux; \
    case "${TARGETARCH:-amd64}" in \
        amd64) np_arch="x86_64-unknown-linux-musl"; np_sha="0f55de7874507b9cf7502113120bd96f2ab6979f78d10eaf2eb2ade9207b3af6" ;; \
        arm64) np_arch="aarch64-unknown-linux-musl"; np_sha="912bd02dd2bb6f9c3a9ed965fe8a68b4aa318dc7a2546e2eca6f2806a894ba39" ;; \
        *) echo "unsupported architecture: ${TARGETARCH}" >&2; exit 1 ;; \
    esac; \
    curl --fail --show-error --silent --location --retry 3 \
        --output /tmp/nixpacks.tgz \
        "https://github.com/railwayapp/nixpacks/releases/download/${NIXPACKS_VERSION}/nixpacks-${NIXPACKS_VERSION}-${np_arch}.tar.gz"; \
    echo "${np_sha}  /tmp/nixpacks.tgz" | sha256sum --check --strict -; \
    tar --extract --gzip --file /tmp/nixpacks.tgz --directory /usr/local/bin nixpacks; \
    chmod 0755 /usr/local/bin/nixpacks; \
    rm -f /tmp/nixpacks.tgz; \
    nixpacks --version

RUN groupadd --gid 10001 runner \
    && useradd --uid 10001 --gid runner --home-dir /home/runner \
        --create-home --shell /usr/sbin/nologin runner \
    && install -d --owner=runner --group=runner --mode=0700 /home/runner/.ssh \
    && install -d --owner=runner --group=runner --mode=0700 /work

WORKDIR /app
COPY build_runner /app/build_runner

USER 10001:10001

EXPOSE 8787

HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD ["python3", "-c", "import os, urllib.request; urllib.request.urlopen('http://127.0.0.1:' + os.environ.get('BUILD_RUNNER_PORT', '8787') + '/healthz', timeout=3).read()"]

STOPSIGNAL SIGTERM
ENTRYPOINT ["tini", "--", "python3", "-m", "build_runner"]
