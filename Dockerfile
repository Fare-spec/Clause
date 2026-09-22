FROM rust:1-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY PRIVACY.md TERMS.md ./
COPY src ./src
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/clause /usr/local/bin/clause
RUN mkdir -p /app/data /app/guilds && chown -R 10001:10001 /app
USER 10001:10001
ENTRYPOINT ["/usr/local/bin/clause"]
