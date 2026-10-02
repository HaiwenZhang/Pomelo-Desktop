// Development-only semantic oracle. It builds the Web database independently from source bytes.
import { readFileSync, writeFileSync, appendFileSync, openSync, readSync, closeSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { resolve, basename } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createPadstackOracle } from './padstack-oracle.mts';

const [webRoot, probeManifest, sourceManifest, caseManifest, indexManifest, outputReport, stage = 'geometry'] = process.argv.slice(2);
if (!outputReport || !['geometry','padstack'].includes(stage)) throw new Error('GEOMETRY_PARITY_USAGE: WEB_ROOT PROBE_MANIFEST SOURCE_MANIFEST CASE_MANIFEST INDEX_MANIFEST OUTPUT_REPORT [geometry|padstack]');
const json = (path: string) => JSON.parse(readFileSync(path, 'utf8').replace(/^\uFEFF/, ''));
const lines = (path: string) => readFileSync(path, 'utf8').replace(/^\uFEFF/, '').trim().split(/\r?\n/).map(line => JSON.parse(line));
const sha = (bytes: Buffer) => createHash('sha256').update(bytes).digest('hex');
for (const file of json(sourceManifest).files) {
  if (sha(readFileSync(resolve(webRoot, file.path))) !== file.sha256) throw new Error(`FROZEN_WEB_CHANGED: ${file.path}`);
}
const frozen = new Map(lines(caseManifest).map(entry => [basename(entry.path).toLowerCase(), entry]));
const indexed = new Map(lines(indexManifest).map(entry => [basename(entry.path).toLowerCase(), entry]));
const entries = lines(probeManifest);
if (entries.length !== frozen.size || new Set(entries.map(e => basename(e.path).toLowerCase())).size !== frozen.size) throw new Error('FROZEN_CASE_SET_DIFFERS');
const load = (path: string) => import(pathToFileURL(resolve(webRoot, path)).href);
const { AllegroParser } = await load('src/lib/allegro/parser.ts');
const { AllegroGeometryDecoder } = await load('src/lib/allegro/decoders/geometry.ts');
const { AllegroLayerDecoder } = await load('src/lib/allegro/decoders/layers.ts');
const { AllegroUnits } = await load('src/lib/allegro/units.ts');
const { Reader } = await load('src/lib/allegro/binary/reader.ts');
const { AllegroRecordReader } = await load('src/lib/allegro/binary/record-reader.ts');
const { BrdTextDecoder } = await load('src/lib/allegro/binary/text-decoder.ts');
const geometryTypes = new Set(stage === 'padstack' ? [28,47,50,51] : [1, 5, 20, 21, 22, 23, 40]);
const colors = ['#58b5ed','#83ce94','#edb963','#ba8bec','#eb819d','#54c7bd','#a5b8df','#e18d61'];
const point = (p: any) => [p.x, p.y];
const edge = (s: any) => ({ id:s.id, trackId:s.track_id, layer:s.layer === 0xffffffff ? -1 : s.layer, net:s.net,
  a:point(s.a), b:point(s.b), width:s.width,
  ...(s.arc ? { arc:{center:point(s.arc.center),radius:s.arc.radius,start:s.arc.start,sweep:s.arc.sweep} } : {}) });
