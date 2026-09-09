import {createServer} from 'node:http';
import {readFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import {resolve,extname} from 'node:path';
const root=resolve(fileURLToPath(new URL('..',import.meta.url)));
const files=new Set(['/web/demo/index.html','/web/demo/style.css','/web/demo/demo.js','/web/pkg/stormrfb_wasm.js','/web/pkg/stormrfb_wasm_bg.wasm','/fixtures/qemu-zrle.rfb']);
const types={'.html':'text/html; charset=utf-8','.css':'text/css','.js':'text/javascript','.wasm':'application/wasm','.rfb':'application/octet-stream'};
export function demoServer(){return createServer(async(req,res)=>{try{const name=new URL(req.url,'http://localhost').pathname;if(name==='/'){res.writeHead(302,{Location:'/web/demo/index.html'}).end();return;}const file=name==='/web/demo/'?'/web/demo/index.html':name;if(!files.has(file)){res.writeHead(404).end();return;}const data=await readFile(root+file);res.writeHead(200,{'Content-Type':types[extname(file)],'Cache-Control':'no-store'});res.end(data);}catch{res.writeHead(404).end();}});}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)){const port=Number(process.env.PORT||8765);demoServer().listen(port,'127.0.0.1',()=>console.log(`stormrfb demo: http://127.0.0.1:${port}/web/demo/`));}
