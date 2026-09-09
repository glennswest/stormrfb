// Independent noVNC oracle; dependency is installed externally, never vendored.
import { readFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import assert from 'node:assert/strict';
const { default: ZRLEDecoder } = await import(pathToFileURL(process.env.NOVNC_ROOT + '/core/decoders/zrle.js'));
const root = new URL('../', import.meta.url);
const fixture = process.argv[2] || 'qemu-zrle';
const data = await readFile(new URL(`fixtures/${fixture}.rfb`, root));
const metadata = JSON.parse(await readFile(new URL(`fixtures/${fixture}.json`, root)));
const { width, height } = metadata;
function replay() {
  const started=performance.now();
  const fb = new Uint8Array(width * height * 4);
  const decoder = new ZRLEDecoder();
  let pos = 0, updates = 0;
  const sock = { rQwait: (_, n) => data.length - pos < n,
    rQshift32: () => { const n = data.readUInt32BE(pos); pos += 4; return n; },
    rQshiftBytes: n => { const b = data.subarray(pos, pos+n); pos += n; return b; } };
  const display = {
    blitImage(x,y,w,h,bytes,offset) {
      for (let row=0; row<h; row++) fb.set(bytes.subarray(offset+row*w*4,offset+(row+1)*w*4),((y+row)*width+x)*4);
    },
    fillRect(x,y,w,h,color) {
      for (let row=y; row<y+h; row++) for (let col=x; col<x+w; col++) { const i=(row*width+col)*4; fb[i]=color[0]; fb[i+1]=color[1]; fb[i+2]=color[2]; fb[i+3]=255; }
    }
  };
  const prepared=performance.now();
  while (pos<data.length) {
    assert.equal(data[pos],0); const count=data.readUInt16BE(pos+2); pos+=4;
    for (let i=0; i<count; i++) {
      const x=data.readUInt16BE(pos), y=data.readUInt16BE(pos+2), w=data.readUInt16BE(pos+4), h=data.readUInt16BE(pos+6), encoding=data.readInt32BE(pos+8); pos+=12;
      assert.equal(encoding,16); assert.equal(decoder.decodeRect(x,y,w,h,sock,display,24),true);
    }
    updates++;
  }
  return { fb, updates, setup:prepared-started, decode:performance.now()-prepared };
}
const { fb, updates } = replay();
let hash=0xcbf29ce484222325n;
for (const b of fb) hash=BigInt.asUintN(64,(hash^BigInt(b))*0x100000001b3n);
assert.equal(hash.toString(16).padStart(16,'0'),metadata.rgba_fnv1a64);
assert.equal(updates,metadata.updates);
for (let i=0;i<20;i++) replay();
const rounds=[];
for(let round=0;round<5;round++) {
  let setup=0,decode=0;
  for(let i=0;i<200;i++) { const r=replay(); setup+=r.setup; decode+=r.decode; }
  rounds.push({setup:setup/400,decode:decode/400,total:(setup+decode)/400});
}
const median=key=>rounds.map(r=>r[key]).sort((a,b)=>a-b)[2];
console.log(JSON.stringify({oracle:'noVNC',fixture,updates,hash:metadata.rgba_fnv1a64,median_ms_per_frame:median('total'),setup_ms_per_frame:median('setup'),decode_ms_per_frame:median('decode'),rounds,bytes_per_frame:data.length/updates,node:process.version}));
