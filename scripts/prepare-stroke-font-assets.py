"""Freeze the audited Web font snapshot for offline desktop loading."""

import argparse
import hashlib
import json
from pathlib import Path

from importlib.util import module_from_spec, spec_from_file_location


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("web_root", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    spec = spec_from_file_location("font_audit", Path(__file__).with_name("audit-stroke-font.py"))
    audit = module_from_spec(spec)
    spec.loader.exec_module(audit)
    core_path = args.web_root / "src/lib/text/stroke-font-data.ts"
    pages_path = args.web_root / "public/fonts/stroke"
    report = audit.audit(pages_path, core_path)
    if report["core_sha256"] != "de17e76e17cada8e3dcf91e8f7bfaad19eb779e546219efaa07add0a4af7a781":
        raise ValueError("core source differs from audited snapshot")
    core = audit.read_core(core_path)
    notice = (pages_path / "LICENSE-KiCad-stroke.txt").read_bytes()
    if hashlib.sha256(notice).hexdigest() != "eb632ce979b47fe8225d3f15a7edf09abba266603834b05f841ae48d8884607e":
        raise ValueError("license notice differs from audited snapshot")
    destination = args.destination.resolve()
    destination.mkdir(parents=True, exist_ok=True)
    core_bytes = (json.dumps(core, ensure_ascii=False, sort_keys=True, indent=2) + "\n").encode("utf-8")
    (destination / "core.json").write_bytes(core_bytes)
    (destination / "LICENSE-KiCad-stroke.txt").write_bytes(notice)
    for entry in report["files"]:
        raw = (pages_path / entry["name"]).read_bytes()
        if hashlib.sha256(raw).hexdigest() != entry["sha256"]:
            raise ValueError("font page changed during preparation")
        (destination / entry["name"]).write_bytes(raw)
    manifest = {"schema": 1, "source": "audited Pomelo Web working-tree snapshot",
                "core_source_sha256": report["core_sha256"],
                "core_json_sha256": hashlib.sha256(core_bytes).hexdigest(),
                "notice_sha256": hashlib.sha256(notice).hexdigest(),
                "pages": report["files"], "glyph_count": report["combined_glyph_count"],
                "licenses": ["GPL-2.0-or-later", "MIT", "OFL-1.1"],
                "release_license_review_complete": False}
    (destination / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    index = ["// Generated resource index. Font data notices: LICENSE-KiCad-stroke.txt.",
             "pub const PAGES: &[(u32, &[u8])] = &["]
    for entry in report["files"]:
        block = int(Path(entry["name"]).stem, 16)
        index.append(f'    (0x{block:x}, include_bytes!("{entry["name"]}")),')
    index.append("];")
    (destination / "pages.rs").write_text("\n".join(index) + "\n", encoding="utf-8")
    print(f"STROKE_ASSETS_READY core={len(core)} pages={len(report['files'])} glyphs={report['combined_glyph_count']}")


if __name__ == "__main__":
    main()
