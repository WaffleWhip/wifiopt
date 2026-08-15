# syntax=docker/dockerfile:1.7

FROM rust:1.97.1-slim-trixie AS builder
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml ./
RUN mkdir src && echo "fn main(){}" > src/main.rs \
    && cargo build --release \
    && rm -rf src target/release/deps/wifiopt*

COPY src ./src
RUN cargo build --release

FROM debian:trixie-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates cron tzdata \
    && rm -rf /var/lib/apt/lists/*

ENV TZ=Asia/Jakarta
RUN ln -snf /usr/share/zoneinfo/$TZ /etc/localtime \
    && echo $TZ > /etc/timezone

WORKDIR /app
COPY --from=builder /app/target/release/wifiopt /usr/local/bin/wifiopt

RUN mkdir -p /app/log \
    && echo '30 16 * * * cd /app && /usr/local/bin/wifiopt --date $(date -d "yesterday" +\%Y\%m\%d) >> /app/log/cron.log 2>&1' > /etc/cron.d/wifiopt-cron \
    && chmod 0644 /etc/cron.d/wifiopt-cron \
    && crontab /etc/cron.d/wifiopt-cron

COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

ENV RUST_LOG=info
VOLUME ["/app/log"]
ENTRYPOINT ["/entrypoint.sh"]