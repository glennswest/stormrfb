# Changelog

## [Unreleased]

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
