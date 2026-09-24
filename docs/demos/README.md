# Framebuffer demonstration

The interactive demo is in `web/demo/`, served by `node tools/demo-server.mjs`.
See [web setup](../../web/README.md) for the Linux build and browser-test setup.

`tools/demo-test.mjs` drives Chromium with the actual release Rust/WASM
client. It writes a WebM recording and screenshots to `DEMO_OUTPUT`
(default `/build/cargo/stormrfb-demo`). Neither is committed to this repo.
The demo shows:

- Moving synthetic desktop encoded as Raw RFB rectangle updates.
- Damage overlay, pause/step, RGBA inspection and packet fragmentation.
- Recorded QEMU firmware screen decoded with persistent ZRLE and checked against
  its independent QMP reference hash, `06133f0cfae0b305`.

The animation is a synthetic input source, not a live VM or a full client/server
integration test. The displayed timing includes decode and Canvas paint calls,
but no network. It is not comparable to the presentation's CPU benchmarks.

Validation on 2026-09-09 used `tools/demo-test.mjs`, Node 22.22.2 and the existing
Chromium 153 runtime on the designated Linux host after commit, push and pull.
Checks passed for changed pixels, unchanged pixels outside damage, opaque alpha,
pause/step, fragmented Raw and ZRLE, pixel inspection, overlay toggle, QEMU hash
and restarting the animation. The first run hit a host `/tmp` permission problem,
resolved with a task-specific temporary directory on the build volume.
