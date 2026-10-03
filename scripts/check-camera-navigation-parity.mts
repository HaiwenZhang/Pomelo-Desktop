// Read-only Web BoardViewport oracle; fake only the canvas bounds, never camera math.
import {readFileSync,writeFileSync,mkdirSync,createReadStream} from 'node:fs';
import {createInterface} from 'node:readline';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
const [webRoot,probe,outputDir,...sources]=process.argv.slice(2);
if(!outputDir)throw Error('CAMERA_PARITY_USAGE');
mkdirSync(outputDir,{recursive:true});
const {BoardViewport}=await import(pathToFileURL(resolve(webRoot,'src/lib/interaction/board-viewport.ts')).href);
const bounds=(x:number,y:number,w:number,h:number)=>({min:{x,y},max:{x:x+w,y:y+h}});
const webBounds=(b:any)=>({minX:b.min.x,minY:b.min.y,maxX:b.max.x,maxY:b.max.y});
const boardSpecs=[[0,0,100,80],[100000,-100000,100,2],[-5,-2,0,0],[-80,-46,82,79]];
const sourceReports=[];
const sceneProbe=process.env.POMELO_SCENE_BOUNDS_PROBE;
let sceneBoundsChecked=0,sceneBoundsMismatches=0;
if(sources.length) {
 const load=(path:string)=>import(pathToFileURL(resolve(webRoot,path)).href);
 const [{AllegroParser},{AllegroSceneBuilder}]=await Promise.all([load('src/lib/allegro/parser.ts'),load('src/lib/allegro/scene-builder.ts')]);
 for(const argument of sources) {
  const colon=argument.lastIndexOf(':');const source=argument.slice(0,colon),encoding=argument.slice(colon+1);
  if(!source || !['utf-8','windows-1252'].includes(encoding))throw Error('CAMERA_PARITY_ENCODING');
  const bytes=readFileSync(source);
  const db=await new AllegroParser(bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength),encoding).parse();
  const scene=await new AllegroSceneBuilder(db).build();const b=scene.bounds;
  boardSpecs.push([b.minX,b.minY,b.maxX-b.minX,b.maxY-b.minY]);
  let nativeBounds:any,maximumBoundsError:number|undefined;
  if(sceneProbe) {
   const journal=resolve(outputDir,`scene-${sourceReports.length}.jsonl`);
   const run=spawnSync(resolve(sceneProbe),['--encoding',encoding,'decode-scene',source,'--report',journal],{stdio:['ignore','pipe','inherit']});
   if(run.status!==0)throw Error('CAMERA_PARITY_SCENE_PROBE_FAILED');
   for await(const line of createInterface({input:createReadStream(journal),crlfDelay:Infinity}))
    if(line.startsWith('{"kind":"bounds"'))nativeBounds=JSON.parse(line).data;
   if(!nativeBounds)throw Error('CAMERA_PARITY_SCENE_BOUNDS_MISSING');
   maximumBoundsError=Math.max(Math.abs(nativeBounds.min.x-b.minX),Math.abs(nativeBounds.min.y-b.minY),Math.abs(nativeBounds.max.x-b.maxX),Math.abs(nativeBounds.max.y-b.maxY));
   sceneBoundsChecked++;if(maximumBoundsError>1e-6)sceneBoundsMismatches++;
  }
  sourceReports.push({source,encoding,sha256:createHash('sha256').update(bytes).digest('hex'),bounds:b,native_bounds:nativeBounds,maximum_bounds_error_mm:maximumBoundsError});
 }
}
const cases:any[]=[];
for(const [x,y,w,h] of boardSpecs)
for(const [width,height] of [[800,600],[1920,1080],[600,1200],[894.6666870117188,741.3333129882812]])
for(const flipped of [false,true]) {
 const b=bounds(x,y,w,h);const at={x:x+w*.37,y:y+h*.61};
 const actions:any[]=[{kind:'resize',width,height},...(flipped?[{kind:'flip'}]:[])];
 actions.push({kind:'fit'}, {kind:'resize',width:width*.63,height:height*.71}, {kind:'fit'});
 for(const factor of [1.2,1/1.2,.01,1e-10,1e10])actions.push({kind:'zoom',anchor:{x:width*.17,y:height*.83},factor});
 actions.push({kind:'pan',delta:{x:25.25,y:-18.75}},{kind:'flip'},
  {kind:'locate',bounds:bounds(at.x,at.y,0,0)},
  {kind:'locate',bounds:bounds(at.x,at.y,12,1)},
  {kind:'locate',bounds:bounds(at.x,at.y,.1,200)},
  {kind:'resize',width,height},{kind:'fit'});
 cases.push({bounds:b,actions});
}
const expected:any[]=[];
for(const test of cases) {
 let width=0,height=0,initialized=false;
 const source=webBounds(test.bounds);
 const canvas={getBoundingClientRect:()=>({width,height,left:0,top:0})};
 const v=new BoardViewport(canvas as any,()=>source,()=>{});
 const snapshots=[];
 for(const action of test.actions) {
  switch(action.kind) {
   case 'resize': width=action.width;height=action.height;if(!initialized){v.fit();initialized=true;}break;
   case 'fit':v.fit();break;
   case 'zoom':v.zoomAt(action.factor,action.anchor.x,action.anchor.y);break;
   case 'pan':v.pan(action.delta.x,action.delta.y);break;
   case 'flip':v.setFlipped(!v.camera.flipped);break;
   case 'locate':v.focusBounds(webBounds(action.bounds),source);break;
   default:throw Error('CAMERA_PARITY_UNKNOWN_ACTION');
  }
  v.publishView();
  snapshots.push({scale:v.camera.scale,flipped:v.camera.flipped,zoom:v.getView().zoom,
   center:{x:(source.minX+source.maxX)/2+v.camera.x,y:(source.minY+source.maxY)/2+v.camera.y},
   points:[[0,0],[width/2,height/2],[width,height]].map(([px,py])=>v.worldPoint(px,py))});
 }
 expected.push(snapshots);v.dispose();
}
const input=resolve(outputDir,'request.json'),output=resolve(outputDir,'native.json');
writeFileSync(input,JSON.stringify(cases));
const run=spawnSync(resolve(probe),[input,output],{stdio:['ignore','pipe','inherit']});
if(run.status!==0)throw Error('CAMERA_PARITY_PROBE_FAILED');
const actual=JSON.parse(readFileSync(output,'utf8'));
if(actual.length!==expected.length)throw Error('CAMERA_PARITY_CASE_COUNT');
let snapshots=0,maxRelativeError=0,maxPhysicalPointError=0;const differences=[];
for(let i=0;i<cases.length;i++) {
 if(actual[i].length!==expected[i].length)throw Error('CAMERA_PARITY_SNAPSHOT_COUNT');
 for(let j=0;j<expected[i].length;j++) {
  snapshots++;const a=actual[i][j],e=expected[i][j];
  const relative=Math.max(Math.abs(a.camera.pixels_per_mm-e.scale)/Math.max(1,Math.abs(e.scale)),
   Math.abs(a.zoom-e.zoom)/Math.max(1,Math.abs(e.zoom)));
  let physical=Math.max(Math.abs(a.camera.center.x-e.center.x),Math.abs(a.camera.center.y-e.center.y))*e.scale;
  for(let p=0;p<3;p++)physical=Math.max(physical,Math.abs(a.points[p].x-e.points[p][0])*e.scale,Math.abs(a.points[p].y-e.points[p][1])*e.scale);
  maxRelativeError=Math.max(maxRelativeError,relative);maxPhysicalPointError=Math.max(maxPhysicalPointError,physical);
  if(relative>1e-12 || physical>1e-3 || a.camera.flipped!==e.flipped)
   differences.push({case:i,step:j,action:cases[i].actions[j],relative_error:relative,physical_error:physical,native:a,web:e});
 }
}
const sourcePaths=['src/lib/interaction/camera.ts','src/lib/interaction/board-viewport.ts','src/lib/render/webgpu-renderer.ts','src/lib/board/bounds.ts','src/lib/allegro/scene/copper.ts','src/lib/allegro/scene/graphics.ts'];
const report={runtime:{node:process.versions.node,v8:process.versions.v8},sources:sourceReports,scene_bounds_checked:sceneBoundsChecked,scene_bounds_mismatches:sceneBoundsMismatches,cases:cases.length,snapshots,mismatches:differences.length,
 max_relative_error:maxRelativeError,max_physical_point_error:maxPhysicalPointError,
 web_sources:sourcePaths.map(path=>({path,sha256:createHash('sha256').update(readFileSync(resolve(webRoot,path))).digest('hex')})),differences};
writeFileSync(resolve(outputDir,'report.json'),JSON.stringify(report,null,2));
console.log(JSON.stringify({...report,differences:undefined}));
if(differences.length || sceneBoundsMismatches)process.exitCode=1;
