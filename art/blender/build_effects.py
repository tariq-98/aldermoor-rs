"""Builds the combat effects' art: the sprite atlas, the tiling strips and the effect meshes.

    art/blender.sh -P art/blender/build_effects.py [-- --only=sprites,volumes,meshes --review]

Writes assets/effects/:

- atlas.png: every sprite and flipbook, packed (linear RGBA8: heat, cover, erosion; fx.py).
- atlas.json: where each sits: {"size": [w, h], "frames": {name: {"rect": [x, y, w, h],
  "count": n, "cols": c}}}. A flipbook's frames run left to right, top to bottom in its rect.
- tile.<name>.png: tiling strips (trail, noise, hex), all the same size, for trails, ground
  marks and shells to scroll through.
- shell.glb, column.glb, crystal.glb, arrow.glb (fxmesh.py).

The Cycles flipbooks (fire, smoke, burst) are slow, so they're kept in art/out/effects and
rendered again only with --only=volumes. --review writes art/out/effects/review.png, every sprite
coloured through the catalogue's ramps on a dark ground.
"""

import json
import math
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aldermoor_art import fx, fxmesh, fxvolume, png, scene as sc  # noqa: E402

ARGS = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
ONLY = next((a.split("=", 1)[1].split(",") for a in ARGS if a.startswith("--only=")), None)
REVIEW = "--review" in ARGS
OUT = sc.OUT / "effects"
CACHE = sc.ROOT / "art" / "out" / "effects"
ATLAS_WIDTH = 2048
PAD = 4


def wanted(name):
    return ONLY is None or name in ONLY


def sprites():
    """(name, image, frame count, columns) for everything painted with numpy."""
    out = [
        ("glow", fx.glow(), 1, 1),
        ("flare", fx.flare(), 1, 1),
        ("spark", fx.spark(), 1, 1),
        ("shock", fx.shock(), 1, 1),
        ("ring", fx.ring(), 1, 1),
        ("seal", fx.ornate_ring(), 1, 1),
        ("runes", fx.runes(), 1, 1),
        ("sigil", fx.sigil(), 1, 1),
        ("star", fx.star(), 1, 1),
        ("snowflake", fx.snowflake(), 1, 1),
        ("shard", fx.shard(), 1, 1),
        ("bubble", fx.bubble(), 1, 1),
        ("ripple", fx.ripple(), 1, 1),
        ("scorch", fx.scorch(), 1, 1),
        ("frost", fx.frost(), 1, 1),
    ]
    for i in range(4):
        out.append((f"lightning{i}", fx.lightning(seed=47 + i * 13), 1, 1))
    out.append(("wisp", fx.sheet(fx.wisp(), 4), 16, 4))
    out.append(("flame", fx.sheet(fx.flame(), 4), 16, 4))
    out.append(("puff", fx.sheet(fx.puff(), 4), 16, 4))
    out.append(("swirl", fx.sheet(fx.swirl(), 4), 16, 4))
    out.append(("starburst", fx.starburst(), 1, 1))
    return out


def volumes():
    """The Cycles flipbooks, rendered now or read back from the cache."""
    CACHE.mkdir(parents=True, exist_ok=True)
    out = []
    for kind in ("fire", "smoke", "burst"):
        cached = CACHE / f"{kind}.npy"
        if wanted("volumes") or not cached.exists():
            frames = fxvolume.flipbook(kind)
            np.save(cached, np.stack(frames))
            print("BUILT", kind, "flipbook")
        frames = list(np.load(cached))
        out.append((kind, fx.sheet(frames, 4), len(frames), 4))
    return out


def pack(items):
    """Shelf-packs sprites into one atlas, tallest first. Returns the image and its index."""
    order = sorted(items, key=lambda it: (-it[1].shape[0], -it[1].shape[1], it[0]))
    x = y = shelf = 0
    places = {}
    for name, img, count, cols in order:
        h, w = img.shape[:2]
        if x + w + PAD > ATLAS_WIDTH:
            x, y, shelf = 0, y + shelf + PAD * 2, 0
        places[name] = (x + PAD, y + PAD, w, h, count, cols)
        x += w + PAD * 2
        shelf = max(shelf, h)
    height = y + shelf + PAD * 2
    height = 1 << math.ceil(math.log2(height))
    atlas = np.zeros((height, ATLAS_WIDTH, 4), np.float32)
    atlas[..., 3] = 1
    for name, img, count, cols in items:
        px, py, w, h, _, _ = places[name]
        atlas[py:py + h, px:px + w] = img
    index = {
        "size": [ATLAS_WIDTH, height],
        "frames": {
            name: {"rect": [px, py, w, h], "count": count, "cols": cols}
            for name, (px, py, w, h, count, cols) in sorted(places.items())
        },
    }
    return atlas, index


