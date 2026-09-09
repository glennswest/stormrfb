# Implementation and validation — 2026-09-09

The subsequent [performance patch](PERFORMANCE.md) fixes the measured WASM
slowdown and records the new validation results. The numbers below preserve
the initial v0.1.0 baseline.

The repository now contains a working RFB 3.8 implementation, not a replacement
for the deployed noVNC client yet. All Rust builds and tests below ran on the
Linux development host after pushing to GitHub and pulling there with `gh`
authentication. No Rust build/test/check ran on the workstation.

## Implemented

- `stormrfb`: sans-I/O bounded wire codec; client handshake and both None/VNC
  authentication primitives; 16/32-bit true-colour conversion in both byte
  orders; all scoped client messages and server controls; Raw, CopyRect,
  Hextile, persistent ZRLE, Cursor, DesktopSize and LastRect.
- `stormrfb-client`: fragmented stream handling, canonical opaque RGBA
  framebuffer, overlap-safe CopyRect, cursor masks, damage events, resize,
  update requests and DOM key/button translation. Renderer trait is available
  to native consumers; the browser uses the same framebuffer through its ABI.
- `stormrfb-server`: per-connection handshake/authentication, input events,
  framebuffer damage, full/incremental requested updates, negotiated pixel
  format/encoding and DesktopSize. Raw, raw-tile Hextile and raw-tile ZRLE
  encoding are supported; optimized palette selection is future work.
- `stormrfb-wasm` and `web/`: private browser package, canvas ImageData views
  into WASM memory, dirty-region paint, view recreation on memory growth,
  pointer capture, wheel, keyboard release on blur, clipboard callbacks and
  local cursor rendering.
- Optional X11 development window, recorded independent conformance fixtures,
  noVNC differential runner, sanitizer fuzz target and repeatable validation.

No crate is publishable and the npm package has `private: true`.

## Results (v0.1.0 baseline)

Host: x86_64 Linux, 8 vCPUs, reported Intel Core Ultra 7 270K Plus;
Rust 1.95.0, Node 22.22.2, wasm-bindgen 0.2.128. Captures and measurements
were made on 2026-09-09. Target directories and downloaded test tools are on
the build volume. Hardware is a VM; these are local baselines, not universal
performance claims.

| Check | Result |
|---|---|
| `cargo test --workspace --all-features --locked` | 24 tests passed |
| Strict Clippy, all targets/features | Passed |
| Release `wasm32-unknown-unknown` build | Passed |
| Real WASM Node tests | 2 passed |
| Chromium 153 canvas test | Pixels/alpha, memory growth, resize, key events, cleanup passed |
| Native window under Xvfb, connected to QEMU | Handshake and 3 blits passed |
| QEMU 10.1.5 fixture | Both updates match independent QMP screendump hash |
| TigerVNC 1.15.0 fixture | Both updates match independent XGetImage hash |
| noVNC 1.7.0 differential | Both fixtures match their independent hashes |
| Sanitizer fuzz, initial decoder target | 4,332,048 executions in 46 seconds, no crash |
| Sanitizer fuzz, including framed mutated compressed tiles | 434,699 executions in 46 seconds, no crash; peak RSS 377 MB |

Fuzz smoke runs provide coverage, not a proof that hostile input cannot expose
bugs. Committed tests explicitly cover oversized text/framebuffers, rectangle
budgets including LastRect sentinel counts, inflated-data caps, terminal
failure, malformed palettes/runs and CopyRect bounds.

## Measurements (v0.1.0 baseline)

Same QEMU 720×400 static firmware session, two ZRLE full updates, 1,804 total
wire bytes (902 bytes/frame). Each microbenchmark warms 20 sessions and times
200 sessions/400 frames, including new decoder/framebuffer allocation per
session. WASM also includes the client handshake and JS ABI event handling.
noVNC uses its unmodified decoder with an in-memory display adapter. None of
these timings include canvas paint, network latency or a moving desktop.

| Runtime | Decode + framebuffer milliseconds/frame |
|---|---:|
| Native Rust release | 0.563 |
| WASM in Node | 1.270 |
| noVNC in Node | 0.914 |

The WASM path is slower than noVNC in this small baseline. Do not use the native
number to claim the browser replacement is faster. Benchmark script sources
are committed; rerun before making performance decisions.

Release package: WASM 89,559 bytes; generated JS plus wrapper 18,234 bytes;
107,793 uncompressed bytes total; 44,879 bytes when the three files are gzipped
individually. This is an isolated package measurement, not a stormconsole
bundler chunk. The design's historical 182 KB noVNC figure is not an equivalent
build measurement and should not be used to claim a size reduction yet.

## Reproduce

On the designated Linux host, pull the pushed commit first. Set
`CARGO_TARGET_DIR` to its build-volume target directory. Put the matching
`wasm-bindgen` CLI on PATH and install the `wasm32-unknown-unknown` target.
For browser/oracle checks, install `@novnc/novnc@1.7.0` and Playwright externally;
set `NOVNC_ROOT` and `PLAYWRIGHT_ROOT` to those packages and
`PLAYWRIGHT_BROWSERS_PATH` to the browser cache on the build volume.

```sh
sh tools/validate.sh
cargo run --release -p stormrfb-client --example replay
node tools/wasm-benchmark.mjs
```

For fuzzing, put cargo-fuzz on PATH and set CARGO_TARGET_DIR to a separate
build-volume fuzz directory:

```sh
cargo +nightly-2026-04-03 fuzz run decoder -- -max_total_time=45 -max_len=16384 -rss_limit_mb=1024
```

Capture regeneration tools are optional and only create disposable servers.
They never attach to an existing guest. Updating a fixture also requires
reviewing its independent metadata and updating the expected checksum in
`crates/stormrfb-client/tests/qemu.rs`.

## Remaining phase exits and limits

- A Windows installer and Linux guest through the actual stormconsole relay,
  with browser performance and production chunk measurements, have not been
  validated. No downstream import or dependency was changed. Keep noVNC until
  that integration gate passes.
- stormvm owns the virtio-gpu/vhost-user integration. The real 1080p moving
  guest server fps/bytes-per-second measurement is pending that integration.
- RFB 3.3/3.7, indexed-colour pixel formats, ExtendedDesktopSize client resize,
  ContinuousUpdates/Fence, Tight/JPEG, IME/composition and a shipped native
  viewer are not implemented. The native harness intentionally has mouse
  input only. The browser supports ordinary DOM keys and Latin-1 clipboard.
- Default bounds are 16,777,216 pixels, 128 MiB per protocol input/output
  unit, 1 MiB text and 4,096 rectangles/update. Feed transport input in chunks
  no larger than the configured buffer allowance and drain outgoing events.
  Hextile framing is rescanned on incomplete input but does not allocate its
  framebuffer-sized output until the payload is available.
- VNC password bytes use classic eight-byte DES semantics. Applications supply
  fresh random server challenges and secured/authorized transport; None and
  VNC Auth do not supply transport confidentiality.
