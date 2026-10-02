// Independent oracle: complete frozen Web SceneBuilder, compared at selected track/via IDs.
import {readFileSync,writeFileSync,appendFileSync,openSync,readSync,closeSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {basename,resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
const [webRoot,probeManifest,sourceManifest,caseManifest,indexManifest,outputReport]=process.argv.slice(2);
if(!outputReport)throw new Error('ROUTING_PARITY_USAGE');
const json=(p:string)=>JSON.parse(readFileSync(p,'utf8').replace(/^\uFEFF/,''));
const lines=(p:string)=>readFileSync(p,'utf8').replace(/^\uFEFF/,'').trim().split(/\r?\n/).map(s=>JSON.parse(s));
const sha=(b:Buffer)=>createHash('sha256').update(b).digest('hex');
const sources=json(sourceManifest).files;
for(const required of ['src/lib/allegro/scene-builder.ts','src/lib/allegro/decoders/bond-wire.ts','src/lib/allegro/decoders/bond-finger.ts','src/lib/allegro/decoders/padstack.ts'])if(!sources.some((s:any)=>s.path===required))throw new Error(`SOURCE_NOT_FROZEN: ${required}`);
for(const source of sources)if(sha(readFileSync(resolve(webRoot,source.path)))!==source.sha256)throw new Error(`FROZEN_WEB_CHANGED: ${source.path}`);
const cases=new Map(lines(caseManifest).map(e=>[basename(e.path).toLowerCase(),e]));
const indexes=new Map(lines(indexManifest).map(e=>[basename(e.path).toLowerCase(),e]));
const entries=lines(probeManifest);
if(entries.length!==cases.size||new Set(entries.map(e=>basename(e.path).toLowerCase())).size!==cases.size||entries.some(e=>!cases.has(basename(e.path).toLowerCase())))throw new Error('FROZEN_CASE_SET_DIFFERS');
const load=(p:string)=>import(pathToFileURL(resolve(webRoot,p)).href);
const [{AllegroParser},{AllegroSceneBuilder},{AllegroUnits},{AllegroPadstackResolver}]=await Promise.all([load('src/lib/allegro/parser.ts'),load('src/lib/allegro/scene-builder.ts'),load('src/lib/allegro/units.ts'),load('src/lib/allegro/decoders/padstack.ts')]);
const point=(p:any)=>[p.x,p.y];
const nativeEdge=(s:any)=>({id:s.id,trackId:s.track_id,layer:s.layer===0xffffffff?-1:s.layer,net:s.net,a:point(s.a),b:point(s.b),width:s.width,
  ...(s.arc?{arc:{center:point(s.arc.center),radius:s.arc.radius,start:s.arc.start,sweep:s.arc.sweep}}:{}),
  ...(s.bond_wire?{bondWire:{profile:s.bond_wire.profile,material:s.bond_wire.material,sourcePin:s.bond_wire.source_pin,finger:s.bond_wire.finger,reference:s.bond_wire.reference,pinName:s.bond_wire.pin_name}}:{})});
const webEdge=(s:any)=>({...s,...(s.bondWire?{bondWire:{...s.bondWire,material:s.bondWire.material??null}}:{})});
const webPad=(p:any)=>({layer:p.layer,type:p.type,width:p.width,height:p.height,offset:p.offset,corner:p.corner??0,innerDiameter:p.innerDiameter??null,
  custom:p.custom?{contours:p.custom,paths:p.customPaths}:null,backdrill:!!p.backdrill,backdrillBase:!!p.backdrillBase});
const nativePad=(p:any)=>({layer:p.layer,type:p.kind,width:p.width,height:p.height,offset:point(p.offset),corner:p.corner,innerDiameter:p.inner_diameter,
  custom:p.custom?{contours:p.custom.contours.map((r:any[])=>r.map(point)),paths:p.custom.paths.map((r:any[])=>r.map(nativeEdge))}:null,backdrill:p.backdrill,backdrillBase:p.backdrill_base});
const nativeVia=(v:any)=>v?({id:v.id,net:v.net,at:point(v.at),drill:v.drill,drillShape:v.drill_shape,padstack:v.padstack,padstackName:v.padstack_name,startLayer:v.start_layer,endLayer:v.end_layer,
  pads:v.pads.map(nativePad),angle:v.angle,back:v.mirrored,finger:v.finger?{reference:v.finger.reference,name:v.finger.name,sourcePin:v.finger.source_pin}:null,
  stackupRegion:v.stackup_region?{sourceReference:v.stackup_region.source_reference,code:v.stackup_region.code}:null,
  backdrill:v.backdrill?{spans:v.backdrill.definition.spans.map((s:any)=>({startLayer:s.start_layer,stopLayer:s.stop_layer,protectedLayer:s.protected_layer})),displayDiameter:v.backdrill.definition.display_diameter,startPadDiameter:v.backdrill.definition.start_pad_diameter,labelDiameter:v.backdrill.definition.label_diameter,sourceReference:v.backdrill.source_reference,rotationDegrees:v.backdrill.rotation_degrees,mirrored:v.backdrill.mirrored}:null}):null;
const webVia=(v:any,emptySpan:boolean)=>v?({id:v.id,net:v.net,at:v.at,drill:v.drill,drillShape:v.drillShape,padstack:v.padstack,padstackName:v.padstackName??'',startLayer:emptySpan?null:v.startLayer,endLayer:emptySpan?null:v.endLayer,pads:v.pads.map(webPad),angle:v.angle??0,back:v.back??false,
  finger:v.finger?{reference:v.finger.reference,name:v.finger.name,sourcePin:v.finger.sourcePin??null}:null,stackupRegion:v.stackupRegion??null,backdrill:v.backdrill??null}):null;
function equal(a:any,b:any,path:string):number {
  if(typeof a!==typeof b||Array.isArray(a)!==Array.isArray(b))throw new Error(`ROUTING_TYPE_DIFFERS: ${path}`);
  if(a!==null&&typeof a==='object') {const keys=Object.keys(a).sort();if(JSON.stringify(keys)!==JSON.stringify(Object.keys(b??{}).sort()))throw new Error(`ROUTING_KEYS_DIFFER: ${path}`);return keys.reduce((n,k)=>n+equal(a[k],b[k],`${path}.${k}`),0);}
  if(typeof a==='number') {const exact=/\.(id|trackId|layer|net|type|padstack|startLayer|endLayer|stopLayer|protectedLayer|sourceReference|sourcePin|finger|code|object|offset|stack|track|via|pad_type|shape)$/.test(path);const tolerance=exact?0:1e-12*Math.max(1,Math.abs(a),Math.abs(b));if(!Number.isFinite(a)||!Number.isFinite(b)||Math.abs(a-b)>tolerance)throw new Error(`ROUTING_VALUE_DIFFERS: ${path} Web=${a} Rust=${b}`);}
  else if(a!==b)throw new Error(`ROUTING_VALUE_DIFFERS: ${path} Web=${a} Rust=${b}`);
  return 1;
}
async function compare(entry:any) {
  const source=cases.get(basename(entry.path).toLowerCase()),index=indexes.get(basename(entry.path).toLowerCase());
  const bytes=readFileSync(entry.path),request=json(entry.request),rows=lines(entry.report),metadata=rows.shift(),diagnostics=rows.pop();
  if(!source||sha(bytes)!==source.sha256||entry.sha256!==source.sha256||bytes.length!==source.bytes||entry.bytes!==source.bytes)throw new Error('FROZEN_CASE_HASH_DIFFERS');
  if(!index||index.stage!=='index'||index.status!=='passed'||index.sha256!==source.sha256||index.encoding!==entry.encoding)throw new Error('INDEX_IDENTITY_DIFFERS');
  if(entry.stage!=='routing'||entry.status!=='passed'||!Number.isInteger(entry.samples_per_type)||entry.samples_per_type<1||entry.samples_per_type>64)throw new Error('PROBE_MODE_INVALID');
  if(request.schema_version!==1||request.sha256!==source.sha256||request.source_size!==source.bytes||rows.length!==entry.records||rows.length!==request.spans.length)throw new Error('REQUEST_IDENTITY_DIFFERS');
  if(metadata.kind!=='metadata'||metadata.stage!=='routing'||metadata.path!==entry.path||metadata.sha256!==source.sha256||metadata.bytes!==source.bytes||metadata.encoding!==entry.encoding||metadata.scene_validated!==false||diagnostics?.kind!=='diagnostics'||diagnostics.stage!=='routing'||diagnostics.scene_validated!==false||diagnostics.localized_messages.length!==diagnostics.diagnostics.length)throw new Error('METADATA_INVALID');
  const db=await new AllegroParser(bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength),entry.encoding).parse();
  if(db.count!==index.index.records||db.strings.size!==index.index.strings||db.header.version!==metadata.version)throw new Error('WEB_INDEX_COUNTS_DIFFER');
  const expectedOffsets=new Set<number>(),special:number[]=[],counts:any={};
  for(const kind of [5,51]) {
    const offsets=Array.from(db.byType.get(kind)??[]) as number[];const indexCount=Object.entries(index.index.by_type).find(([tag])=>parseInt(tag,16)===kind)?.[1]??0;
    if(offsets.length!==indexCount)throw new Error('SOURCE_TYPE_COUNT_DIFFERS');if(offsets.length)counts[kind]=offsets.length;
    const n=Math.min(entry.samples_per_type,offsets.length);
    for(let i=0;i<n;i++)expectedOffsets.add(offsets[n>1?Math.floor(i*(offsets.length-1)/(n-1)):0]);
    for(const offset of offsets)if(bytes.readUInt16LE(offset+2)===(kind===5?0xfd06:0xc012)){special.push(offset);expectedOffsets.add(offset);}
  }
  equal(counts,entry.source_counts,'source_counts');equal(special.sort((a,b)=>a-b),entry.special_offsets,'special_offsets');
  if(request.spans.length!==expectedOffsets.size||JSON.stringify(request.spans.map((s:any)=>s.offset))!==JSON.stringify([...expectedOffsets].sort((a,b)=>a-b)))throw new Error('DETERMINISTIC_SELECTION_DIFFERS');
  const fd=openSync(index.index_data_path,'r'),signature=Buffer.alloc(16),scratch=Buffer.alloc(16);
  try {
    if(readSync(fd,signature,0,16,0)!==16||signature.subarray(0,8).toString()!=='PMIDX001'||signature.readUInt32LE(8)!==db.count||signature.readUInt32LE(12)!==db.strings.size)throw new Error('INDEX_SIGNATURE_INVALID');
    for(const span of request.spans){let low=0,high=db.count,found=false;while(low<high){const mid=low+Math.floor((high-low)/2);if(readSync(fd,scratch,0,16,16+mid*16)!==16)throw new Error('INDEX_TRUNCATED');const offset=scratch.readUInt32LE(0);if(offset<span.offset)low=mid+1;else if(offset>span.offset)high=mid;else{if(scratch.readUInt32LE(4)!==span.byte_length||scratch.readUInt32LE(8)!==span.key||scratch.readUInt32LE(12)!==span.record_type)throw new Error('SPAN_IDENTITY_DIFFERS');found=true;break;}}if(!found||db.offsets.get(span.key)!==span.offset)throw new Error('SPAN_NOT_INDEXED');}
  } finally {closeSync(fd);}
  const scene=await new AllegroSceneBuilder(db).build(),scale=AllegroUnits.toMillimeters(db.header.units,db.header.divisor);
  if(metadata.scale!==scale)throw new Error('SCALE_DIFFERS');
  const layerList=db.get(db.header.layerMap[6]?.recordId);
  const expectedLayers=scene.layers.map((l:any)=>({id:l.id,name:l.name,color:l.color,function:l.layerFunction,flags:l.sourceFlags??null}));
  const actualLayers=metadata.layers.map((l:any,i:number)=>({id:l.id,name:l.name||(!('Name' in layerList.Entries[i])&&!db.strings.has(layerList.Entries[i].NameId)?`Layer ${i+1}`:''),color:l.color,function:l.function.toLowerCase(),flags:l.source_flags}));
  let fields=equal(expectedLayers,actualLayers,'layers');
  const segments=new Map<number,any[]>(),vias=new Map(scene.vias.map((v:any)=>[v.id,v]));
  const trackIds=new Set(request.spans.filter((s:any)=>s.record_type===5).map((s:any)=>s.key)),viaIds=new Set(request.spans.filter((s:any)=>s.record_type===51).map((s:any)=>s.key)),stackIds=new Set<number>();
  for(const segment of scene.segments)if(trackIds.has(segment.trackId)){if(!segments.has(segment.trackId))segments.set(segment.trackId,[]);segments.get(segment.trackId)!.push(webEdge(segment));}
  const resolver=new AllegroPadstackResolver((id:number)=>db.get(id),scene.layers.length,db.header.version);
  let tracks=0,placedVias=0,edges=0,pads=0,backdrills=0,regions=0,fingers=0,wires=0,reverse=0,flip=0,custom=0;
  for(let i=0;i<rows.length;i++){
    const row=rows[i],span=request.spans[i];if(row.kind!=='record'||row.stage!=='routing'||row.path!==entry.path||row.error!==null||row.encoding!==entry.encoding||row.scene_validated!==false||row.key!==span.key||row.offset!==span.offset||row.byte_length!==span.byte_length||row.record_type!==span.record_type)throw new Error('ROW_IDENTITY_DIFFERS');
    if(span.record_type===5){const expected=segments.get(span.key)??[];fields+=equal(expected,row.routing.segments.map(nativeEdge),`track=${span.key}`);tracks++;edges+=expected.length;wires+=expected.filter(s=>s.bondWire).length;}
    else {const original=db.get(span.key),resolved=resolver.resolveVia(original.Padstack,original.Key);if(resolved)stackIds.add(resolved.stack.Key);const expected=webVia(vias.get(span.key),resolved?.stack.LayerCount===0);fields+=equal(expected,nativeVia(row.routing.via),`via=${span.key}`);if(expected){placedVias++;pads+=expected.pads.length;if(expected.backdrill)backdrills++;if(expected.stackupRegion)regions++;if(expected.finger)fingers++;custom+=expected.pads.filter((p:any)=>p.custom).length;if((original.LayerInfo&0x3000)===0x3000)reverse++;else if(original.LayerInfo&0x2000)flip++;}}
  }
  const warnings:any[]=[];
  for(const message of scene.diagnostics??[]){let m:RegExpExecArray|null,code='',key='',args:any,object=0;
    if((m=/^过孔 (\d+) 缺失定义或使用未支持的 Padstack 引用 (\d+)$/.exec(message))&&viaIds.has(+m[1])){code='BRD_VIA_DEFINITION_UNSUPPORTED';key='via_definition_unsupported';args={via:+m[1],stack:+m[2]};object=+m[1];}
    else if((m=/^键合指 (\d+) 的放置或 Padstack 变体尚未支持$/.exec(message))&&viaIds.has(+m[1])){code='BRD_BOND_FINGER_UNSUPPORTED';key='bond_finger_unsupported';args={finger:+m[1]};object=+m[1];}
    else if((m=/^键合线 (\d+) 的端点、profile 或段变体尚未支持$/.exec(message))&&trackIds.has(+m[1])){code='BRD_BOND_WIRE_UNSUPPORTED';key='bond_wire_unsupported';args={track:+m[1]};object=+m[1];}
    else if((m=/^走线 (\d+) 的层 (\d+) 未定义$/.exec(message))&&trackIds.has(+m[1])){code='BRD_TRACK_LAYER_UNDEFINED';key='track_layer_undefined';args={track:+m[1],layer:+m[2]};object=+m[1];}
    else if((m=/^Padstack (\d+) 的类型 (\d+) 焊盘尺寸无效$/.exec(message))&&stackIds.has(+m[1])){code='BRD_PAD_DIMENSIONS';key='pad_dimensions';args={stack:+m[1],pad_type:+m[2]};object=+m[1];}
    else if((m=/^Padstack (\d+) 的圆环内外径无效$/.exec(message))&&stackIds.has(+m[1])){code='BRD_PAD_DONUT';key='pad_donut';args={stack:+m[1]};object=+m[1];}
    else if((m=/^Padstack (\d+) 的焊盘类型 (\d+) 尚无有效几何(?:（形状引用 (\d+)）)?$/.exec(message))&&stackIds.has(+m[1])){code='BRD_PAD_UNSUPPORTED';key='pad_unsupported';args={stack:+m[1],pad_type:+m[2],shape:+(m[3]??0)};object=+m[1];}
    if(code)warnings.push({code,key:`import.allegro.${key}`,args,object,offset:db.offsets.get(object)??0,severity:'Warning'});
  }
  const actualWarnings=diagnostics.diagnostics.map((d:any)=>({code:d.code,key:d.message.key,args:d.message.args,object:d.object,offset:d.offset,severity:d.severity}));
  const order=(v:any[])=>v.sort((a,b)=>a.code.localeCompare(b.code)||a.object-b.object||JSON.stringify(a.args,Object.keys(a.args).sort()).localeCompare(JSON.stringify(b.args,Object.keys(b.args).sort())));
  fields+=equal(order(warnings),order(actualWarnings),'diagnostics');
  return {path:entry.path,matched:true,tracks,vias:placedVias,edges,pads,backdrills,regions,fingers,wires,reverse,flip,custom,special_requests:special.length,scalar_fields:fields,scene_validated:false};
}
writeFileSync(outputReport,'');
const totals:any={tracks:0,vias:0,edges:0,pads:0,backdrills:0,regions:0,fingers:0,wires:0,reverse:0,flip:0,custom:0,special_requests:0,scalar_fields:0};let matched=0;
for(let i=0;i<entries.length;i++){let result:any;try{result=await compare(entries[i]);matched++;for(const key of Object.keys(totals))totals[key]+=result[key];}catch(error){result={path:entries[i].path,matched:false,error:String(error),scene_validated:false};}appendFileSync(outputReport,JSON.stringify(result)+'\n');if((i+1)%10===0)process.stdout.write(JSON.stringify({cases:i+1,matched,...totals})+'\n');}
const summary={stage:'routing',compared:entries.length,matched,failed:entries.length-matched,...totals,oracle:'complete frozen AllegroSceneBuilder; deterministic selected tracks/vias and every bond-wire/finger layer record',scene_validated:false};
writeFileSync(outputReport+'.summary.json',JSON.stringify(summary,null,2)+'\n');process.stdout.write(JSON.stringify(summary)+'\n');if(matched!==entries.length)process.exitCode=1;
