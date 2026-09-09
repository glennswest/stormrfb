#!/bin/sh
set -eu
: "${CARGO_TARGET_DIR:?Set a Linux build-volume target directory}"
: "${NOVNC_ROOT:?Set the external noVNC 1.7.0 package directory}"
: "${PLAYWRIGHT_ROOT:?Set the external Playwright package directory}"
test "$(uname -s)" = Linux
cd "$(dirname "$0")/.."
cargo fmt --all --check
cargo fmt --manifest-path fuzz/Cargo.toml --check
cargo test --workspace --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
sh tools/build-web.sh
(cd web && npm test)
node tools/browser-test.mjs
node tools/differential.mjs
node tools/differential.mjs tigervnc-zrle
python3 tools/harness-smoke.py
