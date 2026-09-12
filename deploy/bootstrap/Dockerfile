FROM rust:1.98-bookworm AS builder

RUN apt-get update \
    && apt-get install --yes --no-install-recommends clang lld ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN rustup target add wasm32-unknown-unknown
RUN cargo install wasm-bindgen-cli --version 0.2.122 --locked

WORKDIR /src
COPY . .
RUN bash scripts/build-bootstrap.sh
RUN cargo build --release --locked -p medousa-site-bootstrap

FROM debian:bookworm-slim

WORKDIR /srv
COPY --from=builder /src/target/release/medousa-site-bootstrap /usr/local/bin/medousa-site-bootstrap
COPY --from=builder /src/apps/bootstrap/public /srv/apps/bootstrap/public

USER 65532:65532
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/medousa-site-bootstrap"]
