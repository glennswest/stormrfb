# CLAUDE.md — stormrfb

RFB (RFC 6143) in Rust: the codec, a client for the browser, a server for
the Rust VMM. Read [docs/DESIGN.md](docs/DESIGN.md) before touching
anything. See [docs/VALIDATION.md](docs/VALIDATION.md) for measured status.

**Version: 0.1.0** — private initial implementation; downstream gates remain open. Version locations: `Cargo.toml`, `Cargo.lock`, `fuzz/Cargo.toml`,
`fuzz/Cargo.lock`, `web/package.json`, this file.

## What this is for, in one line each

- **stormconsole** renders a VM's framebuffer. It uses noVNC today and that
  is fine; this replaces it when it is ready and not before.
- **stormvm#1** wants a display for the Rust VMM (virtio-gpu + virtio-input
  + VNC as a vhost-user device). Cloud Hypervisor has no framebuffer, so
  something must speak RFB on the *server* side. That is the half nothing
  else can supply, and the reason this project exists.

If the server half were not needed, keeping noVNC would be the right answer
and this repo should not have been created. Do not let it drift into a
licence-avoidance exercise.

## Build on dev, never on this Mac

Same rule as every project here: **every `cargo build/test/check` runs on
`root@dev.g8.lo`.** A WASM target and a Linux vhost-user server are both
things a macOS build will either skip or misrepresent.

```
commit  →  push  →  ssh root@dev.g8.lo 'cd /root/stormrfb && git pull && \
    CARGO_TARGET_DIR=/build/cargo/stormrfb cargo test'
```

Target dirs live on dev's 2 TB spinning drive (`/build/cargo/stormrfb`),
never on the SSD root — see ~/CLAUDE.md "Nothing lives on the SSD between
builds".

## Conventions

- **Sans-I/O in the codec.** No sockets, no async, no transport. Bytes in,
  events out. This is the decision the whole design rests on; a PR that
  puts a `TcpStream` in `stormrfb` is the wrong PR.
- **No platform types in the codec.** No `stormview`, no `console-core`.
  It is a protocol implementation and stays one.
- **Fuzz the decoder from phase 1**, not later. Lengths in this protocol
  are attacker-influenced.
- **Every measured number says what it was measured on and when.**

## Work plan

### Phase 0 — definition (2026-09-09)

- [x] `docs/DESIGN.md` — scope, the protocol subset, architecture, phasing
- [x] Repo created
- [x] Settle the four open decisions — all four decided 2026-09-09, with
      the reasoning and the noVNC evidence in DESIGN.md §Decisions:
      **canvas 2D** behind a renderer seam (noVNC is 2D after fifteen
      years; a console is not a video player); **private** repo (can be
      published, cannot be unpublished); **no shipped native viewer** but a
      feature-gated native harness in phase 1, because WASM is miserable to
      debug and the harness exercises the same client crate; **TRLE
      internal**, not advertised, because ZRLE is zlib-wrapped TRLE and the
      frames cross a real network to the browser regardless

### Active performance work (2026-09-09)

User requested fixing the measured browser slowdown. Preserve v0.1.0 as the
baseline. Split setup from decode/ABI costs, repeat same-fixture measurements,
optimize the measured framebuffer/tile hot path, and rerun checksums, strict
lint, WASM and Chromium tests before recording the result.

### Active implementation sequence

Completed initial implementation and Linux/WASM/native validation; see
`docs/VALIDATION.md`. Next: downstream real-guest integration and measurements.

1. Bounded wire primitives, pixel formats, client messages and RFB 3.8 handshake.
2. Rectangle codecs, persistent ZRLE, framebuffer client, round-trip tests and fuzz target.
3. Server session, browser binding and optional native harness.
4. Linux/WASM validation, real server fixtures where available, and honest exit status.

Commit and push each increment before testing on dev. Keep packages private.

### Phase 1 — a client that replaces noVNC

- [x] `stormrfb`: handshake, `None` + VNC Auth, pixel formats, message
      types, encode/decode round-trip
- [x] Encodings: Raw, CopyRect, Hextile, ZRLE (TRLE as needed by ZRLE)
- [x] Pseudo-encodings: `Cursor`/`RichCursor`, `DesktopSize`, `LastRect`
- [x] `stormrfb-client`: framebuffer state, damage rectangles, input
      translation
- [x] Native harness (feature-gated, unpolished): a window and a blit, so
      the client crate is debuggable outside WASM
- [x] `stormrfb-wasm`: wasm-bindgen binding, `ImageData` on dirty rects,
      DOM key/mouse → RFB, private npm package (publication intentionally disabled)
- [x] Conformance fixtures recorded from qemu's VNC server and TigerVNC;
      differential test against noVNC as the oracle
- [x] `cargo-fuzz` on the decoder
- [ ] Exit: a Windows installer and a Linux guest legible in stormconsole
      with `@novnc/novnc` removed. **Measure** decode ms/frame, bytes/frame
      and shipped chunk size against noVNC's 182 KB on the same session

### Phase 2 — the server (stormvm#1)

- [x] `stormrfb-server`: framebuffer + damage → update messages
- [ ] vhost-user integration is stormvm's; this supplies the protocol
- [ ] **Measure** fps and bytes/s for a moving window on a 1080p guest

### Phase 3 — the native viewer

Deferred by decision 3; only the development harness is implemented.

### Phase 4 — latency

- [ ] `ContinuousUpdates` + `Fence`, and whatever phase 1 measured as slow

## Integration, when phase 1 lands

stormconsole's VM page lazily imports `@novnc/novnc` in
`web/src/lib/views/VmDetail.svelte`, behind a capability probe. The door it
dials — `/api/plugins/vm/console/{ns}/{name}/vnc` — is a websocket relay
that passes frames through untouched and knows nothing about RFB. Swapping
the import is the whole integration; do not change the door.
