# syntax=docker/dockerfile:1.10
FROM node:24.18.0-bookworm-slim AS console
WORKDIR /src/apps/console
COPY apps/console/package.json apps/console/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci
COPY apps/console/ ./
RUN npm run build

FROM rust:1.96.1-bookworm AS backend
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY migrations ./migrations
COPY lean ./lean
COPY crates ./crates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --locked --release -p heterocloud-api -p heterocloud-cli -p heterocloud-worker \
    && cp target/release/heterocloud-api /tmp/heterocloud-api \
    && cp target/release/heterocloud /tmp/heterocloud \
    && cp target/release/heterocloud-worker /tmp/heterocloud-worker

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /nonexistent --shell /usr/sbin/nologin heterocloud
COPY --from=backend /tmp/heterocloud-api /usr/local/bin/heterocloud-api
COPY --from=backend /tmp/heterocloud /usr/local/bin/heterocloud
COPY --from=backend /tmp/heterocloud-worker /usr/local/bin/heterocloud-worker
COPY --from=console /src/apps/console/dist /opt/heterocloud/console
USER 10001:10001
EXPOSE 8080 8443
ENTRYPOINT ["/usr/local/bin/heterocloud-api"]
