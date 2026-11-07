# Changelog

## [Unreleased]
<!-- New unreleased changes go here -->

## [v0.3.0] — 2026-11-07

### Added
- ContinuousUpdates (-313) and Fence (-312), client and server, with a fence window on the server (#7)
- ExtendedDesktopSize (-308) and SetDesktopSize: the client can ask for a size (#6)
- `stormrfb-vt`: a VM's display full-screen on a Linux VT (#25)
- `Client::pause`/`resume`/`updates`/`continuous` (#25, #7)
- Real Hextile/ZRLE tile subencodings in `ServerEncoder` (#5)

### Fixed
- `Server::new` built the initial layout after moving `init` (#6)

### Breaking
- New variants: `Rectangle::ExtendedDesktopSize`, `ClientMessage::SetDesktopSize`, `ClientMessage::Fence`, `ClientMessage::EnableContinuousUpdates`, `ServerEvent::Fence`, `ServerEvent::EndOfContinuousUpdates`, `stormrfb_server::Event::SetDesktopSize`; `Client::pause()` returns `Result<Vec<u8>>` (#6, #7)

### Daily log
### 2026-11-07
- **feat:** ContinuousUpdates (-313) and Fence (-312), both ends (#7, owner's decision 2026-10-07). Codec: `CONTINUOUS_UPDATES`, `FENCE`, `FENCE_BLOCK_BEFORE`/`_BLOCK_AFTER`/`_SYNC_NEXT`/`_REQUEST`, `FENCE_FLAGS`, `FENCE_MAX_PAYLOAD` (64); `ClientMessage::Fence` (248) and `EnableContinuousUpdates` (150); `ServerEvent::Fence` (248) and `EndOfContinuousUpdates` (150), decoded and `encode_control`led; both in `ENCODINGS` before -308 (-258 stays last). Client: answers fence requests, turns continuous updates on once the server has sent a fence and EndOfContinuousUpdates, sends no update requests while they are on, re-enables after a resize, falls back to requests if the server ends them; `continuous()`; `send()` refuses `Fence`/`EnableContinuousUpdates`. Server: announces both after `SetEncodings`, answers client fences, pushes updates for the enabled area without requests (incremental requests ignored, non-incremental answered), a fence request after each, held while unanswered bytes reach `set_window` (`DEFAULT_WINDOW` 1 MiB); `continuous()`, `in_flight()`; a resize keeps them on over the new framebuffer; disabling is answered with EndOfContinuousUpdates
- **BREAKING:** public enums gained variants (`ClientMessage::Fence`, `ClientMessage::EnableContinuousUpdates`, `ServerEvent::Fence`, `ServerEvent::EndOfContinuousUpdates`), and `Client::pause()` now returns `Result<Vec<u8>>`, the bytes to send (#7; `pause` is new since v0.2.0, from #25)
- **test:** codec wire layout, fragmentation and 64-byte bounds for both messages; server sessions for the fence window, ignored incremental requests, answered client fences, the encodings both need, pause/resume with continuous updates; stormrfb-vt's viewer tests now run with continuous updates on; `test/` medium gains `latency-50ms` (a delaying TCP proxy, 25 ms each way; updates/s request/response vs continuous) (#7)
- **docs:** README protocol table, client and server sections; DESIGN phase 4; test/README; VALIDATION: latency-50ms on a build VM at f4e50ae, request/response 18.3 updates/s vs continuous 73.5 (4.0x), both exact (#7)
- **feat:** `stormrfb-vt`, a VM's display full-screen on a Linux VT (#25, stormcos#228). New crate `crates/stormrfb-vt` (binary + lib): RFB from stormvm's `/run/stormvm/<ns>/<name>/vnc.sock` (`--vm`), any Unix socket (`--socket`) or TCP (`--connect`); KMS dumb buffers with page flip (copy + DIRTYFB on simpledrm), the guest scaled to fit with its aspect kept and its Cursor pseudo-encoding drawn at the pointer; ExtendedDesktopSize asks once for the screen's size (`--no-resize`); evdev keyboards and pointers (abs and rel, wheels, hotplug rescan every 2 s) to QEMU extended key events (XT scancodes) where acknowledged, US-layout keysyms otherwise; the VT in KD_GRAPHICS, VT_PROCESS and K_OFF, with Alt+Fn / Ctrl+Alt+Fn switching done by the viewer (`--switch ctrl-alt` lets Alt+Fn through to the guest); release lets go of held keys and buttons, closes the devices, drops DRM master and pauses updates; acquire takes them back and shows nothing until a whole new frame has arrived; a status screen and reconnect with backoff (1–30 s) while the VM is down. DRM/VT code and the Terminus font are adapted from stormpanel
- **feat:** `Client::pause()`, `paused()`, `resume()` (a full non-incremental request) and `updates()` (completed FramebufferUpdates) in `stormrfb-client`; additions only, `Event` unchanged (#25)
- **test:** `crates/stormrfb-vt/tests/viewer.rs`: the viewer against `stormrfb-server` — known frame pixel for pixel on a 2x screen, nothing shown before every pixel arrives, keystrokes (scancode + keysym, shift, E0), click/relative/wheel at the guest position, release/acquire with no stale frame and no stuck keys, switch keys in both modes, resize to the screen; unit tests for the fit, sprite, keymap, evdev decoding and ioctl numbers; a server session test for pause/resume (#25)
- **build:** workspace `default-members` leaves out `stormrfb-wasm`, so the musl image build (`cargo build --release --target x86_64-unknown-linux-musl`) has no `cdylib` warning; `--workspace` still covers it (#25)
- **style:** rustfmt the `stormrfb-vt` code (the build VM's `fmt --check` diff, 28 hunks) (#25)
- **docs:** README: the console viewer, the client's pause/resume, how `stormrfb-vt` ships (#25)

### 2026-10-10
- **chore:** rustfmt the #6 code in the workspace (6 hunks the build VM printed); `cargo fmt --all --check` failed on it (#26)
- **chore:** test crate now passes `cargo fmt --check` and `cargo clippy --all-targets -D warnings` on the build VM (#12)

### 2026-10-07
- **fix:** `Server::new` built the initial screen layout from `init` after moving it; first build of #6 (E0382) (#6)
- **feat:** ExtendedDesktopSize (-308) and SetDesktopSize (251): the client can ask for a resize (#6). Codec: `EXTENDED_DESKTOP_SIZE` (advertised in `ENCODINGS` before -258, which stays last), `Screen`, `valid_layout`, `Rectangle::ExtendedDesktopSize { reason, status, width, height, screens }`, `ClientMessage::SetDesktopSize`, `RESIZE_*` reason and status constants, encode and decode. Client: `desktop_resize()`, `screens()`, `request_resize(w, h)`, `resize_status()`; a same-size layout does not resize or clear the framebuffer; the client `Event` enum is unchanged. Server: sends the layout once after -308 is advertised, sends `resize` as -308 when negotiated, reports requests as `Event::SetDesktopSize` for `accept_resize()`/`refuse_resize(status)`, and refuses requests outside `Limits` (2) or with an invalid layout (3) by itself. WASM `can_resize`/`resize`/`resize_status`; `web/client.js` `connect()` returns `resize(w, h)`
- **BREAKING:** public enums gained variants: `stormrfb::Rectangle::ExtendedDesktopSize`, `stormrfb::ClientMessage::SetDesktopSize`, `stormrfb_server::Event::SetDesktopSize` (#6)
- **test:** ExtendedDesktopSize/SetDesktopSize round trips, wire layout, fragmentation and bounds; framebuffer same-size handling; server/client sessions for announce, accept, refuse, automatic refusals, server resize as -308, and SetDesktopSize without negotiation (#6). `web/client.test.js` gained a resize test that cannot run until the build box has the WASM tools (stormcentral#64)
- **docs:** README, DESIGN, VALIDATION and the deck describe ExtendedDesktopSize (#6)
- **feat:** `ServerEncoder` chooses each tile's subencoding by size instead of sending raw tiles (#5). Hextile: background only (an empty tile when the background carries), one foreground in subrectangles, coloured subrectangles, raw when no smaller; bg/fg are respecified after a raw tile, fg after a coloured one. ZRLE: smallest of solid, packed palette (1/2/4-bit), plain RLE, palette RLE and raw per 64×64 tile. Colours are compared as wire pixels, so formats that merge colours (16 bpp) merge them in tiles too. Unit tests check each choice and round-trip eight scenes × five sizes × four pixel formats through the decoders. Measured (test/ medium, build VM, 8c3b91e): `moving-window-1080p` 61,788 bytes/frame against 131,640 with raw tiles, all 300 frames exact; `encodings-and-formats` exact in every encoding × format
- **test:** `inflated_data_is_bounded_and_failure_is_terminal` uses a noise tile, which the encoder still sends raw, so the inflate cap is still what it hits (#5)
- **docs:** stormcentral#56 (the runner's image step) was closed on 2026-09-28, so README, CLAUDE.md and the deck now name only stormcentral#63 as what keeps the test image from running through the runner (stormcentral#57 still keeps `/results`). #10's title was updated to match (#13, stormcos#65)
- **style:** `test/` formatted as `cargo fmt --manifest-path test/Cargo.toml --all --check` asked (long.rs, session.rs, suites.rs; no code change) (#12)
- **fix:** `test/`: `is_multiple_of` where clippy asked for it; the test crate passes `clippy -D warnings` (#12)
- **docs:** README "Build and test" lists the test crate's clippy and fmt commands (it is its own workspace), and the suite is 29 tests (#12)
- **test:** `tools/check-slides.sh` + `tools/slides.browser.cjs`: Marp renders the deck, and headless Chromium measures every slide for overflow and prints it as a PNG in the log (#11)

## [v0.2.0] — 2026-10-06

### Added
- QEMU Extended Key Event (-258): `ClientMessage::QemuKey`, `qemu_keycode`, `Rectangle::QemuExtendedKey`, `Client::extended_keys`/`key_event`, `stormrfb-server` `Event::QemuKey` and its acknowledgement (#1)
- `tools/verify-relay.sh` (real guests through stormconsole's relay, #4) and `tools/verify-extkey.sh` (scancodes against a real qemu, #1)

### Breaking
- Public enums gained variants: `stormrfb::ClientMessage::QemuKey`, `stormrfb::Rectangle::QemuExtendedKey`, `stormrfb_server::Event::QemuKey`. `ENCODINGS` gained `QEMU_EXTENDED_KEY`. `stormrfb_client::Event` is unchanged


### 2026-10-06 (QEMU Extended Key Event, #1)
- **feat:** `stormrfb`: `QEMU_EXTENDED_KEY` (-258) is advertised last in `ENCODINGS`. New `ClientMessage::QemuKey { down, keysym, keycode }` (message 255, submessage 0; encode and decode, with other submessages refused and the down flag checked). `qemu_keycode(make_code, extended)` builds the keycode with the 0xE0 prefix as bit 7, as qemu and noVNC use it. New `Rectangle::QemuExtendedKey`, the server's acknowledgement, decoded as qemu sends it and encoded by `ServerEncoder`
- **feat:** `stormrfb-client`: `Client::extended_keys()` is set by the acknowledgement. `send(QemuKey)` before it is `Unsupported(-258)`. `Client::key_event(down, keysym, keycode)` sends `QemuKey` when it can and `Key` otherwise. `Event` is unchanged
- **feat:** `stormrfb-server`: decodes `QemuKey` into `Event::QemuKey`, and acknowledges -258 once, in the next update, even when there is no damage
- **test:** wire bytes and keycodes, hostile submessage and flag values, decoding qemu's own acknowledgement, the client↔server acknowledgement and fallback. `tools/verify-extkey.sh` with `examples/qemu_keys` types at a real qemu's VNC server by scancode only, with keysym 0 and keypad Enter for E0, and checks the shell's output on the serial line
- **docs:** README protocol subset and client/server APIs, DESIGN, VALIDATION. The browser's use is #16

### 2026-10-06 (phase 1 exit through the relay, #4)
- **test:** `tools/verify-relay.sh` + `tools/relay.browser.cjs`, run with `sc-build tools/verify-relay.sh`. In the job it builds stormconsole (`cf2cbbb`) and stormvm (`4051696`) and starts fastetcd + rustkube with the KubeVirt CRDs. It boots the Alpine 3.20 virt ISO and the Windows Server 2022 evaluation ISO under qemu/KVM as stormvm's qemu driver renders them, behind stormvm's real door and stormconsole's real relay. It then drives the VM page's Graphical console in headless Chromium with the vendored stormrfb and noVNC on the same session: canvases are checked against qemu's screendump, keys are typed through stormrfb, and bytes and handler ms per frame are measured for both clients, along with the shipped chunk sizes
- **docs:** VALIDATION: the 2026-10-06 result. All five screens are pixel-exact in stormrfb. Alpine was logged into and Windows booted and driven to Setup's second page from stormrfb. The two clients are level on bytes and ms per frame on live guests, and stormrfb's chunk is 96,725 B (43,042 gzip) against noVNC's 181,861 (54,420). README, DESIGN and CLAUDE.md now say the gate passed and the default switch is stormconsole#99. Found on the way: stormconsole#94 (a read-only viewer's VNC relay drops the handshake) and stormvm#76 (no absolute pointer)

### 2026-09-27 (docs refresh, re-check)
- **docs:** re-checked README, docs/ and CLAUDE.md against the code: no code change since 13581c5 and `Limits`, `ENCODINGS` and the test image's environment match. README and test/README now say that stormcentral's runner does not collect `/results` yet (stormcentral#57), so `long`'s `waves.jsonl` is not reported through it

### 2026-09-27 (docs refresh)
- **docs:** refresh from the code since 2026-09-18. README: the test image's environment configuration (`STORM_TIMEOUT`, `STORMRFB_TARGET`, `STORMRFB_PASSWORD`, `STORMRFB_RESULTS`, with defaults) and loopback sockets, how the test image ships (stormcentral's runner; not yet run there, #10), stormvm's console door is `/api/v1/vms/{ns}/{name}/console/vnc` after a token mint (was `{id}`), build-box tools are stormcentral#64. DESIGN: the phase 2 state records the in-process 1080p measurement. Presentation: tests, interfaces, shipping and status slides current. CLAUDE.md: status and open issues; the stale "build box lacks clippy/rustfmt" line removed (#11)

### 2026-09-27 (test container)
- **test:** `test/`, the stormcos test image per stormcentral `docs/test-standard.md`: static musl `stormrfb-test` (its own workspace and lock), `build.sh`, `Containerfile` (FROM scratch, `/test short|medium|long`), `stormrfb-test.yaml` (Job and metadata: `requires: []`, no API, optional `STORMRFB_TARGET`). short: loopback server↔client session, VNC auth, fixture replays, Raw/Hextile/ZRLE × four pixel formats; medium: input, resize, one-byte fragmentation, hostile bytes both ways, mutated fixtures, small limits, concurrent sessions, 1080p moving window, optional real server; long: waves of concurrent sessions sized from the pod's cgroup, failing on slowdown or RSS/thread/fd residue (#8)
- **fix:** `test/Cargo.lock` committed so `test/build.sh --locked` builds (#9)
- **docs:** README, VALIDATION (suite results and the in-process 1080p moving-window measurement: 66 fps, 8.6 MB/s on dev, 8 vCPUs), test/README (#8)

### 2026-09-27
- **chore:** verified #3: `sc-build` cargo test (25 passed) and Marp render (12 slides) at a5b5f73
- **docs:** `docs/presentation.md`, a 12-slide Marp deck on purpose and functionality drawn from the code: problem, place in stormcentral's graph, architecture, codec/client/server features, Limits, tests and measurements, interfaces, shipping, planned work and status; linked from README and `docs/presentations/README.md` (#3)

### 2026-09-26
- **chore:** verified #2 with `sc-build 'cargo test --workspace --locked'` at 86f86d9 (25 tests passed); work plan updated

### 2026-09-24
- **docs:** rewrite README from the code: crates, public APIs, `Limits` defaults (the only configuration), protocol subset, no ports/golden, and shipping through stormconsole's vendored package and stormrdp's git pins (#2)
- **docs:** DESIGN marks unbuilt parts (ExtendedDesktopSize, ContinuousUpdates/Fence, native viewer, `$VNCVIEWER` launch) and records current consumers; VALIDATION/PERFORMANCE reproduce through `sc-build` and say which checks cannot run there; demo README no longer claims a committed WebM (#2)
- **docs:** crate docs state the protocol subset; `Renderer` documented as unused in this workspace (#2)
- **docs:** CLAUDE.md builds with `sc-build` instead of `ssh root@dev` (#2)

- **demo:** add an interactive real-WASM framebuffer animation with damage overlays, RGBA inspection, fragmented packet mode and independently checked QEMU ZRLE replay

- **docs:** explain RFB, RGBA and damage in the project deck and remove noVNC deployment-status wording

- **docs:** add an editable project review presentation with implementation challenges, architecture/testing diagrams, elapsed-time and token accounting, measured results and optimizations

## [v0.1.1] — 2026-09-09

### 2026-09-09
- **docs:** record optimized WASM at 0.361 ms/frame versus 1.319 baseline and 0.967 noVNC on the same QEMU fixture; 25 Rust tests, browser/native validation and 421,801 optimized sanitizer fuzz executions passed
- **test:** extend WASM measurement to both independent fixtures and verify dirty-row alpha/untouched-region behavior
- **perf:** reuse bounded ZRLE tile/palette scratch, avoid redundant full-rectangle fills and copy framebuffer rows in bulk; retain alpha normalization and bounds checks
- **test:** establish repeatable median performance comparisons separating session setup from decode/framebuffer work

## [v0.1.0] — 2026-09-09

### 2026-09-09
- **chore:** tag the tested private implementation as v0.1.0; all package and lockfile versions synchronized
- **docs:** record completed scope, Linux/WASM/browser/native validation, independent fixture results, measured performance and outstanding downstream gates; add repeatable validation command
- **fix:** accept pre-resize client requests until DesktopSize is delivered
- **test:** replay TigerVNC pixels, enforce decompression bounds and measure compiled WASM against the same QEMU fixture
- **test:** record TigerVNC striped-screen fixture and fuzz dependency lock after independent capture
- **fix:** preflight incomplete Hextile payloads before framebuffer allocation, check encoder size arithmetic and initialization byte limits
- **test:** add bounded-frame native harness mode for automated window smoke tests
- **test:** add isolated TigerVNC striped-screen capture with XGetImage pixel oracle; generalize noVNC fixture replay
- **test:** expand fuzzing through valid zlib framing into arbitrary tile subencodings; remove per-pixel allocation from the noVNC oracle benchmark shim
- **fix:** enforce rectangle budget for LastRect sentinel updates
- **feat:** negotiate server DesktopSize updates with full refresh after resize
- **test:** add real Chromium canvas validation including WASM memory growth and resize
- **test:** commit QEMU fixture regression against QMP checksum, noVNC differential runner, native replay benchmark and reproducible WASM build script
- **test:** add disposable QEMU capture tool with independent QMP framebuffer checksum and persistent ZRLE session recording
- **feat:** private WASM/browser package with canvas dirty-region rendering, input and cursor handling, real-WASM Node tests and optional X11 development harness
- **fix:** simplify session branches flagged by strict Clippy after 18 passing Linux tests
- **feat:** framebuffer client, renderer seam, DOM key translation and sans-I/O server sessions; byte-fragmented authenticated end-to-end tests and overlap-copy regression tests
- **fix:** remove redundant test clones reported by Linux strict Clippy; all 12 codec tests passed
- **feat:** Raw, CopyRect, Hextile, persistent ZRLE, cursor/resize/LastRect and control messages; shared rectangle encoder, tile conformance tests and decoder fuzz target
- **feat:** private Rust workspace, bounded client messages, true-colour pixel conversion, RFB 3.8 client handshake and VNC DES authentication; fragmentation and known-answer tests
- **docs:** review protocol invariants, correct RGBX/alpha and deferred viewer scope, record implementation sequence
- **docs:** `docs/DESIGN.md` — the definition. RFB in Rust, sans-I/O codec,
  a client for stormconsole and a server for stormvm#1's Rust-VMM display;
  the protocol subset divided into must / worth-having / deliberately out;
  phasing with a measured exit per phase
- **chore:** repo created. No code yet, and nothing depends on this
