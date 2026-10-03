// Compare the native GPU instance geometry to the latest Web PadShape.iterateEdges.
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {resolve,basename} from 'node:path';
import {pathToFileURL} from 'node:url';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
const [webRoot,nativeProbe,source,outputDirectory,encoding='utf-8']=process.argv.slice(2);
if(!outputDirectory)throw Error('CUSTOM_PAD_OUTLINE_PARITY_USAGE');
mkdirSync(outputDirectory,{recursive:true});
const load=(path:string)=>import(pathToFileURL(resolve(webRoot,path)).href);
const [{AllegroParser},{AllegroSceneBuilder},{PadShape}]=await Promise.all([
 load('src/lib/allegro/parser.ts'),load('src/lib/allegro/scene-builder.ts'),load('src/lib/board/shapes/pad.ts'),
]);
const bytes=readFileSync(source),sha256=createHash('sha256').update(bytes).digest('hex');
const db=await new AllegroParser(bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength),encoding).parse();
const scene=await new AllegroSceneBuilder(db).build();
const expected:any[]=[];
for(const [kind,owners] of [['pin',scene.pins],['via',scene.vias]] as const){
 for(const owner of owners)for(const pad of kind==='pin'?owner.shapes:owner.pads){
  if(!pad.custom?.length)continue;
  for(const edge of new PadShape(pad).iterateEdges(owner))expected.push({owner:kind,id:owner.id,layer:pad.layer,net:owner.net,a:edge.a,b:edge.b,arc:edge.arc??null});
 }
}
const nativePath=resolve(outputDirectory,'native.json');
const run=spawnSync(resolve(nativeProbe),[resolve(source),nativePath,encoding],{encoding:'utf8',maxBuffer:1024*1024});
if(run.status!==0)throw Error(`CUSTOM_PAD_OUTLINE_PROBE_FAILED: ${run.stderr}`);
const actual=JSON.parse(readFileSync(nativePath,'utf8'));
if(actual.sha256!==sha256)throw Error('SOURCE_HASH_CHANGED');
// Owners and pads are sorted by layer in native preparation. Compare edge multisets,
// retaining repeated contours/placements instead of silently deduplicating them.
const key=(e:any)=>`${e.owner}:${e.id}:${e.layer}:${e.a.map((n:number)=>n.toFixed(6)).join(',')}:${e.b.map((n:number)=>n.toFixed(6)).join(',')}:${e.arc?'arc':'line'}`;
expected.sort((a,b)=>key(a).localeCompare(key(b)));
actual.edges.sort((a:any,b:any)=>key(a).localeCompare(key(b)));
const near=(a:number,b:number)=>Number.isFinite(a)&&Number.isFinite(b)&&Math.abs(a-b)<=Math.max(1e-7,Math.abs(a)*2e-7);
const point=(a:number[],b:number[])=>a.every((n,i)=>Number.isFinite(n)&&Number.isFinite(b[i])&&Math.abs(n-b[i])<=1e-7);
const angle=(a:number,b:number)=>Math.abs(Math.atan2(Math.sin(a-b),Math.cos(a-b)))<=2e-6;
const differences:any[]=[];
if(expected.length!==actual.edges.length)differences.push({count:{expected:expected.length,actual:actual.edges.length}});
for(let i=0;i<Math.min(expected.length,actual.edges.length);i++){
 const e=expected[i],a=actual.edges[i];
 const arc=!!e.arc===!!a.arc&&(!e.arc||(point(e.arc.center,a.arc.center)&&near(e.arc.radius,a.arc.radius)&&angle(e.arc.start,a.arc.start)&&near(e.arc.sweep,a.arc.sweep)));
 if(e.owner!==a.owner||e.id!==a.id||e.layer!==a.layer||e.net!==a.net||!point(e.a,a.a)||!point(e.b,a.b)||!arc)differences.push({expected:e,actual:a});
}
const report={case:basename(source),sha256,encoding,edges:expected.length,arcs:expected.filter(e=>e.arc).length,mismatches:differences.length,differences:differences.slice(0,30),reference:{path:'src/lib/board/shapes/pad.ts',sha256:createHash('sha256').update(readFileSync(resolve(webRoot,'src/lib/board/shapes/pad.ts'))).digest('hex')}};
writeFileSync(resolve(outputDirectory,'report.json'),JSON.stringify(report,null,2));
console.log(JSON.stringify({case:report.case,edges:report.edges,arcs:report.arcs,mismatches:report.mismatches}));
if(differences.length)process.exitCode=1;
