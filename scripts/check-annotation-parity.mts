// Full placed text and stored Dimension output from the frozen Web builders, without sampling.
import {readFileSync, writeFileSync, appendFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {basename, resolve} from 'node:path';
import {pathToFileURL} from 'node:url';

const [webRoot, probeManifest, sourceManifest, caseManifest, indexManifest, output] = process.argv.slice(2);
if(!output) throw new Error('ANNOTATION_PARITY_USAGE: WEB_ROOT PROBE_MANIFEST SOURCE_MANIFEST CASE_MANIFEST INDEX_MANIFEST OUTPUT');
const json=(p:string)=>JSON.parse(readFileSync(p,'utf8').replace(/^\uFEFF/,''));
const lines=(p:string)=>readFileSync(p,'utf8').replace(/^\uFEFF/,'').trim().split(/\r?\n/).map(s=>JSON.parse(s));
const sha=(b:Buffer)=>createHash('sha256').update(b).digest('hex');
const files=json(sourceManifest).files;
for(const required of ['src/lib/allegro/parser.ts','src/lib/allegro/decoders/text.ts','src/lib/allegro/decoders/text-record.ts','src/lib/allegro/decoders/drawing.ts','src/lib/allegro/decoders/layers.ts'])
    if(!files.some((f:any)=>f.path===required))throw new Error(`SOURCE_NOT_FROZEN: ${required}`);
for(const f of files)if(sha(readFileSync(resolve(webRoot,f.path)))!==f.sha256)throw new Error(`FROZEN_WEB_CHANGED: ${f.path}`);
const cases=new Map(lines(caseManifest).map(r=>[basename(r.path).toLowerCase(),r]));
const indexes=new Map(lines(indexManifest).map(r=>[basename(r.path).toLowerCase(),r]));
const entries=lines(probeManifest);
const required=['15061-1b.brd','agilex_i_series.brd','ml623_brd_revd_rdf0074.brd','s5000c-64_ddr5_bga_v0.61.brd','ss8633a_ampb_fpc_doe_v2_hvt_a_0423_1716.brd','ntpcb_320mb.brd'].sort();
if(JSON.stringify(entries.map(e=>basename(e.path).toLowerCase()).sort())!==JSON.stringify(required))throw new Error('SIX_REPRESENTATIVE_CASES_DIFFER');
const load=(p:string)=>import(pathToFileURL(resolve(webRoot,p)).href);
const [{AllegroParser},{AllegroTextBuilder},{AllegroDrawingBuilder,DIMENSION_LAYER},{AllegroLayerDecoder},{AllegroUnits}]=await Promise.all([
    load('src/lib/allegro/parser.ts'),load('src/lib/allegro/decoders/text.ts'),load('src/lib/allegro/decoders/drawing.ts'),load('src/lib/allegro/decoders/layers.ts'),load('src/lib/allegro/units.ts'),
]);
const point=(p:any)=>[p.x,p.y];
const edge=(s:any)=>({id:s.id,trackId:s.track_id,layer:s.layer,net:s.net,a:point(s.a),b:point(s.b),width:s.width,...(s.arc?{arc:{center:point(s.arc.center),radius:s.arc.radius,start:s.arc.start,sweep:s.arc.sweep}}:{})});
const nativeText=(t:any)=>({id:t.id,ownerId:t.owner_id??null,layer:t.layer,classId:t.class_id,subclass:t.subclass,text:t.text,at:point(t.at),angle:t.angle,mirrored:t.mirrored,align:t.align,fontIndex:t.font_index,width:t.width,height:t.height,spacing:t.spacing,lineSpacing:t.line_spacing,strokeWidth:t.stroke_width});
const webText=(t:any)=>({...t,ownerId:t.ownerId??null});
function equal(e:any,a:any,path:string):number{
    if(typeof e!==typeof a||Array.isArray(e)!==Array.isArray(a))throw new Error(`TYPE_DIFFERS: ${path}`);
    if(e!==null&&typeof e==='object'){
        const keys=Object.keys(e).sort();
        if(JSON.stringify(keys)!==JSON.stringify(Object.keys(a??{}).sort()))throw new Error(`KEYS_DIFFER: ${path}`);
        return keys.reduce((n,k)=>n+equal(e[k],a[k],`${path}.${k}`),0);
    }
    if(typeof e==='number'){
        const exact=/\.(id|ownerId|layer|classId|subclass|fontIndex|trackId|net|key|record_type|font|graphic)$/.test(path)||/\.(graphicIds|textIds)\.\d+$/.test(path);
        const tolerance=exact?0:1e-12*Math.max(1,Math.abs(e),Math.abs(a));
        if(!Number.isFinite(e)||!Number.isFinite(a)||Math.abs(e-a)>tolerance)throw new Error(`VALUE_DIFFERS: ${path} Web=${e} Rust=${a}`);
    }else if(e!==a)throw new Error(`VALUE_DIFFERS: ${path} Web=${e} Rust=${a}`);
    return 1;
}
function warning(message:string):any{
    const rules:[RegExp,string,string,string[]][]=[
        [/^存在多组字体定义，使用第一组；需要核验字体索引$/,'BRD_TEXT_FONT_TABLES_MULTIPLE','text_font_tables_multiple',[]],
        [/^字体定义表字段无效$/,'BRD_TEXT_FONT_TABLE_INVALID','text_font_table_invalid',[]],
        [/^文字链缺失引用 (\d+)$/,'BRD_TEXT_LINK_MISSING','text_link_missing',['key']],
        [/^文字链遇到非文字记录 (\d+) \/ (\d+)$/,'BRD_TEXT_LINK_TYPE','text_link_type',['key','record_type']],
        [/^文字 (\d+) 缺少内容记录$/,'BRD_TEXT_CONTENT_MISSING','text_content_missing',['key']],
        [/^文字 (\d+) 的字体 (\d+) 无有效尺寸$/,'BRD_TEXT_FONT_INVALID','text_font_invalid',['key','font']],
        [/^尺寸图形 (\d+) 缺失或无效路径引用 (\d+)$/,'BRD_DRAWING_PATH_MISSING','drawing_path_missing',['graphic','key']],
        [/^尺寸图形 (\d+) 的路径 (\d+) 坐标或线宽无效$/,'BRD_DRAWING_GEOMETRY_INVALID','drawing_geometry_invalid',['graphic','key']],
        [/^尺寸图形所属链缺失或无效引用 (\d+)$/,'BRD_DRAWING_OWNER_LINK_MISSING','drawing_owner_link_missing',['key']],
        [/^尺寸图形 (\d+) 所属对象与链表不一致$/,'BRD_DRAWING_OWNER_MISMATCH','drawing_owner_mismatch',['key']],
        [/^尺寸图形 (\d+) 未关联到有效板级或已放置实例链$/,'BRD_DRAWING_ORPHAN','drawing_orphan',['key']],
    ];
    for(const [pattern,code,key,names] of rules){const match=pattern.exec(message);if(match){const args=Object.fromEntries(names.map((n,i)=>[n,+match[i+1]]));return{code,severity:'Warning',object:args.graphic??args.key??0,message:{key:`import.allegro.${key}`,args}};}}
    throw new Error(`UNMAPPED_WEB_WARNING: ${message}`);
}
async function compare(entry:any){
    const identity=basename(entry.path).toLowerCase(),source=cases.get(identity),index=indexes.get(identity),bytes=readFileSync(entry.path),report=json(entry.report);
    if(!source||bytes.length!==source.bytes||sha(bytes)!==source.sha256||!index||index.status!=='passed'||index.sha256!==source.sha256)throw new Error('FROZEN_SOURCE_OR_INDEX_DIFFERS');
    if(report.schema_version!==1||report.stage!=='annotations'||report.path!==entry.path||report.encoding!==index.encoding||report.locale!=='zh-CN'||report.sha256!==source.sha256||report.bytes!==source.bytes||report.error!==null||report.scene_validated!==false)throw new Error('NATIVE_METADATA_DIFFERS');
    const db=await new AllegroParser(bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength),report.encoding).parse();
    if(db.count!==report.source_records||db.strings.size!==report.source_strings||db.header.version!==report.version||db.count!==index.index.records||db.strings.size!==index.index.strings)throw new Error('DATABASE_COUNTS_DIFFER');
    const diagnostics:string[]=[],scale=AllegroUnits.toMillimeters(db.header.units,db.header.divisor);
    const texts=await new AllegroTextBuilder(db,scale).build(diagnostics);
    const candidates=[...db.records(0x14)].filter(g=>g.Layer===0xf901);
    const drawings=await new AllegroDrawingBuilder(db,scale).build(candidates,texts.texts,diagnostics);
    if(drawings.length&&!texts.drawingLayers.some(l=>l.id===DIMENSION_LAYER)){
        texts.drawingLayers.push(AllegroLayerDecoder.drawingLayer(0xf901));
        texts.drawingLayers.sort((a,b)=>Number(b.defaultVisible)-Number(a.defaultVisible)||a.id-b.id);
    }
    const native=report.annotations;
    let fields=equal(texts.texts.map(webText),native.texts.map(nativeText),'texts');
    fields+=equal(texts.drawingLayers,native.drawing_layers.map((l:any,i:number)=>({id:l.id,name:report.localized_layers[i],color:l.color,defaultVisible:l.default_visible})),'layers');
    fields+=equal(drawings.map(d=>({id:d.id,ownerId:d.ownerId??null,layer:d.layer,net:d.net,graphicIds:d.graphicIds,segments:d.segments,textIds:d.texts.map(t=>t.id)})),native.drawings.map((d:any)=>({id:d.id,ownerId:d.owner_id??null,layer:d.layer,net:d.net,graphicIds:d.graphic_ids,segments:d.segments.map(edge),textIds:d.text_ids})),'drawings');
    fields+=equal(diagnostics.map(warning),native.diagnostics.map((d:any)=>({code:d.code,severity:d.severity,object:d.object,message:d.message})),'diagnostics');
    if(native.diagnostics.length!==report.localized_diagnostics.length)throw new Error('LOCALIZED_DIAGNOSTIC_COUNT_DIFFERS');
    for(const d of native.diagnostics){if(!Number.isInteger(d.offset)||d.offset<0||d.offset>=bytes.length||d.path!==null||d.technical_details!==null)throw new Error('DIAGNOSTIC_SOURCE_LOCATION_INVALID');}
    return{path:entry.path,matched:true,version:db.header.version,texts:texts.texts.length,layers:texts.drawingLayers.length,drawings:drawings.length,segments:drawings.reduce((n,d)=>n+d.segments.length,0),grouped_texts:drawings.reduce((n,d)=>n+d.texts.length,0),warnings:diagnostics.length,scalar_fields:fields,scene_validated:false};
}
writeFileSync(output,'');let matched=0;const totals:any={texts:0,layers:0,drawings:0,segments:0,grouped_texts:0,warnings:0,scalar_fields:0};
for(const entry of entries){let result:any;try{result=await compare(entry);matched++;for(const key of Object.keys(totals))totals[key]+=result[key];}catch(error){result={path:entry.path,matched:false,error:String(error),scene_validated:false};}appendFileSync(output,JSON.stringify(result)+'\n');process.stdout.write(JSON.stringify(result)+'\n');}
const summary={stage:'annotations',compared:entries.length,matched,failed:entries.length-matched,...totals,oracle:'complete frozen Web TextBuilder and DrawingBuilder; no object sampling',scene_validated:false};
writeFileSync(output+'.summary.json',JSON.stringify(summary,null,2)+'\n');process.stdout.write(JSON.stringify(summary)+'\n');if(matched!==entries.length)process.exitCode=1;
