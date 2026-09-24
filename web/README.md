# Private browser package

## Interactive demonstration

After building WASM on the Linux host, run `node tools/demo-server.mjs` from the
repository root. It binds only to loopback on port 8765. Forward that port over
SSH to view `/web/demo/index.html` from the workstation.

The moving window is a synthetic Raw RFB source fed through the actual compiled
Rust/WASM client. It illustrates incremental damage, opaque RGBA pixels and
fragmented input. Play, pause, step, inspect pixels, or replay the independent
QEMU firmware capture using persistent ZRLE. The capture's pixel hash is checked
in the browser. This is a decoder demonstration, not a live guest or network
benchmark. No production client APIs are changed.

`PLAYWRIGHT_ROOT=/path/to/playwright node tools/demo-test.mjs` checks the demo in
Chromium and records a WebM plus screenshots under `/build/cargo/stormrfb-demo`
(override with `DEMO_OUTPUT`). Use the existing Playwright browser-cache setting.
The 2026-09-09 run on the dev host (before `sc-build`, which cannot build the
WASM package today; see [VALIDATION.md](../docs/VALIDATION.md#reproduce)) used `PLAYWRIGHT_BROWSERS_PATH=/build/cache/stormrfb-browsers`,
`PLAYWRIGHT_ROOT=/build/tools/stormrfb-js/node_modules/playwright` and a writable
build-volume `TMPDIR` such as `/build/cargo/stormrfb-demo/tmp` (create it first).

## Browser integration

Build on Linux with `npm run build` (requires wasm-bindgen-cli matching Cargo.lock, 0.2.128, and the
wasm32-unknown-unknown Rust target, with CARGO_TARGET_DIR set). Serve this directory through the
application's normal same-origin HTTP server; import `connect` from `client.js`. stormconsole
vendors `client.js` and `pkg/` into `web/src/lib/vendor/stormrfb/` and records the source commit
in `VERSION`.

```js
import { connect } from './client.js';
const session = await connect(document.querySelector('canvas'), authorizedWebSocketURL, {
  onready: name => console.log(name),
  onerror: error => console.error(error),
});
// On component destruction:
session.close();
```

The caller supplies the authorized websocket URL, handles clipboard permission
and reports connection errors. Options: `password`, `onready(name)`, `onclipboard(text)`,
`onbell()`, `onerror(error)`. The returned session has `close()` and `clipboard(text)`.
The canvas is sized to the guest framebuffer; scaling is the page's CSS. Text clipboard is
Latin-1 as specified by RFB.
Passwords are encoded as UTF-8 bytes and truncated to eight bytes by VNC Auth.
DOM dead keys and composition/IME are not implemented. The package is private;
no npm publication or stormconsole replacement is performed by this build.
