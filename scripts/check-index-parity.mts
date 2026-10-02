// Development-only oracle. Compares every record field and decoded table string;
// hashes bind evidence to frozen inputs, but do not substitute for field comparisons.
import { readFileSync, writeFileSync, appendFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { resolve, basename } from 'node:path';
import { pathToFileURL } from 'node:url';

const [webRoot, rustReport, sourceManifest, caseManifest, outputReport] = process.argv.slice(2);
if (!outputReport) throw new Error('Usage: check-index-parity.mts WEB_ROOT RUST_REPORT SOURCE_MANIFEST CASE_MANIFEST OUTPUT_REPORT');
const json = (path: string) => JSON.parse(readFileSync(path, 'utf8').replace(/^\uFEFF/, ''));
const lines = (path: string) => readFileSync(path, 'utf8').replace(/^\uFEFF/, '').trim().split(/\r?\n/).map(line => JSON.parse(line));
const sha = (bytes: Buffer) => createHash('sha256').update(bytes).digest('hex');
let frozenFiles = 0;
for (const file of json(sourceManifest).files) {
  if (sha(readFileSync(resolve(webRoot, file.path))) !== file.sha256) throw new Error(`Frozen Web input changed: ${file.path}`);
  frozenFiles++;
}
const cases = new Map(lines(caseManifest).map(entry => [basename(entry.path).toLowerCase(), entry]));
const reports = lines(rustReport);
if (reports.length !== cases.size || new Set(reports.map(e => basename(e.path).toLowerCase())).size !== cases.size) throw new Error('Case set differs from the frozen manifest');
const load = (path: string) => import(pathToFileURL(resolve(webRoot, path)).href);
const { AllegroHeaderReader } = await load('src/lib/allegro/binary/header.ts');
const { AllegroStringTableReader } = await load('src/lib/allegro/binary/string-table.ts');
const { BrdTextDecoder } = await load('src/lib/allegro/binary/text-decoder.ts');
const { Reader } = await load('src/lib/allegro/binary/reader.ts');
const { AllegroRecordReader } = await load('src/lib/allegro/binary/record-reader.ts');
const { OffsetIndex } = await load('src/lib/allegro/binary/offset-index.ts');
writeFileSync(outputReport, '');
let compared = 0, matched = 0, recordsCompared = 0, stringsCompared = 0;

async function compare(entry: any) {
  const bytes = readFileSync(entry.path);
  const frozen: any = cases.get(basename(entry.path).toLowerCase());
  if (!frozen || sha(bytes) !== frozen.sha256 || bytes.length !== frozen.bytes || entry.sha256 !== frozen.sha256) throw new Error('Case hash or length differs from frozen input');
  if (entry.stage !== 'index') throw new Error('Rust report is not an index report');
  const buffer = bytes.byteOffset === 0 && bytes.byteLength === bytes.buffer.byteLength ? bytes.buffer : bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
  const header = new AllegroHeaderReader(buffer).read();
  const decoder = new BrdTextDecoder(entry.encoding);
  const table = await new AllegroStringTableReader(buffer, header, decoder).read();
  const reader = new Reader(buffer, decoder), scanner = new AllegroRecordReader(reader, header), keys = new OffsetIndex();
  reader.seek(table.objectOffset);
  const data = entry.index_data_path ? readFileSync(entry.index_data_path) : null;
  if (data && data.subarray(0, 8).toString() !== 'PMIDX001') throw new Error('Unknown Rust index evidence version');
  const read32 = (offset: number) => {
    if (!data || offset + 4 > data.length) throw new Error(`Truncated Rust evidence at ${offset}`);
    return data.readUInt32LE(offset);
  };
  let count = 0, end = 0;
  const byType = new Map<string, number>();
  while (reader.offset < buffer.byteLength) {
    const start = reader.offset;
    if (start % 4) throw new Error(`Web unaligned record at 0x${start.toString(16)}`);
    const type = reader.recordType(header.version);
    if (!type) {
      if (header.version >= 180) {
        let next = reader.offset;
        while (next < bytes.length && bytes[next] === 0) next++;
        if (next < bytes.length && next % 4 === 0 && bytes[next] <= 0x3e) { reader.seek(next); continue; }
      }
      end = start; break;
    }
    const key = scanner.scanKey(type) ?? 0;
    if (key && !keys.add(key, start)) throw new Error(`Web duplicate key ${key} at 0x${start.toString(16)}`);
    if (data) {
      const expected = [start, reader.offset - start, key, type];
      const fields = ['offset', 'length', 'key', 'type'];
      for (let i = 0; i < 4; i++) {
        const actual = read32(16 + count * 16 + i * 4);
        if (actual !== expected[i]) throw new Error(`Record ${count}, type 0x${type.toString(16)}, offset 0x${start.toString(16)}: ${fields[i]} differs (Web=${expected[i]}, Rust=${actual})`);
      }
    }
    const tag = `0x${type.toString(16).padStart(2, '0')}`;
    byType.set(tag, (byType.get(tag) ?? 0) + 1);
    count++;
  }
  end ||= reader.offset;
  if (entry.status !== 'passed' || !entry.index || !data) throw new Error(`Web scan succeeded but Rust failed: ${JSON.stringify(entry.error)}`);
  for (const [field, expected] of Object.entries({ records: count, strings: table.strings.size, keyed_records: keys.size, object_offset: table.objectOffset, end_offset: end })) {
    if (entry.index[field] !== expected) throw new Error(`Summary ${field} differs: Web=${expected}, Rust=${entry.index[field]}`);
  }
  if (read32(8) !== count || read32(12) !== table.strings.size) throw new Error('Rust evidence counts differ');
  const types = Object.fromEntries([...byType].sort());
  if (JSON.stringify(types) !== JSON.stringify(entry.index.by_type)) throw new Error('Type counts differ');
  let cursor = 16 + count * 16, previous = -1;
  for (let i = 0; i < table.strings.size; i++) {
    const key = read32(cursor), length = read32(cursor + 4); cursor += 8;
    if (key <= previous || cursor + length > data.length) throw new Error('Invalid Rust string evidence order or length');
    const value = data.subarray(cursor, cursor + length).toString('utf8'); cursor += length;
    if (table.strings.get(key) !== value) throw new Error(`String ${key} differs`);
    previous = key;
  }
  if (cursor !== data.length) throw new Error('Unexpected trailing Rust evidence');
  recordsCompared += count; stringsCompared += table.strings.size;
  return { path: entry.path, matched: true, records: count, strings: table.strings.size, objectOffset: table.objectOffset, endOffset: end, textDecodingIssues: decoder.issues.size, encoding: entry.encoding, sceneValidated: false };
}

for (const entry of reports) {
  let result;
  try { result = await compare(entry); matched++; }
  catch (error) { result = { path: entry.path, matched: false, error: error instanceof Error ? error.message : String(error), rustError: entry.error, sceneValidated: false }; }
  appendFileSync(outputReport, JSON.stringify(result) + '\n');
  compared++;
  if (compared % 20 === 0) console.error(JSON.stringify({ compared, matched }));
}
console.log(JSON.stringify({ stage: 'index', frozenFiles, compared, matched, recordsCompared, stringsCompared, report: outputReport, sceneValidated: false }));
if (matched !== compared) process.exitCode = 1;
