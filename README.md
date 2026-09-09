# stormrfb

**RFB (RFC 6143) in Rust — one implementation, both ends.**

The wire protocol behind VNC: a client for the browser, a server for the
Rust VMM, and one codec underneath that neither owns.

- **Client** — what [stormconsole](https://github.com/glennswest/stormconsole)
  renders a VM's framebuffer with. Today that is
  [noVNC](https://github.com/novnc/noVNC), which works well; this replaces
  it when it is ready and not before.
- **Server** — what a VMM with no framebuffer of its own needs
  ([stormvm#1](https://github.com/glennswest/stormvm/issues/1): virtio-gpu
  + virtio-input + VNC as a vhost-user device). Cloud Hypervisor has no
  display; qemu has one. Something has to speak RFB on that side, and it
  is not a browser library's job.
- **Codec** — sans-I/O, shared by both. Two implementations of one wire
  format is the thing worth avoiding, and the server half is being written
  anyway.

**Status: initial implementation tested.** Private Rust codec, framebuffer
client, server session, canvas WASM binding and native development harness.
See [validation results](docs/VALIDATION.md) for tests, measurements and
remaining integration gates. stormconsole continues to use noVNC.

Tests run on the Linux development host using `cargo test --workspace`,
with `CARGO_TARGET_DIR` on its build volume. Push commits to GitHub and
pull them on that host before testing.

## Why not just keep noVNC

Mostly, keep noVNC — it is fifteen years of pixel formats, cursor
handling, resize and performance work, and stormconsole ships it today. Two
things eventually argue the other way, in this order:

1. **The server does not exist and is needed.** stormvm#1 wants a
   framebuffer for the Rust VMM path. noVNC cannot help with that; it is a
   client.
2. **Licence.** noVNC is MPL-2.0 in an MIT stack. That is fine for an
   unmodified library in its own lazily-loaded chunk, which is how
   stormconsole uses it, and it is *not* a reason to write anything on its
   own. It only stops being free if the code ever needs forking.

If only the second were true, this project would not exist.

## Integration, when it is ready

stormconsole's VM page lazily imports `@novnc/novnc` behind a probe; the
door it dials (`/api/plugins/vm/console/{ns}/{name}/vnc`) is a websocket
relay that passes frames through untouched and knows nothing about RFB.
Swapping the import is the whole integration.

See `docs/DESIGN.md` for scope, the protocol subset, architecture and
phasing.

The workspace contains `stormrfb` (wire codec), `stormrfb-client` (RGBA
framebuffer, damage and input), and `stormrfb-server` (one session per
connection, authentication and requested damage updates). Applications own
transport, authorization and fresh VNC-auth challenges. The codec owns no I/O.

`stormrfb-wasm` and `web/` provide the private canvas 2D package. See
[web/README.md](web/README.md) for usage. The optional development window
runs with `cargo run -p stormrfb-client --example viewer --features
native-harness -- HOST:PORT`; it is a mouse-driven debugging harness, not
a shipped viewer.
