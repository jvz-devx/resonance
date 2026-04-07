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

# Stage 5: yt-dlp with built-in PO token provider
FROM ghcr.io/jim60105/yt-dlp:pot AS ytdlp

# Stage 6: Final runtime
FROM alpine:3.21

LABEL org.opencontainers.image.source=https://github.com/jvz-devx/resonance

RUN apk add --no-cache opus ffmpeg ca-certificates

# yt-dlp binary + bgutil-pot CLI + plugin files (no Python/Node needed)
COPY --from=ytdlp /usr/bin/yt-dlp /usr/local/bin/yt-dlp
COPY --from=ytdlp /usr/bin/bgutil-pot /usr/local/bin/bgutil-pot
COPY --from=ytdlp /etc/yt-dlp-plugins /etc/yt-dlp-plugins

COPY --from=builder /app/target/release/resonance /usr/local/bin/resonance

ENV RUST_LOG=info,resonance=debug

CMD ["resonance"]
