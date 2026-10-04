"""Run the full CPU importer once per BRD, isolating memory and preserving compact evidence."""
import argparse
import collections
import json
from pathlib import Path
import subprocess
import statistics
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cases", type=Path)
    parser.add_argument("report", type=Path)
    parser.add_argument("--runner", type=Path, default=Path("target/release/examples/validate_brd_case.exe"))
    parser.add_argument("--timeout", type=int, default=300)
    parser.add_argument("--summarize", action="store_true", help="Regenerate summaries from the existing journal")
    args = parser.parse_args()
    cases = sorted(p for p in args.cases.rglob("*") if p.is_file() and p.suffix.lower() == ".brd")
    if not cases:
        parser.error("No BRD files found")
    runner = args.runner.resolve(strict=True)
    args.report.mkdir(parents=True, exist_ok=True)
    rows = [json.loads(line) for line in (args.report / "cases.jsonl").read_text(encoding="utf-8").splitlines()] if args.summarize else []
    started = time.monotonic()
    with (args.report / "cases.jsonl").open("a" if args.summarize else "w", encoding="utf-8") as journal:
        for index, path in enumerate([] if args.summarize else cases, 1):
            case_started = time.monotonic()
            try:
                run = subprocess.run([str(runner), str(path), "auto"], capture_output=True,
                                     encoding="utf-8", errors="replace", timeout=args.timeout)
                if run.returncode == 0:
                    row = json.loads(run.stdout)
                else:
                    row = {"path": str(path), "status": "process_failed", "scene_built": False,
                           "exit_code": run.returncode, "stderr": run.stderr[-8000:]}
            except subprocess.TimeoutExpired:
                row = {"path": str(path), "status": "timeout", "scene_built": False,
                       "timeout_seconds": args.timeout}
            except (ValueError, OSError) as error:
                row = {"path": str(path), "status": "runner_failed", "scene_built": False,
                       "message": str(error)}
            row["wall_ms"] = round((time.monotonic() - case_started) * 1000)
            rows.append(row)
            journal.write(json.dumps(row, ensure_ascii=False) + "\n")
            journal.flush()
            print(f"[{index}/{len(cases)}] {row['status']} {row.get('encoding', '-')} "
                  f"{row['wall_ms']} ms {path.name}", flush=True)
    summary = {
        "cases_dir": str(args.cases.resolve()), "total": len(cases),
        "counts": dict(collections.Counter(row["status"] for row in rows)),
        "scene_built": sum(bool(row.get("scene_built")) for row in rows),
        "encodings": dict(collections.Counter(row["encoding"] for row in rows if "encoding" in row)),
        "errors": dict(collections.Counter(row["error"]["code"] for row in rows if "error" in row)),
        "elapsed_seconds": json.loads((args.report / "summary.json").read_text(encoding="utf-8"))["elapsed_seconds"] if args.summarize else round(time.monotonic() - started, 2),
        "gpu_validated": False,
    }
    timings = sorted(row["elapsed_ms"] for row in rows if "elapsed_ms" in row)
    summary["import_ms"] = {"median": statistics.median(timings),
                            "p95": timings[min(len(timings) - 1, int(len(timings) * .95))],
                            "max": max(timings)} if timings else None
    summary["versions"] = dict(collections.Counter(str(row["version"]) for row in rows if "version" in row))
    summary["diagnostics"] = dict(sum((collections.Counter(row.get("diagnostics", {})) for row in rows), collections.Counter()))
    (args.report / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8")
    lines = ["# BRD 自动编码与完整 CPU 导入验证", "", f"案例目录：`{args.cases.resolve()}`", "",
             f"共 {len(cases)} 个文件；完整场景构建成功 {summary['scene_built']} 个。",
             f"状态统计：`{summary['counts']}`。无诊断记为 passed，有诊断记为 partial；partial 不计作无问题通过。",
             f"总耗时 {summary['elapsed_seconds']} 秒；编码统计：`{summary['encodings']}`。", "",
             "使用 Release 构建、auto 编码、独立进程串行执行。包含文件读取、编码检测、严格索引、源哈希和完整场景构建；",
             "不包含 GPU 上传或绘制，也未与 Allegro/Web 的实际文字和图形逐项对照，不能据此证明编码语义或画面完全正确。",
             f"每板进程超时上限为 {args.timeout} 秒。未修改源 BRD。", "",
             f"导入时间（毫秒）：`{summary['import_ms']}`。", "", "## 有诊断或失败的案例", ""]
    for row in rows:
        if row["status"] != "passed":
            detail = row.get("message") or row.get("diagnostics") or row.get("stderr", "")
            lines.append(f"- `{Path(row['path']).name}`：{row['status']}；{detail}")
    if all(row["status"] == "passed" for row in rows):
        lines.append("无。")
    lines += ["", "## 导入耗时最高的案例", "", "| 文件 | 大小 MB | 编码 | 导入 ms | 状态 |", "| --- | ---: | --- | ---: | --- |"]
    for row in sorted(rows, key=lambda row: row.get("elapsed_ms", 0), reverse=True)[:10]:
        lines.append(f"| {Path(row['path']).name.replace('|', '/')} | {row.get('bytes', 0) / 1_000_000:.2f} | {row.get('encoding', '-')} | {row.get('elapsed_ms', '-')} | {row['status']} |")
    (args.report / "report.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(json.dumps(summary, ensure_ascii=False, indent=2), flush=True)


if __name__ == "__main__":
    main()
