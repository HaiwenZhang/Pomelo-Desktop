"""Generate an original minimal BRD with 40 recoverable text-content warnings.

The V174 layouts follow pomelo-import/tests/annotations.rs. No external board
bytes are copied. Each wrapper points to a missing text record; the board still
contains one valid track, so the ordinary importer can publish its scene.
"""

from pathlib import Path
import struct


def put(data, offset, value):
    struct.pack_into("<I", data, offset, value)


def record(kind, key, size):
    data = bytearray(size)
    data[0] = kind
    put(data, 4, key)
    return data


def build():
    data = bytearray(0x1200)
    for offset, value in [(0, 0x140900), (0x26C, 1000), (0x8C, 999),
                          (0x90, 10), (0x5C, 888), (0x60, 0),
                          (0x428 + 6 * 8 + 4, 9_000_000)]:
        put(data, offset, value)
    data[0x180] = 3
    track = record(5, 2, 68)
    struct.pack_into("<H", track, 2, 6)
    put(track, 56, 3)
    edge = record(0x16, 3, 44)
    for offset, value in [(8, 2), (24, 100), (28, 1000), (32, 2000),
                          (36, 3000), (40, 4000)]:
        put(edge, offset, value)
    data.extend(track)
    data.extend(edge)
    font = record(0x36, 1, 104)
    struct.pack_into("<H", font, 2, 8)
    for offset, value in [(16, 1), (20, 1), (44, 1000), (48, 500),
                          (52, 30), (56, 40), (68, 10)]:
        put(font, offset, value)
    data.extend(font)
    for key in range(10, 50):
        wrapper = record(0x30, key, 60)
        struct.pack_into("<H", wrapper, 2, 6)
        for offset, value in [(8, key + 1 if key < 49 else 999), (20, 1),
                              (32, 123450 + key), (44, 0xFFFFF448),
                              (48, 2000), (56, 90000)]:
            put(wrapper, offset, value)
        data.extend(wrapper)
    layers = record(0x2A, 0, 24)
    layers[2] = 1
    put(layers, 12, 0x8000)
    put(layers, 20, 9_000_000)
    data.extend(layers)
    data.extend(bytes(4))
    return bytes(data)


if __name__ == "__main__":
    target = Path(__file__).resolve().parents[1] / "tests/fixtures/diagnostics-40.brd"
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(build())
    print(f"Generated {target.name}: {target.stat().st_size} bytes")
