# stormrfb-test

stormrfb's test container, per stormcentral
[`docs/test-standard.md`](https://github.com/glennswest/stormcentral/blob/main/docs/test-standard.md)
(#8). There is one image for all three suites: `/test short|medium|long`.

## What it tests, and what it does not

stormrfb is a library. It has **nothing of its own on a node**: no daemon,
no port and no API. It runs inside stormconsole's browser package and
inside stormrdp. So, as the standard asks of components that do not run on
the node themselves, the suites test what the library does, where it runs:

- the commit's own `stormrfb-server` and `stormrfb-client` talk to each
  other over **real TCP on the pod's loopback**;
- the server paints a deterministic scene (a full screen, then a moving
  window), and the client's framebuffer is checked **pixel for pixel**;
- the recorded QEMU and TigerVNC streams (`fixtures/`) are decoded against
  their independently captured hashes.

**No test creates anything in the cluster or uses the API**, and none
needs hardware (`requires: []`). A real RFB server is read only when
`STORMRFB_TARGET=host:port` names one. Otherwise that test is `skip`, not
`pass`.

**Not covered here:**
- A real guest through stormconsole's relay (stormrfb#4). stormvm's
  console door (`:9095`) mints tokens only from the node's loopback, so a
  pod cannot open it.
- The browser/WASM package. The build box has no `wasm32` target (see
  docs/VALIDATION.md).

## Suites

| suite | budget | tests |
|---|---|---|
| `short` | < 2 min | `loopback-session` (640x480, 30 frames exact over TCP) · `vnc-auth` (the right password is admitted, a wrong or missing one refused) · `fixture-qemu`, `fixture-tigervnc` · `encodings-and-formats` (Raw, Hextile and ZRLE × 32 bpp LE/BE and 16 bpp LE/BE, exact) |
| `medium` | < 30 min | short, then `input-events` (keys, pointer clamped to the screen, cut text both ways, bell) · `resize` (DesktopSize mid-stream) · `fragmentation` (one byte per write, both ways) · `hostile-client-bytes`, `hostile-server-bytes` (each must end the session with the right error, never a panic, and leave it failed or closed) · `mutated-fixtures` (reproducible mutations of the recorded streams, for up to 5 min: no panic) · `small-limits` · `concurrent-sessions` (2 per CPU, 1280x720, all exact) · `moving-window-1080p` (fps and bytes/s reported, exactness gated) · `real-server` (optional) |
| `long` | the night window | waves of concurrent 1280x720 sessions, 1–3 per CPU in a 1,2,3,2 cycle, 60 exact frames each, until `STORM_TIMEOUT` less a margin. A wave fails if its median ms/frame is over 1.5× the first wave of the same size (and over 0.5 ms more), or if the drained process holds more RSS (beyond max(64 MiB, 25%)), threads or fds than after the first wave. `waves-trend` names the first wave that regressed |

Capacity comes from the pod: `available_parallelism()` (which respects the
CPU quota) and the smaller of the cgroup memory limit and `MemAvailable`. A
session is budgeted at 24 MiB, and at most a quarter of memory is used.

## Output

As the standard asks, stdout has one JSON object per test,
`{"test","status","ms","detail"}`, then `{"summary":{"pass","fail","skip"}}`.
The same lines go to `/results/results.jsonl`. `long` also writes one line
per wave to `/results/waves.jsonl`. stormcentral's runner does not collect `/results` yet
(stormcentral#57). Set `STORMRFB_RESULTS` to use another
directory. The exit code is 0 when everything passed, 1 when a test failed,
and 2 when the suite could not run (no loopback, or `STORMRFB_TARGET`
unreachable).

## Build and run

```sh
test/build.sh                    # static musl binary → test/.stage/
podman build -f test/Containerfile --build-arg COMMIT=$(git rev-parse HEAD) -t stormrfb-test .
stormcentral test run stormrfb short
```

On the build box, without a cluster:

```sh
sc-build 'test/build.sh && STORMRFB_RESULTS=$PWD/results test/.stage/stormrfb-test short'
```

`test/` is its own cargo workspace, as `fuzz/` is. The crates'
`cargo test --workspace` does not build it, and the stormrdp pins never
see it. `test/Cargo.lock` holds the same dependency versions as the root
lock.
