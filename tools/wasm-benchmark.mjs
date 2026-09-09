import { readFile } from 'node:fs/promises';
import { gzipSync } from 'node:zlib';
import assert from 'node:assert/strict';
import init, { BrowserClient } from '../web/pkg/stormrfb_wasm.js';
const root=new URL('../',import.meta.url);
const wasm=await readFile(new URL('web/pkg/stormrfb_wasm_bg.wasm',root));
const { memory }=await init({module_or_path:wasm});
const data=await readFile(new URL('fixtures/qemu-zrle.rfb',root));
function replay(check=false) {
  const started=performance.now();
  const c=new BrowserClient();
  c.receive(new TextEncoder().encode('RFB 003.008\n')); c.receive(new Uint8Array([1,1])); c.receive(new Uint8Array(4));
  c.receive(new Uint8Array([2,208,1,144,32,24,0,1,0,255,0,255,0,255,0,8,16,0,0,0,0,0,0,0]));
  const prepared=performance.now();
  c.receive(data);
  const decoded=performance.now();
  if (check) {
    let hash=0xcbf29ce484222325n;
    for (const b of new Uint8Array(memory.buffer,c.framebuffer_ptr(),720*400*4)) hash=BigInt.asUintN(64,(hash^BigInt(b))*0x100000001b3n);
    assert.equal(hash.toString(16),'6133f0cfae0b305');
  }
  c.free();
  return { setup:prepared-started, decode:decoded-prepared, total:performance.now()-started };
}
replay(true); for(let i=0;i<20;i++) replay();
const rounds=[];
for(let round=0;round<5;round++) {
  const sum={setup:0,decode:0,total:0};
  for(let i=0;i<200;i++) { const t=replay(); for(const key of Object.keys(sum)) sum[key]+=t[key]; }
  rounds.push(Object.fromEntries(Object.entries(sum).map(([k,v])=>[k,v/400])));
}
const median=key=>rounds.map(r=>r[key]).sort((a,b)=>a-b)[2];
const wrapper=await readFile(new URL('web/client.js',root)); const glue=await readFile(new URL('web/pkg/stormrfb_wasm.js',root));
console.log(JSON.stringify({runtime:'WASM in Node '+process.version,median_ms_per_frame:median("total"),setup_ms_per_frame:median("setup"),decode_ms_per_frame:median("decode"),rounds,bytes_per_frame:data.length/2,wasm_bytes:wasm.length,js_bytes:wrapper.length+glue.length,gzip_total:[wasm,wrapper,glue].reduce((n,b)=>n+gzipSync(b).length,0)}));
