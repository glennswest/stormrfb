# Decoder performance patch — 2026-09-09

The measured WASM slowdown is fixed on the recorded QEMU workload. The private
v0.1.0 tag preserves the baseline. Optimization commit `1c43752` changes buffer
handling, without changing the wire format, compression, advertised encodings,
framebuffer output, bounds checks or public API.

## What the profile identified

A native Linux sample profile of the unchanged replay (`perf`, cpu-clock,
999 Hz, 250 samples, release opt-level=s with symbols) showed substantial time
in `Vec::extend_with`, redundant output initialization and per-pixel framebuffer
copy/index operations. This was supporting evidence from the native build;
the actual before/after timing below measures the compiled WASM path in Node.

The patch:

- Reuses a bounded 64×64 tile scratch array and a 127-colour palette instead of
  allocating/growing vectors for every ZRLE tile.
- Zero-allocates rectangle output that the tile decoder will completely
  overwrite, eliminating the redundant opaque-black per-pixel initialization.
- Copies complete framebuffer rows in bulk, then normalizes alpha. This keeps
  the framebuffer allocation and address stable for browser ImageData views.

No unsafe code, SIMD requirement, relaxed validation or build-profile change
was introduced. Malformed/truncated data still fails, and partial rectangles
cannot escape the decoder as successful output.

## Repeated comparison

Host/date: 2026-09-09, designated x86_64 Linux development VM, 8 vCPUs, reported
Intel Core Ultra 7 270K Plus; Rust 1.95.0, Node 22.22.2, wasm-bindgen 0.2.128,
noVNC 1.7.0. Same committed QEMU 720×400 firmware fixture, two ZRLE updates
per session, 902 wire bytes/frame. No concurrent fuzz/profile workload ran
during these measurements.

Each result is the median of five rounds. Each round measures 200 sessions /
400 frames after 20 warmup sessions. The benchmark separates setup from
receive/decode/framebuffer work. Each column is independently medianed, so
rounded columns need not sum exactly. WASM decode includes JS input copying
and outgoing event construction; noVNC uses its unmodified decoder and an
in-memory display adapter. Neither includes canvas paint or network latency.

| Path | Total ms/frame | Setup ms/frame | Decode/framebuffer ms/frame |
|---|---:|---:|---:|
| v0.1.0 WASM baseline | 1.319 | 0.078 | 1.241 |
| Optimized WASM | 0.361 | 0.071 | 0.290 |
| noVNC, comparison run | 0.967 | 0.040 | 0.928 |

Total WASM time dropped **72.6%** (about **3.7×** throughput versus baseline).
The optimized path took **62.7% less time than noVNC**, about **2.7×** throughput
on this workload. The earlier single-round 1.27/0.91 ms numbers were replaced
by these repeated measurements, not mixed into the before/after calculation.

The smaller TigerVNC 73×69 striped fixture also matches its independent pixel
oracle: optimized WASM median **0.0187 ms/frame**, including setup. It is a
correctness/secondary workload check, not a representative desktop benchmark.
Native QEMU replay after optimization measured **0.452 ms/frame** in the
original 400-frame run; native allocator/page-fault costs differ from WASM.

## Size and correctness

| Package bytes | Baseline | Optimized |
|---|---:|---:|
| WASM | 89,559 | 88,564 |
| JS wrapper + generated glue | 18,234 | 18,234 |
| Total uncompressed | 107,793 | 106,798 |
| Sum of independently gzipped files | 44,878 | 44,718 |

Both QEMU and TigerVNC fixture framebuffer hashes remain identical to the
independent QMP/XGetImage references and noVNC. The full validation command
passed: 25 Rust tests, strict all-target/all-feature Clippy, release WASM,
2 Node tests, Chromium pixel/alpha/resize/memory-growth/input tests and the
native X11 harness. A sanitizer fuzz run of the optimized decoder completed
421,801 executions with a 45-second time budget, no crash, and 370 MB peak RSS.
A new regression checks dirty rows, untouched surrounding
pixels, alpha normalization and zero-width rectangles.

## Reproduce and limits

After pushing and pulling on the designated host, configure the external test
tools as described in [VALIDATION.md](VALIDATION.md):

```sh
sh tools/validate.sh
node tools/wasm-benchmark.mjs
NOVNC_ROOT=/path/to/external/novnc node tools/differential.mjs
node tools/wasm-benchmark.mjs tigervnc-zrle
```

These are static fixture CPU measurements. They do not establish real guest
latency, network throughput, animated 1080p performance or a production
stormconsole chunk size. The real Windows/Linux integration gates remain open;
keep noVNC deployed until those gates pass.
