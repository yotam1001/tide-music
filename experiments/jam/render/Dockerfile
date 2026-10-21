FROM rust:1-bookworm AS build
WORKDIR /source
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY experiments/jam/Cargo.toml experiments/jam/Cargo.lock experiments/jam/
COPY experiments/jam/src experiments/jam/src
RUN cargo build --manifest-path experiments/jam/Cargo.toml --locked --no-default-features --features relay --release --bin tide-jam-relay

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends nginx-light bash ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home tide
COPY --from=build /source/experiments/jam/target/release/tide-jam-relay /usr/local/bin/tide-jam-relay
COPY experiments/jam/render/start.sh /usr/local/bin/start-relay
RUN chmod 755 /usr/local/bin/start-relay
USER 10001
ENV PORT=10000
EXPOSE 10000
ENTRYPOINT ["/usr/local/bin/start-relay"]
