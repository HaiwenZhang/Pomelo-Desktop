// Run Web BoardIndex and the native canvas picker independently on the same BRD.
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { resolve, basename } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
const [webRoot, nativeProbe, source, outputDirectory, encoding = 'utf-8', focus = 'general'] = process.argv.slice(2);
if (!outputDirectory) throw Error('CANVAS_PICK_PARITY_USAGE: WEB_ROOT NATIVE_PROBE SOURCE OUTPUT_DIR [encoding] [general|backdrills|die-pads]');
if (!['general','backdrills','die-pads'].includes(focus)) throw Error('CANVAS_PICK_PARITY_UNKNOWN_FOCUS');
mkdirSync(outputDirectory, {recursive:true});
const load = (path:string) => import(pathToFileURL(resolve(webRoot,path)).href);
const [{AllegroParser},{AllegroSceneBuilder},{BoardIndex},{BoardDisplay}] = await Promise.all([
 load('src/lib/allegro/parser.ts'), load('src/lib/allegro/scene-builder.ts'),
 load('src/lib/interaction/picking.ts'), load('src/lib/board/display.ts'),
]);
const [{ZoneShape},{SegmentShape}] = await Promise.all([load('src/lib/board/shapes/zone.ts'),load('src/lib/board/shapes/segment.ts')]);
const bytes = readFileSync(source), sha256 = createHash('sha256').update(bytes).digest('hex');
const db = await new AllegroParser(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset+bytes.byteLength),encoding).parse();
const scene = await new AllegroSceneBuilder(db).build();
const [{MsdfFont},{BoardTextGlyphBuilder,glyphCorners}]=await Promise.all([load('src/lib/text/msdf-font.ts'),load('src/lib/text/board-text-glyph-builder.ts')]);
await MsdfFont.prepare(scene.texts,undefined,async(block:number)=>JSON.parse(readFileSync(resolve(webRoot,`public/fonts/source-han-sans/${block.toString(16).padStart(2,'0')}.json`),'utf8')));
const index = await BoardIndex.create(scene);
const defaults = BoardDisplay.createDisplayOptions(scene.drawingLayers);
const points:number[][] = [];
const backdrilledVias = scene.vias.filter((via:any)=>via.backdrill);
const diePins = scene.pins.filter((pin:any)=>pin.die);
if(focus==='backdrills') {
 if(!backdrilledVias.length)throw Error('BACKDRILL_PARITY_REQUIRES_BACKDRILL_CASE');
 for(const via of backdrilledVias) {
  const radius=via.backdrill.displayDiameter/2;
  for(const [dx,dy] of [[0,0],[.9,0],[1.1,0],[-.5,.5]])points.push([via.at[0]+radius*dx,via.at[1]+radius*dy]);
 }
} else if(focus==='die-pads') {
 if(!diePins.length)throw Error('DIE_PAD_PARITY_REQUIRES_DIE_PAD_CASE');
 for(const pin of diePins) {
  const radius=Math.max(...pin.shapes.map((pad:any)=>Math.max(pad.width,pad.height)))/2;
  for(const [dx,dy] of [[0,0],[.9,0],[1.1,0],[-.5,.5]])points.push([pin.at[0]+radius*dx,pin.at[1]+radius*dy]);
 }
} else {
for (let x=0;x<=24;x++) for (let y=0;y<=24;y++) points.push([
 scene.bounds.minX+(scene.bounds.maxX-scene.bounds.minX)*x/24,
 scene.bounds.minY+(scene.bounds.maxY-scene.bounds.minY)*y/24,
]);
for (const segment of scene.segments.slice(0,100)) {
 points.push(segment.a, segment.b, [(segment.a[0]+segment.b[0])/2,(segment.a[1]+segment.b[1])/2]);
}
for (const pad of [...scene.pins.slice(0,100),...scene.vias.slice(0,100)]) points.push(pad.at);
for(const drawing of scene.drawings.slice(0,50))for(const segment of drawing.segments.slice(0,4))points.push(segment.a,segment.b);
for(const text of scene.texts.slice(0,80))for(const glyph of BoardTextGlyphBuilder.build(text).slice(0,4)){
 const corners=glyphCorners(glyph);points.push([corners.reduce((n:number,p:number[])=>n+p[0],0)/4,corners.reduce((n:number,p:number[])=>n+p[1],0)/4]);
}
}
const cutLayers = new Set<number>(backdrilledVias.flatMap((via:any)=>via.pads.filter((pad:any)=>pad.backdrill).map((pad:any)=>pad.layer)));
const allLayers = [...new Set([...scene.layers.map((layer:any)=>layer.id), ...scene.drawingLayers.map((layer:any)=>layer.id), ...(scene.specialLayers??[]).map((layer:any)=>layer.id)])];
const dieLayers = new Set<number>(diePins.flatMap((pin:any)=>pin.shapes.map((pad:any)=>pad.layer)));
const dieVisibility = (etch:boolean,pin:boolean) => new Map([...dieLayers].map(layer=>[layer,{etch,pin}]));
const states = focus==='backdrills' ? [
 ['default',defaults],
 ['no-drills',{...defaults,drills:false}],
 ['no-backdrills',{...defaults,backdrills:false}],
 ['unfilled',{...defaults,filled:false,drills:false}],
 ['unfilled-no-backdrills',{...defaults,filled:false,drills:false,backdrills:false}],
 ['cut-layers-hidden',{...defaults,hidden:new Set([...defaults.hidden,...cutLayers])}],
 ['only-cut-layer',{...defaults,hidden:new Set(allLayers.filter((layer:number)=>layer!==[...cutLayers][0]))}],
 ['both-drill-types-hidden',{...defaults,drills:false,backdrills:false}],
] as const : focus==='die-pads' ? [
 ['default',defaults],
 ['etch-only',{...defaults,layerVisibility:dieVisibility(true,false)}],
 ['pins-only',{...defaults,layerVisibility:dieVisibility(false,true)}],
 ['unfilled',{...defaults,filled:false}],
 ['unfilled-etch-only',{...defaults,filled:false,layerVisibility:dieVisibility(true,false)}],
 ['promote-die-etch',{...defaults,priorities:[...dieLayers].map(layer=>({layer,category:'etch'}))}],
 ['promote-die-pin',{...defaults,priorities:[...dieLayers].map(layer=>({layer,category:'pin'}))}],
 ['die-hidden',{...defaults,hidden:new Set([...defaults.hidden,...dieLayers])}],
 ['active-die',{...defaults,activeLayer:[...dieLayers][0]}],
 ['only-die-layer',{...defaults,hidden:new Set(allLayers.filter(layer=>!dieLayers.has(layer)))}],
] as const : [
 ['default',defaults], ['outlines',{...defaults,shapes:0}], ['unfilled',{...defaults,filled:false}],
 ['hidden-top',{...defaults,hidden:new Set([...defaults.hidden,scene.layers[0]?.id])}],
 ['promote-bottom',{...defaults,priorities:[{layer:scene.layers.at(-1)?.id,category:'etch'},{layer:scene.layers.at(-1)?.id,category:'pin'},{layer:scene.layers.at(-1)?.id,category:'via'}]}],
 ['active-bottom',{...defaults,activeLayer:scene.layers.at(-1)?.id}],
 ['no-drills',{...defaults,drills:false}],
 ['board-text',{...defaults,boardText:true}],
] as const;
const scales = focus==='general' ? [3,30,300] : [100,650,3000];
const queries = scales.flatMap(scale=>points.map(point=>({point,scale})));
const nativeStates = states.map(([name,display])=>({name,display:{
 active_layer:display.activeLayer, filled:display.filled, color_mode:'layer',length_unit:'millimeters',
 hidden_layers:[...display.hidden].filter((id:number)=>Number.isInteger(id)),
 layer_primitives:Object.fromEntries([...display.layerVisibility].map(([id,value])=>[id,{traces:value.etch!==false,vias:value.via!==false,pads:value.pin!==false}])),
 show_drills:display.drills,show_backdrills:display.backdrills!==false,show_copper:true,show_texts:display.boardText,show_drawings:true,
 copper_opacity:display.shapes,
 layer_order:[],priorities:display.priorities,
},queries}));
const requestPath=resolve(outputDirectory,'request.json'),nativePath=resolve(outputDirectory,'native.json');
writeFileSync(requestPath,JSON.stringify({source:resolve(source),encoding,states:nativeStates}));
const probe=spawnSync(resolve(nativeProbe),[requestPath,nativePath],{encoding:'utf8',maxBuffer:1024*1024,stdio:['ignore','pipe','inherit']});
if(probe.status!==0)throw Error(`NATIVE_PROBE_FAILED: exit=${probe.status} ${probe.error ?? ''}`);
const native=JSON.parse(readFileSync(nativePath,'utf8'));
if(native.sha256!==sha256)throw Error('SOURCE_HASH_DIFFERS');
const object=(o:any)=>[o.kind==='finger'?'via':o.kind==='bond-wire'?'segment':o.kind,o.value.id];
let checked=0,matched=0;const differences:any[]=[];
for(let s=0;s<states.length;s++){
 const [name,display]=states[s];
 for(let q=0;q<queries.length;q++){
  const query=queries[q],hit=index.pick(query.point,query.scale,display);
  const modes=['object','track','net','component'].map(mode=>{
   const selected=hit?index.select(hit,mode):null;
   if(!selected)return null;
   return [selected.mode,selected.mode==='object'?object(selected.anchor.object):
    selected.mode==='track'?selected.anchor.object.value.trackId:
    selected.mode==='net'?selected.anchor.object.value.net:
    selected.anchor.object.kind==='pin'?selected.anchor.object.value.reference:selected.anchor.object.value.finger.reference];
  });
  const anchor=hit?{object:object(hit.object),layer:hit.layer===-1?4294967295:hit.layer,category:hit.category}:null;
  const nativeResult=native.states[s].results[q];
  const nativeAnchor=nativeResult.anchor?{object:nativeResult.anchor.object,layer:nativeResult.anchor.layer,category:nativeResult.anchor.category}:null;
  const expected={hit:hit?object(hit.object):null,modes,anchor},actual={hit:nativeResult.hit,modes:nativeResult.modes,anchor:nativeAnchor};
  checked++;if(JSON.stringify(expected)===JSON.stringify(actual))matched++;
  else if(differences.length<100)differences.push({state:name,...query,expected,actual,candidates:nativeResult.candidates,
    webDetails:[expected.hit,actual.hit].filter(Boolean).map(([kind,id])=>{
      if(kind==='zone'){ const zone=scene.zones.find((zone:any)=>zone.id===id),shape=new ZoneShape(zone);
        const edges=zone.paths.flatMap((path:any)=>path).map((edge:any)=>({edge,distance:new SegmentShape(edge).distance(query.point)})).sort((a:any,b:any)=>a.distance-b.distance);
        return {kind,id,layer:zone.layer,sequence:scene.zones.indexOf(zone),pathCount:zone.paths.length,closestEdge:edges[0],contains:shape.contains(query.point),boundary:shape.boundaryDistance(query.point,5/query.scale+.65/query.scale),bounds:shape.bounds()}; }
      if(kind==='segment'){const segment=scene.segments.find((segment:any)=>segment.id===id);return {kind,id,layer:segment.layer,sequence:scene.segments.indexOf(segment),distance:new SegmentShape(segment).distance(query.point)-Math.max(segment.width/2,.5/query.scale)};}
      return {kind,id};
    })});
 }
}
const report={case:basename(source),sha256,webSources:['src/lib/interaction/picking.ts','src/lib/board/display.ts','src/lib/board/shapes/pad.ts','src/lib/board/shapes/pin.ts','src/lib/render/primitive-batch-builder.ts','src/lib/board/shapes/zone.ts','src/lib/board/shapes/path.ts'].map(path=>({path,sha256:createHash('sha256').update(readFileSync(resolve(webRoot,path))).digest('hex')})),points:points.length,checked,matched,mismatches:checked-matched,differences};
writeFileSync(resolve(outputDirectory,'report.json'),JSON.stringify(report,null,2));
console.log(JSON.stringify({case:report.case,checked,matched,mismatches:report.mismatches,report:resolve(outputDirectory,'report.json')}));
if(matched!==checked)process.exitCode=1;
