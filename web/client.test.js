import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import init, { BrowserClient } from './pkg/stormrfb_wasm.js';
import { position } from './client.js';
const wasm = await init({ module_or_path: await readFile(new URL('./pkg/stormrfb_wasm_bg.wasm', import.meta.url)) });
test('CSS-scaled pointer coordinates clamp to framebuffer', () => {
  const canvas = { getBoundingClientRect: () => ({ left: 10, top: 20, width: 100, height: 50 }) };
  assert.deepEqual(position({ clientX: 60, clientY: 45 }, canvas, 200, 100), [100, 50]);
  assert.deepEqual(position({ clientX: -1, clientY: 100 }, canvas, 200, 100), [0, 99]);
});
test('WASM handshake, RGBX alpha and input wire bytes', () => {
  const client = new BrowserClient();
  const receive = b => client.receive(new Uint8Array(b));
  assert.equal(receive(new TextEncoder().encode('RFB 003.008\n'))[0][0], 'send');
  assert.deepEqual([...receive([1, 1])[0][1]], [1]);
  assert.deepEqual([...receive([0, 0, 0, 0])[0][1]], [1]);
  const format = [32,24,0,1,0,255,0,255,0,255,0,8,16,0,0,0];
  assert.equal(receive([0,1,0,1,...format,0,0,0,0])[0][0], 'ready');
  receive([0,0,0,1, 0,0,0,0,0,1,0,1, 0,0,0,0, 10,20,30,0]);
  assert.deepEqual([...new Uint8Array(wasm.memory.buffer, client.framebuffer_ptr(), 4)], [10,20,30,255]);
  assert.deepEqual([...client.key('Enter', true)], [4,1,0,0,0,0,255,13]);
  assert.deepEqual([...client.pointer(2, 100, 100, 0)], [5,4,0,0,0,0]);
  assert.throws(() => client.clipboard('😀'));
  client.free();
});
test('ExtendedDesktopSize: resize requests only after the server announces -308', () => {
  const client = new BrowserClient();
  const receive = b => client.receive(new Uint8Array(b));
  receive(new TextEncoder().encode('RFB 003.008\n')); receive([1, 1]); receive([0, 0, 0, 0]);
  const format = [32,24,0,1,0,255,0,255,0,255,0,8,16,0,0,0];
  receive([0,1,0,1,...format,0,0,0,0]);
  assert.equal(client.can_resize(), false);
  assert.throws(() => client.resize(640, 480));
  // One -308 rectangle: reason 0, status 0, 1x1, one screen (id 7, flags 0).
  const events = receive([0,0,0,1, 0,0,0,0,0,1,0,1, 0xff,0xff,0xfe,0xcc, 1,0,0,0, 0,0,0,7, 0,0,0,0,0,1,0,1, 0,0,0,0]);
  assert.ok(!events.some(e => e[0] === 'resize'));
  assert.equal(client.can_resize(), true);
  assert.deepEqual([...client.resize(640, 480)],
    [251,0, 2,128,1,224, 1,0, 0,0,0,7, 0,0,0,0, 2,128,1,224, 0,0,0,0]);
  assert.equal(client.resize_status(), undefined);
  client.free();
});
