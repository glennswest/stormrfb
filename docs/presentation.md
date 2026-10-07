---
marp: true
title: stormrfb
description: RFB (RFC 6143) in Rust — purpose and functionality
paginate: true
---

# stormrfb

**RFB (RFC 6143) in Rust — one implementation, both ends**

A set of crates, not a service: a sans-I/O codec, a client, a server
session and a WASM/canvas binding for the browser.

v0.2.0 · private, nothing published · `npx @marp-team/marp-cli docs/presentation.md`

---

## The problem

**The server side.** stormvm#1 wants a display for the Rust VMM:
virtio-gpu + virtio-input + VNC, as a vhost-user device. Cloud Hypervisor
has no framebuffer, so *something* has to speak RFB on the server side,
and a browser library such as noVNC cannot. That is why this repo exists.

**The client side.** The same messages, read the other way: bytes in, an
RGBA framebuffer out. stormconsole offers it as an opt-in alternative to
noVNC, which stays the default.

The licence (noVNC is MPL-2.0 in an MIT stack) is **not** a reason on its
own. If the server were not needed, keeping noVNC would be the answer.

---

## Where it sits

stormcentral's graph (`stormcentral check`): **stormrfb** is in group
`ui` and depends on nothing. **stormconsole** depends on it, and stormcos
ships stormconsole.

What the code actually links:

| Consumer | How it takes stormrfb |
|---|---|
| stormconsole | vendors the built browser package, `web/src/lib/vendor/stormrfb/` (`VERSION`: `stormrfb 29305ab`), behind `?rfb=storm` |
| stormrdp | `stormrfb`, `stormrfb-client`, `stormrfb-server` as git deps at rev `29305ab`. `stormrdp-host-rfb` bridges VNC to RDP, and `tools/bench` drives the server |
| stormvm | nothing yet. stormvm#1 (open) is where `stormrfb-server` is meant to go |

The graph lists stormrdp → stormvm only. The missing stormrdp → stormrfb
edge is filed as stormcentral#54.

---

## How it works

```
             bytes in                            events out
 transport ───────────► ┌──────────────────────┐ ───────────► application
 (caller's)             │ stormrfb (codec)     │  Send / Ready / Damage /
 ◄───────────────────── │ sans-I/O, no unsafe, │  Resized / Cursor / Key /
   Event::Send bytes    │ bounded by Limits    │  Pointer / CutText / Bell
                        └──────────┬───────────┘
              ┌────────────────────┼─────────────────────┐
   ┌──────────▼─────────┐  ┌───────▼──────────┐  ┌───────▼────────────┐
   │ stormrfb-client    │  │ stormrfb-server  │  │ fuzz/ (cargo-fuzz) │
   │ Client+Framebuffer │  │ Server session:  │  │ target `decoder`   │
   │ (RGBA), keysyms    │  │ auth, damage →   │  └────────────────────┘
   └──────────┬─────────┘  │ updates          │
   ┌──────────▼─────────┐  └──────────────────┘
   │ stormrfb-wasm      │── web/ `connect(canvas, url, opts)`
   │ BrowserClient      │   canvas 2D putImageData on damage
   └────────────────────┘
```

No crate opens a socket. The application that links stormrfb owns the
transport, authorization, logging and metrics.

---

## What it does today: the codec

From `crates/stormrfb/src`:

| Area | Implemented |
|---|---|
| Version | RFB 3.8 only. Anything else fails with `Invalid("requires RFB 3.8")` |
| Security | None (1), VNC Authentication (2, DES challenge/response) |
| Pixel formats | true colour, 16/32 bpp, both byte orders. Colour maps are rejected |
| Decoded encodings | Raw, CopyRect, Hextile, ZRLE (one persistent zlib stream) |
| Pseudo-encodings | Cursor (-239), DesktopSize (-223), LastRect (-224) |
| Encoder | Raw, Hextile (raw tiles only), ZRLE (raw subencoding only), and CopyRect, Cursor, DesktopSize rectangles |

The client advertises `ZRLE, Hextile, CopyRect, Raw, Cursor, DesktopSize, LastRect`.

---

## What it does today: client and browser

**`stormrfb-client`**
- `Client::new(password, limits)` always requests a shared session.
  `receive(bytes)` accepts any fragmentation.
- It owns the pixel format (RGBX, 32 bpp) and encodings, and sends
  incremental whole-screen requests after each update.
- `Framebuffer`: opaque RGBA. CopyRect handles overlap in every direction.
- `keysym(dom_key)` and `pointer_buttons(dom_buttons)` translate DOM input.

**`stormrfb-wasm` + `web/`**
- `connect(canvas, url, { password, onready, onclipboard, onbell, onerror })`
  returns `{ close, clipboard(text) }`.
- It draws with canvas 2D on damaged rectangles only and renders the
  cursor locally as a CSS cursor. Input is keyboard, pointer with capture,
  and wheel.
- Clipboard is Latin-1 only. No dead keys, no IME, no scaling of its own.

---

## What it does today: the server

**`stormrfb-server`**: `Server::new(ServerInit, Security, limits)`, one
session per connection.

- **VNC auth:** the caller supplies the password and a fresh random
  16-byte challenge per session (the crate has no entropy source). The
  response is compared in constant time.
- **Input:** `receive(bytes)` yields `Key`, `Pointer` (clamped to the
  screen) and `CutText`.
- **Damage:** `damage(rect, &pixels)` updates the RGBA framebuffer and
  grows one dirty bounding box. `update()` answers at most one
  outstanding request.
