// Actual BoardIndex navigation oracle. Web sources remain read-only.
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
const [webRoot,probe,source,outputDir,encoding='utf-8',baselineFlag='baseline']=process.argv.slice(2);
if(!outputDir)throw Error('SELECTION_BOUNDS_PARITY_USAGE');
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
  pins:input.pins.map((p:any)=>({...p,at:point(p.at),back:p.mirrored,drillShape:p.drill_shape,shapes:p.pads.map(pad)})),
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
const nativeBounds=(b:any)=>b?{min:{x:b.minX,y:b.minY},max:{x:b.maxX,y:b.maxY}}:null;
const expected:any[]=[],queries:any[]=[];
const counts={net:0,component:0,object:0,track:0};
for(const item of BoardSearchIndex.buildItems(scene)) {
 let target:any;
 if(item.kind==='net')target={net:item.id};
 else {
  const anchor=(index as any).components.get(item.id)?.[0];
  if(!anchor)throw Error('SELECTION_BOUNDS_GROUP_ANCHOR');
  target={component_group:{[anchor.kind==='pin'?'pin':'finger']:anchor.value.id}};
 }
 const key=`${item.kind}:${item.id}`;
 queries.push({key,target});
 const result=completeSteps(index.locateSteps(item,display as any));
 expected.push({key,bounds:nativeBounds(result.bounds)});
 counts[item.kind as 'net'|'component']++;
}
const sample=(values:any[],limit=64)=>values.length<=limit?values:Array.from({length:limit},(_,i)=>values[Math.round(i*(values.length-1)/(limit-1))]);
for(const kind of ['segment','bond-wire','zone','pin','via','finger','drawing']) {
 for(const value of sample((index as any).objects.filter((object:any)=>object.kind===kind))) {
  const nativeKind=kind==='finger'?'via':kind==='bond-wire'?'segment':kind;
  const key=`object:${kind}:${value.value.id}`;
  queries.push({key,target:{object:{[nativeKind]:value.value.id}}});
  expected.push({key,bounds:nativeBounds(index.boundsFor([value]))});counts.object++;
 }
}
for(const [id,objects] of sample([...((index as any).tracks as Map<number,any[]>).entries()])) {
 const key=`track:${id}`;queries.push({key,target:{track:id}});
 expected.push({key,bounds:nativeBounds(index.boundsFor(objects))});counts.track++;
}
const baseline=baselineFlag==='baseline';
writeFileSync(resolve(outputDir,'request.json'),JSON.stringify({source:resolve(source),encoding,baseline,queries}));
writeFileSync(resolve(outputDir,'web.json'),JSON.stringify({sha256,results:expected}));
const run=spawnSync(resolve(probe),[resolve(outputDir,'request.json'),resolve(outputDir,'native.json')],{stdio:['ignore','pipe','inherit']});
if(run.status!==0)throw Error('SELECTION_BOUNDS_NATIVE_PROBE');
const actual=JSON.parse(readFileSync(resolve(outputDir,'native.json'),'utf8'));
if(actual.sha256!==sha256 || actual.results.length!==expected.length)throw Error('SELECTION_BOUNDS_SOURCE_OR_COUNT');
const error=(a:any,b:any)=>a===null||b===null?(a===b?0:Infinity):Math.max(...['min','max'].flatMap(k=>['x','y'].map(c=>Math.abs(a[k][c]-b[k][c]))));
const differences:any[]=[],sourceDifferences:any[]=[];let maximumError=0;
for(let i=0;i<expected.length;i++) {
 const a=actual.results[i],b=expected[i];if(a.key!==b.key)throw Error('SELECTION_BOUNDS_QUERY_ORDER');
 const e=error(a.bounds,b.bounds);maximumError=Math.max(maximumError,e);
 if(e>1e-6)differences.push({key:b.key,error_mm:Number.isFinite(e)?e:'null-bounds',expected:b.bounds,actual:a.bounds});
 if(baseline&&error(a.source_bounds,b.bounds)>1e-6)sourceDifferences.push({key:b.key,expected:b.bounds,source_bounds:a.source_bounds});
}
const paths=['src/lib/board/search.ts','src/lib/interaction/picking.ts','src/lib/allegro/scene-builder.ts','src/lib/board/shapes/zone.ts','src/lib/text/board-text-glyph-builder.ts'];
const report={source:resolve(source),encoding,sha256,counts,queries:queries.length,mismatches:differences.length,maximum_error_mm:Number.isFinite(maximumError)?maximumError:'null-bounds',baseline_checked:baseline,source_bounds_mismatches:baseline?sourceDifferences.length:null,differences,source_differences:sourceDifferences,runtime:{node:process.versions.node,v8:process.versions.v8},web_sources:paths.map(path=>({path,sha256:createHash('sha256').update(readFileSync(resolve(webRoot,path))).digest('hex')}))};
writeFileSync(resolve(outputDir,'report.json'),JSON.stringify(report,null,2));console.log(JSON.stringify({...report,differences:undefined,source_differences:undefined}));
if(differences.length)process.exitCode=1;
