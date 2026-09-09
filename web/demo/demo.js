import init, { BrowserClient } from '../pkg/stormrfb_wasm.js';
const $ = id => document.getElementById(id);
const screen=$('screen'), overlay=$('overlay'), ctx=screen.getContext('2d'), mark=overlay.getContext('2d');
let memory, client, image, width=720, height=400, frames=0, phase=0, playing=true, mode='animation', lastRect, damage=[], generation=0;
const source=document.createElement('canvas'), paint=source.getContext('2d',{willReadFrequently:true});
const status=message => $('status').textContent=message;
function handshake(w,h) {
  client?.free(); client=new BrowserClient(); width=w;height=h; image=undefined; frames=0;damage=[];
  screen.width=overlay.width=source.width=w;screen.height=overlay.height=source.height=h;
  for(const bytes of [new TextEncoder().encode('RFB 003.008\n'),[1,1],[0,0,0,0],
    [w>>8,w&255,h>>8,h&255,32,24,0,1,0,255,0,255,0,255,0,8,16,0,0,0,0,0,0,0]]) client.receive(new Uint8Array(bytes));
}
function outline() {
  mark.clearRect(0,0,width,height);
  if (!$('highlight').checked) return;
  mark.strokeStyle='#ff4b4b';mark.lineWidth=2;
  for(const [x,y,w,h] of damage) mark.strokeRect(x+1,y+1,Math.max(0,w-2),Math.max(0,h-2));
}
function receive(bytes) {
  const start=performance.now(); const events=[];
  // Deliberately awkward boundaries exercise the real incremental parser.
  const chunk=$('fragment').checked?127:bytes.length;
  for(let at=0;at<bytes.length;at+=chunk) events.push(...client.receive(bytes.subarray(at,at+chunk)));
  const ptr=client.framebuffer_ptr();
  if (!image || image.data.buffer!==memory.buffer || image.data.byteOffset!==ptr) image=new ImageData(new Uint8ClampedArray(memory.buffer,ptr,width*height*4),width,height);
  damage=events.filter(e=>e[0]==='damage').map(e=>e.slice(1));
  for(const [x,y,w,h] of damage) ctx.putImageData(image,0,0,x,y,w,h);
  frames++; $('frames').textContent=String(frames);
  const area=Math.min(width*height,damage.reduce((n,r)=>n+r[2]*r[3],0));
  $('area').textContent=(100*area/(width*height)).toFixed(1)+'%';
  $('bytes').textContent=bytes.length.toLocaleString()+' bytes';
  $('time').textContent=(performance.now()-start).toFixed(2)+' ms';outline();
}
function drawScene(x,y) {
  paint.fillStyle='#20262e';paint.fillRect(0,0,width,height);
  paint.strokeStyle='#303943';paint.lineWidth=1;
  for(let n=0;n<width;n+=40){paint.beginPath();paint.moveTo(n,0);paint.lineTo(n,height);paint.stroke();}
  for(let n=0;n<height;n+=40){paint.beginPath();paint.moveTo(0,n);paint.lineTo(width,n);paint.stroke();}
  paint.fillStyle='#11171e';paint.fillRect(0,height-34,width,34);
  paint.fillStyle='#adb7c5';paint.font='13px sans-serif';paint.fillText('stormrfb  •  synthetic desktop',18,height-12);
  paint.fillStyle='#090c10';paint.fillRect(x+6,y+7,250,154);
  paint.fillStyle='#f2f2f2';paint.fillRect(x,y,250,154);
  paint.fillStyle='#ee0000';paint.fillRect(x,y,250,31);
  paint.fillStyle='#fff';paint.font='bold 13px sans-serif';paint.fillText('A moving window',x+13,y+21);
  paint.fillStyle='#202020';paint.font='bold 21px sans-serif';paint.fillText('Pixels, over RFB.',x+16,y+69);
  paint.font='14px sans-serif';paint.fillText('Only this region needs repainting.',x+16,y+96);
  ['#ee0000','#37b875','#398be8','#181818'].forEach((c,i)=>{paint.fillStyle=c;paint.fillRect(x+16+i*49,y+115,38,17);});
}
function rawPacket(r) {
  const [x,y,w,h]=r, rgba=paint.getImageData(x,y,w,h).data;
  const out=new Uint8Array(16+rgba.length), v=new DataView(out.buffer);
  v.setUint16(2,1);v.setUint16(4,x);v.setUint16(6,y);v.setUint16(8,w);v.setUint16(10,h);
  out.set(rgba,16); // RGBX wire format: spare byte is deliberately zero, not opacity.
  for(let n=19;n<out.length;n+=4) out[n]=0;
  return out;
}
function step() {
  const x=Math.round(225+170*Math.sin(phase)),y=Math.round(93+55*Math.sin(phase*.73));phase+=.045;
  drawScene(x,y); const now=[x,y,256,161]; let r=[0,0,width,height];
  if(lastRect){const left=Math.min(x,lastRect[0]),top=Math.min(y,lastRect[1]);r=[left,top,Math.max(x+256,lastRect[0]+256)-left,Math.max(y+161,lastRect[1]+161)-top];}
  receive(rawPacket(r));lastRect=now;
  status('Real Rust/WASM decoding · Raw RFB rectangles · '+($('fragment').checked?'127-byte fragments':'whole update packets'));
}
function reset() {generation++;mode='animation';phase=0;lastRect=undefined;handshake(720,400);playing=true;$('play').textContent='Pause';$('mode').textContent='Synthetic moving-window RFB source, real Rust/WASM decoder. No network or VM is required for this animation.';step();}
async function replayQemu() {
  const token=++generation;playing=false;$('play').textContent='Play';mode='qemu';status('Loading recorded QEMU ZRLE updates…');
  const response=await fetch('/fixtures/qemu-zrle.rfb'); if(!response.ok)throw new Error('QEMU capture unavailable');
  const bytes=new Uint8Array(await response.arrayBuffer()); if(token!==generation)return;
  handshake(720,400);receive(bytes);$('frames').textContent='2';frames=2;
  let hash=0xcbf29ce484222325n;for(const b of image.data)hash=BigInt.asUintN(64,(hash^BigInt(b))*0x100000001b3n);
  const actual=hash.toString(16).padStart(16,'0');if(actual!=='06133f0cfae0b305')throw new Error('QEMU framebuffer hash mismatch');
  $('mode').textContent='Recorded QEMU 10.1.5 firmware screen. Two persistent-ZRLE updates, checked against the independent QMP pixel hash.';
  status('QEMU capture decoded · 1,804 wire bytes · independent pixel hash matched: '+actual);
}
function fail(e){playing=false;status('Demo error: '+e.message);console.error(e);}
$('play').onclick=()=>{if(mode!=='animation'){reset();return;}playing=!playing;$('play').textContent=playing?'Pause':'Play';};
$('step').onclick=()=>{if(mode!=='animation')reset();playing=false;$('play').textContent='Play';step();};
$('reset').onclick=reset;$('qemu').onclick=()=>replayQemu().catch(fail);$('highlight').onchange=outline;
screen.onpointermove=e=>{if(!image)return;const b=screen.getBoundingClientRect(),x=Math.max(0,Math.min(width-1,Math.floor((e.clientX-b.left)*width/b.width))),y=Math.max(0,Math.min(height-1,Math.floor((e.clientY-b.top)*height/b.height))),at=(y*width+x)*4;const [r,g,bv,a]=image.data.subarray(at,at+4);$('pixel').textContent=`R ${r} · G ${g} · B ${bv} · A ${a}`;$('swatch').style.background=`rgb(${r},${g},${bv})`;};
try {({memory}=await init());reset();setInterval(()=>{if(playing)try{step();}catch(e){fail(e);}},1000/20);}catch(e){fail(e);}
window.addEventListener('pagehide',()=>{playing=false;client?.free();client=undefined;});
