#!/usr/bin/env bash
# QEMU Extended Key Event against a real qemu (#1), on the build box:
#
#   sc-build tools/verify-extkey.sh
#
# Boots Alpine's virt ISO under qemu/KVM with its VNC server on a unix
# socket, as stormvm runs it (q35, OVMF, -nodefaults, virtio-vga), and the
# serial line in a file. examples/qemu_keys connects with stormrfb-client and:
#
#   1. checks qemu acknowledged -258 (Client::extended_keys);
#   2. logs in and runs a command typed **only by scancode**: every key is a
#      QemuKey with keysym 0, Shift is its own scancode, and Enter is keypad
#      Enter (E0 1C, keycode 0x9c). The command writes to the serial port, so
#      its output in serial.log is proof qemu used the keycodes;
#   3. on a second connection, types by keysym (plain KeyEvent), the fallback.
set -euo pipefail

ALPINE_ISO=${ALPINE_ISO:-https://dl-cdn.alpinelinux.org/alpine/v3.20/releases/x86_64/alpine-virt-3.20.3-x86_64.iso}
OVMF_CODE=${OVMF_CODE:-/usr/share/OVMF/OVMF_CODE.fd}
OVMF_VARS=${OVMF_VARS:-/usr/share/OVMF/OVMF_VARS.fd}
cd "$(dirname "$0")/.."
W=$(mktemp -d "${TMPDIR:-/tmp}/ek.XXXXXX")
cleanup() { kill $(jobs -p) 2>/dev/null || true; wait 2>/dev/null || true; rm -rf "$W"; }
trap cleanup EXIT
say() { printf '\n=== %s\n' "$*"; }
FAILED=0
check() { if eval "$2"; then echo "  ok   $1"; else echo "  FAIL $1"; FAILED=$((FAILED + 1)); fi; }

say "build the example; fetch Alpine"
cargo build -q --locked -p stormrfb-client --example qemu_keys
EX="${CARGO_TARGET_DIR:-target}/debug/examples/qemu_keys"
curl -sfL -o "$W/alpine.iso" "$ALPINE_ISO"
echo "  $(qemu-system-x86_64 --version | head -1); alpine.iso $(stat -c %s "$W/alpine.iso") B"

say "boot Alpine"
cp "$OVMF_VARS" "$W/vars.fd"
qemu-system-x86_64 -name alpine -nodefaults -machine q35,accel=kvm -cpu host -smp 2 -m 1024 \
  -drive "if=pflash,format=raw,readonly=on,file=$OVMF_CODE" -drive "if=pflash,format=raw,file=$W/vars.fd" \
  -drive "file=$W/alpine.iso,media=cdrom,if=none,id=cd0,readonly=on" -device ide-cd,drive=cd0,bus=ide.0 \
  -vnc "unix:$W/vnc.sock" -device virtio-vga -serial "file:$W/serial.log" > "$W/qemu.log" 2>&1 &
for _ in $(seq 1 180); do grep -q 'login:' "$W/serial.log" 2>/dev/null && break; sleep 1; done
check "Alpine reached its login prompt" "grep -q 'login:' '$W/serial.log'"
sleep 3

say "scancodes only (QemuKey, keysym 0)"
set +e
"$EX" "$W/vnc.sock" scancode root 'echo STORMRFB-EXTKEY-$((6*7)) > /dev/ttyS0' | sed 's/^/  /'
RC=${PIPESTATUS[0]}
set -e
check "the example ran (exit $RC)" "[ $RC -eq 0 ]"
sleep 2
check "qemu ran the command typed by scancode: STORMRFB-EXTKEY-42 on the serial line" \
  "grep -q 'STORMRFB-EXTKEY-42' '$W/serial.log'"

say "keysyms (KeyEvent), the fallback"
set +e
"$EX" "$W/vnc.sock" keysym 'hostname stormrfb-keysym-ok' 'cp /proc/sys/kernel/hostname /dev/ttyS0' | sed 's/^/  /'
RC=${PIPESTATUS[0]}
set -e
check "the example ran (exit $RC)" "[ $RC -eq 0 ]"
sleep 2
check "qemu ran the command typed by keysym: stormrfb-keysym-ok on the serial line" \
  "grep -q 'stormrfb-keysym-ok' '$W/serial.log'"

say "serial.log tail"
tail -5 "$W/serial.log" | sed 's/^/  /'
say "done: $FAILED failed"
[ $FAILED -eq 0 ]
