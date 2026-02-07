FROM rust:1.88-bookworm AS builder
RUN apt-get update && apt-get install -y pkg-config libssl-dev cmake && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/firm-remote-mcp /usr/local/bin/
ENV PORT=8080
EXPOSE 8080
CMD ["firm-remote-mcp"]
