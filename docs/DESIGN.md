# stormrfb — the design

**Status: definition. Nothing is built.** This document exists so the work
can be picked up cold, and so the shape is decided before anyone types
`cargo new`.

## The one sentence

RFB (RFC 6143) implemented once, in Rust, with no I/O in the core — so the
same code decodes a framebuffer in a browser through WASM, encodes one in a
VMM on a node, and runs against a `Vec<u8>` in a test.

## Why this exists, honestly ranked

**1. The server is needed and does not exist.**
[stormvm#1](https://github.com/glennswest/stormvm/issues/1) asks for a
display for the Rust VMM — virtio-gpu + virtio-input + VNC as a vhost-user
device. Cloud Hypervisor has no framebuffer; qemu has one, which is why
stormvm's DESIGN.md picks qemu as the domain-5 reference VMM and notes that
choosing Cloud Hypervisor "gives up the framebuffer, Windows breadth and
post-copy today". If domain 3 is ever to have a screen, something on this
side has to speak RFB. A browser client cannot help with that.

**2. The client is a client, and we already have a good one.**
stormconsole renders a VM's framebuffer with noVNC today and it works. This
is not urgent and should not be treated as urgent.

**3. One wire format, one implementation.** Once the server exists, the
client is the same message set read in the other direction. Writing the
second one against a different codebase is how two things that must agree
stop agreeing.

**The licence is not reason enough on its own.** noVNC is MPL-2.0 in an MIT
stack, used unmodified as its own lazily-loaded chunk — the ordinary
arrangement, the one OpenShift, KubeVirt and Proxmox all ship. It becomes a
problem only if the code ever needs forking. If reason 1 disappeared, this
project should too.

## What it is not

- **Not a remote desktop product.** No audio, no file transfer, no
  session brokering, no NAT traversal, no rendezvous server. A guest-level
  remote desktop is a workload somebody runs *on* the platform, and a
  different layer entirely.
- **Not a replacement for the serial console.** Serial is bytes and works
  on a machine with no GPU; this is pixels. Both doors exist because both
  situations exist.
- **Not SPICE.** stormvm's DESIGN.md settles this: *"SPICE is not in scope
  — VNC is what every client speaks."*
- **Not a guest agent.** Nothing here runs inside the guest. The
  framebuffer comes from the VMM, which is exactly why it works on a blank
  disk, on a firmware screen, and on a Windows installer that has never
  been booted.

## Scope — the protocol, divided

The console's VNC door is already a websocket on the console's own origin,
carried over the console's own TLS and gated by the console's own
authorization. That removes a whole layer from the client: **RFB security
types and RFB-level TLS are somebody else's problem here.** It matters,
because the security-type zoo is a large fraction of what makes an RFB
client big.

### Must have

| Area | What |
|---|---|
| Handshake | RFB 3.8 version exchange, security type negotiation, `SecurityResult`, `ClientInit`/`ServerInit` |
| Security | `None` (1), and `VNC Authentication` (2) for a server that insists — DES challenge/response, all 16 bytes of it |
| Pixel formats | true-colour 32/16-bit, both endiannesses; `SetPixelFormat` to pin one rather than accepting whatever arrives |
| Encodings | `Raw` (0), `CopyRect` (1), `Hextile` (5), `ZRLE` (16), `TRLE` (15) |
| Client→server | `SetEncodings`, `FramebufferUpdateRequest` (incremental and full), `KeyEvent`, `PointerEvent`, `ClientCutText` |
| Server→client | `FramebufferUpdate`, `SetColourMapEntries`, `Bell`, `ServerCutText` |

`Raw` alone is enough to see a screen and is the correctness baseline every
other encoding is diffed against. `ZRLE` is what makes it usable over a
link that is not a loopback.

### Worth having, roughly in order

- **`DesktopSize` (-223) and `ExtendedDesktopSize` (-308)** — a console
  whose framebuffer does not follow the window is unpleasant to use, and
  `ExtendedDesktopSize` is how a client *asks* for a resize rather than
  only being told about one.
- **`Cursor` (-239) / `RichCursor` (-239)** — a client-side cursor. Without
  it, every mouse move is a framebuffer round trip and the pointer lags
  behind the hand.
- **`LastRect` (-224)** — lets a server end an update without a count.
- **`ContinuousUpdates` (-313) and `Fence` (-312)** — the pair that gets rid
  of request/response latency on a fast link.

### Deliberately out

- **Tight (7) and JPEG.** Patent-free now, but it drags in a JPEG decoder
  and a large amount of specification for a case ZRLE already covers on the
  links this runs over.
- **H.264 / open-h264 encodings.** The reason noVNC 1.7 needs a top-level
  `await` to probe for a hardware decoder. Not for a console.
- **Audio (QEMU audio pseudo-encoding), file transfer, tunnelling.**
- **RFB security types beyond None and VNC Auth** — TLS, VeNCrypt, SASL,
  ARD, UltraVNC's set. The transport is already secured a layer up.

## Architecture

The decision everything else follows from: **sans-I/O**. The codec never
reads a socket and never writes one. It is fed bytes and produces events;
it is asked for messages and produces bytes.

```
stormrfb            the codec. no I/O, no async, no allocation in the hot path
                    where it can be helped. message types, encode/decode, and
                    a state machine: bytes in → events out.

stormrfb-client     drives the codec as a client. tracks the framebuffer,
                    produces damage rectangles, translates input.

stormrfb-server     drives it the other way, for stormvm#1: takes a
                    framebuffer and damage, produces update messages.

stormrfb-wasm       the browser binding. wasm-bindgen, canvas ImageData (or
                    WebGL if measurement says so), DOM key/mouse → RFB
                    events, published as an npm package the way stormview
                    publishes both a crate and a package.

stormrfb-view       optional: a native viewer, so `stormvm vnc` is a real
                    command rather than "open the console in a browser".
```

Three properties this buys:

1. **One codebase for both ends.** The server is not a second
   implementation, it is the same message types constructed instead of
   parsed.
2. **Testable without a network.** Every conformance fixture is a byte
   slice and an expected event stream. No sockets in the test suite.
3. **Transport-agnostic.** A websocket in a browser, a Unix socket on a
   node, a TCP connection from a CLI — the codec cannot tell and does not
   ask.

### Where the pixels go

The client's output should be damage rectangles, not a full frame: the
whole point of `CopyRect` and incremental updates is that most of a screen
did not change. The WASM binding writes into an `ImageData` backed by the
same `ArrayBuffer` across frames and `putImageData`s the dirty rect, rather
than allocating a frame each time. If measurement says a WebGL texture beats
that, take it — but measure first, and record the number.

## Testing, because this is a wire format with an enormous installed base

- **Conformance fixtures.** Record real sessions — qemu's built-in VNC
  server, TigerVNC, and whatever stormvm ends up serving — and replay the
  bytes as fixtures with expected framebuffer checksums. This catches the
  encodings that are "obvious" and are not: Hextile's subrectangle flags and
  ZRLE's tile palettes are where implementations diverge.
- **Round-trip.** Every message type: encode → decode → compare. The server
  half makes this free.
- **Differential.** Decode the same session with noVNC and with this, and
  compare framebuffer hashes per update. While noVNC is the thing shipping,
  it is also the oracle.
- **Fuzz the decoder.** Non-negotiable. A framebuffer decoder consumes
  length-prefixed data whose lengths are attacker-influenced — a guest
  controls what it draws, and a compromised VMM controls the stream
  outright. Rust removes the memory-safety class; it does not remove
  `usize` overflow in a rectangle calculation, an allocation sized from a
  hostile field, or a decode loop that never terminates. `cargo-fuzz` from
  phase 1, not bolted on later.

## Security notes

- The RFB stream arrives from the VMM. On this platform that is a process
  the node runs, so it is not hostile in the way a public VNC server is —
  **but the pixels are the guest's**, and so are the sizes and encodings it
  provokes. Treat every length as untrusted.
- The client must never be given credentials it does not need. Because the
  console's door is authorized at the console, `None` is the expected
  security type on this platform; VNC Auth exists for talking to other
  people's servers, and its DES challenge is not a secret worth protecting
  by that point.
- No `unsafe` in the codec without a comment saying what invariant is being
  asserted and why the bounds check that would replace it is too expensive.
  In a decoder, that bar should almost never be met.

## Phasing

Each phase ends with something that runs and a number that was measured, in
the tradition the rest of these projects follow.

**Phase 0 — this document.** Decisions recorded.

**Phase 1 — a client that replaces noVNC.** Codec, client, WASM binding.
Raw, CopyRect, Hextile, ZRLE; cursor; `DesktopSize`. Exit: a Windows
installer and a Linux guest are both legible in stormconsole with
`@novnc/novnc` removed from `web/package.json`. **Measured:** decode
milliseconds per frame and bytes per frame against noVNC on the same
recorded session, and the size of the shipped chunk against noVNC's 182 KB.

**Phase 2 — the server**, for stormvm#1. Takes a framebuffer and damage
from virtio-gpu, produces updates. **Measured:** frames per second and
bytes per second for a moving window on a 1080p guest.

**Phase 3 — the native viewer.** `stormvm vnc` opens a window rather than a
browser tab.

**Phase 4 — the latency work.** `ContinuousUpdates` + `Fence`, and whatever
the phase-1 measurements said was actually slow.

## Open decisions

1. **Canvas vs WebGL** for the browser. Start with `putImageData` on dirty
   rects; it is simple and likely enough at console resolutions. Revisit
   with a number, not an opinion.
2. **Public or private repo.** Created private. It is a generic protocol
   implementation with no infrastructure detail in it, which is the profile
   of every public repo here (`stormview`, `fastetcd`, `mkfs.ext4.rs`), so
   public is probably right eventually — and unpublishing is not a thing,
   which is why it starts closed.
3. **Whether `stormrfb-view` is worth it**, or whether `stormvm vnc`
   shelling out to a system viewer against a local port is enough. The
   native viewer is the smallest of the four crates and the least load-
   bearing.
4. **TRLE.** Listed as a must because ZRLE is defined in terms of it, but
   almost nothing sends bare TRLE. It may end up as an implementation
   detail of ZRLE rather than a first-class encoding.

## What this depends on, and what depends on it

Depends on: nothing in this stack. It is a protocol implementation and
should stay one — no `stormview`, no `console-core`, no platform types in
the codec.

Depended on by, eventually: `stormconsole` (the graphical console tab, one
import away) and `stormvm` (the Rust VMM display, stormvm#1). Neither today.
