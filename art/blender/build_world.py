"""Builds the Brambleford valley's ground, river and sky, and the light they're seen in.

    cargo run -p world --release --example ground_fields -- art/out/world
    art/blender.sh -P art/blender/build_world.py [-- --only=grass,weights,sky]

Writes assets/terrain/ (four ground layers and the weights that blend them across the valley),
assets/water/ and assets/sky/, and the renderer's inputs: assets/catalog/surfaces.json and
assets/catalog/look.json (docs/ASSETS.md, "Ground, water, sky and light"). The ground fields
come from the world crate, so build them first (art/build_all.sh does).
"""

import json
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aldermoor_art import ground, materials as mt, png, scene as sc, tiles  # noqa: E402

ARGS = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
ONLY = next((a.split("=", 1)[1].split(",") for a in ARGS if a.startswith("--only=")), None)
FIELDS = sc.ROOT / "art" / "out" / "world"

# Towards the sun: late morning, from the south-east, about 38° up. The default camera looks
# north over the village, so the light falls from behind its right shoulder.
SUN = np.array([0.55, 0.62, 0.56]) / np.linalg.norm([0.55, 0.62, 0.56])


def wanted(name):
    return ONLY is None or name in ONLY


def save_colour(colour, path):
    """Linear RGB, top row first, to an sRGB JPEG."""
    h, w = colour.shape[:2]
    rgba = np.ones((h, w, 4), np.float32)
    rgba[..., :3] = mt._linear_to_srgb(np.clip(colour, 0, 1))
    mt._save(rgba[::-1].copy(), Path(path), "sRGB")


def main():
    out = sc.OUT
    layer_files = {}
    for name, (paint, _, _) in ground.LAYERS.items():
        layer_files[name] = (f"terrain/{name}_colour.jpg", f"terrain/{name}_normal.png")
        if wanted(name):
            colour, height, strength, _ = paint()
            ground.write_layer(name, colour, height, strength, out / "terrain", save_colour)
            print("WROTE", *layer_files[name])
    places = ground.places(FIELDS / "places.json")
    if wanted("weights"):
        w = ground.weights(FIELDS / "fields.bin", places, SUN)
        png.write(out / "terrain" / "brambleford_weights.png", w)
        share = w.reshape(-1, 4).mean(0)
        print("WROTE terrain/brambleford_weights.png", "shares", " ".join(f"{n} {s:.2f}" for n, s in zip(ground.LAYERS, share)))
    if wanted("water"):
        png.write(out / "water" / "river_normal.png", tiles.normals(ground.ripples(), 8.0))
        print("WROTE water/river_normal.png")
    if wanted("sky"):
        save_colour(ground.sky(4096, 2048, SUN), out / "sky" / "brambleford_day.jpg")
        print("WROTE sky/brambleford_day.jpg")
    surfaces = ground.surfaces(layer_files, "terrain/brambleford_weights.png", "water/river_normal.png", "sky/brambleford_day.jpg", places["bounds"])
    (out / "catalog" / "surfaces.json").write_text(json.dumps(surfaces, indent=2) + "\n")
    (out / "catalog" / "look.json").write_text(json.dumps(ground.look(SUN), indent=2) + "\n")
    print("WROTE catalog/surfaces.json catalog/look.json")


main()
