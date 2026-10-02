// Development-only oracle: no JavaScript dependency in the shipped application.
import { readFileSync, openSync, readSync, closeSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import { resolve } from 'node:path';

const [webRoot, reportPath] = process.argv.slice(2);
if (!webRoot || !reportPath) throw new Error('Usage: check-header-parity.ts WEB_ROOT RUST_HEADER_REPORT');
const { AllegroHeaderReader } = await import(pathToFileURL(resolve(webRoot, 'src/lib/allegro/binary/header.ts')).href);
let passed = 0;
for (const line of readFileSync(reportPath, 'utf8').trim().split(/\r?\n/)) {
  const entry = JSON.parse(line);
  if (!entry.header || entry.status !== 'passed') throw new Error(`Rust probe failed: ${entry.path}`);
  const source = Buffer.alloc(0x538);
  const fd = openSync(entry.path, 'r');
  try { readSync(fd, source, 0, source.length, 0); } finally { closeSync(fd); }
  const oracle = new AllegroHeaderReader(source.buffer.slice(source.byteOffset, source.byteOffset + source.byteLength)).read();
  const actual = entry.header;
  const pairs = [
    ['magic', oracle.magic, actual.magic], ['version', oracle.version, actual.version],
    ['writerVersion', oracle.writerVersion, actual.writer_version], ['objectCount', oracle.objectCount, actual.object_count],
    ['units', oracle.units, actual.units], ['divisor', oracle.divisor, actual.divisor],
    ['stringCount', oracle.stringCount, actual.string_count], ['constraintEnd', oracle.constraintEnd, actual.constraint_end],
    ['textList', oracle.textList, actual.text_list], ['graphicList', oracle.graphicList, actual.graphic_list],
    ['sentinelKeys', oracle.sentinelKeys, actual.sentinel_keys],
    ['layerMap', oracle.layerMap, actual.layer_map.map((layer: any) => ({classId: layer.class_id, recordId: layer.record_id}))],
  ];
  for (const [field, expected, result] of pairs) {
    if (JSON.stringify(expected) !== JSON.stringify(result)) throw new Error(`${entry.path}: ${field} differs`);
  }
  passed++;
}
console.log(JSON.stringify({stage: 'header', compared: passed, matched: passed, sceneValidated: false}));
