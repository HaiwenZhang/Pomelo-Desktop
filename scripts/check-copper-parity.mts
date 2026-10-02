// Independent oracle: frozen complete Web SceneBuilder; selected copper objects only.
import {readFileSync,writeFileSync,appendFileSync,openSync,readSync,closeSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {basename,resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
const [webRoot,probeManifest,sourceManifest,caseManifest,indexManifest,earcutManifest,output]=process.argv.slice(2);
if(!output)throw new Error('COPPER_PARITY_USAGE');
const json=(p:string)=>JSON.parse(readFileSync(p,'utf8').replace(/^\uFEFF/,''));
const lines=(p:string)=>readFileSync(p,'utf8').replace(/^\uFEFF/,'').trim().split(/\r?\n/).map(line=>JSON.parse(line));
const sha=(bytes:Buffer)=>createHash('sha256').update(bytes).digest('hex');
const sources=json(sourceManifest).files;
for(const required of ['src/lib/allegro/scene-builder.ts','src/lib/board/copper-mesh.ts','src/lib/board/shapes/contour.ts'])if(!sources.some((source:any)=>source.path===required))throw new Error(`SOURCE_NOT_FROZEN: ${required}`);
for(const source of sources)if(sha(readFileSync(resolve(webRoot,source.path)))!==source.sha256)throw new Error(`FROZEN_WEB_CHANGED: ${source.path}`);
const triangulator=json(earcutManifest);
if(triangulator.version!=='3.2.3'||json(resolve(webRoot,'node_modules/earcut/package.json')).version!==triangulator.version)throw new Error('EARCUT_VERSION_DIFFERS');
for(const source of triangulator.files)if(sha(readFileSync(resolve(webRoot,source.path)))!==source.sha256)throw new Error('EARCUT_SOURCE_CHANGED');
const expectedNames=['SS8633A_AMPB_FPC_DOE_V2_HVT_A_0423_1716.brd','ML623_BRD_revD_rdf0074.brd','AGILEX_I_SERIES.brd','ntpcb_320mb.brd','15061-1b.brd','S5000C-64_DDR5_BGA_V0.61.brd'];
const entries=lines(probeManifest);
if(JSON.stringify(entries.map(e=>basename(e.path)).sort())!==JSON.stringify(expectedNames.sort()))throw new Error('REPRESENTATIVE_CASE_SET_DIFFERS');
const cases=new Map(lines(caseManifest).map(entry=>[basename(entry.path).toLowerCase(),entry]));
const indexes=new Map(lines(indexManifest).map(entry=>[basename(entry.path).toLowerCase(),entry]));
const load=(path:string)=>import(pathToFileURL(resolve(webRoot,path)).href);
const [{AllegroParser},{AllegroSceneBuilder},{AllegroGeometryDecoder},{AllegroUnits}]=await Promise.all([load('src/lib/allegro/parser.ts'),load('src/lib/allegro/scene-builder.ts'),load('src/lib/allegro/decoders/geometry.ts'),load('src/lib/allegro/units.ts')]);
const point=(p:any)=>[p.x,p.y];
const box=(b:any)=>({minX:b.min.x,minY:b.min.y,maxX:b.max.x,maxY:b.max.y});
const edge=(s:any)=>({id:s.id,trackId:s.track_id,layer:s.layer===0xffffffff?-1:s.layer,net:s.net,a:point(s.a),b:point(s.b),width:s.width,...(s.arc?{arc:{center:point(s.arc.center),radius:s.arc.radius,start:s.arc.start,sweep:s.arc.sweep}}:{})});
const nativeZone=(zone:any)=>zone?({id:zone.id,layer:zone.layer,net:zone.net,paths:zone.paths.map((path:any[])=>path.map(edge)),points:zone.mesh.vertices.flatMap(point),indices:zone.mesh.indices,outerCount:zone.mesh.outer_count,ringOffsets:zone.mesh.ring_offsets,ringBounds:zone.mesh.ring_bounds.flatMap((b:any)=>[b.min.x,b.min.y,b.max.x,b.max.y]),ringOrder:zone.mesh.ring_order,holeChunks:zone.mesh.hole_chunks.map((c:any)=>({start:c.start,count:c.count,ringStart:c.ring_start,ringCount:c.ring_count,bounds:box(c.bounds)})),curved:zone.mesh.curved}):null;
const webZone=(zone:any)=>zone?({id:zone.id,layer:zone.layer,net:zone.net,paths:zone.paths,points:Array.from(zone.points),indices:Array.from(zone.indices),outerCount:zone.outerCount,ringOffsets:Array.from(zone.ringOffsets),ringBounds:Array.from(zone.ringBounds),ringOrder:Array.from(zone.ringOrder),holeChunks:zone.holeChunks,curved:!!zone.curved}):null;
function equal(a:any,b:any,path:string,integer=false):number {
    if(Array.isArray(a)||Array.isArray(b)){
        if(!Array.isArray(a)||!Array.isArray(b)||a.length!==b.length)throw new Error(`ARRAY_DIFFERS: ${path}`);
        const exact=integer||/\.(indices|ringOffsets|ringOrder)$/.test(path);
        let fields=0;for(let i=0;i<a.length;i++)fields+=equal(a[i],b[i],`${path}.${i}`,exact);return fields;
    }
    if(a!==null&&typeof a==='object'){
        if(b===null||typeof b!=='object'||JSON.stringify(Object.keys(a).sort())!==JSON.stringify(Object.keys(b).sort()))throw new Error(`KEYS_DIFFER: ${path}`);
        let fields=0;for(const key of Object.keys(a))fields+=equal(a[key],b[key],`${path}.${key}`,integer);return fields;
    }
    if(typeof a==='number'){
        const exact=integer||/\.(id|trackId|layer|net|outerCount|ringStart|ringCount|offset|key|record_type|byte_length)$/.test(path)||/\.holeChunks\.\d+\.(start|count)$/.test(path);
        const tolerance=exact?0:1e-12*Math.max(1,Math.abs(a),Math.abs(b));
        if(typeof b!=='number'||!Number.isFinite(a)||!Number.isFinite(b)||Math.abs(a-b)>tolerance)throw new Error(`VALUE_DIFFERS: ${path} Web=${a} Rust=${b}`);
    }else if(a!==b)throw new Error(`VALUE_DIFFERS: ${path} Web=${a} Rust=${b}`);
    return 1;
}
async function compare(entry:any){
    const source=cases.get(basename(entry.path).toLowerCase()),index=indexes.get(basename(entry.path).toLowerCase());
    const bytes=readFileSync(entry.path),request=json(entry.request),rows=lines(entry.report),metadata=rows.shift(),diagnostics=rows.pop();
    if(!source||sha(bytes)!==source.sha256||entry.sha256!==source.sha256||entry.bytes!==source.bytes||bytes.length!==source.bytes)throw new Error('SOURCE_IDENTITY_DIFFERS');
    if(!index||index.stage!=='index'||index.status!=='passed'||index.sha256!==source.sha256||index.encoding!==entry.encoding)throw new Error('INDEX_IDENTITY_DIFFERS');
    if(entry.stage!=='copper'||entry.status!=='passed'||entry.samples_per_type!==32||entry.special_offsets.length!==0)throw new Error('PROBE_MODE_INVALID');
    if(request.schema_version!==1||request.sha256!==source.sha256||request.source_size!==source.bytes||request.spans.length!==rows.length||rows.length!==entry.records)throw new Error('REQUEST_IDENTITY_DIFFERS');
    if(metadata.kind!=='metadata'||metadata.stage!=='copper'||metadata.path!==entry.path||metadata.sha256!==source.sha256||metadata.bytes!==source.bytes||metadata.encoding!==entry.encoding||metadata.scene_validated!==false||diagnostics?.kind!=='diagnostics'||diagnostics.stage!=='copper'||diagnostics.scene_validated!==false||diagnostics.localized_messages.length!==diagnostics.diagnostics.length)throw new Error('REPORT_METADATA_INVALID');
    const db=await new AllegroParser(bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength),entry.encoding).parse();
    if(db.count!==index.index.records||db.strings.size!==index.index.strings||db.header.version!==metadata.version)throw new Error('WEB_INDEX_COUNTS_DIFFER');
    const expectedOffsets:number[]=[],counts:any={};
    for(const kind of [14,20,36,40]){
        const offsets=Array.from(db.byType.get(kind)??[]) as number[];
        const count=Object.entries(index.index.by_type).find(([tag])=>parseInt(tag,16)===kind)?.[1]??0;
        if(offsets.length!==count)throw new Error('SOURCE_TYPE_COUNT_DIFFERS');if(count)counts[kind]=count;
        const n=Math.min(32,offsets.length);for(let i=0;i<n;i++)expectedOffsets.push(offsets[n>1?Math.floor(i*(offsets.length-1)/(n-1)):0]);
    }
    equal(counts,entry.source_counts,'source_counts',true);
    if(JSON.stringify(request.spans.map((span:any)=>span.offset))!==JSON.stringify(expectedOffsets.sort((a,b)=>a-b)))throw new Error('DETERMINISTIC_SELECTION_DIFFERS');
    const fd=openSync(index.index_data_path,'r'),header=Buffer.alloc(16),scratch=Buffer.alloc(16);
    try{
        if(readSync(fd,header,0,16,0)!==16||header.subarray(0,8).toString()!=='PMIDX001'||header.readUInt32LE(8)!==db.count||header.readUInt32LE(12)!==db.strings.size)throw new Error('INDEX_SIGNATURE_DIFFERS');
        for(const span of request.spans){let low=0,high=db.count,found=false;while(low<high){const mid=low+Math.floor((high-low)/2);if(readSync(fd,scratch,0,16,16+mid*16)!==16)throw new Error('INDEX_TRUNCATED');const offset=scratch.readUInt32LE(0);if(offset<span.offset)low=mid+1;else if(offset>span.offset)high=mid;else{if(scratch.readUInt32LE(4)!==span.byte_length||scratch.readUInt32LE(8)!==span.key||scratch.readUInt32LE(12)!==span.record_type)throw new Error('SPAN_IDENTITY_DIFFERS');found=true;break;}}if(!found||db.offsets.get(span.key)!==span.offset)throw new Error('SPAN_NOT_INDEXED');}
    }finally{closeSync(fd);}
    const scene=await new AllegroSceneBuilder(db).build();
    const scale=AllegroUnits.toMillimeters(db.header.units,db.header.divisor),geometry=new AllegroGeometryDecoder(db,scale);
    if(metadata.scale!==scale)throw new Error('SCALE_DIFFERS');
    const zones=new Map(scene.zones.map((zone:any)=>[zone.id,zone]));
    const shapeIds=new Set(request.spans.filter((span:any)=>span.record_type===40).map((span:any)=>span.key));
    const strokes=new Map<number,any[]>();for(const segment of scene.segments)if(shapeIds.has(segment.trackId)){if(!strokes.has(segment.trackId))strokes.set(segment.trackId,[]);strokes.get(segment.trackId)!.push(segment);}
    let fields=0,zoneCount=0,vertices=0,indices=0,holes=0,curved=0,hatchEdges=0,outlineEdges=0;
    for(let i=0;i<rows.length;i++){
        const row=rows[i],span=request.spans[i];
        if(row.kind!=='record'||row.stage!=='copper'||row.path!==entry.path||row.error!==null||row.encoding!==entry.encoding||row.scene_validated!==false)throw new Error('ROW_METADATA_INVALID');
        for(const key of ['key','offset','byte_length','record_type'])if(row[key]!==span[key])throw new Error('ROW_IDENTITY_DIFFERS');
        const record=db.get(span.key);if(!record||record.type!==span.record_type)throw new Error('WEB_RECORD_IDENTITY_DIFFERS');
        const outline=(record.Layer&255)===1&&[0xea,0xfd].includes(record.Layer>>>8)?geometry.readPath(span.record_type===20?record.SegmentPtr:record.FirstSegmentPtr):[];
        if(span.record_type===40){fields+=equal({zone:webZone(zones.get(span.key)),segments:strokes.get(span.key)??[],outline},{zone:nativeZone(row.copper.zone),segments:row.copper.segments.map(edge),outline:row.copper.outline.map(edge)},`shape=${span.key}`);hatchEdges+=(strokes.get(span.key)??[]).length;outlineEdges+=outline.length;}
        else if(span.record_type===20){fields+=equal({outline},{outline:row.copper.outline.map(edge)},`graphic=${span.key}`);outlineEdges+=outline.length;}
        else fields+=equal({zone:webZone(zones.get(span.key))},{zone:nativeZone(row.copper.zone)},`rectangle=${span.key}`);
        const zone:any=zones.get(span.key);if(zone){zoneCount++;vertices+=zone.points.length/2;indices+=zone.indices.length;holes+=zone.ringOffsets.length-2;curved+=+!!zone.curved;}
    }
    const expectedWarnings=(scene.diagnostics??[]).flatMap((message:string)=>{const match=/^铜皮 (\d+) 无有效边界$/.exec(message);return match&&shapeIds.has(+match[1])?[{code:'BRD_COPPER_BOUNDARY_EMPTY',object:+match[1],offset:db.offsets.get(+match[1]),severity:'Warning',message:{key:'import.allegro.copper_boundary_empty',args:{key:+match[1]}}}]:[];});
    fields+=equal(expectedWarnings,diagnostics.diagnostics.map((d:any)=>({code:d.code,object:d.object,offset:d.offset,severity:d.severity,message:d.message})),'diagnostics');
    return {path:entry.path,matched:true,version:db.header.version,records:rows.length,zones:zoneCount,vertices,indices,holes,curved,hatch_edges:hatchEdges,outline_edges:outlineEdges,scalar_fields:fields,scene_validated:false};
}
writeFileSync(output,'');
let matched=0;const totals:any={records:0,zones:0,vertices:0,indices:0,holes:0,curved:0,hatch_edges:0,outline_edges:0,scalar_fields:0};
for(const entry of entries){let result:any;try{result=await compare(entry);matched++;for(const key of Object.keys(totals))totals[key]+=result[key];}catch(error){result={path:entry.path,matched:false,error:String(error),scene_validated:false};}appendFileSync(output,JSON.stringify(result)+'\n');process.stdout.write(JSON.stringify(result)+'\n');}
const summary={stage:'copper',compared:entries.length,matched,failed:entries.length-matched,...totals,earcut:triangulator,oracle:'complete frozen Web SceneBuilder; 32 deterministic source-order queries per copper/rectangle/graphic type on six representative boards',scene_validated:false};
writeFileSync(output+'.summary.json',JSON.stringify(summary,null,2)+'\n');process.stdout.write(JSON.stringify(summary)+'\n');if(matched!==entries.length)process.exitCode=1;
