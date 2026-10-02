"""Development-only field probes selected deterministically from validated index evidence."""
import argparse
import json
import mmap
from pathlib import Path
import struct
import subprocess
import sys


FIXED_TYPES = {1, 4, 5, 6, 7, 8, 9, 10, 12, 13, 14, 15, 16, 17, 18, 20, 21, 22,
               23, 27, 32, 34, 35, 36, 38, 40, 41, 43, 44, 45, 46, 47, 48,
               50, 51, 52, 53, 55, 56, 57, 58, 62}
VARIABLE_TYPES = {3, 26, 28, 29, 30, 31, 33, 39, 42, 49, 54, 59, 60}
GEOMETRY_TYPES = {1, 5, 20, 21, 22, 23, 40}
PADSTACK_TYPES = {28, 47, 50, 51}
PLACEMENT_TYPES = {45}
ROUTING_TYPES = {5, 51}
COPPER_TYPES = {14, 20, 36, 40}


def run() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("index_report", type=Path)
    parser.add_argument("output_dir", type=Path)
    parser.add_argument("executable", type=Path)
    parser.add_argument("--samples", type=int, default=32)
    parser.add_argument("--locale", choices=["en", "zh-CN", "zh-TW", "ja", "ko"], default="en")
    parser.add_argument("--fixed-only", action="store_true")
    parser.add_argument("--geometry", action="store_true", help="Probe path/contour semantics instead of raw fields")
    parser.add_argument("--padstack", action="store_true", help="Probe padstack/pad/drill semantics instead of raw fields")
    parser.add_argument("--placement", action="store_true", help="Probe footprint and all its pin placements")
    parser.add_argument("--routing", action="store_true", help="Probe tracks and vias, including every bond wire and finger")
    parser.add_argument("--copper", action="store_true", help="Probe computed copper, hatches, rectangles and outlines")
    args = parser.parse_args()
    index_report, output_dir, executable = args.index_report, args.output_dir, args.executable
    sample_count, locale = args.samples, args.locale
    if sum([args.geometry, args.padstack, args.placement, args.routing, args.copper, args.fixed_only]) > 1:
        parser.error("Probe mode flags are mutually exclusive")
    record_types = COPPER_TYPES if args.copper else ROUTING_TYPES if args.routing else PLACEMENT_TYPES if args.placement else PADSTACK_TYPES if args.padstack else GEOMETRY_TYPES if args.geometry else FIXED_TYPES if args.fixed_only else FIXED_TYPES | VARIABLE_TYPES
    stage = "copper" if args.copper else "routing" if args.routing else "placement" if args.placement else "padstack" if args.padstack else "geometry" if args.geometry else "fixed-records" if args.fixed_only else "records"
    command = "decode-copper" if args.copper else "decode-routing" if args.routing else "decode-placement" if args.placement else "decode-padstack" if args.padstack else "decode-geometry" if args.geometry else "decode-fixed" if args.fixed_only else "decode-records"
    if not 1 <= sample_count <= 64:
        raise ValueError("FIXED_PROBE_SAMPLE_LIMIT: 1..64")
    output_dir.mkdir(parents=True, exist_ok=True)
    entries = [json.loads(line) for line in index_report.read_text(encoding="utf-8-sig").splitlines()]
    cases = records = failures = 0
    manifest = output_dir / "manifest.jsonl"
    with manifest.open("w", encoding="utf-8") as writer:
        for entry in entries:
            if entry["stage"] != "index" or entry["status"] != "passed" or not entry["index_data_path"]:
                raise ValueError(f"FIXED_PROBE_INDEX_INVALID: {entry['path']}")
            counts = {int(tag, 16): count for tag, count in entry["index"]["by_type"].items() if int(tag, 16) in record_types}
            ranks = {}
            for kind, count in counts.items():
                n = min(sample_count, count)
                ranks[kind] = {i * (count - 1) // (n - 1) for i in range(n)} if n > 1 else {0}
            seen = {kind: 0 for kind in counts}
            spans = []
            special_offsets = set()
            if args.routing:
                # Independently select every special layer record from the frozen source.
                with Path(entry["path"]).open("rb") as raw_source, Path(entry["index_data_path"]).open("rb") as index_source:
                    with mmap.mmap(raw_source.fileno(), 0, access=mmap.ACCESS_READ) as raw, mmap.mmap(index_source.fileno(), 0, access=mmap.ACCESS_READ) as idx:
                        for position in range(16, 16 + 16 * entry["index"]["records"], 16):
                            offset, length, key, kind = struct.unpack_from("<IIII", idx, position)
                            if kind in ROUTING_TYPES and struct.unpack_from("<H", raw, offset + 2)[0] == (0xfd06 if kind == 5 else 0xc012):
                                special_offsets.add(offset)
            with Path(entry["index_data_path"]).open("rb") as source:
                with mmap.mmap(source.fileno(), 0, access=mmap.ACCESS_READ) as data:
                    if data[:8] != b"PMIDX001":
                        raise ValueError("FIXED_PROBE_INDEX_SIGNATURE")
                    total, strings = struct.unpack_from("<II", data, 8)
                    if total != entry["index"]["records"] or strings != entry["index"]["strings"]:
                        raise ValueError("FIXED_PROBE_INDEX_COUNTS")
                    if len(data) < 16 + 16 * total:
                        raise ValueError("FIXED_PROBE_INDEX_TRUNCATED")
                    for position in range(16, 16 + 16 * total, 16):
                        offset, length, key, kind = struct.unpack_from("<IIII", data, position)
                        if kind not in ranks:
                            continue
                        rank = seen[kind]
                        seen[kind] = rank + 1
                        if rank in ranks[kind] or offset in special_offsets:
                            spans.append({"offset": offset, "byte_length": length, "key": key, "record_type": kind})
            if seen != counts:
                raise ValueError("FIXED_PROBE_INDEX_TYPE_COUNTS")
            basename = Path(entry["path"]).name
            request_path = output_dir / (basename + ".request.json")
            report_path = output_dir / (basename + ".fields.jsonl")
            request_path.write_text(json.dumps({"schema_version": 1, "sha256": entry["sha256"], "source_size": entry["bytes"], "spans": spans}), encoding="utf-8")
            process = subprocess.run([str(executable.resolve()), "--locale", locale, "--encoding", entry["encoding"],
                command, entry["path"], "--records", str(request_path), "--report", str(report_path)], capture_output=True)
            cases += 1
            records += len(spans)
            if process.returncode:
                failures += 1
                sys.stderr.buffer.write(process.stderr)
            writer.write(json.dumps({"stage": stage, "path": entry["path"], "encoding": entry["encoding"], "sha256": entry["sha256"], "bytes": entry["bytes"],
                "request": str(request_path), "report": str(report_path), "records": len(spans), "source_counts": counts,
                "samples_per_type": sample_count, "special_offsets": sorted(special_offsets), "status": "passed" if process.returncode == 0 else "failed"}, ensure_ascii=False) + "\n")
            writer.flush()
            if cases % 20 == 0:
                print(json.dumps({"cases": cases, "records": records, "failed": failures}), flush=True)
    print(json.dumps({"stage": stage, "cases": cases, "records": records, "failed": failures, "manifest": str(manifest), "scene_validated": False}))
    if failures:
        sys.exit(1)


if __name__ == "__main__":
    run()
