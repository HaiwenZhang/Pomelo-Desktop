"""Run the production importer serially in isolated processes against every BRD."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time


def sha256(path):
    with path.open("rb") as reader:
        return hashlib.file_digest(reader, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cases", type=Path)
    parser.add_argument("probe", type=Path)
    parser.add_argument("encoding_manifest", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    cases = sorted((p for p in args.cases.iterdir() if p.is_file() and p.suffix.lower() == ".brd"),
                   key=lambda p: (p.stat().st_size, p.name.lower()))
    if not cases:
        raise ValueError("IMPORT_CASES_EMPTY")
    baseline = {Path(row["path"]).name.lower(): row for row in
                (json.loads(line) for line in args.encoding_manifest.read_text(encoding="utf-8-sig").splitlines())}
    if set(baseline) != {p.name.lower() for p in cases}:
        raise ValueError("IMPORT_CASE_SET_CHANGED")
    # A fresh directory prevents stale successful reports from masking a failed process.
    args.output.mkdir(parents=True, exist_ok=False)
    probe = args.probe.resolve(strict=True)
    provenance = {"probe": str(probe), "probe_sha256": sha256(probe),
                  "encoding_manifest_sha256": sha256(args.encoding_manifest),
                  "scope": "complete production CPU import; not field, GPU or source-text validation",
                  "case_count": len(cases)}
    (args.output / "provenance.json").write_text(json.dumps(provenance, indent=2), encoding="utf-8")
    passed = failed = 0
    with (args.output / "manifest.jsonl").open("w", encoding="utf-8") as manifest:
        for index, source in enumerate(cases):
            expected = baseline[source.name.lower()]
            source_hash = sha256(source)
            if source_hash != expected["sha256"] or source.stat().st_size != expected["bytes"]:
                raise ValueError(f"IMPORT_SOURCE_CHANGED: {source.name}")
            report = (args.output / (source.name + ".json")).resolve()
            started = time.monotonic()
            process = subprocess.run([str(probe), str(source.resolve()), expected["encoding"], str(report)],
                                     capture_output=True)
            (args.output / (source.name + ".stderr.log")).write_bytes(process.stderr)
            data = json.loads(report.read_text(encoding="utf-8")) if report.exists() else None
            ok = process.returncode == 0 and data is not None and data["status"] == "passed"
            if data is not None:
                if data["encoding"] != expected["encoding"] or data["bytes"] != expected["bytes"]:
                    raise ValueError("IMPORT_REPORT_IDENTITY_CHANGED")
                if ok and bytes(data["identity"]["sha256"]).hex() != source_hash:
                    raise ValueError("IMPORT_REPORT_HASH_CHANGED")
            passed += int(ok)
            failed += int(not ok)
            row = {"index": index + 1, "path": str(source.resolve()), "bytes": expected["bytes"],
                   "sha256": source_hash, "encoding": expected["encoding"], "report": str(report),
                   "exit_code": process.returncode, "status": "passed" if ok else "failed",
                   "process_ms": round((time.monotonic() - started) * 1000, 2),
                   "error": data.get("error") if data else None}
            manifest.write(json.dumps(row, ensure_ascii=False) + "\n")
            manifest.flush()
            print(json.dumps({"case": source.name, "completed": index + 1, "total": len(cases),
                              "passed": passed, "failed": failed, "error": row["error"]},
                             ensure_ascii=True), flush=True)
    summary = {"cases": len(cases), "passed": passed, "failed": failed,
               "scene_validated": False, "gpu_validated": False}
    (args.output / "summary.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")
    return int(failed != 0)


if __name__ == "__main__":
    sys.exit(main())
