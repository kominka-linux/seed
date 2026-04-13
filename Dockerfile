# Build a statically linked seed binary and bundle it into a minimal scratch image.
#
# Usage:
#   docker build -t seed .
#   docker run --rm seed wget -q -O - https://example.com/
#
# The binary is linked against musl so it has no shared library dependencies.
# The CA bundle from Alpine is copied in so rustls-platform-verifier can find
# it at /etc/ssl/certs/ca-certificates.crt on Linux.

FROM rust:alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /build
COPY . .
RUN cargo build

FROM alpine AS certs
RUN apk add --no-cache ca-certificates

FROM scratch
COPY --from=builder /build/target/debug/seed /seed
COPY --from=certs /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
ENTRYPOINT ["/seed"]
