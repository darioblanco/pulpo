# syntax=docker/dockerfile:1.7

FROM node:22-bookworm-slim AS web-build
WORKDIR /src

COPY web/package.json web/package-lock.json ./web/
RUN cd web && npm ci

COPY web ./web
RUN cd web && npm run build

FROM rust:1.88-bookworm AS rust-build
WORKDIR /src

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY web ./web
COPY contrib ./contrib

# Inject prebuilt web assets so pulpod can embed them.
COPY --from=web-build /src/web/build ./web/build

RUN cargo build --release -p pulpod -p pulpo-cli

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        bash \
        ca-certificates \
        curl \
        git \
        jq \
        openssh-client \
        procps \
        tini \
        tmux \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --uid 10001 --shell /bin/bash pulpo

COPY --from=rust-build /src/target/release/pulpod /usr/local/bin/pulpod
COPY --from=rust-build /src/target/release/pulpo /usr/local/bin/pulpo
COPY docker/base/entrypoint.sh /usr/local/bin/pulpo-entrypoint.sh

RUN chmod +x /usr/local/bin/pulpo-entrypoint.sh

ENV HOME=/home/pulpo
WORKDIR /home/pulpo

EXPOSE 7433

USER pulpo

ENTRYPOINT ["tini", "--", "/usr/local/bin/pulpo-entrypoint.sh"]
CMD ["pulpod"]
