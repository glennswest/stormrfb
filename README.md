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

**Status: definition only.** `docs/DESIGN.md` is the whole project so far.
No code, no crates, no dependency on this from anything yet.

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