def tiles():
    return {"trail": fx.tile_trail(), "noise": fx.tile_noise(), "hex": fx.tile_hex(), "energy": fx.tile_energy()}


def _ramps():
    """The catalogue's ramps if it has them, else a few to look at."""
    path = sc.OUT / "catalog" / "effects.json"
    ramps = {}
    if path.exists():
        ramps = json.loads(path.read_text()).get("ramps", {})
    if not ramps:
        ramps = {
            "fire": [[0.0, 0.0, 0.0, 0.0], [0.3, 0.5, 0.05, 0.0], [0.6, 2.0, 0.6, 0.08], [0.85, 4.0, 2.4, 0.8], [1.0, 6.0, 5.0, 3.5]],
            "arcane": [[0.0, 0.0, 0.0, 0.0], [0.4, 0.35, 0.08, 1.2], [0.75, 1.4, 0.8, 3.0], [1.0, 3.5, 3.0, 4.5]],
            "frost": [[0.0, 0.0, 0.0, 0.0], [0.4, 0.08, 0.35, 1.2], [0.75, 0.7, 1.6, 3.0], [1.0, 3.5, 4.0, 4.5]],
        }
    return ramps


def _ramp(stops, x):
    stops = sorted(stops)
    xs = [s[0] for s in stops]
    return np.stack([np.interp(x, xs, [s[c] for s in stops]) for c in (1, 2, 3)], -1)


def _aces(x):
    return np.clip((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), 0, 1)


def review(items, tile_images):
    """Every sprite through two ramps (fire and arcane, or the first two the catalogue has),
    additively over a dark ground, then tone mapped as the game does."""
    ramps = _ramps()
    names = list(ramps)[:2] if "fire" not in ramps else ["fire", "arcane" if "arcane" in ramps else list(ramps)[-1]]
    cell = 160
    rows = []
    for ramp_name in names:
        tiles_row = []
        for name, img, count, cols in items + [(f"tile.{k}", v, 1, 1) for k, v in tile_images.items()]:
            frames = []
            if count > 1:
                h, w = img.shape[0] // math.ceil(count / cols), img.shape[1] // cols
                picks = [0, count // 3, 2 * count // 3, count - 1]
                frames = [img[(i // cols) * h:(i // cols + 1) * h, (i % cols) * w:(i % cols + 1) * w] for i in picks]
            else:
                frames = [img]
            for f in frames:
                h, w = f.shape[:2]
                scale = cell / max(h, w)
                ys = (np.arange(int(h * scale)) / scale).astype(int)
                xs = (np.arange(int(w * scale)) / scale).astype(int)
                small = f[ys][:, xs]
                colour = _ramp(ramps[ramp_name], small[..., 0]) * small[..., 1:2]
                tile = np.full((cell, cell, 3), 0.025, np.float32)
                oy, ox = (cell - small.shape[0]) // 2, (cell - small.shape[1]) // 2
                tile[oy:oy + small.shape[0], ox:ox + small.shape[1]] += colour
                tiles_row.append(tile)
        per_row = 12
        for i in range(0, len(tiles_row), per_row):
            chunk = tiles_row[i:i + per_row]
            chunk += [np.full((cell, cell, 3), 0.0, np.float32)] * (per_row - len(chunk))
            rows.append(np.concatenate(chunk, 1))
    image = _aces(np.concatenate(rows, 0))
    srgb = np.where(image <= 0.0031308, image * 12.92, 1.055 * np.power(image, 1 / 2.4) - 0.055)
    CACHE.mkdir(parents=True, exist_ok=True)
    png.write(CACHE / "review.png", srgb)
    print("WROTE", CACHE / "review.png")


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    if wanted("sprites") or wanted("volumes"):
        items = sprites() + volumes()
        atlas, index = pack(items)
        png.write(OUT / "atlas.png", atlas)
        (OUT / "atlas.json").write_text(json.dumps(index, indent=1) + "\n")
        print("WROTE effects/atlas.png", f"{atlas.shape[1]}x{atlas.shape[0]}", len(items), "sprites")
        tile_images = tiles()
        for name, img in tile_images.items():
            png.write(OUT / f"tile.{name}.png", img)
            print("WROTE", f"effects/tile.{name}.png")
        if REVIEW:
            review(items, tile_images)
    if wanted("meshes"):
        for name in fxmesh.build(OUT):
            print("WROTE", f"effects/{name}")


main()
