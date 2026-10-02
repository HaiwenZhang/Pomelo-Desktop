// Development-only oracle for the complete network loops in the frozen Web scene-builder.ts.
// Both databases are constructed independently from original BRD bytes; native output is not input to the oracle.
import { readFileSync, writeFileSync, appendFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { basename, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const [webRoot, nativeManifest, sourceManifest, caseManifest, indexManifest, outputReport] = process.argv.slice(2);
if (!outputReport) throw new Error('CONNECTIVITY_PARITY_USAGE: WEB_ROOT NATIVE_MANIFEST SOURCE_MANIFEST CASE_MANIFEST INDEX_MANIFEST OUTPUT_REPORT');
const json = (path: string) => JSON.parse(readFileSync(path, 'utf8').replace(/^\uFEFF/, ''));
const lines = (path: string) => readFileSync(path, 'utf8').replace(/^\uFEFF/, '').trim().split(/\r?\n/).map(line => JSON.parse(line));
const sha = (data: Buffer) => createHash('sha256').update(data).digest('hex');
const sourceFiles = json(sourceManifest).files;
if (!sourceFiles.some((file: any) => file.path === 'src/lib/allegro/scene-builder.ts')) throw new Error('SCENE_BUILDER_NOT_FROZEN');
for (const file of sourceFiles) {
  if (sha(readFileSync(resolve(webRoot, file.path))) !== file.sha256) throw new Error(`FROZEN_WEB_CHANGED: ${file.path}`);
}
const cases = new Map(lines(caseManifest).map(row => [basename(row.path).toLowerCase(), row]));
const indexes = new Map(lines(indexManifest).map(row => [basename(row.path).toLowerCase(), row]));
const entries = lines(nativeManifest);
if (entries.length !== cases.size || new Set(entries.map(row => basename(row.path).toLowerCase())).size !== cases.size
    || entries.some(row => !cases.has(basename(row.path).toLowerCase()))) throw new Error('FROZEN_CASE_SET_DIFFERS');
const { AllegroParser } = await import(pathToFileURL(resolve(webRoot, 'src/lib/allegro/parser.ts')).href);

function equal(expected: any, actual: any, path: string): number {
  if (typeof expected !== typeof actual || expected === null || actual === null) {
    if (expected !== actual) throw new Error(`CONNECTIVITY_TYPE_DIFFERS: ${path}`);
    return 1;
  }
  if (typeof expected === 'object') {
    const keys = Object.keys(expected).sort(), other = Object.keys(actual).sort();
    if (JSON.stringify(keys) !== JSON.stringify(other)) throw new Error(`CONNECTIVITY_KEYS_DIFFER: ${path}`);
    return keys.reduce((count, key) => count + equal(expected[key], actual[key], `${path}.${key}`), 0);
  }
  if (expected !== actual) throw new Error(`CONNECTIVITY_VALUE_DIFFERS: ${path} Web=${expected} Rust=${actual}`);
  return 1;
}

async function compare(entry: any) {
  const name = basename(entry.path).toLowerCase(), source = cases.get(name), index = indexes.get(name);
  const bytes = readFileSync(entry.path), actual = json(entry.report);
  if (!source || sha(bytes) !== source.sha256 || entry.sha256 !== source.sha256 || bytes.length !== source.bytes || entry.bytes !== source.bytes)
    throw new Error('FROZEN_CASE_HASH_DIFFERS');
  if (!index || index.stage !== 'index' || index.status !== 'passed' || index.sha256 !== source.sha256 || index.encoding !== entry.encoding)
    throw new Error('VALIDATED_INDEX_DIFFERS');
  if (entry.stage !== 'connectivity' || entry.status !== 'passed' || actual.schema_version !== 1 || actual.stage !== 'connectivity'
      || actual.scene_validated !== false || actual.sha256 !== source.sha256 || actual.bytes !== source.bytes
      || actual.encoding !== entry.encoding || actual.path !== entry.path || actual.error !== null || actual.localized_message !== null)
    throw new Error('CONNECTIVITY_METADATA_INVALID');
  const buffer = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
  const db = await new AllegroParser(buffer, entry.encoding).parse();
  if (db.count !== actual.source_records || db.count !== index.index.records || db.strings.size !== actual.source_strings
      || db.strings.size !== index.index.strings || db.header.version !== actual.version) throw new Error('WEB_DATABASE_COUNTS_DIFFER');
  const nets = new Map<number, string>(), owners = new Map<number, number>(), ownerTypes: Record<string, number> = {};
  for (const net of db.records(0x1b)) nets.set(net.Key, db.strings.get(net.NetName) ?? '');
  let assignmentCount = 0, visits = 0, overwritten = 0;
  for (const assignment of db.records(4)) {
    assignmentCount++;
    let key = assignment.ConnItem;
    const seen = new Set<number>();
    while (key && key !== assignment.Key) {
      if (seen.has(key)) throw new Error(`WEB_NETWORK_CYCLE: ${key}`);
      seen.add(key);
      const item = db.get(key);
      if (!item) throw new Error(`WEB_NETWORK_MISSING: ${key}`);
      if (owners.has(key)) overwritten++;
      else ownerTypes[item.type] = (ownerTypes[item.type] ?? 0) + 1;
      owners.set(key, assignment.Net);
      visits++;
      key = item.Next;
    }
  }
  const expected = {nets: Object.fromEntries(nets), owners: Object.fromEntries(owners), assignment_count: assignmentCount,
    link_visits: visits, overwritten_owners: overwritten, owner_types: ownerTypes};
  const scalarFields = equal(expected, actual.network, 'network');
  return {path:entry.path, matched:true, nets:nets.size, owners:owners.size, link_visits:visits,
    assignment_count:assignmentCount, owner_types:ownerTypes, overwritten_owners:overwritten,
    scalar_fields:scalarFields, scene_validated:false};
}

writeFileSync(outputReport, '');
let failed = 0, nets = 0, owners = 0, visits = 0, fields = 0;
for (let i = 0; i < entries.length; i++) {
  try {
    const result = await compare(entries[i]);
    nets += result.nets; owners += result.owners; visits += result.link_visits; fields += result.scalar_fields;
    appendFileSync(outputReport, JSON.stringify(result) + '\n');
  } catch (error) {
    failed++;
    appendFileSync(outputReport, JSON.stringify({path:entries[i].path, matched:false, error:String(error)}) + '\n');
  }
  if ((i + 1) % 20 === 0) process.stdout.write(JSON.stringify({cases:i + 1, failed, nets, owners}) + '\n');
}
const summary = {stage:'connectivity', compared:entries.length, matched:entries.length - failed, failed, nets, owners,
  link_visits:visits, scalar_fields:fields, scene_validated:false};
writeFileSync(outputReport + '.summary.json', JSON.stringify(summary, null, 2) + '\n');
process.stdout.write(JSON.stringify(summary) + '\n');
if (failed) process.exitCode = 1;
