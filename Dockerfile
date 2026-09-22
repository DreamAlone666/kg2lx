# syntax=docker/dockerfile:1.7

FROM node:22-bookworm-slim AS web-builder

ENV PNPM_HOME=/pnpm
ENV PATH=${PNPM_HOME}:${PATH}

RUN corepack enable

WORKDIR /src/web

COPY web/pnpm-lock.yaml ./
# 刻意不带 `--frozen-lockfile`。pnpm 12 把这个参数从 `fetch` 里删掉了（`install` 仍保留），
# 而这份 Dockerfile 只 `corepack enable`、从不固定 pnpm 版本，corepack 挑中的因此是「它当时
# 认为的最新版」：2026-09 它挑中 12.5.1，构建在这一步以
# `error: unexpected argument '--frozen-lockfile' found` 退出，kg2lx 镜像与 Release 都没产出。
#
# 删参数而不是固定 pnpm 版本，是因为在这个位置这个参数是**装饰性的**：此刻镜像里只有
# lockfile、还没有 package.json，没有可解析的对象，`frozen` 无从生效；lockfile 真的缺失时
# 上一行 COPY 就已经失败了。真正保证「package.json 与 lockfile 一致」的是下一步的
# `install --frozen-lockfile --offline`，而那个参数 11 与 12 都还在。
#
# 证据（两侧各完整跑一遍 fetch、`install --frozen-lockfile --offline`（全新 store）、`run build`）：
# pnpm 11.26.0 与 12.5.1 均通过，且 lockfile 保持 lockfileVersion 9.0 未被改写。
# 真机 docker 构建未验证。
#
# 代价与失败方向：不固定版本 ⇒ 容器里的 pnpm 跟着 corepack 的默认值走，下次生态再改命令会再断
# 一次——但它以**构建失败**的形式暴露，不会静默产出旧产物。将来若要固定，固定到当时的当前
# major，并同时确认两端都能读同一份 lockfile。曾考虑 `npm install -g pnpm@11.26.0`，未采用：
# 那等于把一个 12 已删掉的参数连同旧 major 一起冻住。
RUN pnpm fetch

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
