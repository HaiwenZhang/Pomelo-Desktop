"""Developer-only regeneration of the frozen offline search collation blob."""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import subprocess
import uuid

ROOT = Path(__file__).resolve().parents[1]
EXPECTED = "37c34672d6bae78434672a07d301c9320181d3416edba80cf69d3bdeb8c85dab"
MARKERS = [
    "CollationSpecialPrimariesV1", "CollationRootV1", "CollationTailoringV1",
    "CollationDiacriticsV1", "CollationJamoV1", "CollationMetadataV1",
    "CollationReorderingV1", "NormalizerNfdDataV1", "NormalizerNfdTablesV1",
]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("datagen", type=Path)
    args = parser.parse_args()
    generator = args.datagen.resolve()
    version = subprocess.check_output([str(generator), "--version"], text=True).strip()
    if version != "icu4x-datagen 2.1.1":
        raise ValueError("SEARCH_DATA_GENERATOR_VERSION")
    destination = ROOT / ".cache" / "search-parity" / f"unihan-{uuid.uuid4().hex}.postcard"
    destination.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run([
        str(generator), "--format", "blob", "--out", str(destination),
        "--locales", "en", "zh-CN", "zh-TW", "ja", "ko",
        "--markers", *MARKERS, "--collation-root-han", "unihan",
        "--cldr-tag", "48.0.0", "--icuexport-tag", "release-78.1rc",
        "--deduplication", "none",
    ], check=True)
    checksum = hashlib.sha256(destination.read_bytes()).hexdigest()
    print(f"SEARCH_DATA_SHA256 {checksum}\nSEARCH_DATA_OUTPUT {destination}")
    if checksum != EXPECTED:
        raise ValueError("SEARCH_DATA_BASELINE_CHANGED")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
