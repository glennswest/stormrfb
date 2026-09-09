#!/usr/bin/env python3
"""Exercise the actual native window against a disposable QEMU VNC server."""
import os
import pathlib
import socket
import subprocess
import time

read_fd, write_fd = os.pipe()
xvfb = subprocess.Popen(['Xvfb', '-displayfd', str(write_fd), '-screen', '0', '1024x768x24', '-nolisten', 'tcp'], pass_fds=(write_fd,))
os.close(write_fd)
qemu = None
try:
    with os.fdopen(read_fd) as pipe:
        display = pipe.readline().strip()
    assert display
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0)); port = sock.getsockname()[1]
    qemu = subprocess.Popen(['qemu-system-x86_64', '-accel', 'tcg', '-m', '64', '-display', 'none', '-vnc', f'127.0.0.1:{port-5900}', '-nic', 'none', '-S'])
    time.sleep(1)
    binary = pathlib.Path(os.environ['CARGO_TARGET_DIR']) / 'debug/examples/viewer'
    subprocess.run([str(binary), f'127.0.0.1:{port}'], env={**os.environ, 'DISPLAY': f':{display}', 'STORMRFB_HARNESS_FRAMES': '3'}, timeout=15, check=True)
    print('Native harness: QEMU handshake and three X11 window blits passed')
finally:
    for proc in [qemu, xvfb]:
        if proc:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill(); proc.wait()
