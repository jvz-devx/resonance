# Stage 1: Build
FROM rust:1.91-alpine AS builder

RUN apk add --no-cache musl-dev cmake make gcc g++ pkgconf opus-dev

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ src/

# Dynamically link against libopus
ENV RUSTFLAGS="-C target-feature=-crt-static"

RUN cargo build --release

# Stage 2: Runtime
FROM alpine:3.21

RUN apk add --no-cache opus ffmpeg python3 py3-pip ca-certificates \
    && pip3 install --break-system-packages yt-dlp

COPY --from=builder /app/target/release/resonance /usr/local/bin/resonance

ENV RUST_LOG=info,resonance=debug

CMD ["resonance"]