function equal(expected: any, actual: any, path: string): number {
  if (typeof expected !== typeof actual || Array.isArray(expected) !== Array.isArray(actual)) throw new Error(`GEOMETRY_TYPE_DIFFERS: ${path}`);
  if (expected !== null && typeof expected === 'object') {
    const keys = Object.keys(expected).sort(), other = Object.keys(actual ?? {}).sort();
    if (JSON.stringify(keys) !== JSON.stringify(other)) throw new Error(`GEOMETRY_KEYS_DIFFER: ${path}`);
    return keys.reduce((count, key) => count + equal(expected[key], actual[key], `${path}.${key}`), 0);
  }
  if (typeof expected === 'number' && typeof actual === 'number') {
    const exact = /\.(id|trackId|layer|net|sourceFlags|scale|version|type|stackKey|startLayer|stopLayer|protectedLayer|layerCount|padType|embeddedLayer|regionCode|object|offset|stack|pad_type|shape)$/.test(path);
    const tolerance = exact ? 0 : 1e-12 * Math.max(1, Math.abs(expected), Math.abs(actual));
    if (!Number.isFinite(expected) || !Number.isFinite(actual) || Math.abs(expected-actual) > tolerance) throw new Error(`GEOMETRY_VALUE_DIFFERS: ${path} Web=${expected} Rust=${actual}`);
  } else if (expected !== actual) throw new Error(`GEOMETRY_VALUE_DIFFERS: ${path} Web=${expected} Rust=${actual}`);
  return 1;
}
async function compare(entry: any) {
  const name = basename(entry.path).toLowerCase(), source: any = frozen.get(name), index: any = indexed.get(name);
  const bytes = readFileSync(entry.path);
  if (!source || sha(bytes) !== source.sha256 || entry.sha256 !== source.sha256 || bytes.length !== source.bytes || entry.bytes !== source.bytes) throw new Error('FROZEN_CASE_HASH_DIFFERS');
  if (!index || index.stage !== 'index' || index.status !== 'passed' || index.sha256 !== source.sha256 || index.encoding !== entry.encoding) throw new Error('VALIDATED_INDEX_DIFFERS');
  if (entry.stage !== stage || entry.status !== 'passed' || !Number.isInteger(entry.samples_per_type) || entry.samples_per_type < 1 || entry.samples_per_type > 64) throw new Error('PROBE_MODE_OR_STATUS_INVALID');
  const counts = Object.fromEntries(Object.entries(index.index.by_type).map(([kind,count]) => [parseInt(kind,16),count]).filter(([kind]) => geometryTypes.has(kind as number)));
  if (JSON.stringify(counts) !== JSON.stringify(entry.source_counts)) throw new Error('SOURCE_TYPE_COVERAGE_DIFFERS');
  const request = json(entry.request), rows = lines(entry.report), metadata = rows.shift();
  const diagnostics = stage === 'padstack' ? rows.pop() : undefined;
  if (request.schema_version !== 1 || request.sha256 !== source.sha256 || request.source_size !== source.bytes || rows.length !== request.spans.length || rows.length !== entry.records) throw new Error('REQUEST_COUNTS_OR_IDENTITY_DIFFERS');
  if (metadata?.kind !== 'metadata' || metadata.stage !== stage || metadata.scene_validated !== false || metadata.sha256 !== source.sha256 || metadata.bytes !== source.bytes || metadata.encoding !== entry.encoding) throw new Error('METADATA_INVALID');
  const buffer = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
  const db = await new AllegroParser(buffer, entry.encoding).parse();
  if (db.count !== index.index.records || db.strings.size !== index.index.strings) throw new Error('WEB_DATABASE_COUNTS_DIFFER');
  const scale = AllegroUnits.toMillimeters(db.header.units, db.header.divisor);
  const geometry = new AllegroGeometryDecoder(db, scale);
  const list = db.get(db.header.layerMap[6]?.recordId);
  if (list?.type !== 42) throw new Error('WEB_LAYER_LIST_MISSING');
  const layers = list.Entries.map((e: any,id: number) => ({id, name:'Name' in e ? e.Name : db.strings.get(e.NameId) ?? `Layer ${id+1}`,
    color:colors[id % colors.length], layerFunction:AllegroLayerDecoder.functionFromFlags(e.Properties), ...(e.Properties !== undefined ? {sourceFlags:e.Properties} : {})}));
  const nativeLayers = metadata.layers.map((layer: any,id: number) => ({id:layer.id,
    name:layer.name || (!('Name' in list.Entries[id]) && !db.strings.has(list.Entries[id].NameId) ? `Layer ${id+1}` : ''),
    color:layer.color, layerFunction:layer.function.toLowerCase(), ...(layer.source_flags !== null ? {sourceFlags:layer.source_flags} : {})}));
  let scalarFields = equal({scale,version:db.header.version,layers}, {scale:metadata.scale,version:metadata.version,layers:nativeLayers}, 'metadata');
  const padstack = stage === 'padstack' ? await createPadstackOracle(load,db,scale,geometry,layers.length) : undefined;
  const reader = new Reader(buffer, new BrdTextDecoder(entry.encoding)), recordReader = new AllegroRecordReader(reader, db.header);
  const descriptor = openSync(index.index_data_path,'r'), header = Buffer.alloc(16), scratch = Buffer.alloc(16);
  const seen = new Set<number>(), byType = new Map<number, number>();
  let paths = 0, rings = 0, points = 0, edges = 0, padValues = 0;
  try {
    if (readSync(descriptor,header,0,16,0)!==16 || header.subarray(0,8).toString()!=='PMIDX001' || header.readUInt32LE(8)!==db.count || header.readUInt32LE(12)!==db.strings.size) throw new Error('INDEX_SIGNATURE_OR_COUNTS_DIFFER');
    for (let i=0;i<rows.length;i++) {
      const row=rows[i], span=request.spans[i];
      if (seen.has(span.offset)) throw new Error(`DUPLICATE_PROBE_OFFSET: ${span.offset}`);
      seen.add(span.offset);
      let low=0, high=db.count, found=false;
      while(low<high) {
        const middle=low+Math.floor((high-low)/2);
        if(readSync(descriptor,scratch,0,16,16+middle*16)!==16) throw new Error('INDEX_TRUNCATED');
        const offset=scratch.readUInt32LE(0);
        if(offset===span.offset) {
          if(scratch.readUInt32LE(4)!==span.byte_length || scratch.readUInt32LE(8)!==span.key || scratch.readUInt32LE(12)!==span.record_type) throw new Error('INDEX_SPAN_DIFFERS');
          found=true;break;
        }
        if(offset<span.offset) low=middle+1;else high=middle;
      }
      if(!found) throw new Error('PROBE_NOT_IN_INDEX');
      for(const key of ['offset','byte_length','key','record_type']) if(row[key]!==span[key]) throw new Error(`REQUEST_SPAN_DIFFERS: ${i}.${key}`);
      if(row.kind!=='record' || row.stage!==stage || row.encoding!==entry.encoding || row.scene_validated!==false || row.error || !row[stage]) throw new Error(`RUST_GEOMETRY_INVALID: ${JSON.stringify(row.error)}`);
      reader.seek(span.offset);
      const kind=reader.recordType(db.header.version), record=recordReader.read(kind);
      if(kind!==span.record_type || reader.offset!==span.offset+span.byte_length || (record.Key ?? 0)!==span.key) throw new Error('SOURCE_RECORD_BOUNDARY_OR_IDENTITY_DIFFERS');
      let expected: any, actual: any;
      if(padstack) {
        expected=padstack.expected(kind,record);
        actual=padstack.actual(kind,row.padstack);
        for(const preview of [expected.definition,expected.pin?.preview,expected.via?.preview].filter(Boolean)) {
          padValues+=preview.regularPads.length;
          for(const pad of preview.regularPads) if(pad.custom) {
            paths+=pad.custom.paths.length;rings+=pad.custom.contours.length;
            points+=pad.custom.contours.reduce((count: number,ring: any[])=>count+ring.length,0);
            edges+=pad.custom.paths.reduce((count: number,path: any[])=>count+path.length,0);
          }
        }
      } else if([1,21,22,23].includes(kind)) {
        expected={edge:geometry.readPath(record.Key)[0],hatch_edge:geometry.readPath(record.Key,true)[0]};
        actual={edge:edge(row.geometry.edge),hatch_edge:edge(row.geometry.hatch_edge)};
      } else if(kind===5 || kind===20) {
        expected={path:geometry.readPath(kind===5 ? record.FirstSegPtr : record.SegmentPtr)};
        actual={path:row.geometry.path.map(edge)}; paths++; edges+=expected.path.length;
      } else if(kind===40) {
        expected={contours:await geometry.readContours({...record,type:kind})};
        actual={contours:{paths:row.geometry.contours.paths.map((p: any[])=>p.map(edge)),rings:row.geometry.contours.rings.map((r: any[])=>r.map(point))}};
        paths+=expected.contours.paths.length; rings+=expected.contours.rings.length;
        points+=expected.contours.rings.reduce((n: number,r: any[])=>n+r.length,0);
        edges+=expected.contours.paths.reduce((n: number,p: any[])=>n+p.length,0);
      } else throw new Error('UNSUPPORTED_GEOMETRY_PROBE_TYPE');
      scalarFields+=equal(expected,actual,`record=${i} type=0x${kind.toString(16)} key=${span.key}`);
      byType.set(kind,(byType.get(kind) ?? 0)+1);
    }
    for(const [kind,count] of Object.entries(counts)) {
      if(byType.get(Number(kind))!==Math.min(entry.samples_per_type,count as number)) throw new Error(`SAMPLE_COVERAGE_DIFFERS: ${kind}`);
      if(db.byType.get(Number(kind))?.length!==count) throw new Error(`WEB_TYPE_COUNT_DIFFERS: ${kind}`);
    }
  } finally { closeSync(descriptor); }
  if(padstack) scalarFields+=equal(padstack.expectedDiagnostics(),padstack.actualDiagnostics(diagnostics),'diagnostics');
  return {path:entry.path,matched:true,version:db.header.version,records:rows.length,scalarFields,paths,rings,points,edges,...(padstack ? {padValues} : {}),layers:layers.length,byType:Object.fromEntries(byType),encoding:entry.encoding,sceneValidated:false};
}
writeFileSync(outputReport,'');
let cases=0,matched=0,records=0,scalarFields=0,paths=0,rings=0,points=0,edges=0,layers=0,padValues=0;
for(const entry of entries) {
  let result;
  try {
    result=await compare(entry); matched++; records+=result.records;scalarFields+=result.scalarFields;
    paths+=result.paths;rings+=result.rings;points+=result.points;edges+=result.edges;layers+=result.layers;
    padValues+=result.padValues ?? 0;
  } catch(error) {result={path:entry.path,matched:false,error:error instanceof Error?error.message:String(error),sceneValidated:false};}
  appendFileSync(outputReport,JSON.stringify(result)+'\n');cases++;
  if(cases%10===0) console.error(JSON.stringify({cases,matched,records}));
}
const summary={stage,cases,matched,records,scalarFields,paths,rings,points,edges,...(stage==='padstack' ? {padValues} : {}),layers,floatTolerance:'1e-12 * max(1, abs(Web), abs(Rust)); IDs, enums, flags and scale exact',sceneValidated:false,report:outputReport};
writeFileSync(`${outputReport}.summary.json`,JSON.stringify(summary,null,2)+'\n');
console.log(JSON.stringify(summary));
if(matched!==cases) process.exitCode=1;
