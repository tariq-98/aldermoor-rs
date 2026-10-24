"""Dye masks for painted textures: which texels are the main cloth (R) and the trim (G).

Garments paint their masks along with their cloth (garments.cloth_texture). Outfits painted by
hand, like the Quaternius ones, get theirs by colour: each channel names a band of hue,
saturation and value, softened at its edges, so a dye recolours just the cloth it means.
"""

from pathlib import Path

import numpy as np

from . import materials as mt


def hsv(rgb):
    """Hue in degrees, saturation and value, for sRGB values in 0..1."""
    r, g, b = rgb[..., 0], rgb[..., 1], rgb[..., 2]
    mx = rgb.max(-1)
    mn = rgb.min(-1)
    d = mx - mn
    safe = np.where(d > 1e-6, d, 1.0)
    h = np.where(mx == r, ((g - b) / safe) % 6.0, np.where(mx == g, (b - r) / safe + 2.0, (r - g) / safe + 4.0)) * 60.0
    h = np.where(d > 1e-6, h, 0.0)
    s = np.where(mx > 1e-6, d / np.where(mx > 1e-6, mx, 1.0), 0.0)
    return h, s, mx


def _band(x, lo, hi, soft):
    return np.clip(np.minimum((x - (lo - soft)) / soft, ((hi + soft) - x) / soft), 0.0, 1.0)


def _hue_band(h, lo, hi, soft):
    """Membership of a hue band that may wrap past 360 degrees."""
    centre = (lo + (hi - lo if hi >= lo else hi + 360 - lo) / 2) % 360
    half = ((hi - lo) % 360) / 2
    dist = np.abs((h - centre + 180) % 360 - 180)
    return np.clip(1.0 - (dist - half) / soft, 0.0, 1.0)


def _blur(a, passes=2):
    for _ in range(passes):
        p = np.pad(a, 1, mode="edge")
        a = sum(p[1 + dy:1 + dy + a.shape[0], 1 + dx:1 + dx + a.shape[1]] for dy in (-1, 0, 1) for dx in (-1, 0, 1)) / 9.0
    return a


def mask_from_texture(src, out, channels, size=1024):
    """Writes `out` (R and G masks) from a painted texture; returns each channel's reference.

    `channels` is up to two (name, rule) pairs; a rule has `hue`, `sat` and `val` ranges
    (degrees, 0..1, 0..1). The reference is the channel's mean painted colour, linear RGB.
    """
    img = mt._load(src)
    img.colorspace_settings.name = "sRGB"
    px = mt.resized(img, size)
    srgb = np.clip(px[..., :3], 0.0, 1.0)
    h, s, v = hsv(srgb)
    lin = mt._srgb_to_linear(srgb)
    out_px = np.zeros(px.shape, dtype=np.float32)
    out_px[..., 3] = 1.0
    refs = []
    for k, (name, rule) in enumerate(channels):
        m = _hue_band(h, *rule["hue"], rule.get("hue_soft", 12.0))
        m = m * _band(s, *rule["sat"], 0.06) * _band(v, *rule["val"], 0.06)
        m = _blur(m)
        out_px[..., k] = m
        w = (m > 0.5).astype(np.float32)
        ref = (lin * w[..., None]).sum((0, 1)) / max(w.sum(), 1.0)
        refs.append({"channel": "rg"[k], "name": name, "reference": [round(float(c), 4) for c in ref],
                     "coverage": round(float(w.mean()), 3)})
    mt._save(out_px, Path(out), "Non-Color")
    return refs
