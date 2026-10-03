// Actual BoardIndex search-anchor oracle. Web sources remain read-only.
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
const [webRoot,probe,source,outputDir,encoding='utf-8']=process.argv.slice(2);
if(!outputDir)throw Error('SELECTION_ANCHOR_PARITY_USAGE');
mkdirSync(outputDir,{recursive:true});
const load=(path:string)=>import(pathToFileURL(resolve(webRoot,path)).href);
const [{AllegroParser},{AllegroSceneBuilder},{BoardSearchIndex},{BoardIndex},{MsdfFont},{completeSteps},{BoardDisplay}]=await Promise.all([
 load('src/lib/allegro/parser.ts'),load('src/lib/allegro/scene-builder.ts'),load('src/lib/board/search.ts'),
 load('src/lib/interaction/picking.ts'),load('src/lib/text/msdf-font.ts'),load('src/lib/iteration.ts'),load('src/lib/board/display.ts'),
]);
const bytes=readFileSync(source),sha256=createHash('sha256').update(bytes).digest('hex');
let scene:any;
if(encoding==='synthetic') {
 const input=JSON.parse(bytes.toString('utf8'));
 // Convert only the model layout. All bounds and glyph geometry below are
 // produced by the actual Web BoardIndex, never by this fixture adapter.
 const point=(p:any)=>[p.x,p.y];
 const segment=(s:any)=>({...s,a:point(s.a),b:point(s.b),trackId:s.track_id,
  arc:s.arc?{...s.arc,center:point(s.arc.center)}:undefined,bondWire:s.bond_wire});
 const pad=(p:any)=>({...p,type:p.kind,offset:point(p.offset),innerDiameter:p.inner_diameter,
  custom:p.custom?.contours.map((r:any[])=>r.map(point)),customPaths:p.custom?.paths.map((r:any[])=>r.map(segment)),backdrillBase:p.backdrill_base});
 const text=(t:any)=>({...t,at:point(t.at),ownerId:t.owner_id,classId:t.class_id,fontIndex:t.font_index,lineSpacing:t.line_spacing,strokeWidth:t.stroke_width});
 const texts=input.texts.map(text),byText=new Map(texts.map((t:any)=>[t.id,t]));
 scene={layers:input.layers,specialLayers:[],drawingLayers:[],outline:input.outline.map(segment),texts,diagnostics:[],
  nets:new Map(Object.entries(input.nets).map(([id,name])=>[Number(id),name])),
  bounds:{minX:input.bounds.min.x,minY:input.bounds.min.y,maxX:input.bounds.max.x,maxY:input.bounds.max.y},
  segments:input.segments.map(segment),
  // Web distinguishes pin drills from via drills by the presence of `pads`.
  // Remove the native pin field rather than leaving both `pads` and `shapes`.
  pins:input.pins.map(({pads,...p}:any)=>({...p,at:point(p.at),back:p.mirrored,drillShape:p.drill_shape,shapes:pads.map(pad)})),
  vias:input.vias.map((v:any)=>({...v,at:point(v.at),back:v.mirrored,drillShape:v.drill_shape,pads:v.pads.map(pad),backdrill:v.backdrill?{...v.backdrill,...v.backdrill.definition}:undefined})),
  drawings:input.drawings.map((d:any)=>({...d,segments:d.segments.map(segment),texts:d.text_ids.map((id:number)=>byText.get(id))})),
  zones:input.zones.map((z:any)=>({...z,paths:z.paths.map((p:any[])=>p.map(segment)),
   points:new Float64Array(z.mesh.vertices.flatMap(point)),ringOffsets:new Uint32Array(z.mesh.ring_offsets),rings:[],
   ringBounds:new Float64Array(z.mesh.ring_bounds.flatMap((b:any)=>[b.min.x,b.min.y,b.max.x,b.max.y]))})),
 };
} else {
 const db=await new AllegroParser(bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength),encoding).parse();
 scene=await new AllegroSceneBuilder(db).build();
}
await MsdfFont.prepare(scene.texts,undefined,async(block:number)=>JSON.parse(readFileSync(resolve(webRoot,`public/fonts/source-han-sans/${block.toString(16).padStart(2,'0')}.json`),'utf8')));
const index=await BoardIndex.create(scene);

