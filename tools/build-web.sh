#!/bin/sh
set -eu
: "${CARGO_TARGET_DIR:?Set CARGO_TARGET_DIR to the Linux build volume}"
test "$(uname -s)" = Linux
cd "$(dirname "$0")/.."
cargo build --locked --release -p stormrfb-wasm --target wasm32-unknown-unknown
wasm-bindgen "$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/stormrfb_wasm.wasm" --target web --out-dir web/pkg
