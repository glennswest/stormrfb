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
