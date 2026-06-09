FROM rust:1.88-bookworm AS builder

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY crates ./crates

RUN cargo build --manifest-path crates/shacs-cli/Cargo.toml --release --locked

FROM node:24-bookworm-slim AS node

FROM python:3.14-slim-bookworm AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 1000 --shell /bin/bash shacs \
    && mkdir -p /home/shacs/.shacs-bot \
    && chown -R shacs:shacs /home/shacs

COPY --from=node /usr/local/bin/ /usr/local/bin/
COPY --from=node /usr/local/lib/node_modules /usr/local/lib/node_modules

COPY --from=builder /app/crates/shacs-cli/target/release/shacs-bot /usr/local/bin/shacs-bot

USER shacs
ENV HOME=/home/shacs
ENV SHACS_RUNTIME_PACKAGE=shacs-bot-official-container
WORKDIR /home/shacs

EXPOSE 18790 8900 8765

ENTRYPOINT ["shacs-bot"]
CMD ["status"]
