#!/usr/bin/env python3
"""Capture an isolated TigerVNC X server; use XGetImage as the pixel oracle."""
import ctypes as C
import json
import os
import pathlib
import socket
import struct
import subprocess
import tempfile
import time
from capture_qemu import ROOT, exact, fnv


def main():
    binary = os.environ.get('XVNC', 'Xvnc')
    with tempfile.TemporaryDirectory(prefix='stormrfb-tiger-') as tmp:
        path = pathlib.Path(tmp) / 'vnc'
        # -displayfd chooses an unused X display atomically.
        read_fd, write_fd = os.pipe()
        proc = subprocess.Popen([binary, '-displayfd', str(write_fd), '-geometry', '73x69', '-depth', '24',
            '-rfbunixpath', str(path), '-rfbport', '-1', '-SecurityTypes', 'None', '-ac', '-nolisten', 'tcp'],
            pass_fds=(write_fd,), stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        os.close(write_fd)
        display = None
        try:
            with os.fdopen(read_fd) as pipe:
                number = pipe.readline().strip()
            if not number:
                raise RuntimeError(proc.stderr.read().decode())
            x = C.CDLL('libX11.so.6')
            def bind(name, result, *args):
                f = getattr(x, name); f.restype = result; f.argtypes = args; return f
            display = bind('XOpenDisplay', C.c_void_p, C.c_char_p)(f':{number}'.encode())
            if not display:
                raise RuntimeError('XOpenDisplay failed')
            root = bind('XDefaultRootWindow', C.c_ulong, C.c_void_p)(display)
            gc = bind('XCreateGC', C.c_void_p, C.c_void_p, C.c_ulong, C.c_ulong, C.c_void_p)(display, root, 0, None)
            foreground = bind('XSetForeground', C.c_int, C.c_void_p, C.c_void_p, C.c_ulong)
            fill = bind('XFillRectangle', C.c_int, C.c_void_p, C.c_ulong, C.c_void_p, C.c_int, C.c_int, C.c_uint, C.c_uint)
            for row in range(69):
                foreground(display, gc, ((row * 3) << 16) | ((row * 2) << 8) | row)
                fill(display, root, gc, 0, row, 73, 1)
            bind('XSync', C.c_int, C.c_void_p, C.c_int)(display, 0)
            image = bind('XGetImage', C.c_void_p, C.c_void_p, C.c_ulong, C.c_int, C.c_int, C.c_uint, C.c_uint, C.c_ulong, C.c_int)(display, root, 0, 0, 73, 69, 0xffffffff, 2)
            pixel = bind('XGetPixel', C.c_ulong, C.c_void_p, C.c_int, C.c_int)
            rgba = bytearray()
            for row in range(69):
                for col in range(73):
                    p = pixel(image, col, row)
                    rgba.extend([(p >> 16) & 255, (p >> 8) & 255, p & 255, 255])
            bind('XDestroyImage', C.c_int, C.c_void_p)(image)
            stream = bytearray()
            with socket.socket(socket.AF_UNIX) as vnc:
                vnc.settimeout(10); vnc.connect(str(path))
                assert exact(vnc, 12) == b'RFB 003.008\n'
                vnc.sendall(b'RFB 003.008\n')
                assert 1 in exact(vnc, exact(vnc, 1)[0])
                vnc.sendall(b'\x01'); assert exact(vnc, 4) == bytes(4)
                vnc.sendall(b'\x01'); init = exact(vnc, 24)
                assert struct.unpack('>HH', init[:4]) == (73, 69)
                exact(vnc, struct.unpack('>I', init[20:])[0])
                vnc.sendall(bytes(4) + bytes([32,24,0,1,0,255,0,255,0,255,0,8,16,0,0,0]))
                vnc.sendall(struct.pack('>BBHii', 2, 0, 2, 16, 0))
                encodings = []
                for _ in range(2):
                    vnc.sendall(struct.pack('>BBHHHH', 3, 0, 0, 0, 73, 69))
                    header = exact(vnc, 4); assert header[0] == 0; stream.extend(header)
                    for _ in range(struct.unpack('>H', header[2:])[0]):
                        rect = exact(vnc, 12); stream.extend(rect)
                        _,_,w,h,encoding = struct.unpack('>HHHHi', rect); encodings.append(encoding)
                        if encoding == 16:
                            length = exact(vnc, 4); stream.extend(length)
                            stream.extend(exact(vnc, struct.unpack('>I', length)[0]))
                        elif encoding == 0:
                            stream.extend(exact(vnc, w*h*4))
                        else:
                            raise ValueError(encoding)
            assert 16 in encodings
            (ROOT / 'fixtures/tigervnc-zrle.rfb').write_bytes(stream)
            metadata = {'source': subprocess.run([binary, '-version'], capture_output=True, text=True).stderr.strip(),
                'captured_utc': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()), 'width': 73, 'height': 69,
                'updates': 2, 'encodings': encodings, 'bytes': len(stream), 'rgba_fnv1a64': fnv(rgba),
                'reference': 'XGetImage of an isolated 73x69 true-colour X root window with 69 colour stripes'}
            (ROOT / 'fixtures/tigervnc-zrle.json').write_text(json.dumps(metadata, indent=2) + '\n')
            print(json.dumps(metadata))
        finally:
            if display:
                bind('XCloseDisplay', C.c_int, C.c_void_p)(display)
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill(); proc.wait()

if __name__ == '__main__':
    main()
