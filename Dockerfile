FROM rust:1-bookworm@sha256:9181b56c2a21ab1d5e73cffe04fc762426d30aa7fac3c91141dd033f49a7fc8b AS chef
RUN cargo install cargo-chef --locked
WORKDIR /build

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --locked --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY static ./static
RUN cargo build --release --locked

FROM cloudflare/cloudflared:2026.10.0@sha256:9b49eed8f62806d5d45ddf59ecefb5710429598ea6d3fcccd2af938f621b2b07 AS cloudflared

FROM debian:bookworm-slim@sha256:99c0b25c46d0214d5a2091092e7908907dcaecd6a962cde0ac700acee222b6f3 AS runtime
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 hakobi \
    && useradd --uid 10001 --gid hakobi --no-create-home hakobi \
    && mkdir /data \
    && chown hakobi:hakobi /data
COPY --from=build /build/target/release/hakobi /usr/local/bin/hakobi
COPY --from=build /build/target/release/hakobi-agent /usr/local/bin/hakobi-agent
COPY --from=cloudflared /usr/local/bin/cloudflared /usr/local/bin/cloudflared
ENV HAKOBI_DATA_DIR=/data \
    PORT=3000 \
    RUST_LOG=hakobi=info,tower_http=info
USER 10001:10001
VOLUME ["/data"]
EXPOSE 3000
ENTRYPOINT ["/usr/local/bin/hakobi"]
