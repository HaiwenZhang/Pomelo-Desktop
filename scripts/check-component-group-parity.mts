// Read-only actual Web search and component selection oracle.
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
const [webRoot,probe,source,outputDir,encoding='utf-8']=process.argv.slice(2);
if(!outputDir)throw Error('COMPONENT_PARITY_USAGE');
mkdirSync(outputDir,{recursive:true});
const load=(path:string)=>import(pathToFileURL(resolve(webRoot,path)).href);
const [{AllegroParser},{AllegroSceneBuilder},{BoardSearchIndex},{BoardIndex},{MsdfFont}]=await Promise.all([
 load('src/lib/allegro/parser.ts'),load('src/lib/allegro/scene-builder.ts'),load('src/lib/board/search.ts'),
 load('src/lib/interaction/picking.ts'),load('src/lib/text/msdf-font.ts'),
]);
const bytes=readFileSync(source),sha256=createHash('sha256').update(bytes).digest('hex');
let scene:any;
if(encoding==='synthetic') {
 const input=JSON.parse(bytes.toString('utf8'));
 // Fixture adapter only converts the two source model layouts. Geometry is
 // deliberately empty; actual BoardIndex still builds its reference groups.
 scene={segments:[],zones:[],outline:[],texts:[],drawings:[],drawingLayers:[],diagnostics:[],
  nets:new Map(Object.entries(input.nets).map(([id,name])=>[Number(id),name])),
  bounds:{minX:input.bounds.min.x,minY:input.bounds.min.y,maxX:input.bounds.max.x,maxY:input.bounds.max.y},
  pins:input.pins.map((pin:any)=>({id:pin.id,net:pin.net,name:pin.name,reference:pin.reference,at:[pin.at.x,pin.at.y],angle:pin.angle,back:pin.mirrored,drill:0,shapes:[]})),
  vias:input.vias.map((via:any)=>({id:via.id,net:via.net,at:[via.at.x,via.at.y],angle:via.angle,back:via.mirrored,drill:0,pads:[],finger:via.finger?{reference:via.finger.reference,name:via.finger.name,sourcePin:via.finger.source_pin}:undefined}))};
} else {
 const db=await new AllegroParser(bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength),encoding).parse();
 scene=await new AllegroSceneBuilder(db).build();
}
const entries=BoardSearchIndex.buildItems(scene);
await MsdfFont.prepare(scene.texts,undefined,async(block:number)=>JSON.parse(readFileSync(resolve(webRoot,`public/fonts/source-han-sans/${block.toString(16).padStart(2,'0')}.json`),'utf8')));
const index=await BoardIndex.create(scene);
const groupEntries=entries.filter((entry:any)=>entry.kind==='component');
const object=(value:any)=>[value.kind==='finger'?'via':value.kind,value.value.id];
const groups=groupEntries.map((entry:any)=>{
 // Inspect the actual grouping prepared by BoardIndex. select() independently
 // exercises component-mode expansion even when a group has no visible entry.
 const values=(index as any).components.get(entry.id)??[];
 const anchor=values.at(-1);
 const selection=anchor?index.select({object:anchor,distance:0,sequence:0},'component'):undefined;
 if(selection?.mode!=='component')throw Error('COMPONENT_PARITY_WEB_SELECTION');
 const members=selection.objects.map(object);
 return {name:entry.name,count:entry.count,members,pages:members,
  hover_members:[...members].sort((a:any,b:any)=>a[0].localeCompare(b[0])||a[1]-b[1]),
  selected_pin_count:members.filter((item:any)=>item[0]==='pin').length,
  summary:{pins:members.filter((item:any)=>item[0]==='pin').length,vias:members.filter((item:any)=>item[0]==='via').length,segments:0,zones:0}};
});
const expected={sha256,entries,groups};
writeFileSync(resolve(outputDir,'web.json'),JSON.stringify(expected));
const nativePath=resolve(outputDir,'native.json');
const run=spawnSync(resolve(probe),[resolve(source),encoding,nativePath],{stdio:['ignore','pipe','inherit']});
if(run.status!==0)throw Error('COMPONENT_PARITY_NATIVE_PROBE');
const actual=JSON.parse(readFileSync(nativePath,'utf8'));
const normalize=(value:any):any=>Array.isArray(value)?value.map(normalize):value!==null&&typeof value==='object'
 ?Object.fromEntries(Object.keys(value).sort().map(key=>[key,normalize(value[key])])):value;
const differences=[];
for(const key of ['sha256','entries','groups'])if(JSON.stringify(normalize(actual[key]))!==JSON.stringify(normalize((expected as any)[key])))differences.push(key);
const paths=['src/lib/board/search.ts','src/lib/interaction/picking.ts','src/lib/allegro/scene-builder.ts'];
const report={source:resolve(source),encoding,sha256,groups:groups.length,members:groups.reduce((sum:number,g:any)=>sum+g.count,0),
 duplicate_placement_references:actual.duplicate_placement_references,
 finger_only_groups:groups.filter((g:any)=>g.summary.pins===0).length,
 source_placements:actual.source_placements,source_pins:actual.source_pins,source_fingers:actual.source_fingers,
 mismatches:differences.length,differences,runtime:{node:process.versions.node,v8:process.versions.v8},
 web_sources:paths.map(path=>({path,sha256:createHash('sha256').update(readFileSync(resolve(webRoot,path))).digest('hex')}))};
writeFileSync(resolve(outputDir,'report.json'),JSON.stringify(report,null,2));
console.log(JSON.stringify(report));
if(differences.length)process.exitCode=1;
