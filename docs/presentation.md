---
marp: true
title: stormrfb
description: RFB (RFC 6143) in Rust — purpose and functionality
paginate: true
size: 16:9
style: |
  section { font-size: 21px; line-height: 1.25; padding: 44px 56px; }
  h1 { font-size: 34px; margin: 0 0 .3em; }
  h2 { font-size: 26px; margin: 0 0 .35em; }
  ul, ol { margin: .25em 0; } li { margin: .12em 0; line-height: 1.22; }
  p { margin: .4em 0; }
  pre, code { font-size: 14px; } pre { line-height: 1.18; margin: .3em 0; }
  table { font-size: 15px; border-collapse: collapse; } th, td { padding: 2px 8px; }
---

<!-- Every claim here is checkable against the code and docs/VALIDATION.md.
     Check that every slide fits the 16:9 frame on a build VM:
     `sc-build deploy/check-slides.sh` (stormcentral#77, stormrfb#11).
     Refreshed 2026-11-07 for v0.3.0. -->

# stormrfb

**RFB (RFC 6143) in Rust — one implementation, both ends**

A set of crates: a sans-I/O codec, a client, a server session, a
WASM/canvas binding for the browser, and `stormrfb-vt`, a console viewer
for a node's own screen.

v0.3.0 · private, nothing published

---

## The problem

**The server side.** stormvm#1 wants a display for the Rust VMM:
virtio-gpu + virtio-input + VNC, as a vhost-user device. Cloud Hypervisor
has no framebuffer, so *something* has to speak RFB on the server side,
and a browser library such as noVNC cannot. That is why this repo exists.

**The client side.** The same messages, read the other way: bytes in, an
RGBA framebuffer out. stormconsole offers it as an opt-in alternative to
noVNC, which stays the default.

**On the node's console.** stormcos#228 puts VM desktops on tty5/tty6:
`stormrfb-vt` is the client that draws them with KMS (#25).

The licence (noVNC is MPL-2.0 in an MIT stack) is **not** a reason on its
own. If the server were not needed, keeping noVNC would be the answer.

---

## Where it sits

| Consumer | How it takes stormrfb |
|---|---|
| stormconsole | vendors the built browser package (`web/src/lib/vendor/stormrfb/`, `VERSION` `29305ab`), behind `?rfb=storm` |
| stormrdp | `stormrfb`, `-client`, `-server` as git deps at rev `29305ab`; `stormrdp-host-rfb` bridges VNC to RDP |
| stormvm | nothing yet: stormvm#1 is where `stormrfb-server` is meant to go |
| stormcos | `stormrfb-vt` on tty5/tty6 (stormcos#228); its image waits on a registry entry |

stormcentral's graph (`stormcentral check`) has stormrfb in group `ui`,
depending on nothing. The missing stormrdp → stormrfb edge is
stormcentral#54.

Consumers get a change only by bumping their pin.

---

## How it works

```
             bytes in                            events out
 transport ───────────► ┌──────────────────────┐ ───────────► application
 (caller's)             │ stormrfb (codec)     │  Send / Ready / Damage /
 ◄───────────────────── │ sans-I/O, no unsafe, │  Resized / Cursor / Key /
   Event::Send bytes    │ bounded by Limits    │  Pointer / CutText / Bell
                        └──────────┬───────────┘
        ┌──────────────────┬───────┴──────────┬─────────────────────┐
 ┌──────▼───────────┐ ┌────▼─────────────┐ ┌──▼───────────────┐ ┌───▼──────────┐
 │ stormrfb-client  │ │ stormrfb-server  │ │ stormrfb-vt      │ │ fuzz/        │
 │ Client +         │ │ one session:     │ │ Viewer → KMS,    │ │ cargo-fuzz   │
 │ Framebuffer RGBA │ │ auth, damage →   │ │ evdev, VT switch │ │ `decoder`    │
 └──────┬───────────┘ │ updates          │ └──────────────────┘ └──────────────┘
 ┌──────▼───────────┐ └──────────────────┘
 │ stormrfb-wasm    │── web/ connect(canvas, url, opts): canvas 2D on damage
 └──────────────────┘
```

No library crate opens a socket: the application owns the transport,
authorization, logging and metrics. `stormrfb-vt` is the one binary, and
its `Viewer` is sans-I/O too.

---

## The codec (`crates/stormrfb`)

| Area | Implemented |
|---|---|
| Version | RFB 3.8 only; anything else is `Invalid("requires RFB 3.8")` |
| Security | None (1), VNC Authentication (2, DES challenge/response) |
| Pixel formats | true colour, 16/32 bpp, both byte orders; colour maps rejected |
| Decoded encodings | Raw, CopyRect, Hextile, ZRLE (one persistent zlib stream) |
| Pseudo-encodings | Cursor (-239), DesktopSize (-223), LastRect (-224), ContinuousUpdates (-313), Fence (-312), ExtendedDesktopSize (-308), QEMU keys (-258) |
| Messages beyond the core | SetDesktopSize (251), EnableContinuousUpdates / EndOfContinuousUpdates (150), Fence (248), QEMU Extended Key (255/0) |
| Encoder | Raw, Hextile, ZRLE with tile subencodings chosen by size; CopyRect, Cursor, size rectangles |

`Limits` bound every decoder and encoder (defaults: 16,777,216 pixels,
128 MiB, 1 MiB text, 4,096 rectangles). Over a limit is `Error::Limit`, and
every error but `Incomplete` is terminal.

---

## The client and the browser

**`stormrfb-client`**
- `Client::new(password, limits)`: a shared session; `receive(bytes)`
  takes any fragmentation. It owns the pixel format (RGBX) and encodings.
- Continuous updates and fences on whenever the server offers both (#7);
  otherwise an incremental request after each update.
- `request_resize(w, h)` where the server takes -308 (#6); `key_event`
  sends scancodes where the server acknowledged -258 (#1).
- `pause()` / `resume()` for a viewer that is not on screen (#25).

**`stormrfb-wasm` + `web/`**
- `connect(canvas, url, { password, onready, onclipboard, onbell, onerror })`
  returns `{ close, clipboard(text), resize(w, h) }`.
- Canvas 2D on damaged rectangles, a CSS cursor, keyboard, pointer with
  capture, wheel. Latin-1 clipboard, no IME, no scaling of its own.

---

## The server (`stormrfb-server`)

`Server::new(ServerInit, Security, limits)`, one session per connection.

- **VNC auth:** the caller gives the password and a fresh random 16-byte
  challenge; the response is compared in constant time.
- **Input:** `receive(bytes)` yields `Key`, `QemuKey`, `Pointer` (clamped)
  and `CutText`.
- **Damage:** `damage(rect, &pixels)` grows one dirty box; `update()`
  answers the outstanding request, encoded with the first of
  Raw/Hextile/ZRLE in the client's order.
- **Continuous updates (#7):** pushed for the enabled area on damage, a
  fence after each, held while unanswered bytes reach `set_window`
  (1 MiB by default).
- **Resize:** `resize(w, h)`, sent as -308 where advertised; a client's
  request arrives as `Event::SetDesktopSize` for `accept_resize` /
  `refuse_resize` (#6).

---

## `stormrfb-vt`: a VM on a VT (#25)

`stormrfb-vt --vm default/desk` reads `/run/stormvm/default/desk/vnc.sock`
(or `--socket`, `--connect host:port`) and draws it full-screen.

- **Display:** KMS dumb buffers, page flip (copy + DIRTYFB on simpledrm),
  scaled with the aspect kept; asks the guest for the screen's size.
- **Input:** evdev keyboards and pointers → QEMU scancodes (US keysyms
  otherwise), absolute and relative pointers, wheels.
- **VT:** `VT_PROCESS` + `K_OFF`; Alt+Fn and Ctrl+Alt+Fn switch
  (`--switch ctrl-alt` passes Alt+Fn to the guest). Release lets go of
  held keys and pauses; acquire shows nothing until a whole new frame.
- **VM down:** a status screen and reconnect with backoff (1–30 s).

Tested against `stormrfb-server` in process (frame, keys, click,
release/acquire). The KMS/VT/evdev half needs a real console.

---

## Tests and numbers

- **68 Rust tests** (`sc-build`, f4e50ae): round trips, fragmentation,
  hostile lengths, tile subencodings, server sessions, the viewer, and
  replays of recorded QEMU and TigerVNC streams against captured hashes.
- **Test image** (`test/`, `/test short|medium|long`): server and client
  over the pod's loopback, every frame checked pixel for pixel.

| Measured (docs/VALIDATION.md) | Result |
|---|---|
| 50 ms round trip, request/response vs continuous (#7) | 17.8 vs 72.8 updates/s (4.1×), pvetest1 |
| 1080p, 640×480 moving window, ZRLE | 69 fps, 61,801 B/frame, pvetest1 |
| Real guests through stormconsole's relay (#4) | pixel-exact, level with noVNC per frame |
| WASM decode, QEMU 720×400 fixture (2026-09-09) | 0.361 ms/frame vs noVNC 0.967 |

---

## Interfaces

| | |
|---|---|
| **API** | Rust crates `stormrfb`, `-client`, `-server`; JS `@stormrfb/client` `connect()`; WASM `BrowserClient` |
| **CLI** | `stormrfb-vt` (#25). A dev-only native harness: `cargo run -p stormrfb-client --example viewer --features native-harness -- HOST:PORT` |
| **Config** | `stormrfb::Limits` only. The test image reads `STORM_TIMEOUT` and optionally `STORMRFB_TARGET` |
| **Ports** | none. The test image uses ephemeral loopback ports in its pod |
| **Health / metrics** | none: the linking application's job |

All crates are `publish = false`, edition 2024, rust-version 1.85.

---

## How it ships

- **Libraries:** no image of their own. stormconsole re-vendors the web
  package (`tools/build-web.sh`), stormrdp bumps its git `rev`.
- **`stormrfb-vt`:** a static musl binary, to ship as its own golden under
  stormpump; its registry entry is the owner's to add.
- **Build and test:** `git push`, then `sc-build` on a fresh build VM;
  zero warnings, `clippy -D warnings`, `fmt --check`.
- **Test image:** stormcentral's runner builds `test/` and runs it as a
  Job: `short` and `medium` passed on pvetest1 and pvetest2 (#10); the
  nightly plan runs `medium`.
- **Not on the build box:** the `wasm32` target, `wasm-bindgen`, nightly
  and cargo-fuzz (stormcentral#64), so the browser package and fuzzing
  last ran on 2026-09-09.

---

## Status and what is next

- **v0.3.0 (2026-11-07):** ExtendedDesktopSize (#6), ContinuousUpdates +
  Fence (#7), `stormrfb-vt` (#25).
- **v0.2.0 (2026-10-06):** QEMU Extended Key Event (#1); the phase 1 gate
  (#4): real guests pixel-exact through stormconsole's relay.

| Open | What |
|---|---|
| #16 | the browser sends scancodes; needs the WASM tools |
| #22 | browser p95 decode+paint about 2× noVNC on a text console |
| #24 | a licence for the crates (the owner's call) |
| stormconsole#99 | stormrfb the default, noVNC removed |
| stormvm#1 | the Rust-VMM display `stormrfb-server` exists for |

Not planned: RFB 3.3/3.7, Tight/JPEG/H.264, TLS (secured a layer up), a
windowed native viewer (DESIGN.md decision 3).
