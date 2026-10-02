// Full scene counts, bounds and source-order object identities. This is not a field/pixel oracle.
import {readFileSync, writeFileSync, appendFileSync, createReadStream} from 'node:fs';
import {createHash} from 'node:crypto';
import {basename, resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {createInterface} from 'node:readline';
const [webRoot,probeManifest,sourceManifest,caseManifest,indexManifest,earcutManifest,output]=process.argv.slice(2);
if(!output)throw new Error('SCENE_SUMMARY_PARITY_USAGE');
const json=(p:string)=>JSON.parse(readFileSync(p,'utf8').replace(/^\uFEFF/,''));
const lines=(p:string)=>readFileSync(p,'utf8').replace(/^\uFEFF/,'').trim().split(/\r?\n/).map(s=>JSON.parse(s));
const sha=(b:Buffer)=>createHash('sha256').update(b).digest('hex');
const files=json(sourceManifest).files;
if(!files.some((f:any)=>f.path==='src/lib/allegro/scene-builder.ts'))throw new Error('SCENE_BUILDER_NOT_FROZEN');
for(const f of files)if(sha(readFileSync(resolve(webRoot,f.path)))!==f.sha256)throw new Error(`FROZEN_WEB_CHANGED: ${f.path}`);
const triangulator=json(earcutManifest);
for(const f of triangulator.files)if(sha(readFileSync(resolve(webRoot,f.path)))!==f.sha256)throw new Error('EARCUT_CHANGED');
const cases=new Map(lines(caseManifest).map(r=>[basename(r.path).toLowerCase(),r]));
const indexes=new Map(lines(indexManifest).map(r=>[basename(r.path).toLowerCase(),r]));
const entries=lines(probeManifest);
const names=['15061-1b.brd','agilex_i_series.brd','ml623_brd_revd_rdf0074.brd','s5000c-64_ddr5_bga_v0.61.brd','ss8633a_ampb_fpc_doe_v2_hvt_a_0423_1716.brd','ntpcb_320mb.brd'];
if(JSON.stringify(entries.map(e=>basename(e.path).toLowerCase()).sort())!==JSON.stringify(names.sort()))throw new Error('SIX_CASE_SET_DIFFERS');
const load=(p:string)=>import(pathToFileURL(resolve(webRoot,p)).href);
const [{AllegroParser},{AllegroSceneBuilder}]=await Promise.all([load('src/lib/allegro/parser.ts'),load('src/lib/allegro/scene-builder.ts')]);
async function compare(entry:any){
 const source=cases.get(basename(entry.path).toLowerCase()),index=indexes.get(basename(entry.path).toLowerCase()),bytes=readFileSync(entry.path);
 if(!source||!index||index.status!=='passed'||index.sha256!==source.sha256||bytes.length!==source.bytes||sha(bytes)!==source.sha256||entry.status!=='passed')throw new Error('SOURCE_IDENTITY_DIFFERS');
 const db=await new AllegroParser(bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength),index.encoding).parse();
 const scene=await new AllegroSceneBuilder(db).build();
 const ids:any={};
 for(const kind of ['layers','segments','vias','pins','zones','outline','texts','drawings'])ids[kind]=scene[kind].map((o:any)=>o.id);
 ids.special_layers=(scene.specialLayers??[]).map((o:any)=>o.id);
 ids.drawing_layers=scene.drawingLayers.map((o:any)=>o.id);
 ids.components=[...db.records(0x2d)].map(o=>o.Key);
 ids.nets=[...scene.nets.keys()].sort((a,b)=>a-b);
 const expected:any=Object.fromEntries(Object.entries(ids).map(([k,v]:[string,any])=>[k,v.length]));
 expected.diagnostics=(scene.diagnostics??[]).length;expected.bounds=1;
 const seen:any=Object.fromEntries(Object.keys(expected).map(k=>[k,0]));
 let metadata:any,complete=false,rows=0;
 const input=createInterface({input:createReadStream(entry.report,{encoding:'utf8',highWaterMark:64*1024}),crlfDelay:Infinity});
 try { for await(const line of input){
  const row=JSON.parse(line);rows++;
  if(!metadata){metadata=row;
   if(row.kind!=='metadata'||row.schema_version!==1||row.stage!=='scene'||row.sha256!==source.sha256||row.bytes!==source.bytes||row.path!==entry.path||row.error!==null||row.encoding!==index.encoding||row.version!==db.header.version||row.source_records!==db.count||row.source_strings!==db.strings.size||row.scene_validated!==false)throw new Error('METADATA_DIFFERS');
   for(const k of Object.keys(expected))if(row.counts[k]!==expected[k])throw new Error(`SCENE_COUNT_DIFFERS: ${k} Web=${expected[k]} Rust=${row.counts[k]}`);
   if(Object.keys(row.counts).length!==Object.keys(expected).length)throw new Error('EXTRA_COUNTS');
   continue;
  }
  if(complete)throw new Error('ROWS_AFTER_COMPLETE');
  if(row.kind==='complete'){
   if(row.index!==0||JSON.stringify(row.data)!==JSON.stringify(metadata.counts))throw new Error('FOOTER_DIFFERS');complete=true;continue;
  }
  if(!(row.kind in seen)||row.index!==seen[row.kind]||row.index>=expected[row.kind])throw new Error('ROW_COVERAGE_DIFFERS');
  seen[row.kind]++;
  if(row.kind==='bounds'){
   for(const [e,a] of [[scene.bounds.minX,row.data.min.x],[scene.bounds.minY,row.data.min.y],[scene.bounds.maxX,row.data.max.x],[scene.bounds.maxY,row.data.max.y]])if(!Number.isFinite(e)||!Number.isFinite(a)||Math.abs(e-a)>1e-12*Math.max(1,Math.abs(e),Math.abs(a)))throw new Error(`BOUNDS_DIFFER: Web=${e} Rust=${a}`);
  }else if(row.kind!=='diagnostics'){
   const id=row.kind==='nets'?row.data[0]:row.data.id;
   if(id!==ids[row.kind][row.index])throw new Error(`OBJECT_ORDER_DIFFERS: ${row.kind}.${row.index} Web=${ids[row.kind][row.index]} Rust=${id}`);
  }
 }}finally{input.close();}
 if(!metadata||!complete||Object.keys(expected).some(k=>seen[k]!==expected[k]))throw new Error('JOURNAL_TRUNCATED');
 return{path:entry.path,matched:true,version:db.header.version,counts:expected,rows,scope:'complete counts, bounds and source-order identities; not complete field or GPU validation',scene_validated:false};
}
writeFileSync(output,'');let matched=0;const totals:any={};
for(const entry of entries){let result:any;try{result=await compare(entry);matched++;for(const[k,v]of Object.entries(result.counts))totals[k]=(totals[k]??0)+(v as number);}catch(error){result={path:entry.path,matched:false,error:String(error),scene_validated:false};}appendFileSync(output,JSON.stringify(result)+'\n');process.stdout.write(JSON.stringify(result)+'\n');}
const summary={stage:'scene-summary',compared:entries.length,matched,failed:entries.length-matched,totals,scope:'full frozen Web SceneBuilder counts, bounds and source-order identities; journal integrity; not full field or GPU validation',scene_validated:false};
writeFileSync(output+'.summary.json',JSON.stringify(summary,null,2)+'\n');process.stdout.write(JSON.stringify(summary)+'\n');if(matched!==entries.length)process.exitCode=1;