- **Encoding:** the first of Raw/Hextile/ZRLE in the client's order. Pixels
  go out in whatever format the client set.
- **Resize:** `resize(w, h)` works once the client advertised DesktopSize.

A session never sends CopyRect, Cursor, Bell or cut text by itself. The
encoder can build them if the application writes the bytes.

---

## Hostile input, tests and numbers

**`Limits`**, taken by every decoder and encoder (defaults):
`max_pixels` 16,777,216 · `max_bytes` 128 MiB · `max_text` 1 MiB ·
`max_rectangles` 4,096. Going over returns `Error::Limit`, and every error
except `Incomplete` is terminal.

**Tests:** a 25-test Rust suite covering round trips, fragmentation,
hostile lengths, tile subencodings, server sessions, and replays of
recorded QEMU and TigerVNC ZRLE fixtures against independently captured
pixel hashes. It passed through `sc-build` at `86f86d9` on 2026-09-26.
The stormcos **test image** (`test/`, `/test short|medium|long`) runs the
server and client against each other over the pod's loopback. On dev on
2026-09-27 it measured a 640×480 window moving on 1080p at 66 fps.

**Measured** on 2026-09-09 (x86_64 dev VM, Node 22, QEMU 720×400 fixture):

| | ms/frame |
|---|---:|
| stormrfb WASM (optimised) | 0.361 |
| noVNC 1.7.0 | 0.967 |

WASM + JS package: 106,798 bytes, 44,718 gzipped (docs/PERFORMANCE.md).

---

## Interfaces

| | |
|---|---|
| **API** | Rust crates: `stormrfb`, `stormrfb-client`, `stormrfb-server`. JS: `@stormrfb/client` `connect()`. WASM: `BrowserClient` |
| **Config** | `stormrfb::Limits` only. The WASM binding uses `Limits::default()`. The test image reads `STORM_TIMEOUT` and optionally `STORMRFB_TARGET` |
| **Ports** | none. The dev demo uses 127.0.0.1:8765 (`PORT`), and the test image uses ephemeral loopback ports inside its pod |
| **Health / metrics** | none. That is the linking application's job |
| **CLI** | none shipped. A dev-only native harness: `cargo run -p stormrfb-client --example viewer --features native-harness -- HOST:PORT` |

All crates are `publish = false`, edition 2024, rust-version 1.85.

---

## How it ships and is operated

- **No golden, no stormcos component, no daemon.** Nothing starts it.
  It runs inside whatever links it.
- **Build and test:** `git push`, then `sc-build 'cargo test --workspace --locked'`
  on dev.g8.lo as the unprivileged build user.
- **To stormconsole:** rebuild the web package with `tools/build-web.sh`,
  re-vendor it there with the new commit in `VERSION`, and it ships inside
  the stormconsole golden.
- **To stormrdp:** stormrdp bumps its git `rev` pin.
- **Updating** means a consumer bumps its pin. Changing this repo changes
  nothing deployed until then.
- **Test image:** stormcentral's runner builds `test/` into
  `test-stormrfb-<suite>:<commit12>` and runs it as a Job. It has not yet
  completed a run there (stormcentral#63), so #10 tracks re-running it.
- Not available through `sc-build` today: the `wasm32` target,
  `wasm-bindgen`, nightly and cargo-fuzz, and the Playwright/noVNC checks
  (stormcentral#64). Those last ran on 2026-09-09 (docs/VALIDATION.md).

---

## Planned — not in the code

| Issue | What |
|---|---|
| #16 | The browser sends scancodes (`KeyboardEvent.code` → QemuKey). Needs the WASM tools on the build box |
| stormconsole#99 | stormconsole makes stormrfb the default and removes noVNC (#4's gate passed) |
| #5 | Server encoder: real Hextile/ZRLE tile subencodings (solid, palette, RLE) |
| #6 | ExtendedDesktopSize (-308): the client asks for a resize |
| #7 | Phase 4 latency: ContinuousUpdates (-313) and Fence (-312) |
| stormvm#1 | The Rust-VMM display that `stormrfb-server` exists for. That work is stormvm's |

Not planned: RFB 3.3/3.7, Tight/JPEG/H.264, TLS/VeNCrypt (the transport is
secured a layer up), and a shipped native viewer (DESIGN.md decision 3).

---

## Status

- **v0.2.0 (2026-10-06).** QEMU Extended Key Event (#1) was checked
  against a real qemu. The phase 1 gate (#4) passed: Alpine and a Windows
  Server installer were pixel-exact and driven through stormconsole's
  relay, level with noVNC per frame, from a chunk half noVNC's size.
- **Consumers:** stormconsole (opt-in, noVNC default) and stormrdp, both at
  `29305ab`. They need a pin bump to get #1.
- **Open issues that matter:**
  - stormconsole#99 switches the default and drops noVNC
  - #16 sends scancodes from the browser too
  - #10 re-runs the test image through stormcentral's runner once
    stormcentral#63 is fixed
  - stormcentral#64 installs the WASM and fuzz tools on the build box,
    without which the browser package can't be rebuilt
  - stormcentral#54 adds the missing stormrdp → stormrfb graph edge
- More: [README.md](../README.md), [DESIGN.md](DESIGN.md),
  [VALIDATION.md](VALIDATION.md), [PERFORMANCE.md](PERFORMANCE.md).
  The longer 2026-09-09 project review deck is in `presentations/`.
