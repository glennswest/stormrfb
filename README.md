# stormrfb

**RFB (RFC 6143) in Rust — one implementation, both ends.**

The wire protocol behind VNC, as a library: a codec that never touches a
socket, a client that turns a byte stream into an RGBA framebuffer, a server
session that turns a framebuffer and damage into updates, and a WASM/canvas
binding for the browser. Private, version **0.1.1**, nothing published.

It is a set of crates, not a service: **no daemon, no ports, no config
file, no health or metrics endpoints.** The application that links it owns
the transport, authorization, logging and metrics.

## Why it exists

- **Server:** a VMM with no display of its own needs one
  ([stormvm#1](https://github.com/glennswest/stormvm/issues/1): virtio-gpu +
  virtio-input + VNC as a vhost-user device, still open). Cloud Hypervisor
  has no framebuffer, so something has to speak RFB on the server side. A
  browser library cannot. This is the reason the repo exists.
- **Client:** the same messages read the other way. stormconsole renders VM
  framebuffers with noVNC by default. stormrfb is an opt-in alternative
  there (see [Who uses it](#who-uses-it)).
- **Licence** (noVNC is MPL-2.0 in an MIT stack) is not a reason on its own.
  If the server were not needed, keeping noVNC would be the answer.

## What it does today

### Crates

| Crate | What it is | Depends on |
|---|---|---|
| `stormrfb` | Sans-I/O codec: handshake, messages, pixel formats, rectangle encode/decode. `#![forbid(unsafe_code)]` | `des`, `flate2` (pure-Rust backend) |
| `stormrfb-client` | `Client` (bytes in → events out) + `Framebuffer` (canonical RGBA) + DOM key/button helpers | `stormrfb`; `minifb` only with feature `native-harness` |
| `stormrfb-server` | `Server`: one session per connection. It handles authentication, input events, damage and requested updates | `stormrfb` |
| `stormrfb-wasm` | `BrowserClient`, the wasm-bindgen wrapper around `Client` | `stormrfb-client`, `wasm-bindgen`, `js-sys` |
| `web/` | `@stormrfb/client`: `connect(canvas, url, opts)`, canvas 2D, `private: true` | the built `web/pkg/` |
| `fuzz/` | `cargo-fuzz` target `decoder` (separate workspace) | `libfuzzer-sys` |

Every crate has `publish = false`. Rust edition 2024, `rust-version = 1.85`.

### Protocol subset (from `crates/stormrfb/src`)

| Area | Implemented |
|---|---|
| Version | **RFB 3.8 only** (`RFB 003.008\n`). Anything else fails with `Invalid("requires RFB 3.8")` |
| Security | `None` (1) and VNC Authentication (2, DES challenge/response). No TLS/VeNCrypt/SASL. The transport is secured a layer up |
| Pixel formats | True colour only, 16 or 32 bpp, both byte orders, validated channel masks. Colour-map formats are rejected |
| Client → server | `SetPixelFormat`, `SetEncodings`, `FramebufferUpdateRequest`, `KeyEvent`, `PointerEvent`, `ClientCutText` (encode and decode) |
| Server → client | `FramebufferUpdate`, `SetColourMapEntries` (decoded; the client ignores it), `Bell`, `ServerCutText` |
| Decoded encodings | Raw (0), CopyRect (1), Hextile (5), ZRLE (16, one persistent zlib stream per connection) |
| Pseudo-encodings | Cursor (-239), DesktopSize (-223), LastRect (-224) |
| Encoder (`ServerEncoder::update`) | Raw, Hextile and ZRLE. Hextile emits only raw tiles, and ZRLE only raw (subencoding 0) tiles, deflated with `Compression::fast()`. Also encodes CopyRect, Cursor and DesktopSize rectangles |
| Not implemented | RFB 3.3/3.7, TRLE (15) as an advertised encoding, Tight/JPEG/H.264, ExtendedDesktopSize (-308), ContinuousUpdates (-313)/Fence (-312), QEMU Extended Key Event (-258, #1), XCursor (-240), colour-map pixel formats |

`ENCODINGS`, the list the client advertises, in preference order, is
`ZRLE, Hextile, CopyRect, Raw, Cursor, DesktopSize, LastRect`.

### Limits: the only configuration

Every decoder and encoder takes a `stormrfb::Limits`. `Limits::default()`:

| Field | Default | Bounds |
|---|---|---|
| `max_pixels` | 16,777,216 | pixels in one framebuffer or rectangle |
| `max_bytes` | 128 MiB | one protocol unit, the client/server input buffer, inflated ZRLE data, and `width × height × 4` |
| `max_text` | 1 MiB | cut-text and reason strings |
| `max_rectangles` | 4,096 | rectangles per update (including LastRect updates), `SetEncodings` length; events per `receive` call are capped at `4 × max_rectangles + 16` |

`stormrfb-wasm` always uses `Limits::default()`. Exceeding a limit returns
`Error::Limit`. Every error other than `Error::Incomplete` is terminal for
that decoder, client or session: reconnect with a fresh one.

### Client (`stormrfb-client`)

- `Client::new(password: Option<Vec<u8>>, limits)` always requests a
  **shared** session. `receive(bytes) -> Vec<Event>` accepts any
  fragmentation. The caller writes every `Event::Send` to the transport in
  order.
- On `ServerInit` it sends `SetPixelFormat(RGBX)` (32 bpp, depth 24,
  little-endian, R/G/B shifts 0/8/16), then `SetEncodings(ENCODINGS)` and a
  full update request. After each update it sends an incremental request
  for the whole screen (a full one after a resize). The client owns format
  and encodings: `send()` rejects `SetPixelFormat`/`SetEncodings`.
- Events: `Send`, `Ready { name }`, `Damage(Rect)`, `Resized`, `Cursor`
  (RGBA, alpha from the mask), `Bell`, `CutText`.
- `Framebuffer` is opaque RGBA (alpha forced to 255; the wire's spare byte is
  not alpha). CopyRect handles overlap in every direction.
- `Renderer` is a trait for native consumers. Nothing in this repo
  implements it: the browser and the harness read `Framebuffer::rgba()`
  directly.
- `keysym(dom_key)` maps DOM `KeyboardEvent.key` to X keysyms: named keys,
  F1–F12, Latin-1, and Unicode keysyms (`0x01000000 | c`) above that.
  `pointer_buttons(dom_buttons)` maps DOM buttons to the RFB button mask.

### Server (`stormrfb-server`)

- `Server::new(ServerInit, Security, limits)`. With `Security::Vnc` the
  caller supplies the password and a **fresh random 16-byte challenge per
  session**. The crate has no entropy source. The response is compared in
  constant time. A failure sends `SecurityResult` 1 plus
  `"Authentication failed"`, and the session becomes terminal.
- `greeting()` returns the version string to send first. `receive(bytes)`
  yields `Send`, `Ready { shared }`, `Key`, `Pointer` (clamped to the
  screen) and `CutText`.
- `damage(rect, &pixels)` updates the server's RGBA framebuffer and extends
  the dirty rectangle (a single bounding box). `update()` answers **at most
  one** outstanding request. Incremental requests wait for damage that
  intersects them. The encoding is the first of Raw/Hextile/ZRLE in the
  client's `SetEncodings` order, and Raw until one arrives. Pixels are sent
  in whatever format the client set.
- `resize(w, h)` works only after the client advertised DesktopSize. It is
  delivered as the next update, followed by a full refresh.
- The server never sends CopyRect, Cursor, Bell, cut text or colour maps
  from a session. `ServerEvent::encode_control` and `ServerEncoder` can build
  them if the application writes the bytes itself.

### Browser (`stormrfb-wasm` + `web/`)

`BrowserClient` (`new(password?)`, `receive`, `width`, `height`,
`framebuffer_ptr`, `key`, `key_sym`, `pointer`, `clipboard`) returns events
as `['send', bytes]`, `['ready', name]`, `['damage', x, y, w, h]`,
`['resize', w, h]`, `['cursor', x, y, w, h, rgba]`, `['clipboard', text]`
and `['bell']`.

`web/client.js` exports `connect(canvas, url, { password, onready,
onclipboard, onbell, onerror })`, which returns `{ close, clipboard(text) }`.
It draws with canvas 2D `putImageData` on damaged rectangles from an
`ImageData` over WASM memory, and rebuilds the view after memory growth.
It renders the cursor locally as a CSS cursor. Input is keyboard (release
on blur), pointer with capture, and wheel. Clipboard text is Latin-1 only.
It has no dead keys, no IME, no ExtendedDesktopSize and no scaling of its
own. The canvas is the guest's size and CSS scales it. See
[web/README.md](web/README.md).

### Native harness (development only)

```
cargo run -p stormrfb-client --example viewer --features native-harness -- HOST:PORT
```

It opens an X11 window (`minifb`) and sends mouse input only.
`VNC_PASSWORD` sets the password, and `STORMRFB_HARNESS_FRAMES=n` exits
after n blits (used by `tools/harness-smoke.py`). Not a shipped viewer
([DESIGN.md §Decisions 3](docs/DESIGN.md#3-no-shipped-native-viewer--but-a-native-harness-early)).

## Build and test

Build and test on the build box with **`sc-build`**, after `git push`. It
fetches the pushed commit onto `dev.g8.lo` as the unprivileged build user,
builds it in a scratch directory, and deletes that directory. No step needs
root, and nothing is built on the workstation.

```sh
git push
sc-build                                   # cargo build && cargo test
sc-build 'cargo test --workspace --locked'
sc-build 'cargo clippy --workspace --all-targets --locked -- -D warnings'
```

The Rust suite is 25 tests: codec round trips, fragmentation, hostile
lengths, tile subencodings, server sessions, and replays of the two recorded
fixtures (`fixtures/qemu-zrle.rfb`, `fixtures/tigervnc-zrle.rfb`) against
independently captured pixel hashes.

**Not available through `sc-build` today:** the WASM package
(`tools/build-web.sh` needs the `wasm32-unknown-unknown` target, which is
not installed for the build user, and `wasm-bindgen` 0.2.128 on `PATH`).
The same applies to the browser, noVNC-differential and harness checks in
`tools/validate.sh`, which need Playwright, noVNC 1.7.0 and Xvfb, and to
fuzzing (`cargo +nightly fuzz run decoder`). Those were run on 2026-09-09.
See [docs/VALIDATION.md](docs/VALIDATION.md) for what they need.

Demo: `node tools/demo-server.mjs` serves `web/demo/` on
**127.0.0.1:8765** (`PORT` overrides it). This is the only listening socket
anything in this repo opens. It needs a built `web/pkg/`.

## How it ships

stormrfb has **no golden and no stormcos component of its own**. It is not
in stormcos `deploy/build-goldens.sh` `COMPONENTS`. It reaches a release
through its consumers:

- **stormconsole** vendors the built browser package into
  `web/src/lib/vendor/stormrfb/`, with the source commit in `VERSION`
  (currently `stormrfb 29305ab`). It ships inside the stormconsole golden.
  Rebuild with `tools/build-web.sh` and re-vendor there.
- **stormrdp** depends on `stormrfb`, `stormrfb-client` and
  `stormrfb-server` as git dependencies pinned by `rev`.

Changing this repo changes nothing deployed until one of those updates its
pin.

## Who uses it

- **stormconsole** (`web/src/lib/views/VmDetail.svelte`): a switch on the VM
  page, `?rfb=storm`, remembered in `localStorage` `vm.rfb`. **noVNC stays
  the default** until a Linux guest and a Windows installer have been driven
  through the relay. Both clients dial the same door,
  `/api/plugins/vm/console/{ns}/{name}/vnc`. That route is a websocket relay
  to stormvm `:9095 /api/v1/vms/{id}/console/vnc`, and it passes RFB through
  untouched.
- **stormrdp**: `stormrdp-host-rfb` bridges VNC servers to RDP with
  `stormrfb-client`. `tools/bench` drives `stormrfb-server`. It maps RDP
  scancodes to US keysyms until #1 (QEMU Extended Key Event) lands.
- **stormvm**: nothing yet. `stormvm vnc` bridges the hypervisor's unix VNC
  socket to a TCP port (`127.0.0.1:5900` by default). The Rust-VMM display
  server (stormvm#1) is where `stormrfb-server` is meant to be used.

## Documents

- [docs/DESIGN.md](docs/DESIGN.md): scope, architecture, decisions and
  phasing. Parts marked *design* are not built.
- [docs/VALIDATION.md](docs/VALIDATION.md): what was validated on
  2026-09-09, and how.
- [docs/PERFORMANCE.md](docs/PERFORMANCE.md): the decoder optimisation and
  its measurements.
- [fixtures/README.md](fixtures/README.md): the recorded conformance inputs.
- [web/README.md](web/README.md) and [docs/demos/README.md](docs/demos/README.md):
  the browser package and the demo.
