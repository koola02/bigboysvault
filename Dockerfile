FROM rust:1-slim AS build
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config build-essential curl ca-certificates && rm -rf /var/lib/apt/lists/*
# Tools for the browser widget. The wasm-bindgen version must equal the one pinned in crates/finder/Cargo.toml.
RUN rustup target add wasm32-unknown-unknown \
 && curl -fsSL https://github.com/wasm-bindgen/wasm-bindgen/releases/download/0.2.130/wasm-bindgen-0.2.130-x86_64-unknown-linux-musl.tar.gz | tar xz -C /usr/local/bin --strip-components=1 --wildcards '*/wasm-bindgen' \
 && curl -fsSL https://github.com/WebAssembly/binaryen/releases/download/version_133/binaryen-version_133-x86_64-linux.tar.gz | tar xz -C /usr/local --strip-components=1
COPY . .
RUN cargo build --release --bin marcuru-rs
RUN sh build-wasm.sh

FROM debian:stable-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /app/target/release/marcuru-rs .
COPY --from=build /app/static ./static
# Everything that must survive a redeploy lives on the mounted volume at /data.
ENV DATABASE=/data/data.db UPLOADS=/data/uploads PORT=8080 HOST=0.0.0.0
EXPOSE 8080
CMD ["./marcuru-rs"]
