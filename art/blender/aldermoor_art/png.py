"""Exact PNG files from numpy arrays: no colour management, no alpha premultiplication.

Blender's image saving treats a fourth channel as transparency, which ruins data maps whose
alpha is a value of its own (the terrain's rock weight). Data maps are written byte for byte
here instead: 8 bits a channel, rows top first, each row filtered against the one above.
"""

import struct
import zlib
from pathlib import Path

import numpy as np


def write(path, pixels, level=7):
    """Writes `pixels` (h × w × 1|2|3|4, uint8, or floats in 0..1; top row first) to `path`."""
    a = np.asarray(pixels)
    if a.dtype != np.uint8:
        a = np.clip(np.round(np.asarray(a, np.float64) * 255.0), 0, 255).astype(np.uint8)
    if a.ndim == 2:
        a = a[..., None]
    h, w, c = a.shape
    colour_type = {1: 0, 2: 4, 3: 2, 4: 6}[c]
    rows = a.reshape(h, w * c)
    # Filter 2 ("up"): each byte less the byte above it; smooth maps compress far better.
    up = np.empty_like(rows)
    up[0] = rows[0]
    up[1:] = rows[1:] - rows[:-1]
    raw = np.empty((h, 1 + w * c), np.uint8)
    raw[:, 0] = 2
    raw[:, 1:] = up

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, colour_type, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw.tobytes(), level))
        + chunk(b"IEND", b"")
    )
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(png)
    return path


def read(path):
    """Reads back a PNG written by `write` (8-bit, filters 0 and 2), top row first, as uint8."""
    data = Path(path).read_bytes()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", f"{path} is not a PNG"
    pos, idat = 8, b""
    while pos < len(data):
        n = struct.unpack(">I", data[pos:pos + 4])[0]
        kind = data[pos + 4:pos + 8]
        body = data[pos + 8:pos + 8 + n]
        if kind == b"IHDR":
            w, h, depth, colour_type = struct.unpack(">IIBB", body[:10])
            assert depth == 8, "8-bit only"
            c = {0: 1, 4: 2, 2: 3, 6: 4}[colour_type]
        elif kind == b"IDAT":
            idat += body
        pos += 12 + n
    raw = np.frombuffer(zlib.decompress(idat), np.uint8).reshape(h, 1 + w * c)
    out = np.zeros((h, w * c), np.uint8)
    for y in range(h):
        f, row = raw[y, 0], raw[y, 1:]
        assert f in (0, 2), f"filter {f} isn't one this reader handles"
        out[y] = row if f == 0 or y == 0 else row + out[y - 1]
    return out.reshape(h, w, c)
