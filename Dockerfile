FROM rust:1.94-bookworm AS builder

ARG TARGETARCH

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY crates ./crates

# On arm64, gemm-f16 (used by candle for memory search) only compiles with the FP16
# instructions, so the arm64 image needs a CPU that has them (Spec 047).
RUN if [ "$TARGETARCH" = "arm64" ]; then export RUSTFLAGS="-C target-feature=+fp16"; fi; \
    cargo build --manifest-path crates/Cargo.toml -p shacs-cli --release --locked

FROM node:24-bookworm-slim AS node

FROM python:3.14-slim-bookworm AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        chromium \
        curl \
        fonts-noto-cjk \
        libssl3 \
        novnc \
        websockify \
        x11vnc \
        xauth \
        xvfb \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 1000 --shell /bin/bash shacs \
    && mkdir -p /home/shacs/.shacs-bot \
    && chown -R shacs:shacs /home/shacs \
    && chmod 1777 /home/shacs \
    && install -d -m 0755 /etc/chromium/policies/managed

COPY --from=node /usr/local/bin/ /usr/local/bin/
COPY --from=node /usr/local/lib/node_modules /usr/local/lib/node_modules

RUN npm install -g @steipete/summarize \
    && npm cache clean --force

COPY --from=builder /app/crates/target/release/shacs-bot /usr/local/bin/shacs-bot
# The directory above is made first: COPY --chmod would give new parents 0644,
# which the non-root runtime user cannot traverse.
COPY --chown=root:root --chmod=0644 docker/shacs-browser.json /etc/chromium/policies/managed/shacs-browser.json

USER shacs
ENV HOME=/home/shacs
ENV SHACS_RUNTIME_PACKAGE=shacs-bot-official-container
WORKDIR /home/shacs

EXPOSE 18790 8900 8765 6080

ENTRYPOINT ["shacs-bot"]
CMD ["status"]
