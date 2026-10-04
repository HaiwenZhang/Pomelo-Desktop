"""Freeze Allegro's exported display parameters without guessing undocumented flags."""
import argparse
import hashlib
import json
from pathlib import Path
import xml.etree.ElementTree as ET


def sha256(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def tree(element):
    if not len(element):
        return element.text or ""
    result = {}
    for child in element:
        result.setdefault(child.tag, []).append(tree(child))
    return {key: values[0] if len(values) == 1 else values for key, values in result.items()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("parameters", type=Path)
    parser.add_argument("board", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    if args.parameters.stat().st_size > 8 * 1024 * 1024:
        raise ValueError("ALLEGRO_PARAMETERS_SIZE_LIMIT")
    data = args.parameters.read_bytes()
    if b"<!DOCTYPE" in data.upper() or b"<!ENTITY" in data.upper():
        raise ValueError("ALLEGRO_PARAMETERS_DTD_UNSUPPORTED")
    root = ET.fromstring(data)
    if root.tag != "CadenceAllegroParameter":
        raise ValueError("ALLEGRO_PARAMETERS_ROOT_INVALID")
    palette = root.find("ColorParmType")
    table = root.find("color_table_table")
    common = root.find("db_common_type")
    if palette is None or table is None or common is None:
        raise ValueError("ALLEGRO_PARAMETERS_DISPLAY_SECTIONS_MISSING")
    # Keep repeated color numbers: an export can contain auxiliary color records.
    # Neither their visibility field nor class-table high bits are decoded here.
    colors = [tree(color) for color in palette.findall("colors")]
    result = {
        "schema_version": 1,
        "allegro_parameter_version": root.findtext("parameter_header/version"),
        "parameters": str(args.parameters.resolve()),
        "parameters_sha256": sha256(args.parameters),
        "board": str(args.board.resolve()),
        "board_sha256": sha256(args.board),
        "board_association": "operator-recorded; the parameter XML contains no board hash",
        "scope": "exported display settings; not geometry, viewport opacity or visual validation",
        "palette": colors,
        "class_subclass_raw_words": tree(table),
        "common_display_parameters": {
            child.tag: tree(child) for child in common
            if child.tag.endswith("_color") or child.tag in ("dispflag", "active_class", "active_subclass")
        },
        "mixed_color_priorities": [item.text for item in palette.findall("mixedPriority")],
        "text_size_table": tree(root.find("text_size_table")) if root.find("text_size_table") is not None else None,
    }
    if args.output.suffix.lower() != ".json":
        raise ValueError("ALLEGRO_PARAMETERS_OUTPUT_EXTENSION")
    with args.output.open("x", encoding="utf-8") as writer:
        json.dump(result, writer, ensure_ascii=False, indent=2)
        writer.write("\n")
    print(json.dumps({"output": str(args.output), "palette_records": len(colors),
                      "parameter_sha256": result["parameters_sha256"]}))


if __name__ == "__main__":
    main()
