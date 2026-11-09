#!/bin/sh
# Builds the card finder (crates/finder) into static/wasm. Needs wasm-bindgen 0.2.130 and wasm-opt on the PATH.
set -e
cargo build -p finder --target wasm32-unknown-unknown --profile wasm-release
wasm-bindgen target/wasm32-unknown-unknown/wasm-release/finder.wasm --out-dir static/wasm --target web --no-typescript
# Only long-established WebAssembly features, so every browser that runs WebAssembly can run this.
wasm-opt static/wasm/finder_bg.wasm -Oz --strip-debug --strip-producers --enable-bulk-memory --enable-bulk-memory-opt --enable-mutable-globals --enable-nontrapping-float-to-int --enable-sign-ext --enable-reference-types --enable-multivalue -o static/wasm/finder_bg.opt.wasm
mv static/wasm/finder_bg.opt.wasm static/wasm/finder_bg.wasm
ls -l static/wasm
