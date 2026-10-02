"""Export complete native network maps from the frozen, validated BRD case set."""
import argparse
import json
from pathlib import Path
import subprocess
import sys


def run() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("index_report", type=Path)
    parser.add_argument("output_dir", type=Path)
    parser.add_argument("executable", type=Path)
    args = parser.parse_args()
    entries = [json.loads(line) for line in args.index_report.read_text(encoding="utf-8-sig").splitlines()]
    args.output_dir.mkdir(parents=True, exist_ok=True)
    cases = owners = visits = failures = 0
    with (args.output_dir / "manifest.jsonl").open("w", encoding="utf-8") as writer:
        for entry in entries:
            if entry["stage"] != "index" or entry["status"] != "passed":
                raise ValueError("CONNECTIVITY_INDEX_INVALID")
            report = args.output_dir / (Path(entry["path"]).name + ".network.json")
            result = subprocess.run([str(args.executable.resolve()), "--locale", "en", "--encoding", entry["encoding"],
                "decode-connectivity", entry["path"], "--report", str(report)], capture_output=True)
            data = json.loads(report.read_text(encoding="utf-8")) if report.exists() else None
            passed = result.returncode == 0
            if data is not None:
                for field in ["sha256", "bytes", "encoding"]:
                    if data[field] != entry[field]:
                        raise ValueError(f"CONNECTIVITY_SOURCE_CHANGED: {entry['path']}")
            if passed:
                if not data or data["stage"] != "connectivity" or data["scene_validated"] is not False or data["error"] is not None:
                    raise ValueError("CONNECTIVITY_REPORT_INVALID")
                if data["source_records"] != entry["index"]["records"] or data["source_strings"] != entry["index"]["strings"]:
                    raise ValueError("CONNECTIVITY_INDEX_COUNTS_DIFFER")
                owners += len(data["network"]["owners"])
                visits += data["network"]["link_visits"]
            else:
                failures += 1
                sys.stderr.buffer.write(result.stderr)
            writer.write(json.dumps({"stage": "connectivity", "path": entry["path"], "encoding": entry["encoding"],
                "sha256": entry["sha256"], "bytes": entry["bytes"], "report": str(report),
                "status": "passed" if passed else "failed"}, ensure_ascii=False) + "\n")
            writer.flush()
            cases += 1
            if cases % 20 == 0:
                print(json.dumps({"cases": cases, "owners": owners, "link_visits": visits, "failed": failures}), flush=True)
    print(json.dumps({"cases": cases, "owners": owners, "link_visits": visits, "failed": failures, "scene_validated": False}))
    if failures:
        sys.exit(1)


if __name__ == "__main__":
    run()
