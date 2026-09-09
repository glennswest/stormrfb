import fs from 'node:fs/promises';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
import {Presentation,PresentationFile} from '@oai/artifact-tool';
const skill=process.env.SKILL_DIR;
const {resolvePresentationFont,applyPresentationChartFont,finalizePresentation}=await import(pathToFileURL(skill+'/container_tools/artifact_tool_utils.mjs'));
const root=path.resolve(process.env.REPO_ROOT ?? process.cwd());
const work=path.resolve(process.env.PRESENTATION_WORK_DIR ?? '/private/tmp/stormrfb-presentation-rebuild');
const family=resolvePresentationFont();
const fontPolicy={basis:'design',families:[family]};
const p=Presentation.create({slideSize:{width:1280,height:720}});
const red='#EE0000', ink='#151515', gray='#555555';
let count=0; const tables=[],charts=[];
function text(s,str,x,y,w,h,size=26,color=ink,bold=false){const a=s.shapes.add({geometry:'textbox',position:{left:x,top:y,width:w,height:h},fill:'none',line:{fill:'none',width:0}}); a.text=str;a.text.style={typeface:family,fontSize:size,color,bold,autoFit:'none'};return a;}
function slide(title,sub='',notes='',dark=false){const s=p.slides.add();count++;s.background.fill=dark?ink:'#FFFFFF';text(s,'STORMRFB  /  PROJECT REVIEW',64,25,1000,30,16,dark?'#BBBBBB':gray,true);text(s,title,64,83,1152,75,44,dark?'#FFFFFF':ink,true);if(sub)text(s,sub,64,164,1152,67,25,dark?'#DDDDDD':gray);text(s,'PRIVATE  ·  9 SEPTEMBER 2026',64,671,800,25,15,dark?'#BBBBBB':gray);text(s,String(count).padStart(2,'0'),1150,671,65,25,16,red,true);s.speakerNotes.textFrame.setText(notes);return s;}
function table(s,values,widths,y=250,h=350,size=25){tables.push(count);const t=s.tables.add({rows:values.length,columns:values[0].length,left:64,top:y,width:1152,height:h,columnWidths:widths,values});t.borders.assign({fill:'#D8D8D8',width:1,style:'solid'});for(let r=0;r<values.length;r++){t.rows[r].height=h/values.length;for(let c=0;c<values[0].length;c++){const a=t.getCell(r,c);a.fill=r===0?ink:(r%2?'#FFFFFF':'#F3F3F3');a.text.style={typeface:family,fontSize:size,color:r===0?'#FFFFFF':ink,bold:r===0};}}return t;}
function box(s,label,x,y,w=260,h=90,accent=false){const a=s.shapes.add({geometry:'rect',position:{left:x,top:y,width:w,height:h},fill:accent?red:'#F3F3F3',line:{fill:accent?red:'#A0A0A0',width:1}});text(s,label,x+18,y+15,w-36,h-20,25,accent?'#FFFFFF':ink,true);return a;}
function link(s,a,b,from='right',to='left'){s.shapes.connect(a,b,{kind:'straight',fromSide:from,toSide:to,line:{fill:red,width:3},tail:{type:'triangle',width:'med',length:'med'}});}
function chart(s,cats,vals,y=250,h=330){charts.push(count);const c=s.charts.add('bar',{position:{left:80,top:y,width:1110,height:h},categories:cats,series:[{name:'Milliseconds per frame',values:vals,fill:red,points:vals.map((_,idx)=>({idx,fill:idx===vals.length-1?red:idx===0?'#777777':ink})),valuesFormatCode:'0.000'}],barOptions:{direction:'column',grouping:'clustered',gapWidth:130},hasLegend:false,dataLabels:{showValue:true,position:'outEnd',textStyle:{fontSize:27,fill:ink,typeface:family}},xAxis:{textStyle:{fontSize:24,fill:ink,typeface:family}},yAxis:{min:0,max:1.5,majorUnit:0.5,numberFormatCode:'0.0',textStyle:{fontSize:20,fill:gray,typeface:family}},chartFill:'#FFFFFF',plotAreaFill:'#FFFFFF'});applyPresentationChartFont(c,{fontFamily:family});}
const source='Sources: docs/DESIGN.md, docs/VALIDATION.md and docs/PERFORMANCE.md at private release v0.1.1 (ea3ced1), 9 September 2026. ';
{
 const s=p.slides.add();count++;s.background.fill='#101010';s.images.add({blob:new Uint8Array(await fs.readFile(root+'/docs/presentations/assets/cover.png')),contentType:'image/png',alt:'Abstract red framebuffer tiles on black, without logos',fit:'cover',position:{left:0,top:0,width:1280,height:720}});
 text(s,'STORMRFB',64,90,740,90,72,'#FFFFFF',true);text(s,'One RFB core.\nBrowser client and VMM server.',64,230,720,140,39,'#FFFFFF');text(s,'Build, verification and performance',64,455,720,60,28,'#DDDDDD');text(s,'Private v0.1.1  /  9 September 2026',64,632,800,40,21,'#CCCCCC');s.speakerNotes.textFrame.setText(source+'Red, black and white theme requested by user. No Red Hat logo or affiliation claim. Background generated with ImageGen, details in README.');
}
{
 const s=slide('The server requirement drives the project','A shared Rust protocol core supports both ends of a VM console.',source);
 text(s,'stormvm needs an RFB server',64,263,750,65,36,red,true);text(s,'Encode a VMM framebuffer and receive guest input.\nThe virtio-gpu and vhost-user integration belongs to stormvm.',64,338,1110,105,29);
 text(s,'stormconsole can reuse the same core',64,480,1100,60,36,ink,true);text(s,'The WASM client exposes the shared framebuffer core to the browser.\nIt is designed to use the existing stormconsole relay.',64,551,1135,83,26);
}
{
 const s=slide('RFB, RGBA and damage','The protocol moves screen updates. The client turns them into pixels to paint.',source+'Terminology: RFB means Remote Framebuffer. A framebuffer stores the screen pixels. RFB carries rectangle updates from server to client and keyboard/pointer events back. RGBA is red, green, blue and alpha, with 8 bits per channel in the canonical client framebuffer. Damage is a set of changed rectangles that need repainting, not corrupted pixels.');
 table(s,[['Term','Meaning','Role in stormrfb'],['RFB','Remote Framebuffer, a protocol for remote screen access.','Screen updates to the client. Keyboard and mouse input back.'],['RGBA','Red, green, blue and alpha (opacity) for each pixel.','Four bytes per pixel. Normal screen pixels have alpha 255.'],['Damage','Rectangular regions whose pixels have changed.','Repaint those regions instead of repainting the whole screen.']],[210,475,467],250,350,25);
 text(s,'Example: a moving window changes parts of the screen. Those areas become damage.',64,618,1152,40,24,red,true);
}
{
 const s=slide('Architecture and ownership','The codec consumes bytes and emits events. It owns no sockets or platform types.',source);
 const core=box(s,'stormrfb\nBounded wire codec',450,270,375,115,true),client=box(s,'stormrfb-client\nRGBA and damage',70,440,335,112),server=box(s,'stormrfb-server\nSessions and updates',875,440,335,112),wasm=box(s,'stormrfb-wasm + web\nCanvas and DOM input',450,515,375,110);
 link(s,core,client,'left','top');link(s,core,server,'right','top');link(s,client,wasm,'right','left');text(s,'Native harness also uses the client',64,595,360,53,21,gray);text(s,'stormvm supplies framebuffer\nand transport integration',885,585,325,65,21,gray);
}
{
 const s=slide('Delivery took 3 hours 55 minutes','Elapsed session time, including builds, tool waits, approvals and discussion.',source+'Timing from initial user task 2026-09-09 13:44:56.287 UTC to final development counter at 17:39:58.478 UTC. v0.1.0 commit 16:08:54 UTC. Optimization request 16:09:19.427 UTC, v0.1.1 commit 17:39:35 UTC. 23 commits after spec-only b7a6727, before presentation.');
 table(s,[['Milestone','Central daylight time','Elapsed from request'],['Implementation request','08:44:56','Start'],['Tested v0.1.0 baseline','11:08:54','2 h 24 min'],['Optimization requested','11:09:19','2 h 24 min'],['Tested v0.1.1 release','12:39:35','3 h 55 min']],[535,290,327],250,315,25);text(s,'23 commits  /  About 1 h 30 min for performance analysis, fixes and validation',64,600,1152,50,27,red,true);
}
{
 const s=slide('Recorded session tokens','Development cutoff: 12:39:58 CDT. Presentation generation is excluded.', 'Source: local session cumulative token event at 2026-09-09 17:39:58.478 UTC. input_tokens 10342460, cached_input_tokens 10152320, output_tokens 68266, reasoning_output_tokens 12425, total_tokens 10410726. New input = input minus cached input = 190140. Reasoning output is part of reported output and is not added again. Includes reused context, tools and side questions, not an isolated coding invoice. Raw logs and session identifiers are intentionally not distributed.');
 text(s,'10.41 million',64,255,1100,100,76,red,true);text(s,'Total recorded tokens, including reused context',64,356,1100,50,30);
 table(s,[['Cached input','New input','Output'],['10,152,320','190,140','68,266']],[440,356,356],439,140,30);text(s,'98.2% of input was cached. These counts are not unique text or a dollar cost.',64,611,1152,40,24,gray);
}
{
 const s=slide('Hard part: incomplete bytes, persistent state','A network chunk may end inside a header, palette or compressed tile.',source+'Incomplete input is retriable without consuming the incomplete protocol item. ZRLE maintains one zlib stream across rectangles. Tile decoding is internal TRLE, encoding 15 is not advertised. Hextile preflights framing before allocating full rectangle output.');
 const a=box(s,'Fragmented\ntransport bytes',64,285,250,110),b=box(s,'Buffered parser\nComplete item?',414,285,310,110),c=box(s,'Persistent zlib\nTile decoder',824,285,380,110,true);link(s,a,b);link(s,b,c);
 text(s,'Incomplete',414,428,310,45,28,red,true);text(s,'Wait for more bytes.\nPreserve protocol state.',414,475,355,100,26);text(s,'Complete',824,428,350,45,28,red,true);text(s,'Commit decoded output.\nKeep compression history\nfor the next rectangle.',824,475,365,120,26);text(s,'Failure example\nRestarting zlib per rectangle\nbreaks later updates.',64,450,330,130,25);
}
{
 const s=slide('Hard part: pixels across memory boundaries','Correct colour bytes are only one part of a correct browser framebuffer.',source);
 table(s,[['Failure mode','Implementation response','Verification'],['RGBX spare byte treated as alpha','Normalize framebuffer alpha to 255. Cursor masks supply transparency.','Opaque pixels and cursor masks'],['CopyRect source overlaps destination','Copy rows in the safe direction, including bottom-up moves.','Overlap and bounds regressions'],['WASM memory growth detaches views','Recreate ImageData when buffer, pointer or dimensions change.','Real Chromium growth and resize']],[328,506,318],255,350,25);
}
{
 const s=slide('Hard part: hostile lengths and resize races','Bounds must survive compressed data and otherwise valid protocol sequences.',source);
 text(s,'Allocation and work budgets',64,263,530,50,33,red,true);text(s,'16,777,216 pixels\n128 MiB protocol unit / input buffer\n1 MiB text\n4,096 rectangles per update',64,329,545,176,28);text(s,'Checked arithmetic and inflated-data caps.\nLastRect sentinel counts cannot bypass limits.\nNon-incomplete decode errors are terminal.',64,540,550,101,24);
 text(s,'The resize ordering trap',690,263,520,50,33,red,true);text(s,'The server has resized, but the client\ncan still request the old dimensions.',690,330,515,100,28);text(s,'Accept the outstanding old-size request.\nSend DesktopSize last in the update.\nThe client then requests a full refresh.',690,469,525,141,27);
}
{
 const s=slide('Testing followed the deployment policy','Every Rust build, test and check ran on the designated Linux host.',source+'Policy and reproduction commands in CLAUDE.md and docs/VALIDATION.md. Main suite: sh tools/validate.sh. Builds use /build/cargo/stormrfb. External noVNC and Playwright dependencies live outside the private package.');
 const a=box(s,'Workstation\nEdit and commit',64,265,270,110),b=box(s,'Private GitHub\nPush commit',491,265,270,110),c=box(s,'Linux test host\nPull same commit',918,265,290,110,true);link(s,a,b);link(s,b,c);
 const d=box(s,'Rust tests\nStrict Clippy',64,480,310,105),e=box(s,'Release WASM\nNode and Chromium',475,480,330,105),f=box(s,'Fixtures and fuzz\nNative X11 harness',894,480,314,105);link(s,c,f,'bottom','top');link(s,c,e,'bottom','top');link(s,c,d,'bottom','top');
 text(s,'Failures were fixed in commits and tested after the next push and pull.',64,616,1140,40,25,gray);
}
{
 const s=slide('Independent pixel oracles checked correctness','A shared encoder and decoder can agree on the same mistake.',source+'QEMU 10.1.5: QMP screendump gives independent pixels. TigerVNC 1.15.0: XGetImage gives independent pixels. Same recorded RFB updates replayed by stormrfb and unmodified noVNC 1.7.0 with in-memory display adapter. Both updates checked per fixture. QEMU hash 06133f0cfae0b305. TigerVNC hash 091a3de23b92f334.');
 const a=box(s,'Disposable QEMU\nor TigerVNC server',64,320,300,112),b=box(s,'Recorded\nRFB updates',495,255,290,100),c=box(s,'QMP screendump\nor XGetImage',495,466,290,100),d=box(s,'stormrfb / noVNC\nDecoded pixels',900,255,310,110),e=box(s,'Exact framebuffer\nhash comparison',900,466,310,100,true);link(s,a,b);link(s,a,c);link(s,b,d);link(s,c,e);link(s,d,e,'bottom','top');text(s,'Wire data and reference pixels come through separate capture paths.',64,611,1152,45,25,gray);
}
{
 const s=slide('Tests covered protocol, runtime and hostile input','Validation covered these layers on 9 September 2026.',source+'25 Rust tests, 2 Node tests. Fuzz initial 4332048 / 46 s, deep compressed tiles 434699 / 46 s, optimized 421801 / 45 s budget. Total 5188548 executions across three smoke runs. Peak RSS 377 MB deep and 370 MB optimized. No crash observed. Smoke fuzzing is not a security proof.');
 table(s,[['Layer','What was exercised','Result'],['Rust and WASM','Framing, auth, pixels, copy and resize','25 Rust + 2 Node tests'],['Chromium 153','Canvas pixels, growth, resize, keys and cleanup','Passed'],['Independent servers','QEMU 720 × 400 and TigerVNC 73 × 69','Both updates match'],['Native X11 under Xvfb','Real QEMU handshake and window blits','3 blits passed'],['Sanitizer fuzzing','Framing and mutated compressed tile payloads','5.19 M executions']],[295,555,302],240,350,23);text(s,'Fuzz total spans three short runs. No crash was observed, which does not prove safety.',64,624,1150,32,21,gray);
}
{
 const s=slide('First result: WASM needed optimization','Initial single round, ms/frame. QEMU 720 × 400 static firmware.\nNode timings exclude paint and network. Native Rust is a separate runtime.',source+'v0.1.0 first measurement: native 0.563 ms/frame, WASM 1.270, noVNC 0.914. QEMU 720x400 static firmware, two updates, 902 bytes/frame. 20 warmups, 200 sessions / 400 frames. Session setup included, no network or canvas paint. Native number is not a browser comparison.');chart(s,['Native Rust','WASM in Node','noVNC in Node'],[.563,1.270,.914]);text(s,'WASM was about 39% slower than noVNC on this initial static workload.',64,607,1152,45,27,red,true);
}
{
 const s=slide('Profiling exposed avoidable memory work','Native samples guided the investigation. Repeated WASM measurements judged the fix.',source+'Native perf cpu-clock at 999 Hz, 250 samples: Vec::extend_with 22.4%, user page faults 22.8% mostly fills, replay including framebuffer work 13.6%, Framebuffer::new 12%. Sampling is supporting evidence, not a WASM profile. Optimization 1c43752, opt-level=s unchanged.');
 table(s,[['Observed cost','Optimization','Correctness constraint'],['Per-tile allocation and vector growth','Reuse 64 × 64 tile scratch and a 127-colour palette.','Bounded scratch memory'],['Redundant full-rectangle initialization','Allocate zeroed output, then overwrite all decoded pixels.','No partial successful rectangle'],['Per-pixel framebuffer indexing and copies','Bulk-copy each row, then normalize alpha.','Stable framebuffer allocation']],[339,515,298],250,345,25);text(s,'No unsafe code, SIMD requirement, relaxed bounds or build-profile change.',64,612,1152,40,26,red,true);
}
{
 const s=slide('Repeated result: 72.6% less WASM time','Median total ms/frame, five rounds of 400 frames after 20 warmup sessions.\nSame QEMU 720 × 400 fixture and Linux VM. Paint and network excluded.',source+'Each round: 20 warmup sessions, 200 measured sessions / 400 frames. Same QEMU firmware capture and host. Baseline WASM 1.319 total / 0.078 setup / 1.241 decode. Optimized 0.361 / 0.071 / 0.290. noVNC 0.967 / 0.040 / 0.928. Columns independently medianed. WASM includes JS input copy and event construction. noVNC has in-memory display. Neither includes network or paint.');chart(s,['v0.1.0 WASM','noVNC comparison','Optimized WASM'],[1.319,.967,.361]);text(s,'3.7× baseline throughput  /  2.7× noVNC throughput on this fixture',64,607,1152,45,28,red,true);
}
{
 const s=slide('Size decreased and the second fixture stayed correct','Same release settings, with no change to JS wrapper or generated glue.',source+'Size numbers from docs/PERFORMANCE.md. Gzip is the sum of independently compressed files. Historical noVNC 182 KB production chunk is not a like-for-like build. TigerVNC timing is a secondary correctness workload, not a desktop benchmark.');
 table(s,[['Package bytes','Baseline','Optimized'],['WASM','89,559','88,564'],['JS wrapper and glue','18,234','18,234'],['Uncompressed total','107,793','106,798'],['Individually gzipped total','44,878','44,718']],[560,296,296],245,310,25);text(s,'TigerVNC striped fixture: 0.0187 ms/frame, with independent pixel match.',64,591,1152,48,26,red,true);
}
{
 const s=slide('Next: real-guest integration','The private implementation is tested. Next steps focus on real guest workloads.',source,true);
 text(s,'Windows installer and Linux guest',64,270,1110,55,35,'#FFFFFF',true);text(s,'Run through the actual stormconsole relay and browser.',64,329,1110,55,28,'#CCCCCC');text(s,'Production performance',64,432,1110,55,35,'#FFFFFF',true);text(s,'Measure paint, network latency, shipped chunk size and moving 1080p content.',64,491,1140,83,28,'#CCCCCC');
}
await fs.mkdir(work+'/.build',{recursive:true});await fs.mkdir(work+'/output',{recursive:true});
await (await PresentationFile.exportPptx(p)).save(work+'/.build/candidate.pptx');
const result=await finalizePresentation({workspaceDir:work,candidatePath:work+'/.build/candidate.pptx',finalPath:work+'/output/stormrfb-project-review.pptx',pythonExecutable:process.env.RUNTIME_PYTHON,integrityValidatorPath:skill+'/container_tools/inspect_presentation_package_integrity.py',layoutValidatorPath:skill+'/container_tools/inspect_presentation_layout_geometry.py',layoutArgs:['--expected-slide-size-emu','12192000,6858000','--validate-bullet-geometry','--validate-heading-fit',...tables.flatMap(n=>['--require-native-table-slide',String(n)])],requiredNativeTableOwnerSlides:tables,requiredNativeChartOwnerSlides:charts,materializeLiteralChartWorkbooks:true,fontPolicy,verifyArtifactToolImport:true,receiptPath:work+'/.build/validation.json'});
console.log(JSON.stringify({family,count,result}));
