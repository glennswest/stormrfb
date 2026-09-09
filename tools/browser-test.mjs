import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { resolve, extname } from 'node:path';
import { pathToFileURL } from 'node:url';
import assert from 'node:assert/strict';
const { chromium } = await import(pathToFileURL(process.env.PLAYWRIGHT_ROOT + '/index.mjs'));
const root=resolve(import.meta.dirname,'..');
const server=createServer(async (req,res) => {
  try {
    const path=resolve(root,'.'+decodeURIComponent(req.url.split('?')[0]));
    if (!path.startsWith(root+'/')) { res.writeHead(403).end(); return; }
    const data=await readFile(path);
    res.setHeader('Content-Type',extname(path)==='.wasm'?'application/wasm':'text/javascript'); res.end(data);
  } catch { res.writeHead(404).end(); }
});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
const browser=await chromium.launch({headless:true,args:['--no-sandbox']});
try {
  const page=await browser.newPage();
  await page.goto(`http://127.0.0.1:${server.address().port}/web/client.js`);
  const result=await page.evaluate(async () => {
    const { connect }=await import('/web/client.js');
    const { default:init }=await import('/web/pkg/stormrfb_wasm.js');
    const { memory }=await init();
    const canvas=document.createElement('canvas'); document.body.replaceChildren(canvas);
    canvas.style.width='100px'; canvas.style.height='100px';
    const sent=[]; let socket;
    window.WebSocket=class {
      static OPEN=1;
      constructor() { socket=this; this.readyState=1; }
      send(b) { sent.push([...b]); }
      close() { this.readyState=3; }
    };
    let error;
    const session=await connect(canvas,'ws://fixture',{onerror:e=>error=String(e)});
    const feed=b=>socket.onmessage({data:new Uint8Array(b).buffer});
    feed(new TextEncoder().encode('RFB 003.008\n')); feed([1,1]); feed([0,0,0,0]);
    feed([0,1,0,1,32,24,0,1,0,255,0,255,0,255,0,8,16,0,0,0,0,0,0,0]);
    feed([0,0,0,1,0,0,0,0,0,1,0,1,0,0,0,0,11,22,33,0]);
    const pixel=()=>[...canvas.getContext('2d').getImageData(0,0,1,1).data];
    const first=pixel(); memory.grow(1); feed([2]); const afterGrowth=pixel();
    canvas.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',code:'Enter',cancelable:true}));
    canvas.dispatchEvent(new KeyboardEvent('keyup',{key:'Enter',code:'Enter',cancelable:true}));
    feed([0,0,0,1,0,0,0,0,0,2,0,1,255,255,255,33]); // DesktopSize -223
    feed([0,0,0,1,0,0,0,0,0,2,0,1,0,0,0,0,44,55,66,0,77,88,99,0]);
    const resized=pixel(), dimensions=[canvas.width,canvas.height];
    session.close(); session.close();
    return {first,afterGrowth,resized,dimensions,sent,error};
  });
  assert.equal(result.error,undefined); assert.deepEqual(result.first,[11,22,33,255]); assert.deepEqual(result.afterGrowth,result.first);
  assert.deepEqual(result.resized,[44,55,66,255]); assert.deepEqual(result.dimensions,[2,1]);
  assert(result.sent.some(b=>b.join(',')==='4,1,0,0,0,0,255,13'));
  assert(result.sent.some(b=>b.join(',')==='4,0,0,0,0,0,255,13'));
  console.log('Chromium canvas: pixels, alpha, WASM memory growth, resize, keyboard and cleanup passed');
} finally { await browser.close(); await new Promise(r=>server.close(r)); }
