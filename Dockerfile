# One-command Port Mortem artifact: build the Rust port and run smoke tests.
FROM rust:1.83-bookworm

WORKDIR /app

COPY Cargo.toml ./
COPY src ./src
COPY tests/port ./tests/port
COPY README.md DECISIONS.md .port-mortem.toml ./

RUN cargo test --release

CMD ["cargo", "test", "--release"]
