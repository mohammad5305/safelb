FROM rust:1.77.2-slim-buster AS chef
RUN cargo install cargo-chef 
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder 
COPY --from=planner /app/recipe.json recipe.json

# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --recipe-path recipe.json

COPY . .
RUN cargo build --release 

FROM debian:bookworm-slim

RUN apt update && apt -y install tcpdump

COPY --from=builder /app/target/release/safelb /usr/local/bin

EXPOSE 8080

ENTRYPOINT ["/usr/local/bin/safelb"]