const display=BoardDisplay.createDisplayOptions(scene.drawingLayers);
const allLayers=[...new Set((index as any).objects.flatMap((object:any)=>((index as any).entriesByObject.get(object)??[]).map((entry:any)=>entry.layer)).filter((id:number)=>id>=0))] as number[];
const states:any[]=[
 ['default',display],
 ['drills-off',{...display,drills:false}],
 ['all-hidden',{...display,hidden:new Set(allLayers),drills:false,backdrills:false,boardText:false}],
 ['active-bottom',{...display,activeLayer:scene.layers.length-1,drills:false}],
 ['via-hidden',{...display,layerVisibility:new Map(allLayers.map(id=>[id,{via:false}]))}],
 ['pin-hidden',{...display,layerVisibility:new Map(allLayers.map(id=>[id,{pin:false}]))}],
 ['manual-bottom',{...display,priorities:[{layer:scene.layers.length-1,category:'pin'},{layer:scene.layers.length-1,category:'via'},{layer:scene.layers.length-1,category:'etch'}],drills:false}],
 ['backdrills-off',{...display,backdrills:false}],
 ['text-on',{...display,boardText:true,drills:false}],
 ['etch-hidden',{...display,layerVisibility:new Map(allLayers.map(id=>[id,{etch:false}]))}],
];
const nativeDisplay=(d:any)=>({priorities:d.priorities,active_layer:d.activeLayer,hidden_layers:[...d.hidden],
 layer_primitives:Object.fromEntries([...d.layerVisibility].map(([id,v]:any)=>[id,{traces:v.etch!==false,vias:v.via!==false,pads:v.pin!==false}])),
 show_drills:d.drills,show_backdrills:d.backdrills!==false,show_copper:true,show_texts:d.boardText,show_drawings:true,filled:d.filled,copper_opacity:0.25,layer_order:[]});
const nativeAnchor=(a:any)=>a?{object:{[a.object.kind==='finger'?'via':a.object.kind==='bond-wire'?'segment':a.object.kind]:a.object.value.id},layer:a.layer<0?4294967295:a.layer,category:a.category}:null;
const items=BoardSearchIndex.buildItems(scene);
const sample=(values:any[],limit=64)=>values.length<=limit?values:Array.from({length:limit},(_,i)=>values[Math.round(i*(values.length-1)/(limit-1))]);
const expected:any[]=[],queries:any[]=[],counts:any={};
for(const [name,options] of states) {
 const subset=name==='default'?items:sample(items);
 counts[name]=subset.length;
 for(const item of subset) {
  let target:any;
  if(item.kind==='net')target={net:item.id};
  else {
   const first=(index as any).components.get(item.id)?.[0];
   if(!first)throw Error('SELECTION_ANCHOR_GROUP_IDENTITY');
   target={component_group:{[first.kind==='pin'?'pin':'finger']:first.value.id}};
  }
  const key=`${name}:${item.kind}:${item.id}`;
  const result=completeSteps(index.locateSteps(item,options));
  queries.push({key,target,display:nativeDisplay(options)});
  expected.push({key,anchor:nativeAnchor(result.selection?.anchor)});
 }
}
writeFileSync(resolve(outputDir,'request.json'),JSON.stringify({source:resolve(source),encoding,baseline:false,queries}));
writeFileSync(resolve(outputDir,'web.json'),JSON.stringify({sha256,results:expected}));
const start=performance.now();
const run=spawnSync(resolve(probe),[resolve(outputDir,'request.json'),resolve(outputDir,'native.json')],{stdio:['ignore','pipe','inherit']});
if(run.status!==0)throw Error('SELECTION_ANCHOR_NATIVE_PROBE');
const elapsed=performance.now()-start;
const actual=JSON.parse(readFileSync(resolve(outputDir,'native.json'),'utf8'));
if(actual.sha256!==sha256 || actual.results.length!==expected.length)throw Error('SELECTION_ANCHOR_SOURCE_OR_COUNT');
const differences:any[]=[];
for(let i=0;i<expected.length;i++) {
 const a=actual.results[i],b=expected[i];if(a.key!==b.key)throw Error('SELECTION_ANCHOR_QUERY_ORDER');
 const equal=(!a.anchor||!b.anchor)?a.anchor===b.anchor:
  a.anchor.layer===b.anchor.layer && a.anchor.category===b.anchor.category && JSON.stringify(a.anchor.object)===JSON.stringify(b.anchor.object);
 if(!equal)differences.push({key:b.key,expected:b.anchor,actual:a.anchor});
}
const paths=['src/lib/board/search.ts','src/lib/interaction/picking.ts','src/lib/board/display.ts','src/lib/board/shapes/pin.ts','src/lib/board/shapes/via.ts','src/lib/allegro/scene-builder.ts'];
const report={source:resolve(source),encoding,sha256,counts,queries:queries.length,mismatches:differences.length,differences,native_probe_ms:elapsed,
 runtime:{node:process.versions.node,v8:process.versions.v8},web_sources:paths.map(path=>({path,sha256:createHash('sha256').update(readFileSync(resolve(webRoot,path))).digest('hex')}))};
writeFileSync(resolve(outputDir,'report.json'),JSON.stringify(report,null,2));console.log(JSON.stringify({...report,differences:undefined}));
if(differences.length)process.exitCode=1;
