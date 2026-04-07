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

# Stage 5: Runtime
FROM alpine:3.21

LABEL org.opencontainers.image.source=https://github.com/jvz-devx/resonance

RUN apk add --no-cache opus ffmpeg python3 py3-pip ca-certificates curl \
    && pip3 install --break-system-packages yt-dlp bgutil-ytdlp-pot-provider \
    && curl -fsSL https://deno.land/install.sh | DENO_INSTALL=/usr/local sh

# Default PO token server URL (override via env var)
ENV POT_SERVER_URL=http://pot-server:4416

# yt-dlp config generated at container start from env var
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

COPY --from=builder /app/target/release/resonance /usr/local/bin/resonance

ENV RUST_LOG=info,resonance=debug

ENTRYPOINT ["entrypoint.sh"]
