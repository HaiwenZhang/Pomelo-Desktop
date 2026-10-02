"""Audit external Web stroke JSON resources without copying font data."""

import argparse
import ast
import hashlib
import json
import re
from pathlib import Path


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate glyph U+{ord(key):04X}")
        result[key] = value
    return result


def read_core(path):
    source = path.read_text(encoding="utf-8")
    body = source.split('export const strokeFont: Record<string, string> = {', 1)[1]
    body, remainder = body.split('};', 1)
    if remainder.strip():
        raise ValueError("unexpected content after core font map")
    pattern = re.compile(r'''\s*("(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|[^\s:]+)\s*:\s*("(?:\\.|[^"\\])*")\s*,\s*''')
    pairs = []
    for line in body.splitlines():
        if not line.strip():
            continue
        match = pattern.fullmatch(line)
        if not match:
            raise ValueError("unsupported core font source syntax")
        key, value = match.groups()
        character = ast.literal_eval(key) if key.startswith(("'", '"')) else key
        encoded = json.loads(value)
        if len(character) != 1 or len(encoded) < 2 or len(encoded) % 2 or any(not 32 <= ord(c) <= 126 for c in encoded):
            raise ValueError("invalid core glyph encoding")
        pairs.append((character, encoded))
    return unique_object(pairs)


def audit(root, core_path=None):
    files = []
    glyphs = set()
    total_points = 0
    max_points = 0
    max_character = None
    empty = 0
    for path in sorted(root.glob("*.json")):
        block = int(path.stem, 16)
        raw = path.read_bytes()
        data = json.loads(raw.decode("utf-8"), object_pairs_hook=unique_object)
        if not isinstance(data, dict):
            raise ValueError(f"{path.name}: expected glyph map")
        for character, encoded in data.items():
            if len(character) != 1 or ord(character) >> 8 != block:
                raise ValueError(f"{path.name}: invalid character block")
            if character in glyphs:
                raise ValueError(f"{path.name}: glyph appears in multiple pages")
            if (not isinstance(encoded, str) or len(encoded) < 2
                    or len(encoded) % 2 or any(not 32 <= ord(c) <= 126 for c in encoded)):
                raise ValueError(f"{path.name}: invalid encoding U+{ord(character):04X}")
            points = sum(encoded[i:i + 2] != " R" for i in range(2, len(encoded), 2))
            total_points += points
            empty += points == 0
            if points > max_points:
                max_points, max_character = points, f"U+{ord(character):04X}"
            glyphs.add(character)
        files.append({"name": path.name, "bytes": len(raw), "glyphs": len(data),
                      "sha256": hashlib.sha256(raw).hexdigest()})
    if not files:
        raise ValueError("no stroke font JSON resources found")
    core = read_core(core_path) if core_path else {}
    conflicts = []
    if core:
        for path in sorted(root.glob("*.json")):
            page = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_object)
            conflicts.extend(f"U+{ord(c):04X}" for c in core.keys() & page.keys() if core[c] != page[c])
    if conflicts:
        raise ValueError(f"conflicting core/extended glyphs: {conflicts}")
    all_glyphs = glyphs | core.keys()
    probes = "Aa09Ω中文繁體日本語한글가나다"
    return {"scope": "external_web_font_encoding_audit", "files": files,
            "file_count": len(files), "glyph_count": len(glyphs),
            "total_bytes": sum(file["bytes"] for file in files),
            "total_points": total_points, "empty_glyphs": empty,
            "max_glyph_points": max_points, "max_glyph_character": max_character,
            "probe_coverage": {c: c in all_glyphs for c in probes},
            "core_typescript_checked": core_path is not None,
            "core_glyph_count": len(core), "combined_glyph_count": len(all_glyphs),
            "core_extended_overlap": len(glyphs & core.keys()),
            "core_sha256": hashlib.sha256(core_path.read_bytes()).hexdigest() if core_path else None,
            "license_approved": False,
            "gpu_render_verified": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("font_directory", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--core", type=Path)
    args = parser.parse_args()
    result = audit(args.font_directory, args.core)
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({key: value for key, value in result.items() if key != "files"}, ensure_ascii=False))


if __name__ == "__main__":
    main()
