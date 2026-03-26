FROM rust:1.85 AS builder

RUN apt-get update && apt-get install -y clang libclang-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Cache dependencies
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm -rf src

COPY src ./src
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y libgcc-s1 ca-certificates libstdc++6 && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/blog ./blog
COPY posts ./posts
COPY static ./static

EXPOSE 8080
CMD ["./blog"]
