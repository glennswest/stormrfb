# Implementation and validation — 2026-09-09

The subsequent [performance patch](PERFORMANCE.md) fixes the measured WASM
slowdown and records the new validation results. The numbers below preserve
the initial v0.1.0 baseline.

The repository now contains a working RFB 3.8 implementation, not a replacement
for the deployed noVNC client yet. All Rust builds and tests below ran on the
Linux development host after pushing to GitHub and pulling there with `gh`
authentication. No Rust build/test/check ran on the workstation. (That
checkout on dev has since been retired. Builds now go through `sc-build`;
see §Reproduce.)

## Implemented

- `stormrfb`: sans-I/O bounded wire codec; client handshake and both None/VNC
  authentication primitives; 16/32-bit true-colour conversion in both byte
  orders; all scoped client messages and server controls; Raw, CopyRect,
  Hextile, persistent ZRLE, Cursor, DesktopSize, LastRect,
  ExtendedDesktopSize/SetDesktopSize (#6) and QEMU Extended Key Event.
- `stormrfb-client`: fragmented stream handling, canonical opaque RGBA
  framebuffer, overlap-safe CopyRect, cursor masks, damage events, resize,
  update requests and DOM key/button translation. Renderer trait is available
  to native consumers; the browser uses the same framebuffer through its ABI.
- `stormrfb-server`: per-connection handshake/authentication, input events,
  framebuffer damage, full/incremental requested updates, negotiated pixel
  format/encoding, DesktopSize, and ExtendedDesktopSize resize requests
  answered by the application. Raw, Hextile and ZRLE are supported; each
  Hextile and ZRLE tile's subencoding is chosen by size (#5).
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

The Rust part runs through `sc-build` after `git push`. It fetches the
pushed commit onto dev as the unprivileged build user, builds it in a
scratch directory and deletes it:

```sh
sc-build 'cargo test --workspace --locked'
sc-build 'cargo clippy --workspace --all-targets --locked -- -D warnings'
sc-build 'cargo run --release -p stormrfb-client --example replay'
```

The rest of `tools/validate.sh` (WASM build, Node/Chromium tests, noVNC
differential, X11 harness) and fuzzing cannot run under `sc-build` today:
the build user has no `wasm32-unknown-unknown` target, no nightly
toolchain or cargo-fuzz, and no `wasm-bindgen` on `PATH`. The 2026-09-09
runs used the setup below, as root on dev. Running them again needs that
toolchain installed for the build user, which is a host change for the
owner. Setup: set
`CARGO_TARGET_DIR` to a build-volume target directory. Put the matching
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

## Test container — 2026-09-27

`test/` holds the stormcos test image (#8). It has short, medium and long
suites, all run through `sc-build` on dev.g8.lo (8 vCPUs, 13 GiB free) at
`6ba5511`. See [test/README.md](../test/README.md).

| suite | result | notable |
|---|---|---|
| short | 5 pass, exit 0, < 1 s | 640x480 loopback session 1.78 ms/frame; Raw/Hextile/ZRLE × four pixel formats exact |
| medium | 14 pass, 1 skip (`real-server`: no `STORMRFB_TARGET`), exit 0 | 200,000 mutated fixture streams in 41 s, no panic; 16 concurrent 1280x720 sessions exact; hostile bytes rejected both ways |
| long (`STORM_TIMEOUT=180`) | 34 waves + trend pass, exit 0 | 8/16/24 sessions: 5.1–6.5 / 11.8–13.7 / 18.1–19.0 ms/frame with no drift; after every drain 1 thread, 5 fds, RSS 880–1004 KiB |

**Moving window on 1080p, in-process** (medium `moving-window-1080p`,
2026-09-27, dev.g8.lo, 8 vCPUs). A 640×480 window moves across 1920×1080.
One `stormrfb-server` and one `stormrfb-client` share the machine over
loopback TCP, with ZRLE: 300 frames exact in 4.57 s, which is **66 fps,
8.6 MB/s and 131,640 bytes/frame**, with every ZRLE tile raw. This is the
protocol half of the phase 2 measurement. A guest behind stormvm's display
is still pending stormvm#1.

**With tile subencodings chosen by size** (#5, 2026-10-07, `8c3b91e`, a
fresh build VM: 8 cores, 7 GB, Fedora 43, `SC_BUILD_VM=1 sc-build`). The
same test: 300 frames exact in 4.94 s, **61 fps, 3.7 MB/s, 61,788
bytes/frame**, 53% fewer bytes than the raw tiles. fps is level within
the difference between the two machines. `encodings-and-formats` (short and
medium) is exact in all 12 encoding × format pairs; its 200x150 scene went
to 128,590 B (Hextile) and 89,852 B (ZRLE) at 32 bpp against 172,009 B raw,
and to 40,820 / 23,929 B at rgb565le against 86,109 B. Short 5/5, medium
14 + 1 skip (`real-server`), exit 0. The noVNC differential
(`tools/validate.sh`) was not rerun for this change: it needs the wasm32
target and `wasm-bindgen`, which the build user did not have on dev
(stormcentral#64).

## Real guests through stormconsole's relay — 2026-10-06 (#4)

`sc-build tools/verify-relay.sh` at `e31cf8a` on dev.g8.lo (Fedora 43,
16 CPUs, 62 GiB, QEMU 10.1.5, KVM), exit 0, 632 s. One run, nothing kept.

**What runs.** stormconsole `cf2cbbb` (its SPA built by vite, its server)
and stormvm `4051696` (`stormvm serve`), both built in the job; fastetcd
v1.2.0 and rustkube v0.15.3 with the KubeVirt CRDs; two qemu guests
started with the arguments stormvm's qemu driver renders for a framebuffer
(q35, KVM, OVMF, `-nodefaults`, `virtio-vga`, `-vnc unix:…/vnc.sock`, QMP
on `control.sock`) and registered as `stormvm start` registers them; and
headless Chromium (Playwright 1.63.0) on stormconsole's VM page,
Graphical console tab. The path is the production one:

```
Chromium ⇄ stormconsole /api/plugins/vm/console/default/<vm>/vnc   (relay)
         ⇄ stormvm serve /api/v1/vms/default/<vm>/console/vnc     (door, token minted)
         ⇄ qemu's VNC server on vnc.sock
```

Not real: no kubelet or stormpump run on the build box, so the script
plays their part (starts qemu, writes `vm.json` and the VMI status). The
stormrfb client is the build stormconsole vendors (`29305ab`; its
`client.js` is this commit's `web/client.js`, and the crates have had only
doc changes since). noVNC is stormconsole's own 1.7.0. Both clients are
attached to the **same guest at the same time** before it is unpaused.

**Guests.** Alpine 3.20.3 `virt` ISO (1 GiB, 2 vCPU) and Microsoft's
Windows Server 2022 evaluation ISO (4 GiB, 2 vCPU), both downloaded per run.
Both run at 1280×800 (OVMF's GOP mode).

**Legible** is checked as pixels, not by eye: each client's canvas is read
back and compared with qemu's own `screendump` (QMP) of a settled screen.

| screen | stormrfb | noVNC |
|---|---|---|
| Alpine at its login prompt | 100.000% exact | 100.000% exact |
| Alpine after `ls -lR` scrolled | 100.000% | 100.000% |
| Alpine after `clear` | 100.000% | 99.998% (the blinking cursor) |
| Windows Setup, language page | 100.000% | 100.000% |
| Windows Setup, after Next | 100.000% | 100.000% |

**Driven**, all by keys typed at stormrfb's canvas, through the relay:
- Alpine: `root` logs in (7,213 pixels change), `ls -lR …` scrolls, and
  `clear` leaves 0.03% of the screen lit.
- Windows: the ISO boots only if a key is pressed at "Press any key to boot
  from CD or DVD", so reaching Setup (24 s after unpausing) is the first
  proof. On Setup's first page, **Alt+N** (Next's mnemonic) moved it on
  (30,576 pixels changed). Enter does nothing there because the focus is on
  the Language list, and Setup ignores keys for a few seconds after it
  first draws (an earlier run pressed too soon). The run's screenshots are
  in its log as base64 PNG lines.
- The pointer was not checked in Windows. stormvm gives a framebuffer VM
  only q35's relative PS/2 mouse, which is filed as stormvm#76.

**Measured**, per client, on the same session. "Handler" is the time
inside the socket's `message` handler. For both clients that is decode
plus canvas paint, done synchronously. "FBUR" counts the
FramebufferUpdateRequests the client sent, one per completed update, so
per-FBUR is per frame. Chromium's `performance.now()` resolution is
0.1 ms, so p50s are coarse.

| phase (seconds) | client | bytes | frames | B/frame | handler ms | ms/frame | p95 ms |
|---|---|---:|---:|---:|---:|---:|---:|
| Alpine boot (8.0) | stormrfb | 24,131 | 36 | 670 | 23.8 | 0.661 | 4.2 |
| | noVNC | 25,765 | 35 | 736 | 18.5 | 0.529 | 1.9 |
| Alpine `ls -lR` (6.5) | stormrfb | 93,259 | 57 | 1,636 | 23.2 | 0.407 | 3.1 |
| | noVNC | 96,786 | 65 | 1,489 | 20.7 | 0.318 | 1.8 |
| Windows boot to Setup (24.1) | stormrfb | 44,238 | 147 | 301 | 39.1 | 0.266 | 0.5 |
| | noVNC | 50,210 | 151 | 333 | 47.2 | 0.313 | 0.8 |
| Windows Setup, keys to Next (12.7) | stormrfb | 4,823 | 4 | 1,206 | 1.2 | 0.300 | 0.4 |
| | noVNC | 4,732 | 4 | 1,183 | 1.6 | 0.400 | 0.6 |

Read honestly: **on live guests the two clients are level.** Bytes are
within about 10% of each other, and so is total handler time: stormrfb
spends 0.1 ms/frame more on Alpine's text console and 0.05 less on the
Windows boot. The 2.7× decode advantage in [PERFORMANCE.md](PERFORMANCE.md)
is real on its recorded ZRLE replay, but at these frame sizes (a few hundred
bytes to 2 KB) neither client spends long enough decoding for it to show.
Earlier runs of the same script varied by guest timing; for example the
Windows boot window held 131 KB (stormrfb) and 206–220 KB (noVNC) in
two runs where more of the boot animation fell inside it. Nothing here
measures a busy 1080p desktop. That needs a desktop guest, which this does
not have.

**Shipped size**, from stormconsole's own vite build, and what the browser
actually fetched when the tab opened:

| client | files | bytes | gzip |
|---|---|---:|---:|
| stormrfb | `app.wasm` 88,564 + `client.js` 8,161 | 96,725 | 43,042 |
| noVNC 1.7.0 | `rfb.js` | 181,861 | 54,420 |

stormrfb is 53% of noVNC's chunk uncompressed and 79% gzipped.

**Found on the way:** stormconsole#94, where a read-only viewer's VNC
relay drops the RFB handshake, so that viewer sees nothing with either
client (found by reading the code, not reproduced here). Also stormvm#76.

## QEMU Extended Key Event against a real qemu — 2026-10-06 (#1)

`sc-build tools/verify-extkey.sh` at `ac4d991` on dev.g8.lo (QEMU 10.1.5,
KVM). Exit 0, 29 s. The only later commit is rustfmt. The script boots
Alpine 3.20's virt ISO the way stormvm runs qemu (q35, OVMF, `-nodefaults`,
virtio-vga, VNC on a unix socket) and connects `examples/qemu_keys`, which
is `stormrfb-client` over a `UnixStream`.

- qemu acknowledged -258 in its first update, and `extended_keys()` became
  true.
- `root`, then `echo STORMRFB-EXTKEY-$((6*7)) > /dev/ttyS0`, was typed
  **only as QemuKey events with keysym 0**. Shift went as its own scancode
  (0x2a), and every Enter was keypad Enter (E0 1C, keycode 0x9c).
  `STORMRFB-EXTKEY-42` appeared on the serial line, which means qemu took
  every keycode, including the 0xE0 one.
- On a second connection, `poweroff` typed as plain KeyEvents (the
  fallback) powered the guest off, and qemu exited.
- The first run tried the fallback with `/dev/ttyS0` and showed what this
  feature is for. qemu lower-cases an uppercase keysym on a graphic
  console, so the shell got `/dev/ttys0`. The scancode path has no such
  loss. A non-US guest layout was not tried.

## Remaining phase exits and limits

- The real-guest part of the phase 1 exit passed on 2026-10-06 (above): a
  Linux guest and a Windows installer are legible and driven through
  stormconsole's relay, with measurements. Making stormrfb the default and
  removing `@novnc/novnc` is stormconsole's change (stormconsole#99). Until
  it lands, stormconsole offers stormrfb behind `?rfb=storm` (vendored at
  `29305ab`).
- stormvm owns the virtio-gpu/vhost-user integration. The real 1080p moving
  guest server fps/bytes-per-second measurement is pending that integration.
  The in-process number above is the library's part of it.
- RFB 3.3/3.7, indexed-colour pixel formats,
  ContinuousUpdates/Fence, Tight/JPEG, IME/composition and a shipped native
  viewer are not implemented. The native harness intentionally has mouse
  input only. The browser supports ordinary DOM keys and Latin-1 clipboard,
  and it sends keysyms, not scancodes. The crates support QEMU Extended Key
  Event (#1), but the browser does not use it yet (#16).
- Default bounds are 16,777,216 pixels, 128 MiB per protocol input/output
  unit, 1 MiB text and 4,096 rectangles/update. Feed transport input in chunks
  no larger than the configured buffer allowance and drain outgoing events.
  Hextile framing is rescanned on incomplete input but does not allocate its
  framebuffer-sized output until the payload is available.
- VNC password bytes use classic eight-byte DES semantics. Applications supply
  fresh random server challenges and secured/authorized transport; None and
  VNC Auth do not supply transport confidentiality.
