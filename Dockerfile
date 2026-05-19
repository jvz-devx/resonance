# Stage 1: Generate dependency recipe
FROM rust:1.91-alpine AS chef
RUN apk add --no-cache musl-dev cmake make gcc g++ pkgconf opus-dev
RUN cargo install cargo-chef
WORKDIR /app

# Stage 2: Plan (only depends on Cargo.toml/Cargo.lock)
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo chef prepare --recipe-path recipe.json

# Stage 3: Build dependencies (cached unless deps change)
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
ENV RUSTFLAGS="-C target-feature=-crt-static"
RUN cargo chef cook --release --recipe-path recipe.json

# Stage 4: Build application (only rebuilds on src changes)
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo build --release

# Stage 5: Final runtime
FROM alpine:3.21

LABEL org.opencontainers.image.source=https://github.com/jvz-devx/resonance

RUN apk add --no-cache \
    ca-certificates \
    chromium \
    ffmpeg \
    nodejs \
    opus \
    py3-pip \
    python3 \
  && python3 -m pip install --no-cache-dir --break-system-packages \
    "yt-dlp>=2026.03.17" \
    "yt-dlp-getpot-wpc==1.0.0" \
    "bgutil-ytdlp-pot-provider==1.3.1" \
  && printf '%s\n' '#!/bin/sh' 'exec /usr/bin/chromium --no-sandbox --headless=new "$@"' > /usr/local/bin/chromium-wpc \
  && chmod +x /usr/local/bin/chromium-wpc

# Enable Node.js and remote EJS challenge components for YouTube player JS.
RUN mkdir -p /etc \
  && printf '%s\n' "--js-runtimes node" "--remote-components ejs:github" > /etc/yt-dlp.conf

RUN adduser -D -h /home/resonance resonance \
  && mkdir -p /home/resonance/.cache /tmp/resonance-cache \
  && chown -R resonance:resonance /home/resonance /tmp/resonance-cache

COPY --from=builder /app/target/release/resonance /usr/local/bin/resonance

ENV RUST_LOG=info,resonance=debug
ENV HOME=/home/resonance
ENV XDG_CACHE_HOME=/tmp/resonance-cache

USER resonance

CMD ["resonance"]
