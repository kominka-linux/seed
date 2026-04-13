# Build a statically linked seed binary and bundle it into a minimal scratch image.
#
# Usage:
#   docker build -t seed .
#   docker run --rm seed wget -qO- https://example.com/
#
# The binary is linked against musl so it has no shared library dependencies.
# Mozilla's root CAs are compiled into the binary via webpki-roots, so no
# CA bundle needs to be mounted at runtime. You can still override with
# --ca-certificate or SSL_CERT_FILE if you need custom/private CAs.

FROM rust:alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /build
COPY . .
RUN cargo build --target x86_64-unknown-linux-musl

FROM scratch
COPY --from=builder /build/target/x86_64-unknown-linux-musl/debug/seed /seed
ENTRYPOINT ["/seed"]
