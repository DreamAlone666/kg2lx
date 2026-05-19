# syntax=docker/dockerfile:1.7

FROM node:22-bookworm-slim AS web-builder

ENV PNPM_HOME=/pnpm
ENV PATH=${PNPM_HOME}:${PATH}

RUN corepack enable

WORKDIR /src/web

COPY web/pnpm-lock.yaml ./
RUN pnpm fetch --frozen-lockfile

COPY web/package.json ./
RUN pnpm install --frozen-lockfile --offline

COPY web/ ./
RUN pnpm build


FROM rust:bookworm AS server-builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src/server

COPY server/Cargo.toml server/Cargo.lock ./
RUN mkdir src \
    && printf 'fn main() {}\n' > src/main.rs \
    && cargo build --locked --release \
    && rm -rf src

COPY server/src ./src
RUN touch src/main.rs && cargo build --locked --release


FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --home-dir /app --shell /usr/sbin/nologin --uid 10001 app

WORKDIR /app

ENV LISTEN_ADDR=0.0.0.0:8787
ENV DATA_DIR=/data
ENV WEB_DIST_DIR=/app/web

COPY --from=server-builder /src/server/target/release/kg2lx-server /usr/local/bin/kg2lx-server
COPY --from=web-builder /src/web/build /app/web

RUN mkdir -p /data && chown -R app:app /app /data

USER app

VOLUME ["/data"]

EXPOSE 8787

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["curl", "-fsS", "http://127.0.0.1:8787/healthz"]

CMD ["kg2lx-server"]
