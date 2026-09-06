# ビルド用。実行環境(debian:bookworm-slim)と同じ bookworm を明示して glibc を揃える
FROM rust:1.98-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/fxtwitter-bot /usr/local/bin/fxtwitter-bot
CMD ["fxtwitter-bot"]