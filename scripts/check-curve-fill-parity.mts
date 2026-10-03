// Directly compare the current Web curve-tessellator with shared native geometry.
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {resolve,basename} from 'node:path';
import {pathToFileURL} from 'node:url';
import {spawnSync} from 'node:child_process';
const [webRoot,probe,outputDirectory,...sources] = process.argv.slice(2);
if (!outputDirectory) throw Error('CURVE_FILL_PARITY_USAGE: WEB_ROOT NATIVE_PROBE OUTPUT_DIR [BRD:ENCODING ...]');
mkdirSync(outputDirectory,{recursive:true});
const load = (p:string)=>import(pathToFileURL(resolve(webRoot,p)).href);
const [{tessellateCurveRing},{AllegroParser},{AllegroSceneBuilder},{ZoneShape},{ArcShape}] = await Promise.all([
 load('src/lib/render/curve-tessellator.ts'),load('src/lib/allegro/parser.ts'),load('src/lib/allegro/scene-builder.ts'),
 load('src/lib/board/shapes/zone.ts'),load('src/lib/board/shapes/arc.ts'),
]);
const paths:any[][]=[];const queries:any[]=[];const expected:number[][][]=[];const cases:any[]=[];
const nativePoint=(p:number[])=>({x:p[0],y:p[1]});
const convertNativePath=(path:any[])=>path.map((edge,i)=>({id:i,track_id:0,layer:0,net:0,a:nativePoint(edge.a),b:nativePoint(edge.b),width:edge.width??0,
 arc:edge.arc?{...edge.arc,center:nativePoint(edge.arc.center)}:null,bond_wire:null}));
