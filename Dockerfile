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

LABEL org.opencontainers.image.source=https://github.com/jvz-devx/resonance

RUN apk add --no-cache opus ffmpeg python3 py3-pip ca-certificates \
    && pip3 install --break-system-packages yt-dlp bgutil-ytdlp-pot-provider

# yt-dlp config: use the PO token server sidecar
RUN printf '%s\n' '--extractor-args' 'youtubepot-bgutilhttp:base_url=http://pot-server:4416' \
    > /etc/yt-dlp.conf

COPY --from=builder /app/target/release/resonance /usr/local/bin/resonance

ENV RUST_LOG=info,resonance=debug

CMD ["resonance"]
