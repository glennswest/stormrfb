# CLAUDE.md — stormrfb

RFB (RFC 6143) in Rust: the codec, a client for the browser, a server for
the Rust VMM. Read [docs/DESIGN.md](docs/DESIGN.md) before touching
anything. See [docs/VALIDATION.md](docs/VALIDATION.md) for measured status.

**Version: 0.2.0** — private; phase 1 real-guest gate passed (#4), QEMU Extended Key Event (#1). Version locations: `Cargo.toml`, `Cargo.lock`, `fuzz/Cargo.toml`,
`fuzz/Cargo.lock`, `test/Cargo.lock` (path deps), `web/package.json`, this file.

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

## Build with sc-build, never locally, never as root

```
commit  →  push  →  sc-build 'cargo test --workspace --locked'
```

`sc-build` builds the pushed commit on dev as the unprivileged build user in
a scratch directory. There is no checkout on dev and no `ssh root@`.

The build user has no `wasm32-unknown-unknown` target, no nightly toolchain
or cargo-fuzz, and no `wasm-bindgen` on `PATH`. So the WASM package, the
browser/noVNC checks in `tools/validate.sh` and fuzzing cannot be rerun
today. Installing them is a host change for the owner, not something to
work around (stormcentral#64). clippy and rustfmt are there: workspace
clippy `-D warnings` and `fmt --check` pass (2026-09-27). The `test/`
crate does not yet (#12).

**Shipping:** no golden of its own. stormconsole vendors the built web
package (`web/src/lib/vendor/stormrfb/VERSION`), and stormrdp pins the
crates by git rev. Consumers pick up a change only by bumping their pin.
The stormcos test image (`test/`, `/test short|medium|long`) is built and
run by stormcentral's test runner, and has not completed a run there yet
(#10: stormcentral#63; #56 was fixed 2026-09-28).

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

### Status (2026-10-06)

v0.2.0: #1 done (QEMU Extended Key Event, checked against a real qemu).
#4 done: phase 1's real-guest gate passed through stormconsole's relay
(`sc-build tools/verify-relay.sh`, docs/VALIDATION.md). Removing noVNC is
stormconsole#99.

### Status (2026-09-27)

v0.1.1. Crate code unchanged since the 2026-09-09 performance patch
(since then only doc comments, the demo and `test/`). Docs rewritten from
the code in #2 and refreshed 2026-09-27. Consumers: stormconsole (opt-in
`?rfb=storm`, vendored at `29305ab`; noVNC default) and stormrdp (git pins
at `29305ab`). Test container: `test/` (#8). Deck: `docs/presentation.md`
(#3). Open: #1 (P2), #4 (P2), #5–#7 (P3), #10 runner re-run, #11 slide
overflow, #12 test-crate lint.

### Done — #5 real Hextile/ZRLE tile subencodings (2026-10-07)

Baseline: 131,640 bytes/frame, moving-window-1080p, raw ZRLE tiles.
- [x] `tiles.rs` encoder side: Hextile per tile (bg-only, carried bg,
      fg + subrects, coloured subrects, raw when smaller); ZRLE per tile
      (smallest of solid, packed palette, plain RLE, palette RLE, raw).
      Colours compared as wire pixels, so 16 bpp quantization is exact
      (0ac0c54)
- [x] unit tests in tiles.rs: each subencoding chosen; 8 scenes × 5 sizes
      × 4 pixel formats round-trip through the decoders (0ac0c54)
- [x] build VM (dev is gone, stormcentral#107: `SC_BUILD_VM=1 sc-build`,
      `stormcentral buildvm log <id>`): at 8c3b91e test/ short 5/5, medium
      14 + 1 skip, moving-window-1080p 61,788 B/frame (was 131,640), all
      exact; at e2cfdc8 workspace fmt --check, clippy -D warnings, 31 tests.
      The inflate-cap test needed a noise tile (cd26dc3). Test crate fmt
      still differs (#12; the build refiled it as #21)
- [x] docs (README, server crate doc, VALIDATION, DESIGN, deck), CHANGELOG
      (98ae1fc, e2cfdc8)

### Done — #13 stormcentral#56 references (2026-10-07)

- [x] README, CLAUDE.md shipping line, presentation (2 places): only
      stormcentral#63 blocks the runner (#56 closed 2026-09-28); dated
      done-items above keep #56 as history. #10's title and body updated

### Active — #12 test-crate lint (2026-10-07)

- [x] fix clippy's `is_multiple_of` at test/src/session.rs:80 (5d41ca4);
      sc-build at 15efd10: test crate clippy -D warnings passes
- [ ] rustfmt `test/`: at 15efd10 `fmt --check` failed but `cargo fmt`
      left no git diff; next run prints the check's own output
- [x] README "Build and test": the test crate's clippy and fmt commands
- [ ] sc-build: workspace + test crate clippy -D warnings and fmt --check
- BLOCKED 2026-10-07 ~12:20Z: dev.g8.lo unreachable (stormcentral#517, P0);
  both items proposed --after it

### Active — #11 slide overflow (2026-10-06)

Part 2 (stale clippy/rustfmt line) was done in 13581c5.
- [x] `tools/check-slides.sh` + `tools/slides.browser.cjs`: Marp → HTML,
      Playwright Chromium measures each `section` (scroll vs client box) and
      prints each slide as a base64 PNG line, so it can be looked at
- [ ] first run (3931eb3) failed in silent npm setup after 12 min; bc15d04
      makes each step print why; not run yet — dev down (stormcentral#517)
- [ ] split or trim any slide that overflows; rerun until none does

### Done — #1 QEMU Extended Key Event (-258), v0.2.0 (2026-10-06)

- [x] codec: `QEMU_EXTENDED_KEY` last in `ENCODINGS`; `ClientMessage::QemuKey`
      (255/0, u16 down, keysym, keycode); `qemu_keycode` (0xE0 → bit 7);
      `Rectangle::QemuExtendedKey` decoded and encoded
- [x] client: `extended_keys()`, `send(QemuKey)` gated, `key_event`
      (client `Event` unchanged — stormrdp matches it exhaustively)
- [x] server: `Event::QemuKey`; acks -258 once in the next update
- [x] tests (29 workspace tests at cabbc5a/66b39c8); `tools/verify-extkey.sh`
      green at ac4d991 against qemu 10.1.5: scancode-only typing ran a
      command (STORMRFB-EXTKEY-42 on serial), keysym `poweroff` worked.
      Found: qemu lower-cases uppercase keysyms on a graphic console
- [x] docs; v0.2.0 tagged at 66b39c8 (test/ short 5/5, medium 14+1 skip);
      browser follow-up #16 (blocked on stormcentral#64)

### Done — #4 phase 1 exit through the relay (2026-10-06)

Dev has qemu + KVM, OVMF, node/npm (Playwright's Chromium per job), and
reaches GitHub (stormvm is cloneable), Alpine's CDN and Microsoft's
Windows Server 2022 evaluation ISO, so the gate runs as one
`sc-build tools/verify-relay.sh`, nothing kept.
- [x] `tools/verify-relay.sh` + `tools/relay.browser.cjs`: stormconsole
      `cf2cbbb` + stormvm `4051696` built in the job, fastetcd + rustkube,
      qemu as stormvm renders it, stormvm's door, stormconsole's relay,
      the VM page in Chromium with stormrfb and noVNC on one session
- [x] green at e31cf8a (632 s): 5 screens pixel-exact in both clients,
      Alpine login/`clear` and Windows press-any-key + Alt+N through
      stormrfb; level on B/frame and ms/frame; chunk 96,725 vs 181,861 B.
      Lessons: noVNC 1.7 rejects a subclassed WebSocket (patch the
      prototype); Setup ignores keys for a while after drawing and its
      focus is the Language list (Enter does nothing, Alt+N works)
- [x] VALIDATION/README/DESIGN/CHANGELOG; filed stormconsole#99 (default
      switch + noVNC removal), stormconsole#94 (read-only VNC relay drops
      the handshake), stormvm#76 (no absolute pointer)

### Done — docs refresh since 2026-09-18 (2026-09-27)

Only doc comments and `test/` changed in code since 2026-09-18.
- [x] README: the test image's env configuration, its loopback sockets,
      how it ships (runner, stormcentral#56/#63/#64), docs list; stormvm
      door is `{ns}/{name}` plus a token (checked in both repos' code)
- [x] DESIGN phase 2 state: the in-process 1080p measurement
- [x] presentation: tests, interfaces, shipping and status slides
- [x] CLAUDE.md: status; dropped the stale clippy/rustfmt line (#11)
- [x] sc-build at 1392373: workspace clippy -D warnings and fmt pass;
      test crate does not (filed #12); stormconsole's console.rs comment
      says `{id}` (filed stormconsole#39)
- [x] sc-build at 13581c5: 25 tests pass; deck renders 12 slides
- [x] Re-check (2026-09-27, after issue validation): no code change since
      13581c5; Limits/ENCODINGS/test env match; added stormcentral#57
      (`/results` not collected) to README and test/README

### Done — #8 test container (2026-09-27)

stormrfb is a library with nothing of its own on a node (stormview's
case), so per stormcentral `docs/test-standard.md` the suites run the
commit's own server and client against each other over loopback TCP in the
pod, and say so. A real RFB server is optional (`STORMRFB_TARGET`), skip
when unset. stormvm's console door mints tokens only from node loopback,
and the real-guest path is #4.

- [x] `test/`: own workspace (`stormrfb-test`), `build.sh` (static musl),
      `Containerfile` (FROM scratch, `/test <suite>`), `stormrfb-test.yaml`
- [x] short: loopback session, VNC auth, fixture replays, encoding × pixel
      format matrix
- [x] medium: + input, resize, fragmentation, hostile input, small limits,
      concurrent sessions, 1080p moving window, optional real server
- [x] long: waves of concurrent sessions sized from cgroup CPU/memory;
      per-wave ms/frame, RSS, fds, threads; regression = failure
- [x] sc-build at 6ba5511: short 5/5, medium 14 + 1 skip, long (180 s)
      34 waves + trend, all exit 0; podman image 860 KB, `/test short` in
      it as uid 65532 with no network: 5/5; workspace 25 tests
- [x] `stormcentral test run stormrfb short` tried: run d105522374 errored
      before the image step, C2NR0Q2's apiserver never answered /readyz
      (stormview and stormcoredns too), filed stormcentral#63; the image
      step is also broken for everyone (stormcentral#56), and /results is
      not collected yet (stormcentral#57). Rerun when those are fixed
- [x] README/VALIDATION/CHANGELOG
- [x] closed #8 and #9 (sc-build's lockfile failure)

### Done — #3 presentation (2026-09-27)

- [x] `docs/presentation.md`: Marp deck, 8–15 slides, every claim from
      the code/README (the 17-slide pptx in `docs/presentations/` stays as
      the 2026-09-09 project review)
- [x] Graph slide matches `stormcentral check`: stormconsole → stormrfb;
      stormrdp's code pins stormrfb but the graph lists only stormvm —
      filed as stormcentral#54
- [x] Link from README and docs/presentations/README; CHANGELOG
- [x] sc-build at a5b5f73: 25 tests passed; Marp renders 12 slides on
      dev (`npx @marp-team/marp-cli`); #3 closed. Slide layout (overflow)
      not inspected visually: no browser on the build box

### Done — #2 docs from the code (2026-09-24, closed 2026-09-26)

- [x] CLAUDE.md: replace the stale root@dev build recipe with `sc-build`
- [x] README.md rewritten from the code (crates, APIs, Limits, subset,
      no ports/golden, how it ships)
- [x] docs/DESIGN.md: mark design-only parts; correct consumers
- [x] VALIDATION/PERFORMANCE/demos/web README: sc-build, downstream status,
      demo WebM not committed
- [x] Crate doc comments; CHANGELOG; follow-up issues #4–#7
- [x] `sc-build` rustdoc with `-D warnings` at c9bf72f passed
- [x] `sc-build 'cargo test --workspace --locked'` at 86f86d9 passed
      (2026-09-26, 25 tests, exit 0, no sc-build script errors); #2 closed

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
baseline. Setup/decode split and profiling identified redundant initialization,
tile allocations and per-pixel framebuffer copies. The optimized QEMU replay
measured 0.361 ms/frame vs 1.319 before and 0.967 for noVNC. Final
Chromium, native, 25-test Rust suite and sanitizer fuzz validation passed.
See `docs/PERFORMANCE.md`; remaining work is the downstream integration gates.

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
- [x] Pseudo-encodings: `Cursor` (-239, a.k.a. RichCursor), `DesktopSize`, `LastRect`
- [x] `stormrfb-client`: framebuffer state, damage rectangles, input
      translation
- [x] Native harness (feature-gated, unpolished): a window and a blit, so
      the client crate is debuggable outside WASM
- [x] `stormrfb-wasm`: wasm-bindgen binding, `ImageData` on dirty rects,
      DOM key/mouse → RFB, private npm package (publication intentionally disabled)
- [x] Conformance fixtures recorded from qemu's VNC server and TigerVNC;
      differential test against noVNC as the oracle
- [x] `cargo-fuzz` on the decoder
- [x] Exit: a Windows installer and a Linux guest legible in stormconsole
      (2026-10-06, #4: pixel-exact through the relay, driven by keys) with
      decode ms/frame, bytes/frame and chunk size measured against noVNC on
      the same session (level; 96.7 KB vs 181.9 KB). Removing
      `@novnc/novnc` is stormconsole#99

### Phase 2 — the server (stormvm#1)

- [x] `stormrfb-server`: framebuffer + damage → update messages
- [ ] vhost-user integration is stormvm's; this supplies the protocol
- [ ] **Measure** fps and bytes/s for a moving window on a 1080p guest
      (in-process protocol half measured 2026-09-27: 66 fps, 8.6 MB/s,
      docs/VALIDATION.md; the guest waits on stormvm#1)

### Phase 3 — the native viewer

Deferred by decision 3; only the development harness is implemented.

### Phase 4 — latency

- [ ] `ContinuousUpdates` + `Fence`, and whatever phase 1 measured as slow

## Integration, when phase 1 lands

stormconsole's VM page (`web/src/lib/views/VmDetail.svelte`) lazily imports
`@novnc/novnc` by default and the vendored stormrfb client with `?rfb=storm`. The door it
dials — `/api/plugins/vm/console/{ns}/{name}/vnc` — is a websocket relay
that passes frames through untouched and knows nothing about RFB. Swapping
the import is the whole integration; do not change the door.