function add(path:any[],view:any,physicalScale:number,metadata:any,pathIndex?:number) {
 const index=pathIndex??paths.length;
 if(pathIndex===undefined) paths.push(convertNativePath(path));
 const tolerance=.25/2**Math.ceil(Math.log2(physicalScale));
 queries.push({path:index,view:{min:{x:view.minX,y:view.minY},max:{x:view.maxX,y:view.maxY}},tolerance});
 expected.push(tessellateCurveRing(path,view,tolerance));
 cases.push({...metadata,physical_scale:physicalScale,tolerance,view});
 return index;
}
const circle=(center:number[],radius:number,start:number,sweep:number)=>{
 const arc={center,radius,start,sweep};const shape=new ArcShape(arc);
 return [{a:shape.point(start),b:shape.point(start+sweep),width:0,arc}];
};
for(const center of [[0,0],[100000,-100000]])for(const radius of [1,1000000])for(const sweep of [Math.PI*2,-Math.PI*2]) {
 const path=circle(center,radius,0,sweep);
 let pathIndex:number|undefined;
 for(const scale of [1001,8192,50000,100000000]) {
  const angle=Math.PI/4;const at=new ArcShape(path[0].arc).point(angle);
  const half=64/scale;
  for(const [label,p]of [['arc',at],['center',center],['outside',[center[0]+radius*2,center[1]+radius*2]]] as const) {
   const view={minX:p[0]-half,maxX:p[0]+half,minY:p[1]-half,maxY:p[1]+half};
   pathIndex=add(path,view,scale,{case:'synthetic-circle',center,radius,sweep,view_kind:label},pathIndex);
  }
 }
}
const u=[[-3,-2],[3,-2],[3,3],[1,3],[1,0],[-1,0],[-1,3],[-3,3]];
add(u.map((a,i)=>({a,b:u[(i+1)%u.length],width:0})),{minX:-2.5,maxX:2.5,minY:1,maxY:2},8192,{case:'synthetic-disjoint-islands'});
const sourceReports:any[]=[];
for(const argument of sources) {
 const split=argument.lastIndexOf(':');const source=argument.slice(0,split),encoding=argument.slice(split+1);
 if(!source || !['utf-8','windows-1252'].includes(encoding))throw Error('CURVE_FILL_PARITY_SOURCE_REQUIRES_ENCODING');
 const bytes=readFileSync(source);const sha256=createHash('sha256').update(bytes).digest('hex');
 const db=await new AllegroParser(bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength),encoding).parse();
 const scene=await new AllegroSceneBuilder(db).build();
 const curved=scene.zones.filter((zone:any)=>zone.paths.some((path:any[])=>path.some(edge=>edge.arc)));
 const start=queries.length;const pathIndexes=new Map<string,number>();
 for(const zone of curved) {
  const arcEdge=zone.paths.flat().find((edge:any)=>edge.arc);
  const at=new ArcShape(arcEdge.arc).point(arcEdge.arc.start+arcEdge.arc.sweep/2);
  const b=new ZoneShape(zone).bounds();const center=[(b.minX+b.maxX)/2,(b.minY+b.maxY)/2];
  for(const scale of [1001,8192,50000])for(const dpi of [1,2])for(const [label,p]of [['arc',at],['center',center]] as const) {
   const half=64/scale;const view={minX:p[0]-half,maxX:p[0]+half,minY:p[1]-half,maxY:p[1]+half};
   const cached={minX:view.minX-half,maxX:view.maxX+half,minY:view.minY-half,maxY:view.maxY+half};
   for(const ring of [0,...new ZoneShape(zone).holeCandidates(cached)]) {
    const key=`${zone.id}:${ring}`;
    const index=add(zone.paths[ring],cached,scale*dpi,{case:basename(source),zone:zone.id,ring,scale,dpi,view_kind:label},pathIndexes.get(key));
    pathIndexes.set(key,index);
   }
  }
 }
 sourceReports.push({source,encoding,sha256,zones:scene.zones.length,curved_zones:curved.length,queries:queries.length-start});
 console.log(JSON.stringify(sourceReports.at(-1)));
}
const requestPath=resolve(outputDirectory,'request.json'),nativePath=resolve(outputDirectory,'native.json');
writeFileSync(requestPath,JSON.stringify({paths,queries}));
const run=spawnSync(resolve(probe),[requestPath,nativePath],{encoding:'utf8',stdio:['ignore','pipe','inherit']});
if(run.status!==0)throw Error(`CURVE_FILL_NATIVE_PROBE_FAILED exit=${run.status} error=${run.error??''}`);
const actual=JSON.parse(readFileSync(nativePath,'utf8')).results;
if(actual.length!==expected.length)throw Error('CURVE_FILL_QUERY_COUNT_DIFFERS');
let maxPhysicalError=0,points=0;const differences:any[]=[];
for(let i=0;i<expected.length;i++) {
 const a=actual[i],e=expected[i];points+=e.length;
 let error=Infinity;
 if(a.length===e.length) {
  error=0;
  for(let j=0;j<e.length;j++)error=Math.max(error,Math.abs(a[j].x-e[j][0]),Math.abs(a[j].y-e[j][1]));
  error*=cases[i].physical_scale;
 }
 maxPhysicalError=Math.max(maxPhysicalError,error);
 if(!(error<=1e-6))differences.push({query:i,...cases[i],native_points:a.length,web_points:e.length,physical_error:Number.isFinite(error)?error:null,
   ...(a.length!==e.length?{web:e,native:a}:{} )});
}
const webSources=['src/lib/render/curve-tessellator.ts','src/lib/render/curve-fill-layer.ts','src/lib/board/shapes/arc.ts','src/lib/board/shapes/zone.ts','src/lib/render/view-culling.ts'];
const report={runtime:{node:process.versions.node,v8:process.versions.v8},sources:sourceReports,web_sources:webSources.map(path=>({path,sha256:createHash('sha256').update(readFileSync(resolve(webRoot,path))).digest('hex')})),
 queries:queries.length,points,mismatches:differences.length,max_physical_error:Number.isFinite(maxPhysicalError)?maxPhysicalError:null,differences};
writeFileSync(resolve(outputDirectory,'report.json'),JSON.stringify(report,null,2));
console.log(JSON.stringify({queries:report.queries,points,mismatches:report.mismatches,max_physical_error:maxPhysicalError}));
if(differences.length)process.exitCode=1;
