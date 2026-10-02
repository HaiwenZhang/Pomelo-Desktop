// Compare native footprint queries to pins produced by the complete frozen Web SceneBuilder.
// No placement equations or substitutes for copper/text builders are used by this oracle.
import { readFileSync, writeFileSync, appendFileSync, openSync, readSync, closeSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { basename, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const [webRoot, probeManifest, sourceManifest, caseManifest, indexManifest, outputReport] = process.argv.slice(2);
if (!outputReport) throw new Error('PLACEMENT_PARITY_USAGE: WEB_ROOT PROBE_MANIFEST SOURCE_MANIFEST CASE_MANIFEST INDEX_MANIFEST OUTPUT_REPORT');
const json = (path: string) => JSON.parse(readFileSync(path,'utf8').replace(/^\uFEFF/,''));
const lines = (path: string) => readFileSync(path,'utf8').replace(/^\uFEFF/,'').trim().split(/\r?\n/).map(line=>JSON.parse(line));
const sha = (data: Buffer) => createHash('sha256').update(data).digest('hex');
const sourceFiles=json(sourceManifest).files;
for(const required of ['src/lib/allegro/scene-builder.ts','src/lib/allegro/decoders/padstack.ts','src/lib/allegro/units.ts']) {
  if(!sourceFiles.some((file:any)=>file.path===required)) throw new Error(`REQUIRED_SOURCE_NOT_FROZEN: ${required}`);
}
for (const file of sourceFiles) {
  if (sha(readFileSync(resolve(webRoot,file.path))) !== file.sha256) throw new Error(`FROZEN_WEB_CHANGED: ${file.path}`);
}
const cases = new Map(lines(caseManifest).map(row=>[basename(row.path).toLowerCase(),row]));
const indexes = new Map(lines(indexManifest).map(row=>[basename(row.path).toLowerCase(),row]));
const entries = lines(probeManifest);
if (entries.length!==cases.size || new Set(entries.map(row=>basename(row.path).toLowerCase())).size!==cases.size
    || entries.some(row=>!cases.has(basename(row.path).toLowerCase()))) throw new Error('FROZEN_CASE_SET_DIFFERS');
const load = (path: string) => import(pathToFileURL(resolve(webRoot,path)).href);
const [{AllegroParser},{AllegroSceneBuilder},{AllegroPadstackResolver},{AllegroUnits}] = await Promise.all([
  load('src/lib/allegro/parser.ts'),load('src/lib/allegro/scene-builder.ts'),load('src/lib/allegro/decoders/padstack.ts'),load('src/lib/allegro/units.ts'),
]);
const point = (p: any) => [p.x,p.y];
const edge = (s: any) => ({id:s.id,trackId:s.track_id,layer:s.layer===0xffffffff?-1:s.layer,net:s.net,
  a:point(s.a),b:point(s.b),width:s.width,...(s.arc?{arc:{center:point(s.arc.center),radius:s.arc.radius,start:s.arc.start,sweep:s.arc.sweep}}:{})});
const webPad = (p: any) => ({layer:p.layer,type:p.type,width:p.width,height:p.height,offset:p.offset,corner:p.corner??0,
  innerDiameter:p.innerDiameter??null,custom:p.custom?{contours:p.custom,paths:p.customPaths}:null,backdrill:!!p.backdrill,backdrillBase:!!p.backdrillBase});
const nativePad = (p: any) => ({layer:p.layer,type:p.kind,width:p.width,height:p.height,offset:point(p.offset),corner:p.corner,
  innerDiameter:p.inner_diameter,custom:p.custom?{contours:p.custom.contours.map((r:any[])=>r.map(point)),paths:p.custom.paths.map((r:any[])=>r.map(edge))}:null,
  backdrill:p.backdrill,backdrillBase:p.backdrill_base});
const webPin = (p:any,owner:number) => ({id:p.id,owner,net:p.net,name:p.name,reference:p.reference,at:p.at,angle:p.angle,mirrored:p.back,
  drill:p.drill,drillShape:p.drillShape,pads:p.shapes.map(webPad),stackupRegion:p.stackupRegion??null,die:p.die??null});
const nativePin = (p:any) => ({id:p.id,owner:p.owner_id,net:p.net,name:p.name,reference:p.reference,at:point(p.at),angle:p.angle,mirrored:p.mirrored,
  drill:p.drill,drillShape:p.drill_shape,pads:p.pads.map(nativePad),
  stackupRegion:p.stackup_region?{sourceReference:p.stackup_region.source_reference,code:p.stackup_region.code}:null,
  die:p.die?{sourceReference:p.die.source_reference,padstackName:p.die.padstack_name}:null});
function equal(expected:any, actual:any, path:string):number {
  if (typeof expected!==typeof actual || Array.isArray(expected)!==Array.isArray(actual)) throw new Error(`PLACEMENT_TYPE_DIFFERS: ${path}`);
  if (expected!==null && typeof expected==='object') {
    const keys=Object.keys(expected).sort(), other=Object.keys(actual??{}).sort();
    if (JSON.stringify(keys)!==JSON.stringify(other)) throw new Error(`PLACEMENT_KEYS_DIFFER: ${path}`);
    return keys.reduce((n,key)=>n+equal(expected[key],actual[key],`${path}.${key}`),0);
  }
  if (typeof expected==='number' && typeof actual==='number') {
    const exact=/\.(id|owner|net|layer|type|sourceReference|code|object|offset|pin|stack|pad_type|shape)$/.test(path);
    const tolerance=exact?0:1e-12*Math.max(1,Math.abs(expected),Math.abs(actual));
    if (!Number.isFinite(expected)||!Number.isFinite(actual)||Math.abs(expected-actual)>tolerance) throw new Error(`PLACEMENT_VALUE_DIFFERS: ${path} Web=${expected} Rust=${actual}`);
  } else if (expected!==actual) throw new Error(`PLACEMENT_VALUE_DIFFERS: ${path} Web=${expected} Rust=${actual}`);
  return 1;
}
async function compare(entry:any) {
  const source=cases.get(basename(entry.path).toLowerCase()), index=indexes.get(basename(entry.path).toLowerCase());
  const bytes=readFileSync(entry.path), rows=lines(entry.report), metadata=rows.shift(), diagnostics=rows.pop(), request=json(entry.request);
  if (!source || sha(bytes)!==source.sha256 || entry.sha256!==source.sha256 || bytes.length!==source.bytes || entry.bytes!==source.bytes) throw new Error('FROZEN_CASE_HASH_DIFFERS');
  if (!index || index.stage!=='index' || index.status!=='passed' || index.sha256!==source.sha256 || index.encoding!==entry.encoding) throw new Error('VALIDATED_INDEX_DIFFERS');
  if (entry.stage!=='placement' || entry.status!=='passed' || !Number.isInteger(entry.samples_per_type) || entry.samples_per_type<1 || entry.samples_per_type>64) throw new Error('PROBE_MODE_INVALID');
  // Source count spelling is fixed by the validated index producer.
  const sourceCount=Object.entries(index.index.by_type).find(([tag])=>parseInt(tag,16)===45)?.[1]??0;
  if (Object.keys(entry.source_counts).length!==(sourceCount?1:0) || (entry.source_counts['45']??0)!==sourceCount) throw new Error('SOURCE_TYPE_COVERAGE_DIFFERS');
  if (request.schema_version!==1 || request.sha256!==source.sha256 || request.source_size!==source.bytes || rows.length!==request.spans.length || rows.length!==entry.records
      || rows.length!==Math.min(entry.samples_per_type,sourceCount as number)) throw new Error('REQUEST_COUNTS_OR_IDENTITY_DIFFERS');
  if (metadata.kind!=='metadata' || metadata.stage!=='placement' || metadata.path!==entry.path || metadata.scene_validated!==false || metadata.sha256!==source.sha256 || metadata.bytes!==source.bytes
      || metadata.encoding!==entry.encoding || diagnostics?.kind!=='diagnostics' || diagnostics.stage!=='placement' || diagnostics.scene_validated!==false
      || diagnostics.localized_messages.length!==diagnostics.diagnostics.length) throw new Error('METADATA_INVALID');
  const db=await new AllegroParser(bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength),entry.encoding).parse();
  if (db.count!==index.index.records || db.strings.size!==index.index.strings || db.header.version!==metadata.version) throw new Error('WEB_DATABASE_COUNTS_DIFFER');
  const scene=await new AllegroSceneBuilder(db).build();
  const scale=AllegroUnits.toMillimeters(db.header.units,db.header.divisor);
  if(metadata.scale!==scale) throw new Error('PLACEMENT_SCALE_DIFFERS');
  const layerList=db.get(db.header.layerMap[6]?.recordId);
  const expectedLayers=scene.layers.map((l:any)=>({id:l.id,name:l.name,color:l.color,function:l.layerFunction,flags:l.sourceFlags??null}));
  const actualLayers=metadata.layers.map((l:any,i:number)=>({id:l.id,name:l.name||(!('Name' in layerList.Entries[i])&&!db.strings.has(layerList.Entries[i].NameId)?`Layer ${i+1}`:''),
    color:l.color,function:l.function.toLowerCase(),flags:l.source_flags}));
  const layerFields=equal(expectedLayers,actualLayers,'layers');
  const requested=new Map<number,any[]>(), seen=new Set<number>(), pinIds=new Set<number>(), stackIds=new Set<number>();
  const resolver=new AllegroPadstackResolver((key:number)=>db.get(key),scene.layers.length,db.header.version);
  const descriptor=openSync(index.index_data_path,'r'), signature=Buffer.alloc(16), scratch=Buffer.alloc(16);
  try {
    if (readSync(descriptor,signature,0,16,0)!==16 || signature.subarray(0,8).toString()!=='PMIDX001' || signature.readUInt32LE(8)!==db.count || signature.readUInt32LE(12)!==db.strings.size) throw new Error('INDEX_SIGNATURE_OR_COUNTS_DIFFER');
    const offsets=Array.from(db.byType.get(45)??[]) as number[];
    if(offsets.length!==sourceCount) throw new Error('WEB_FOOTPRINT_COUNTS_DIFFER');
    for (let i=0;i<request.spans.length;i++) {
      const span=request.spans[i],rank=request.spans.length>1?Math.floor(i*(offsets.length-1)/(request.spans.length-1)):0;
      if(span.offset!==offsets[rank]) throw new Error('DETERMINISTIC_SAMPLE_RANK_DIFFERS');
      if (seen.has(span.offset) || span.record_type!==45) throw new Error('DUPLICATE_OFFSET_OR_TYPE_INVALID'); seen.add(span.offset);
      let low=0,high=db.count,found=false;
      while (low<high) {
        const mid=low+Math.floor((high-low)/2);
        if (readSync(descriptor,scratch,0,16,16+mid*16)!==16) throw new Error('INDEX_TRUNCATED');
        const at=scratch.readUInt32LE(0);
        if (at<span.offset) low=mid+1; else if(at>span.offset) high=mid;
        else { if(scratch.readUInt32LE(4)!==span.byte_length || scratch.readUInt32LE(8)!==span.key || scratch.readUInt32LE(12)!==45) throw new Error('SPAN_IDENTITY_DIFFERS'); found=true;break; }
      }
      if(!found) throw new Error('SPAN_NOT_INDEXED');
      requested.set(span.key,[]);
      const fp=db.get(span.key); let key=fp.FirstPadPtr; const visited=new Set<number>();
      while(key && key!==fp.Key) {
        if(visited.has(key)) throw new Error('WEB_FP_CHAIN_CYCLE'); visited.add(key); pinIds.add(key);
        const pin=db.get(key); if(pin?.type!==50) throw new Error('WEB_FP_CHAIN_MISSING');
        const pad=db.get(pin.PadPtr),stack=pad&&resolver.resolvePin(pad.PadStack,pin.Key)?.stack;
        if(stack) stackIds.add(stack.Key); key=pin.NextInFp;
      }
    }
  } finally {closeSync(descriptor);}
  for(const pin of scene.pins) {
    const owner=db.get(pin.id)?.ParentFp;
    requested.get(owner)?.push(webPin(pin,owner));
  }
  let fields=layerFields,pins=0,back=0,die=0,pads=0,custom=0,regions=0;
  for(let i=0;i<rows.length;i++) {
    const row=rows[i],span=request.spans[i];
    if(row.kind!=='record'||row.stage!=='placement'||row.path!==entry.path||row.scene_validated!==false||row.error!==null||row.encoding!==entry.encoding
        ||row.key!==span.key||row.offset!==span.offset||row.byte_length!==span.byte_length||row.record_type!==45) throw new Error('ROW_IDENTITY_OR_STATUS_DIFFERS');
    const fp=db.get(span.key),component=db.get(fp.InstRef),expectedPins=requested.get(span.key)!;
    const expected={component:{id:fp.Key,sourceReference:fp.InstRef||null,reference:component?.RefDes??db.strings.get(component?.RefDesStrPtr)??'',
      at:[fp.CoordX*scale,fp.CoordY*scale],angle:fp.Rotation*Math.PI/180000,mirrored:fp.Layer!==0,pins:expectedPins.map(p=>p.id)},pins:expectedPins};
    const c=row.placement.component;
    const actual={component:{id:c.id,sourceReference:c.source_reference,reference:c.reference,at:point(c.at),angle:c.angle,mirrored:c.mirrored,pins:c.pins},pins:row.placement.pins.map(nativePin)};
    fields+=equal(expected,actual,`fp=${span.key}`);
    for(const p of expectedPins){pins++;if(p.mirrored)back++;if(p.die)die++;if(p.stackupRegion)regions++;pads+=p.pads.length;custom+=p.pads.filter((pad:any)=>pad.custom!==null).length;}
  }
  const expectedWarnings:any[]=[];
  for(const message of scene.diagnostics??[]) {
    let match:RegExpExecArray|null; let code='',key='',args:any,object=0;
    if((match=/^焊盘 (\d+) 缺失定义或使用未支持的 Padstack 引用 (\d+)$/.exec(message)) && pinIds.has(+match[1])) {code='BRD_PIN_DEFINITION_UNSUPPORTED';key='import.allegro.pin_definition_unsupported';args={pin:+match[1],stack:+match[2]};object=+match[1];}
    else if((match=/^裸片焊盘 (\d+) 的背面放置尚未核验$/.exec(message)) && pinIds.has(+match[1])) {code='BRD_DIE_BACK_UNSUPPORTED';key='import.allegro.die_back_unsupported';args={pin:+match[1]};object=+match[1];}
    else if((match=/^Padstack (\d+) 的类型 (\d+) 焊盘尺寸无效$/.exec(message)) && stackIds.has(+match[1])) {code='BRD_PAD_DIMENSIONS';key='import.allegro.pad_dimensions';args={stack:+match[1],pad_type:+match[2]};object=+match[1];}
    else if((match=/^Padstack (\d+) 的圆环内外径无效$/.exec(message)) && stackIds.has(+match[1])) {code='BRD_PAD_DONUT';key='import.allegro.pad_donut';args={stack:+match[1]};object=+match[1];}
    else if((match=/^Padstack (\d+) 的焊盘类型 (\d+) 尚无有效几何(?:（形状引用 (\d+)）)?$/.exec(message)) && stackIds.has(+match[1])) {code='BRD_PAD_UNSUPPORTED';key='import.allegro.pad_unsupported';args={stack:+match[1],pad_type:+match[2],shape:+(match[3]??0)};object=+match[1];}
    if(code) expectedWarnings.push({code,key,args,object,offset:db.offsets.get(object)??0,severity:'Warning'});
  }
  const actualWarnings=diagnostics.diagnostics.map((d:any)=>({code:d.code,key:d.message.key,args:d.message.args,object:d.object,offset:d.offset,severity:d.severity}));
  const order=(values:any[])=>values.sort((a,b)=>a.code.localeCompare(b.code)||a.object-b.object||JSON.stringify(a.args,Object.keys(a.args).sort()).localeCompare(JSON.stringify(b.args,Object.keys(b.args).sort())));
  fields+=equal(order(expectedWarnings),order(actualWarnings),'diagnostics');
  return {path:entry.path,matched:true,footprints:rows.length,pins,pads,back,die,custom,regions,scalar_fields:fields,web_scene_pins:scene.pins.length,scene_validated:false};
}
writeFileSync(outputReport,'');
let matched=0,pins=0,footprints=0,pads=0,fields=0;
for(let i=0;i<entries.length;i++) {
  let result:any;
  try {result=await compare(entries[i]);matched++;pins+=result.pins;footprints+=result.footprints;pads+=result.pads;fields+=result.scalar_fields;}
  catch(error){result={path:entries[i].path,matched:false,error:String(error),scene_validated:false};}
  appendFileSync(outputReport,JSON.stringify(result)+'\n');
  if((i+1)%10===0) process.stdout.write(JSON.stringify({cases:i+1,matched,pins,footprints})+'\n');
}
const summary={stage:'placement',compared:entries.length,matched,failed:entries.length-matched,footprints,pins,pads,scalar_fields:fields,
  oracle:'full frozen AllegroSceneBuilder; native checks selected footprints only',scene_validated:false};
writeFileSync(outputReport+'.summary.json',JSON.stringify(summary,null,2)+'\n');
process.stdout.write(JSON.stringify(summary)+'\n');
if(matched!==entries.length) process.exitCode=1;
