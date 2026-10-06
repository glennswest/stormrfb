#!/usr/bin/env bash
# Phase 1 exit (#4): a Linux guest and a Windows installer, driven through
# stormconsole's VM relay with stormrfb, beside noVNC on the same session.
#
#   sc-build tools/verify-relay.sh
#
# Everything is real except the node: no kubelet and no stormpump run on the
# build box, so this script starts qemu itself with the arguments stormvm's
# qemu driver renders for a framebuffer (q35, KVM, OVMF, -nodefaults,
# virtio-vga, `-vnc unix:<run>/<ns>/<name>/vnc.sock`, QMP on control.sock),
# writes the registration `stormvm start` would write, and writes the VMI
# status the kubelet would. The rest is the real thing at pinned revisions:
#
#   Chromium ⇄ stormconsole /api/plugins/vm/console/default/<vm>/vnc   (relay)
#            ⇄ stormvm serve /api/v1/vms/default/<vm>/console/vnc     (door)
#            ⇄ qemu's VNC server on vnc.sock
#
# stormrfb is the build stormconsole vendors (web/src/lib/vendor/stormrfb),
# i.e. what ships. noVNC is stormconsole's own dependency. The guests are
# Alpine's virt ISO and Microsoft's Windows Server 2022 evaluation ISO
# (downloaded per run; nothing is kept). tools/relay.browser.cjs is the
# browser half: legibility is checked as pixels against qemu's own
# screendump, input is typed through stormrfb, and both clients' bytes and
# decode time are measured on the same session.
set -euo pipefail

