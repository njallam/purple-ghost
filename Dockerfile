FROM rust:1.69 AS builder
WORKDIR /usr/src

RUN cargo new purple-ghost
WORKDIR /usr/src/purple-ghost
COPY Cargo.toml Cargo.lock ./
RUN cargo build --release

COPY src ./src
RUN cargo install --path .

FROM gcr.io/distroless/cc
COPY --from=builder /usr/local/cargo/bin/purple-ghost .
CMD ["./purple-ghost"]