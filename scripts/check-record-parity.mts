// Development-only field oracle. Native application has no TypeScript dependency.
import { readFileSync, writeFileSync, appendFileSync, openSync, readSync, closeSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { resolve, basename } from 'node:path';
import { pathToFileURL } from 'node:url';

const [webRoot, probeManifest, sourceManifest, caseManifest, indexManifest, outputReport] = process.argv.slice(2);
if (!outputReport) throw new Error('RECORD_PARITY_USAGE: WEB_ROOT PROBE_MANIFEST SOURCE_MANIFEST CASE_MANIFEST INDEX_MANIFEST OUTPUT_REPORT');
const json = (path: string) => JSON.parse(readFileSync(path, 'utf8').replace(/^\uFEFF/, ''));
const lines = (path: string) => readFileSync(path, 'utf8').replace(/^\uFEFF/, '').trim().split(/\r?\n/).map(line => JSON.parse(line));
const sha = (bytes: Buffer) => createHash('sha256').update(bytes).digest('hex');
for (const file of json(sourceManifest).files) {
  if (sha(readFileSync(resolve(webRoot, file.path))) !== file.sha256) throw new Error(`FROZEN_WEB_CHANGED: ${file.path}`);
}
const frozenCases = new Map(lines(caseManifest).map(entry => [basename(entry.path).toLowerCase(), entry]));
const entries = lines(probeManifest);
const indexedCases = new Map(lines(indexManifest).map(entry => [basename(entry.path).toLowerCase(), entry]));
const fixedTypes = new Set([1, 4, 5, 6, 7, 8, 9, 10, 12, 13, 14, 15, 16, 17, 18, 20, 21, 22,
  23, 27, 32, 34, 35, 36, 38, 40, 41, 43, 44, 45, 46, 47, 48, 50, 51, 52, 53, 55, 56, 57, 58, 62]);
if (entries.length !== frozenCases.size || new Set(entries.map(e => basename(e.path).toLowerCase())).size !== frozenCases.size) throw new Error('FROZEN_CASE_SET_DIFFERS');
const load = (path: string) => import(pathToFileURL(resolve(webRoot, path)).href);
const { AllegroHeaderReader } = await load('src/lib/allegro/binary/header.ts');
const { BrdTextDecoder } = await load('src/lib/allegro/binary/text-decoder.ts');
const { Reader } = await load('src/lib/allegro/binary/reader.ts');
const { AllegroRecordReader } = await load('src/lib/allegro/binary/record-reader.ts');

let cases = 0, matched = 0, records = 0, scalarFields = 0;
const totals = new Map<number, number>();
function equal(expected: any, actual: any, field: string): number {
  if (typeof expected !== typeof actual || Array.isArray(expected) !== Array.isArray(actual)) throw new Error(`FIELD_TYPE_DIFFERS: ${field}`);
  if (expected !== null && typeof expected === 'object') {
    const keys = Object.keys(expected).sort(), other = Object.keys(actual ?? {}).sort();
    if (JSON.stringify(keys) !== JSON.stringify(other)) throw new Error(`FIELD_KEYS_DIFFER: ${field} Web=${keys} Rust=${other}`);
    let scalars = 0;
    for (const key of keys) scalars += equal(expected[key], actual[key], `${field}.${key}`);
    return scalars;
  } else {
    if (expected !== actual) throw new Error(`FIELD_VALUE_DIFFERS: ${field} Web=${JSON.stringify(expected)} Rust=${JSON.stringify(actual)}`);
    return 1;
  }
}
function compare(entry: any) {
  if (entry.status !== 'passed') throw new Error('RUST_DECODE_FAILED');
  const bytes = readFileSync(entry.path), frozen: any = frozenCases.get(basename(entry.path).toLowerCase());
  if (!frozen || sha(bytes) !== frozen.sha256 || entry.sha256 !== frozen.sha256 || bytes.length !== frozen.bytes) throw new Error('FROZEN_CASE_HASH_DIFFERS');
  const request = json(entry.request), rows = lines(entry.report);
  const stage = entry.stage ?? 'fixed-records';
  const indexed: any = indexedCases.get(basename(entry.path).toLowerCase());
  if (!indexed || indexed.stage !== 'index' || indexed.status !== 'passed' || indexed.sha256 !== entry.sha256 || indexed.encoding !== entry.encoding) throw new Error('VALIDATED_INDEX_DIFFERS');
  if (!['records', 'fixed-records'].includes(stage) || !Number.isInteger(entry.samples_per_type) || entry.samples_per_type < 1 || entry.samples_per_type > 64) throw new Error('PROBE_MODE_OR_SAMPLE_LIMIT_DIFFERS');
  const sourceCounts = Object.fromEntries(Object.entries(indexed.index.by_type)
    .map(([kind, count]) => [Number.parseInt(kind, 16), count])
    .filter(([kind]) => stage === 'records' || fixedTypes.has(kind as number)));
  if (JSON.stringify(sourceCounts) !== JSON.stringify(entry.source_counts)) throw new Error('SOURCE_TYPE_COVERAGE_DIFFERS');
  if (request.sha256 !== entry.sha256 || request.source_size !== entry.bytes || rows.length !== request.spans.length || rows.length !== entry.records) throw new Error('REQUEST_COUNTS_OR_IDENTITY_DIFFERS');
  const buffer = bytes.byteOffset === 0 && bytes.byteLength === bytes.buffer.byteLength ? bytes.buffer : bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
  const header = new AllegroHeaderReader(buffer).read(), reader = new Reader(buffer, new BrdTextDecoder(entry.encoding));
  const decoder = new AllegroRecordReader(reader, header), byType = new Map<number, number>();
  let scalars = 0;
  const seen = new Set<number>();
  const descriptor = openSync(indexed.index_data_path, 'r'), indexHeader = Buffer.alloc(16), scratch = Buffer.alloc(16);
  try {
  if (readSync(descriptor, indexHeader, 0, 16, 0) !== 16 || indexHeader.subarray(0, 8).toString() !== 'PMIDX001'
      || indexHeader.readUInt32LE(8) !== indexed.index.records || indexHeader.readUInt32LE(12) !== indexed.index.strings) throw new Error('INDEX_SIGNATURE_OR_COUNTS_DIFFERS');
  for (let i = 0; i < rows.length; i++) {
    const row = rows[i], span = request.spans[i];
    if (seen.has(span.offset)) throw new Error(`DUPLICATE_PROBE_OFFSET: ${span.offset}`);
    seen.add(span.offset);
    let low = 0, high = indexed.index.records, found = false;
    while (low < high) {
      const middle = low + Math.floor((high - low) / 2);
      if (readSync(descriptor, scratch, 0, 16, 16 + middle * 16) !== 16) throw new Error('INDEX_TRUNCATED');
      const offset = scratch.readUInt32LE(0);
      if (offset === span.offset) {
        if (scratch.readUInt32LE(4) !== span.byte_length || scratch.readUInt32LE(8) !== span.key || scratch.readUInt32LE(12) !== span.record_type) throw new Error(`INDEX_SPAN_DIFFERS: ${i}`);
        found = true; break;
      }
      if (offset < span.offset) low = middle + 1; else high = middle;
    }
    if (!found) throw new Error(`PROBE_NOT_IN_INDEX: ${span.offset}`);
    for (const key of ['offset', 'byte_length', 'key', 'record_type']) if (row[key] !== span[key]) throw new Error(`REQUEST_SPAN_DIFFERS: ${i}.${key}`);
    if (row.error || !row.fields || row.encoding !== entry.encoding || row.version !== header.version || row.stage !== (entry.stage ?? 'fixed-records')) throw new Error(`RUST_RECORD_INVALID: ${JSON.stringify(row.error)}`);
    reader.seek(span.offset);
    const kind = reader.recordType(header.version);
    if (kind !== span.record_type) throw new Error('SOURCE_RECORD_TYPE_DIFFERS');
    const expected = decoder.read(kind);
    if (reader.offset !== span.offset + span.byte_length || (expected.Key ?? 0) !== span.key) throw new Error('SOURCE_RECORD_BOUNDARY_OR_KEY_DIFFERS');
    // Normalize Web typed bytes to native byte arrays without dropping NULs or changing values.
    // JSON canonicalizes absent optional fields and nonfinite floats on both sides.
    // Integer/ID/text fields and finite source doubles are compared exactly, not by hash.
    const canonical = JSON.parse(JSON.stringify(expected, (_key, value) => value instanceof Uint8Array ? Array.from(value) : value));
    scalars += equal(canonical, row.fields, `record=${i} type=0x${kind.toString(16)} offset=0x${span.offset.toString(16)}`);
    byType.set(kind, (byType.get(kind) ?? 0) + 1);
  }
  for (const [kind, count] of Object.entries(entry.source_counts)) {
    if (byType.get(Number(kind)) !== Math.min(entry.samples_per_type, count as number)) throw new Error(`SAMPLE_COVERAGE_DIFFERS: ${kind}`);
  }
  } finally { closeSync(descriptor); }
  for (const [kind, count] of byType) totals.set(kind, (totals.get(kind) ?? 0) + count);
  records += rows.length;
  scalarFields += scalars;
  return { path: entry.path, matched: true, version: header.version, records: rows.length, scalarFields: scalars, byType: Object.fromEntries(byType), encoding: entry.encoding, sceneValidated: false };
}
writeFileSync(outputReport, '');
for (const entry of entries) {
  let result;
  try { result = compare(entry); matched++; }
  catch (error) { result = { path: entry.path, matched: false, error: error instanceof Error ? error.message : String(error), sceneValidated: false }; }
  appendFileSync(outputReport, JSON.stringify(result) + '\n');
  cases++;
  if (cases % 20 === 0) console.error(JSON.stringify({ cases, matched, records }));
}
const summary = { stage: entries[0]?.stage ?? 'fixed-records', cases, matched, records, scalarFields, byType: Object.fromEntries(totals), sceneValidated: false, report: outputReport };
writeFileSync(`${outputReport}.summary.json`, JSON.stringify(summary, null, 2) + '\n');
console.log(JSON.stringify(summary));
if (matched !== cases) process.exitCode = 1;
