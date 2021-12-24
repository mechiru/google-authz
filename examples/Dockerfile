ARG RUST_VERSION=1
ARG DEBIAN_CODE_NAME=bullseye

FROM rust:${RUST_VERSION}-slim-${DEBIAN_CODE_NAME} AS builder
WORKDIR /workdir
COPY ./ ./
RUN cargo build -p examples --release

FROM debian:${DEBIAN_CODE_NAME}-slim AS runtime
WORKDIR /app
COPY --from=builder /workdir/target/release/tonic /app/tonic
RUN groupadd rust && \
  useradd -g rust rust && \
  chown rust:rust /app
USER rust
ENTRYPOINT ["./tonic"]
