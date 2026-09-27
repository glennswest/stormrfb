#!/bin/sh
# Stage stormrfb's test binary for test/Containerfile (#8).
#
#   test/build.sh [target]        default x86_64-unknown-linux-musl
#
# Per stormcentral docs/test-standard.md the runner runs this first, in the
# checkout on the build box, with cargo, then runs
# `podman build -f test/Containerfile <repo root>` itself. This builds the
# static test binary (its own workspace, test/Cargo.toml, compiled against
# the crates of this checkout) and stages it in test/.stage/ for the
# Containerfile to COPY. By hand, after this:
#
#   podman build -f test/Containerfile --build-arg COMMIT=$(git rev-parse HEAD) -t stormrfb-test .
set -eu
target=${1:-x86_64-unknown-linux-musl}
root=$(cd "$(dirname "$0")/.." && pwd)
manifest="$root/test/Cargo.toml"

cargo build --release --locked --target "$target" --manifest-path "$manifest"
tdir=$(cargo metadata --format-version 1 --no-deps --manifest-path "$manifest" |
    sed 's/.*"target_directory":"\([^"]*\)".*/\1/')
stage="$root/test/.stage"
rm -rf "$stage"
mkdir -p "$stage"
cp "$tdir/$target/release/stormrfb-test" "$stage/"
echo "$stage"
