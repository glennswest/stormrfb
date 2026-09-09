import init, { BrowserClient } from './pkg/stormrfb_wasm.js';

export function position(event, canvas, width, height) {
  const box = canvas.getBoundingClientRect();
  return [Math.max(0, Math.min(width - 1, Math.floor((event.clientX - box.left) * width / box.width))),
    Math.max(0, Math.min(height - 1, Math.floor((event.clientY - box.top) * height / box.height)))];
}

/** Canvas 2D client for an already authorized binary WebSocket relay. */
export async function connect(canvas, url, { password, onready, onclipboard, onbell, onerror } = {}) {
  const { memory } = await init();
  const client = new BrowserClient(password);
  const socket = new WebSocket(url);
  socket.binaryType = 'arraybuffer';
  const context = canvas.getContext('2d');
  if (!context) { socket.close(); client.free(); throw new Error('Canvas 2D unavailable'); }
  const listeners = new AbortController();
  const pressed = new Map();
  let image, closed = false, ready = false, last = [0, 0];
  canvas.tabIndex = 0;
  const send = bytes => { if (socket.readyState === WebSocket.OPEN) socket.send(bytes); };
  const close = () => { if (closed) return; closed = true; ready = false; listeners.abort(); socket.close(); client.free(); };
  const fail = error => { close(); onerror?.(error); };
  function refreshImage() {
    const width = client.width(), height = client.height(), ptr = client.framebuffer_ptr();
    if (!image || image.data.buffer !== memory.buffer || image.data.byteOffset !== ptr || image.width !== width || image.height !== height) {
      canvas.width = width; canvas.height = height;
      image = new ImageData(new Uint8ClampedArray(memory.buffer, ptr, width * height * 4), width, height);
      return true;
    }
    return false;
  }
  function cursor(e) {
    const [, x, y, width, height, rgba] = e;
    if (!width || !height) { canvas.style.cursor = 'none'; return; }
    const c = document.createElement('canvas'); c.width = width; c.height = height;
    c.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(rgba), width, height), 0, 0);
    canvas.style.cursor = `url(${c.toDataURL()}) ${x} ${y}, default`;
  }
  socket.onmessage = ({ data }) => {
    if (closed) return;
    try {
      const events = client.receive(new Uint8Array(data));
      let full = false;
      if (client.width()) full = refreshImage();
      for (const e of events) {
        switch (e[0]) {
          case 'send': send(e[1]); break;
          case 'ready': ready = true; onready?.(e[1]); break;
          case 'damage': if (!full) context.putImageData(image, 0, 0, e[1], e[2], e[3], e[4]); break;
          case 'resize': full = true; break;
          case 'cursor': cursor(e); break;
          case 'clipboard': onclipboard?.(e[1]); break;
          case 'bell': onbell?.(); break;
        }
      }
      if (full) context.putImageData(image, 0, 0);
    } catch (e) { fail(e); }
  };
  socket.onerror = () => fail(new Error('VNC WebSocket failed'));
  socket.onclose = close;
  const listen = (type, fn) => canvas.addEventListener(type, fn, { signal: listeners.signal, passive: false });
  listen('keydown', e => {
    if (!ready || e.isComposing) return;
    try { send(client.key(e.key, true)); pressed.set(e.code, e.key); e.preventDefault(); } catch { /* dead/unmapped keys */ }
  });
  listen('keyup', e => {
    if (!ready) return;
    const key = pressed.get(e.code); if (!key) return;
    send(client.key(key, false)); pressed.delete(e.code); e.preventDefault();
  });
  listen('blur', () => {
    if (!ready) return;
    for (const key of pressed.values()) send(client.key(key, false));
    pressed.clear(); send(client.pointer(0, ...last, 0));
  });
  for (const type of ['pointerdown', 'pointerup', 'pointermove', 'pointercancel']) listen(type, e => {
    if (!ready) return;
    if (type === 'pointerdown') { canvas.focus(); canvas.setPointerCapture(e.pointerId); }
    last = position(e, canvas, client.width(), client.height());
    send(client.pointer(type === 'pointercancel' ? 0 : e.buttons, ...last, 0)); e.preventDefault();
  });
  listen('wheel', e => {
    if (!ready || !e.deltaY) return;
    last = position(e, canvas, client.width(), client.height());
    send(client.pointer(e.buttons, ...last, Math.sign(e.deltaY)));
    send(client.pointer(e.buttons, ...last, 0)); e.preventDefault();
  });
  listen('contextmenu', e => e.preventDefault());
  return { close, clipboard: text => { if (ready) send(client.clipboard(text)); } };
}