STORMCONSOLE_REV=${STORMCONSOLE_REV:-cf2cbbb89eb70d5fea0683c6cbb6bdfbe0395042}
STORMVM_REV=${STORMVM_REV:-4051696e3807f8b59f5a8f916eb8ecddb6d82e22}
FASTETCD_VER=${FASTETCD_VER:-v1.2.0}
RUSTKUBE_VER=${RUSTKUBE_VER:-v0.15.3}
ALPINE_ISO=${ALPINE_ISO:-https://dl-cdn.alpinelinux.org/alpine/v3.20/releases/x86_64/alpine-virt-3.20.3-x86_64.iso}
WINDOWS_ISO=${WINDOWS_ISO:-https://go.microsoft.com/fwlink/p/?LinkID=2195280&clcid=0x409&culture=en-us&country=US}
OVMF_CODE=${OVMF_CODE:-/usr/share/OVMF/OVMF_CODE.fd}
OVMF_VARS=${OVMF_VARS:-/usr/share/OVMF/OVMF_VARS.fd}

HERE=$(cd "$(dirname "$0")" && pwd)
W=$(mktemp -d "${TMPDIR:-/tmp}/vr.XXXXXX")  # short: unix socket paths live under it
cleanup() { kill $(jobs -p) 2>/dev/null || true; wait 2>/dev/null || true; rm -rf "$W"; }
trap cleanup EXIT

say() { printf '\n=== %s\n' "$*"; }
wait_for() { for _ in $(seq 1 120); do curl -sf -o /dev/null "$1" && return 0; sleep 0.5; done; echo "timed out: $1" >&2; return 1; }
API=http://127.0.0.1:26471
FP=http://127.0.0.1:23856
SV=http://127.0.0.1:19195
P=19131
C=http://127.0.0.1:$P
RUN=$W/run
k() { # method, path, [json body], [content type]
  curl -sf -X "$1" "$API$2" -H "content-type: ${4:-application/json}" ${3:+-d "$3"} >/dev/null \
    || { echo "FAILED: $1 $2" >&2; curl -s -X "$1" "$API$2" -H "content-type: ${4:-application/json}" ${3:+-d "$3"} >&2; return 1; }
}

say "host"
echo "  $(uname -srm), $(nproc) CPUs, $(free -g | awk '/Mem:/{print $2}') GiB, $(qemu-system-x86_64 --version | head -1)"
[ -w /dev/kvm ] || { echo "no writable /dev/kvm: this check needs KVM" >&2; exit 1; }

say "downloads in the background: the two ISOs"
curl -sfL -o "$W/alpine.iso" "$ALPINE_ISO" &
ALPINE_DL=$!
curl -sfL -o "$W/windows.iso" "$WINDOWS_ISO" &
WINDOWS_DL=$!

say "stormconsole $STORMCONSOLE_REV and stormvm $STORMVM_REV"
for r in stormconsole stormvm; do
  git clone -q "https://github.com/glennswest/$r" "$W/$r"
done
git -C "$W/stormconsole" checkout -q "$STORMCONSOLE_REV"
git -C "$W/stormvm" checkout -q "$STORMVM_REV"
echo "  vendored stormrfb: $(cat "$W/stormconsole/web/src/lib/vendor/stormrfb/VERSION")"
if cmp -s "$W/stormconsole/web/src/lib/vendor/stormrfb/client.js" "$HERE/../web/client.js"; then
  echo "  its client.js is this commit's web/client.js"
else
  echo "  its client.js differs from this commit's web/client.js"
fi

say "build the SPA, the console and stormvm"
(cd "$W/stormconsole/web" && npm ci --no-audit --no-fund >/dev/null && npx vite build --logLevel warn)
cargo build -q --manifest-path "$W/stormconsole/Cargo.toml" -p stormconsole
cargo build -q --manifest-path "$W/stormvm/Cargo.toml" -p stormvm
bin() { if [ -n "${CARGO_TARGET_DIR:-}" ]; then echo "$CARGO_TARGET_DIR/debug/$2"; else echo "$W/$1/target/debug/$2"; fi; }
CONSOLE_BIN=$(bin stormconsole stormconsole)
STORMVM_BIN=$(bin stormvm stormvm)

say "the shipped chunks (stormconsole's vite build)"
python3 - "$W/stormconsole/web/dist" <<'EOF'
import gzip, os, sys
root = sys.argv[1]
rows = []
for d, _, fs in os.walk(root):
    for f in fs:
        p = os.path.join(d, f)
        if not f.endswith(('.js', '.wasm')):
            continue
        b = open(p, 'rb').read()
        # By what is in them, not by name: the console's own chunk says
        # "noVNC" and "stormrfb" on its buttons, so only code markers count.
        kind = ''
        if f.endswith('.wasm') or b'BrowserClient' in b:
            kind = 'stormrfb'
        elif b'_handleFramebufferUpdate' in b or b'_negotiateSecurity' in b:
            kind = 'noVNC'
        if kind:
            rows.append((kind, os.path.relpath(p, root), len(b), len(gzip.compress(b, 9))))
for k, p, n, g in sorted(rows):
    print(f"  chunk {k:8} {p:48} {n:>8} B  {g:>7} B gzip")
for k in ('stormrfb', 'noVNC'):
    n = sum(r[2] for r in rows if r[0] == k); g = sum(r[3] for r in rows if r[0] == k)
    print(f"  total {k:8} {n:>8} B  {g:>7} B gzip")
EOF

say "fastetcd $FASTETCD_VER and rustkube $RUSTKUBE_VER"
curl -sfL "https://github.com/glennswest/fastetcd/releases/download/$FASTETCD_VER/fastetcd-$FASTETCD_VER-x86_64-linux-musl.tar.gz" | tar xz -C "$W"
curl -sfL "https://github.com/glennswest/rustkube/releases/download/$RUSTKUBE_VER/rustkube-apiserver-$RUSTKUBE_VER-x86_64-linux-musl.tar.gz" | tar xz -C "$W"
FE=$(find "$W" -maxdepth 3 -type f -name fastetcd -perm -u+x | head -1)
KA=$(find "$W" -maxdepth 3 -type f -name 'kube-apiserver' -perm -u+x | head -1)
[ -n "$KA" ] || KA=$(find "$W" -maxdepth 3 -type f -name '*apiserver*' -perm -u+x | head -1)
"$FE" --name f1 --data-dir "$W/fastetcd" \
  --listen-client-urls $FP --advertise-client-urls $FP \
  --listen-peer-urls http://127.0.0.1:23866 --initial-advertise-peer-urls http://127.0.0.1:23866 \
  --listen-metrics-url 127.0.0.1:23876 > "$W/fastetcd.log" 2>&1 &
wait_for $FP/health
"$KA" --bind-addr 127.0.0.1 --secure-port 26471 --etcd-servers $FP \
  --insecure true --dev-anonymous-admin true > "$W/apiserver.log" 2>&1 &
wait_for $API/readyz || { tail -20 "$W/apiserver.log"; exit 1; }
for kind in VirtualMachine:virtualmachines VirtualMachineInstance:virtualmachineinstances; do
  k POST /apis/apiextensions.k8s.io/v1/customresourcedefinitions "{
    \"apiVersion\":\"apiextensions.k8s.io/v1\",\"kind\":\"CustomResourceDefinition\",
    \"metadata\":{\"name\":\"${kind#*:}.kubevirt.io\"},
    \"spec\":{\"group\":\"kubevirt.io\",\"scope\":\"Namespaced\",
      \"names\":{\"kind\":\"${kind%:*}\",\"plural\":\"${kind#*:}\",\"singular\":\"$(echo "${kind%:*}" | tr A-Z a-z)\"},
      \"versions\":[{\"name\":\"v1\",\"served\":true,\"storage\":true,\"subresources\":{\"status\":{}},
        \"schema\":{\"openAPIV3Schema\":{\"type\":\"object\",\"x-kubernetes-preserve-unknown-fields\":true}}}]}}"
done
sleep 2

wait "$ALPINE_DL" || { echo "Alpine ISO download failed: $ALPINE_ISO" >&2; exit 1; }
wait "$WINDOWS_DL" || { echo "Windows ISO download failed: $WINDOWS_ISO" >&2; exit 1; }
echo "  alpine.iso $(stat -c %s "$W/alpine.iso") B, windows.iso $(stat -c %s "$W/windows.iso") B"

# One guest the way stormvm's qemu driver renders it, held at -S so both
# clients are attached before the first pixel, and registered the way
# `stormvm start` registers it. qmp.sock is this script's own (screendump);
# control.sock is stormvm's.
guest() { # name, iso, memory MiB, vcpus
  local d="$RUN/default/$1"
  mkdir -p "$d"
  cp "$OVMF_VARS" "$d/OVMF_VARS.fd"
  qemu-system-x86_64 -name "$1" -nodefaults -machine q35,accel=kvm -cpu host -smp "$4" -m "$3" \
    -drive "if=pflash,format=raw,readonly=on,file=$OVMF_CODE" \
    -drive "if=pflash,format=raw,file=$d/OVMF_VARS.fd" \
    -drive "file=$2,media=cdrom,if=none,id=cd0,readonly=on" -device ide-cd,drive=cd0,bus=ide.0,bootindex=0 \
    -vnc "unix:$d/vnc.sock" -device virtio-vga \
    -qmp "unix:$d/control.sock,server=on,wait=off" -qmp "unix:$d/qmp.sock,server=on,wait=off" \
    -serial "file:$d/serial.log" -S > "$d/hypervisor.log" 2>&1 &
  cat > "$d/vm.json" <<EOF
{"namespace":"default","name":"$1","uid":"u-$1","vnc_socket":"$d/vnc.sock",
 "serial_log":"$d/serial.log","hypervisor_log":"$d/hypervisor.log",
 "control_socket":"$d/control.sock","vmm":"qemu","started":$(date +%s)}
EOF
  k POST /apis/kubevirt.io/v1/namespaces/default/virtualmachineinstances \
    "{\"apiVersion\":\"kubevirt.io/v1\",\"kind\":\"VirtualMachineInstance\",
      \"metadata\":{\"name\":\"$1\",\"namespace\":\"default\"},
      \"spec\":{\"domain\":{\"cpu\":{\"cores\":$4},\"memory\":{\"guest\":\"${3}Mi\"},
        \"devices\":{\"disks\":[{\"name\":\"iso\",\"cdrom\":{\"bus\":\"sata\"}}]}},
        \"volumes\":[{\"name\":\"iso\",\"containerDisk\":{\"image\":\"$1\"}}]}}"
  k PATCH "/apis/kubevirt.io/v1/namespaces/default/virtualmachineinstances/$1/status" \
    '{"status":{"phase":"Running","nodeName":"dev"}}' application/merge-patch+json
}

say "guests: alpine (1 GiB, 2 vCPU) and windows (4 GiB, 2 vCPU), paused"
guest alpine "$W/alpine.iso" 1024 2
guest windows "$W/windows.iso" 4096 2
for v in alpine windows; do
  for _ in $(seq 1 40); do [ -S "$RUN/default/$v/vnc.sock" ] && break; sleep 0.25; done
  [ -S "$RUN/default/$v/vnc.sock" ] || { echo "$v: qemu did not bind vnc.sock" >&2; cat "$RUN/default/$v/hypervisor.log" >&2; exit 1; }
done

say "stormvm serve, and the console over it"
"$STORMVM_BIN" serve --addr 127.0.0.1:19195 --run-dir "$RUN" > "$W/stormvm.log" 2>&1 &
wait_for $SV/api/v1/vms
curl -sf $SV/api/v1/vms | python3 -c 'import json,sys
for v in json.load(sys.stdin)["items"]: print("  stormvm:", v["name"], "running" if v["running"] else "stopped", v["console"])'
cat > "$W/c.toml" <<EOF
listen_addr = "127.0.0.1:$P"
data_dir = "$W/c"
[kubernetes]
server = "$API"
[vm]
url = "$SV"
[fleet]
enabled = false
[logs]
enabled = false
[stormdrive]
enabled = false
[stormstorage]
enabled = false
[stormblock]
enabled = false
[sbregistry]
enabled = false
[vmimages]
enabled = false
[fastetcd]
enabled = false
EOF
mkdir -p "$W/c"
(cd "$W/stormconsole" && "$CONSOLE_BIN" --config "$W/c.toml" > "$W/c.log" 2>&1 &)
wait_for $C/healthz
sleep 4
curl -sf "$C/api/plugins/vm/vms/default/alpine" | python3 -c 'import json,sys; print("  console sees alpine, doors:", json.load(sys.stdin).get("console"))'

say "Playwright and a headless Chromium"
mkdir -p "$W/pw"
(cd "$W/pw" && npm init -y >/dev/null && npm i --no-audit --no-fund playwright@1 >/dev/null 2>&1 \
  && npx playwright install chromium-headless-shell >/dev/null 2>&1)
NOVNC_VER=$(node -p "require('$W/stormconsole/web/node_modules/@novnc/novnc/package.json').version")
echo "  playwright $(node -p "require('$W/pw/node_modules/playwright/package.json').version"), noVNC $NOVNC_VER"
cp "$HERE/relay.browser.cjs" "$W/pw/"
set +e
(cd "$W/pw" && CONSOLE="$C" RUN="$RUN" node relay.browser.cjs)
RC=$?
set -e

say "logs (warnings and errors only)"
grep -hiE "warn|error" "$W/c.log" "$W/stormvm.log" | grep -v 'no users and no auth_token' | head -20 || true
for v in alpine windows; do
  echo "  $v hypervisor.log:"; sed 's/^/    /' "$RUN/default/$v/hypervisor.log" | head -10
done
echo "  alpine serial.log tail:"; tail -5 "$RUN/default/alpine/serial.log" | sed 's/^/    /'
say "done: browser half exit $RC"
exit $RC
