#!/usr/bin/env python3
"""Record a disposable QEMU firmware screen and independent QMP framebuffer hash."""
import json
import pathlib
import socket
import struct
import subprocess
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]

def exact(sock, n):
    if n > 128 * 1024 * 1024:
        raise ValueError('capture limit')
    out = bytearray()
    while len(out) < n:
        b = sock.recv(n - len(out))
        if not b:
            raise EOFError('VNC disconnected')
        out.extend(b)
    return bytes(out)

def fnv(data):
    h = 0xcbf29ce484222325
    for b in data:
        h = ((h ^ b) * 0x100000001b3) & ((1 << 64) - 1)
    return f'{h:016x}'

def main():
    with tempfile.TemporaryDirectory(prefix='stormrfb-qemu-') as tmp:
        tmp = pathlib.Path(tmp)
        qmp_path, vnc_path = tmp / 'qmp', tmp / 'vnc'
        proc = subprocess.Popen(['qemu-system-x86_64', '-accel', 'tcg', '-m', '64',
            '-display', 'none', '-vnc', f'unix:{vnc_path}', '-qmp', f'unix:{qmp_path},server=on,wait=off',
            '-no-reboot', '-nic', 'none'], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        try:
            for _ in range(100):
                if qmp_path.exists() and vnc_path.exists():
                    break
                if proc.poll() is not None:
                    raise RuntimeError(proc.stderr.read().decode())
                time.sleep(.1)
            time.sleep(4)
            with socket.socket(socket.AF_UNIX) as qmp:
                qmp.settimeout(10)
                qmp.connect(str(qmp_path))
                qfile = qmp.makefile('rwb', buffering=0)
                json.loads(qfile.readline())
                def command(name, args=None):
                    qfile.write((json.dumps({'execute': name, 'arguments': args or {}}) + '\n').encode())
                    while True:
                        result = json.loads(qfile.readline())
                        if 'error' in result:
                            raise RuntimeError(result)
                        if 'return' in result:
                            return result['return']
                command('qmp_capabilities')
                command('stop')
                command('screendump', {'filename': str(tmp / 'screen.ppm')})
                with socket.socket(socket.AF_UNIX) as vnc:
                    vnc.settimeout(10)
                    vnc.connect(str(vnc_path))
                    assert exact(vnc, 12) == b'RFB 003.008\n'
                    vnc.sendall(b'RFB 003.008\n')
                    count = exact(vnc, 1)[0]
                    assert 1 in exact(vnc, count)
                    vnc.sendall(b'\x01')
                    assert exact(vnc, 4) == bytes(4)
                    vnc.sendall(b'\x01')
                    init = exact(vnc, 24)
                    width, height = struct.unpack('>HH', init[:4])
                    exact(vnc, struct.unpack('>I', init[20:])[0])
                    rgbx = bytes([32,24,0,1,0,255,0,255,0,255,0,8,16,0,0,0])
                    vnc.sendall(bytes(4) + rgbx)
                    vnc.sendall(struct.pack('>BBHii', 2, 0, 2, 16, 0))
                    stream = bytearray()
                    encodings = []
                    for _ in range(2):
                        vnc.sendall(struct.pack('>BBHHHH', 3, 0, 0, 0, width, height))
                        header = exact(vnc, 4)
                        assert header[0] == 0
                        stream.extend(header)
                        for _ in range(struct.unpack('>H', header[2:])[0]):
                            rect = exact(vnc, 12)
                            x,y,w,h,encoding = struct.unpack('>HHHHi', rect)
                            stream.extend(rect)
                            encodings.append(encoding)
                            if encoding == 16:
                                length = exact(vnc, 4)
                                stream.extend(length)
                                stream.extend(exact(vnc, struct.unpack('>I', length)[0]))
                            elif encoding == 0:
                                stream.extend(exact(vnc, w*h*4))
                            else:
                                raise ValueError(f'unrequested encoding {encoding}')
                ppm = (tmp / 'screen.ppm').read_bytes()
                magic, dims, maximum, rgb = ppm.split(b'\n', 3)
                assert magic == b'P6' and maximum == b'255'
                assert tuple(map(int, dims.split())) == (width, height)
                rgba = bytearray()
                for i in range(0, len(rgb), 3):
                    rgba.extend(rgb[i:i+3]); rgba.append(255)
                assert 16 in encodings
                (ROOT / 'fixtures/qemu-zrle.rfb').write_bytes(stream)
                metadata = {'source': subprocess.check_output(['qemu-system-x86_64','--version'], text=True).splitlines()[0],
                    'captured_utc': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()), 'width': width, 'height': height,
                    'updates': 2, 'encodings': encodings, 'bytes': len(stream), 'rgba_fnv1a64': fnv(rgba),
                    'reference': 'QMP screendump of paused disposable firmware-only guest; no disk, network or credentials'}
                (ROOT / 'fixtures/qemu-zrle.json').write_text(json.dumps(metadata, indent=2) + '\n')
                print(json.dumps(metadata))
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill(); proc.wait()

if __name__ == '__main__':
    main()
