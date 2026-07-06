ARG COMPOSER_IMAGE=composer:2
ARG RUST_IMAGE=rust:1-bookworm

FROM ${RUST_IMAGE} AS build
WORKDIR /src

COPY Cargo.lock Cargo.toml ./
COPY src ./src

RUN cargo build --release --locked

FROM ${COMPOSER_IMAGE}
COPY --from=build /src/target/release/concerto /usr/local/bin/concerto

ENTRYPOINT ["concerto"]
