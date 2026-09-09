# Changelog

## [Unreleased]

### 2026-09-09
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
