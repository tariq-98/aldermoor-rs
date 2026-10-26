"""Builds the environment kit: cottages, the shed, the windmill, the bridge, trees, rocks,
plants and village props, and writes assets/catalog/environment.json.

    art/blender.sh -P art/blender/build_environment.py [-- --review] [-- --only=cottage]

Props are authored in Blender's frame (up +Z, front -Y, metres), centred on their footprint at
ground level, so the glTF export puts them on +Y up, facing +Z, like every other asset. Sizes
match the `world` crate: a cottage's roof and the timbers you climb are where `world::cottage`
has them (read from art/out/world/places.json, and checked), its plinth fills 7 x 6.2 m at scale
1, the shed 8.8 x 7.4 m, the windmill 5 x 5 m, and the bridge deck follows `Valley::ground`
exactly. aldermoor_art/env.py has the materials, UVs, shading and wind.
"""

import json
import math
import random
import sys
from pathlib import Path

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Vector

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aldermoor_art import env, export, fur, golem, masonry, review, scene as sc, sdf, shapes as sh  # noqa: E402

ARGS = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
ONLY = next((a.split("=", 1)[1].split(",") for a in ARGS if a.startswith("--only=")), None)
REVIEW = sc.ROOT / "art" / "out" / "review" / "environment"
CAT_PATH = sc.OUT / "catalog" / "environment.json"

Z = Vector((0, 0, 1))


def timber(name, centre, size, mat, rot=None):
    ob = env.box(name, centre, size, mat, bevel=0.012, rot=rot)
    env.beam_uvs(ob)
    return ob


def brace(name, a, b, section, mat):
    """A timber from point a to point b (square section)."""
    a, b = Vector(a), Vector(b)
    d = b - a
    rot = Vector((1, 0, 0)).rotation_difference(d.normalized()).to_matrix()
    return timber(name, (a + b) / 2, (d.length, section, section), mat, rot=rot)


# --- Cottages ------------------------------------------------------------------------------------

COTTAGES = {
    "cottage.thatch": dict(label="Thatched cottage", roof="thatch", lower=None, shutters=(0.16, 0.26, 0.12),
                          plaster_tint=(1.0, 0.98, 0.93), timber="planks_dark", chimney=-1, rise=3.0),
    "cottage.tiles": dict(label="Tiled cottage", roof="roof_tiles", lower=None, shutters=(0.08, 0.17, 0.3),
                         plaster_tint=(1.0, 0.95, 0.86), timber="planks_dark", chimney=1, rise=2.5),
    "cottage.stone": dict(label="Stone cottage", roof="roof_tiles", lower="stone", shutters=(0.32, 0.08, 0.06),
                         plaster_tint=(0.98, 0.97, 0.95), timber="planks_dark", chimney=-1, rise=2.6),
}

W, D = 6.6, 5.8          # Wall footprint (the plinth adds 0.2 all round: 7.0 x 6.2).
WALL0, WALL1 = 0.6, 3.4  # Wall bottom and top.
EAVES, VERGE = 0.55, 0.12  # How far the roof runs out past the side walls and past the gable ends.
SINK = 3.0               # How far the plinth goes down, so a house on a slope stands on it.

# The cottages as `world` climbs them (crates/world/src/cottage.rs): the art is built to the same
# measurements, and its roof and the timbers you climb are checked against them.
WORLD_COTTAGES = masonry.places()["cottages"]
assert [WORLD_COTTAGES[k] for k in ("width", "depth", "wall0", "wall1", "eaves", "verge")] == [W, D, WALL0, WALL1, EAVES, VERGE], \
    "world::cottage's measurements changed: update the cottages here to match"


def roof_slab(name, side, rise, overhang, thick, length, mat, round_edge):
    """One roof slope as a prism: from the ridge out over the eave, along the ridge (y)."""
    half = W / 2
    run = half + overhang
    slope = rise / half
    ridge_z = WALL1 + rise
    eave_z = WALL1 - overhang * slope
    top_r, top_e = (0.0, ridge_z + thick * 0.5), (side * run, eave_z + thick * 0.5)
    n = Vector((-(top_e[1] - top_r[1]), 0, top_e[0] - top_r[0])).normalized() * (1 if side > 0 else -1)
    if n.z < 0:
        n = -n
    pts = [Vector((top_r[0], 0, top_r[1])), Vector((top_e[0], 0, top_e[1])),
           Vector((top_e[0], 0, top_e[1])) - n * thick, Vector((top_r[0], 0, top_r[1])) - n * thick]
    ob = env.prism(name, [(p.x, 0.0, p.z) for p in pts], length, (0, 1, 0), mat)
    if round_edge:
        mod = ob.modifiers.new("bevel", "BEVEL")
        mod.width = thick * 0.45
        mod.segments = 4
        mod.limit_method = "ANGLE"
        sc.activate(ob)
        bpy.ops.object.modifier_apply(modifier=mod.name)
    return ob


def window(parts, x, y, z, w, h, facing, timber_mat, shutter_col, box=True, shutters=True, holds=None, sill=None, head=None):
    """A window on the wall facing `facing` (a unit vector out of the wall), with shutters. `sill`
    and `head` name the timbers under and over it that are holds (as `world::cottage` names their
    grips), recorded in `holds`."""
    out = Vector(facing)
    side = Z.cross(out).normalized()
    c = Vector((x, y, z))
    glass = env.flat("glass_dark", (0.05, 0.08, 0.09), rough=0.15)
    parts.append(_oriented_box("pane", c + out * 0.015, w, h, 0.03, out, glass))
    for dx in (-0.5, 0.5):
        parts.append(_oriented_box("jamb", c + side * (dx * (w + 0.12)) + out * 0.05, 0.12, h, 0.1, out, timber_mat, beam=True))
    for dz, grip in ((-0.5, sill), (0.5, head)):
        centre = c + Z * (dz * (h + 0.12)) + out * 0.06
        if grip:
            parts.append(_hold_box("sill", centre, w + 0.36, 0.12, 0.12, out, timber_mat))
            holds.append(_grip(grip, centre, w + 0.36, 0.12, 0.12, out))
        else:
            parts.append(_oriented_box("sill", centre, w + 0.36, 0.12, 0.12, out, timber_mat, beam=True))
    parts.append(_oriented_box("mullion", c + out * 0.045, 0.05, h, 0.05, out, timber_mat, beam=True))
    parts.append(_oriented_box("transom", c + out * 0.045, w, 0.05, 0.05, out, timber_mat, beam=True))
    paint = env.flat(f"paint_{'%.2f_%.2f_%.2f' % shutter_col}", shutter_col, rough=0.7)
    for dx in ((-1, 1) if shutters else ()):
        sc_ = c + side * dx * (w / 2 + 0.12 + w * 0.28) + out * 0.06
        parts.append(_oriented_box("shutter", sc_, w * 0.52, h + 0.1, 0.05, out, paint))
        for dz in (-0.3, 0.0, 0.3):
            parts.append(_oriented_box("batten", sc_ + Z * (dz * h) + out * 0.03, w * 0.5, 0.06, 0.03, out, paint))
    if box:
        bc = c - Z * (h / 2 + 0.2) + out * 0.17
        parts.append(_oriented_box("flowerbox", bc, w + 0.2, 0.2, 0.26, out, timber_mat, beam=True))
        rng = random.Random(int(abs(x * 100 + y * 31)))
        for k in range(7):
            t = (k + 0.5) / 7 - 0.5
            col = rng.choice([(0.75, 0.12, 0.1), (0.85, 0.6, 0.12), (0.7, 0.62, 0.85), (0.9, 0.85, 0.8)])
            leaf = env.flat("leaf_box", (0.12, 0.25, 0.06), rough=0.85)
            p = bc + side * (t * (w + 0.05)) + Z * (0.14 + rng.random() * 0.05) + out * (rng.random() - 0.5) * 0.08
            parts.append(_blob("leaves", p, 0.11, leaf, rng))
            parts.append(_blob("flower", p + Z * 0.08 + out * 0.04, 0.05, env.flat(f"petal_{k % 4}", col, rough=0.7), rng))


def _oriented_box(name, centre, w, h, d, out, mat, beam=False):
    """A box w wide (along the wall), h tall, d deep (out of the wall)."""
    out = Vector(out).normalized()
    side = Z.cross(out).normalized()
    rot = Matrix((side, out, Z)).transposed()
    ob = env.box(name, centre, (w, d, h), mat, bevel=0.008 if min(w, h, d) > 0.03 else 0.0, rot=rot)
    if beam:
        env.beam_uvs(ob)
    return ob


def _hold_box(name, centre, w, h, d, out, mat):
    """A timber you climb by (docs/ART.md, "Reading the world"): a beam w along the wall, h tall and
    d out from it, dark, with a light, rubbed top whose front edge is worn back to a broad chamfer,
    which shows from the ground below as a pale line."""
    out = Vector(out).normalized()
    side = Z.cross(out).normalized()
    c = min(0.07, h * 0.45, d * 0.45)
    centre = Vector(centre)
    profile = [(-d / 2, -h / 2), (d / 2, -h / 2), (d / 2, h / 2 - c), (d / 2 - c, h / 2), (-d / 2, h / 2)]
    ob = env.prism(name, [tuple(centre + out * u + Z * v) for u, v in profile], w, tuple(side), mat)
    _rub(ob)
    mod = ob.modifiers.new("bevel", "BEVEL")
    mod.width = 0.008
    mod.segments = 2
    mod.limit_method = "ANGLE"
    sc.activate(ob)
    bpy.ops.object.modifier_apply(modifier=mod.name)
    env.beam_uvs(ob)
    return ob


def _rub(ob):
    """Gives a timber the light, rubbed top of a hold: its faces turned up are bare, pale wood."""
    ob.data.materials.append(env.flat("timber_worn", (0.56, 0.44, 0.3), rough=0.6))
    for p in ob.data.polygons:
        if p.normal.z > 0.3:
            p.material_index = len(ob.data.materials) - 1


def _grip(part, centre, w, h, d, out):
    """The grip a hold timber is to `world` (`world::Grip`): its top's front edge and the way out
    from the wall, in the game's frame (x across, y up, z toward the door, Blender's -y)."""
    out = Vector(out).normalized()
    side = Z.cross(out).normalized()
    front = Vector(centre) + out * (d / 2) + Z * (h / 2)
    a, b = sorted((front - side * (w / 2), front + side * (w / 2)), key=lambda v: v.x)
    return {"part": part, "a": [a.x, a.z, -a.y], "b": [b.x, b.z, -b.y], "out": [out.x, -out.y]}


def _blob(name, centre, r, mat, rng):
    bm = bmesh.new()
    bmesh.ops.create_icosphere(bm, subdivisions=1, radius=r)
    for v in bm.verts:
        v.co *= 0.8 + 0.4 * rng.random()
        v.co += Vector(centre)
    ob = env.mesh(name, bm, mat, smooth=True)
    return ob


def cottage(cid):
    """A cottage, built to `world::cottage`'s measurements: you climb its gable ends by their timbers
    (dark, with light, rubbed tops), and walk its roof."""
    spec = COTTAGES[cid]
    parts, holds = [], []
    stone = env.surface("stone")[0]
    plaster = env.surface("plaster")[0]
    tmat = env.surface(spec["timber"])[0]
    roof = env.surface(spec["roof"])[0]
    thatch = spec["roof"] == "thatch"
    # Plinth: rubble stone, running down into the ground so a house on a slope stands on it.
    parts.append(env.box("plinth", (0, 0, (WALL0 - SINK) / 2), (W + 0.4, D + 0.4, WALL0 + SINK), stone, bevel=0.04))
    # Walls.
    if spec["lower"]:
        parts.append(env.box("lower", (0, 0, (WALL0 + 2.0) / 2), (W + 0.06, D + 0.06, 2.0 - WALL0), env.surface(spec["lower"])[0], bevel=0.02))
    parts.append(env.box("walls", (0, 0, (WALL0 + WALL1) / 2), (W, D, WALL1 - WALL0), plaster))
    rise = spec["rise"]
    for y in (-(D / 2 - 0.1), D / 2 - 0.1):
        parts.append(env.prism("gable", [(-W / 2, y, WALL1), (W / 2, y, WALL1), (0, y, WALL1 + rise)], 0.2, (0, 1, 0), plaster))
    # Timber frame: sill, corner posts, wall plate, a mid rail and braces on the long sides. The
    # plate across each gable end is a hold, standing out as far as the roof's verge above it.
    s = 0.2
    for y in (-D / 2, D / 2):
        out = (0, 1 if y > 0 else -1, 0)
        parts.append(timber("sill", (0, y, WALL0 + 0.09), (W + 0.12, s, 0.18), tmat))
        plate = (0, y + out[1] * 0.02, WALL1 - 0.09)
        parts.append(_hold_box("plate", plate, W + 0.12, 0.18, s, out, tmat))
        holds.append(_grip("wall plate", plate, W + 0.12, 0.18, s, out))
    for x in (-W / 2, W / 2):
        parts.append(timber("sill", (x, 0, WALL0 + 0.09), (s, D + 0.12, 0.18), tmat))
        # Ending inside the gable ends' plates, behind their worn tops.
        parts.append(timber("plate", (x, 0, WALL1 - 0.09), (s, D, 0.18), tmat))
        parts.append(timber("rail", (x, 0, 2.05), (s, D, 0.14), tmat))
        for y0, y1 in ((-D / 2, -D / 2 + 1.4), (D / 2, D / 2 - 1.4)):
            parts.append(brace("brace", (x, y0, WALL0 + 0.2), (x, y1, 2.0), 0.14, tmat))
        parts.append(timber("post", (x, 0, (WALL0 + WALL1) / 2), (s, 0.18, WALL1 - WALL0), tmat))
    for x in (-W / 2, W / 2):
        for y in (-D / 2, D / 2):
            parts.append(timber("corner", (x, y, (WALL0 + WALL1 - 0.18) / 2), (s + 0.02, s + 0.02, WALL1 - 0.18 - WALL0), tmat))
    # Gable timbers: a collar (a hold, out as far as the verge) and two studs on each end, the
    # gable window between them.
    for y in (-D / 2, D / 2):
        out = (0, 1 if y > 0 else -1, 0)
        collar = (0, y + out[1] * 0.04, WALL1 + rise * 0.48)
        parts.append(_hold_box("collar", collar, W * 0.52, 0.14, 0.16, out, tmat))
        holds.append(_grip("collar", collar, W * 0.52, 0.14, 0.16, out))
        for sx in (-1, 1):
            # Shallow enough to pass behind the collar's worn top.
            top = WALL1 + rise * (1 - 1.05 / (W / 2))
            parts.append(timber("stud", (sx * 1.05, y, (WALL1 + top) / 2), (0.14, 0.1, top - WALL1), tmat))
    # The door, centred on the front (-Y), and its step.
    front = (0, -1, 0)
    door_y = -D / 2
    planks = env.surface("planks")[0]
    parts.append(_oriented_box("door", (0, door_y - 0.03, WALL0 + 1.0), 1.1, 2.0, 0.08, front, planks, beam=True))
    for dx in (-0.62, 0.62):
        parts.append(_oriented_box("doorpost", (dx, door_y - 0.07, WALL0 + 1.1), 0.16, 2.2, 0.14, front, tmat, beam=True))
    lintel = (0, door_y - 0.08, WALL0 + 2.18)
    parts.append(_hold_box("lintel", lintel, 1.5, 0.18, 0.16, front, tmat))
    holds.append(_grip("lintel", lintel, 1.5, 0.18, 0.16, front))
    iron = env.flat("iron", (0.08, 0.08, 0.085), rough=0.5, metal=1.0)
    for dz in (0.45, 1.55):
        parts.append(_oriented_box("hinge", (-0.2, door_y - 0.08, WALL0 + dz), 0.7, 0.05, 0.02, front, iron))
    ring = _blob("handle", (0.4, door_y - 0.1, WALL0 + 1.0), 0.04, iron, random.Random(4))
    parts.append(ring)
    parts.append(env.box("step", (0, -(D / 2 + 0.55), 0.15), (1.6, 0.7, 0.3), stone, bevel=0.03))
    # Windows: two in front, one on each side, one at the back, one in the front gable. The heads
    # of those on the gable ends are holds, and the gable window's sill.
    for x in (-2.05, 2.05):
        window(parts, x, door_y, WALL0 + 1.55, 0.85, 0.95, front, tmat, spec["shutters"], holds=holds, head="window head")
    for x, f in ((-W / 2, (-1, 0, 0)), (W / 2, (1, 0, 0))):
        window(parts, x, 0.75, WALL0 + 1.55, 0.7, 0.9, f, tmat, spec["shutters"], shutters=False)
    window(parts, 0.9, D / 2, WALL0 + 1.55, 0.85, 0.95, (0, 1, 0), tmat, spec["shutters"], box=False, holds=holds, head="window head")
    window(parts, 0, door_y, WALL1 + rise * 0.24, 0.5, 0.55, front, tmat, spec["shutters"], box=False, shutters=False,
           holds=holds, sill="gable window sill", head="gable window head")
    # The roof: out past the side walls over the eaves, and only a little past the gable ends, where
    # the timbers you climb by stand out under the verge.
    thick = 0.42 if thatch else 0.14
    length = D + 2 * VERGE
    for side in (-1, 1):
        parts.append(roof_slab("roof", side, rise, EAVES, thick, length, roof, round_edge=thatch))
    ridge_z = WALL1 + rise
    if thatch:
        bm = bmesh.new()
        bmesh.ops.create_cone(bm, cap_ends=True, segments=14, radius1=0.32, radius2=0.32, depth=length + 0.06)
        bmesh.ops.rotate(bm, verts=bm.verts, cent=(0, 0, 0), matrix=Matrix.Rotation(math.pi / 2, 3, "X"))
        bmesh.ops.translate(bm, verts=bm.verts, vec=(0, 0, ridge_z + thick * 0.35))
        parts.append(env.mesh("ridge", bm, roof, smooth=True))
        peak = ridge_z + thick * 0.35 + 0.32
    else:
        bm = bmesh.new()
        bmesh.ops.create_cone(bm, cap_ends=True, segments=10, radius1=0.13, radius2=0.13, depth=length + 0.04)
        bmesh.ops.rotate(bm, verts=bm.verts, cent=(0, 0, 0), matrix=Matrix.Rotation(math.pi / 2, 3, "X"))
        bmesh.ops.translate(bm, verts=bm.verts, vec=(0, 0, ridge_z + thick * 0.75))
        parts.append(env.mesh("ridge", bm, roof, smooth=True))
        peak = ridge_z + thick * 0.75 + 0.13
        # Bargeboards along the gable edges: the verges you hang from, with the same rubbed tops.
        half = W / 2 + EAVES
        slope = rise / (W / 2)
        for y in (-length / 2 - 0.02, length / 2 + 0.02):
            for sx in (-1, 1):
                a = (0, y, ridge_z + thick + 0.02)
                b = (sx * half, y, WALL1 - EAVES * slope + thick + 0.02)
                barge = brace("barge", a, b, 0.07, tmat)
                _rub(barge)
                parts.append(barge)
    # Chimney through one slope, near the back.
    cx = spec["chimney"] * 1.55
    parts.append(env.box("chimney", (cx, 1.4, (WALL1 + ridge_z + 1.0) / 2 + 0.2), (0.75, 0.75, ridge_z + 1.0 - WALL1 + 0.4), stone, bevel=0.02))
    parts.append(env.box("chimney_cap", (cx, 1.4, ridge_z + 1.45), (0.95, 0.95, 0.14), stone, bevel=0.02))
    soot = env.flat("soot", (0.03, 0.03, 0.03), rough=0.9)
    parts.append(env.box("flue", (cx, 1.4, ridge_z + 1.53), (0.42, 0.42, 0.04), soot))
    # A lantern beside the door.
    glow = env.flat("lamp_glow", (1.0, 0.72, 0.38), rough=0.4, glow=3.0)
    parts.append(_oriented_box("lantern", (0.95, door_y - 0.25, WALL0 + 1.95), 0.18, 0.26, 0.18, front, glow))
    parts.append(_oriented_box("lantern_top", (0.95, door_y - 0.25, WALL0 + 2.12), 0.24, 0.06, 0.24, front, iron))
    parts.append(_oriented_box("lantern_arm", (0.95, door_y - 0.12, WALL0 + 2.2), 0.04, 0.04, 0.26, front, iron))
    _check_against_world(cid, rise, thick, peak, cx, ridge_z + 1.52, holds)
    return parts


def _check_against_world(cid, rise, thick, peak, chimney_x, chimney_top, holds):
    """Stops the build if this cottage isn't the one `world::cottage` climbs: its pitch, roof, ridge
    and chimney, and every timber you climb by, where the world has its grip."""
    world = next(k for k in WORLD_COTTAGES["kinds"] if k["prop"] == cid)
    near = lambda u, v: abs(u - v) < 0.002
    ridge_top = WALL1 + world["rise"] + world["thick"] / 2
    stack = next(b for b in world["blocks"] if b["part"] == "chimney")
    assert near(rise, world["rise"]) and near(thick, world["thick"]), f"{cid}'s roof isn't world::cottage's"
    assert near(peak, ridge_top + world["ridge"][1]), f"{cid}'s ridge peaks at {peak}, world::cottage's at {ridge_top + world['ridge'][1]}"
    assert near(chimney_x, stack["center"][0]) and near(-1.4, stack["center"][1]) and near(chimney_top, stack["top"]), f"{cid}'s chimney isn't world::cottage's"
    key = lambda g: (g["part"], round(g["a"][2], 3), round(g["a"][0], 3))
    ours, theirs = sorted(holds, key=key), sorted(world["grips"], key=key)
    assert len(ours) == len(theirs), f"{cid} has {len(ours)} timbers to climb by, world::cottage {len(theirs)} grips"
    for a, b in zip(ours, theirs):
        same = a["part"] == b["part"] and all(near(u, v) for k in ("a", "b", "out") for u, v in zip(a[k], b[k]))
        assert same, f"{cid}'s {a['part']} is at {a}, world::cottage's grip at {b}"


def finish(parts, name, ground_grime=True, eaves=None, shade=None):
    """UVs for tiled surfaces, shading (`shade`, or grime toward the ground), then one object."""
    for ob in sc.alive(parts):
        mat = ob.material_slots[0].material if ob.material_slots else None
        if mat is not None and "metres" in mat and not ob.data.uv_layers:
            env.world_uvs(ob)
        elif not ob.data.uv_layers:
            ob.data.uv_layers.new(name="UVMap")
    ob = env.join(parts, name)
    env.shade(ob, shade or env.grime(0.0, 0.8, 0.32 if ground_grime else 0.0, eaves))
    return ob


def cottage_shade(co, n):
    """A cottage's vertex shading: darker toward the ground (rain splash and soil, and the damp
    plinth below it where the house stands on a slope), and on the long walls under the eaves. The
    gable ends, under only a narrow verge, stay light, and so do the tops of the timbers you climb."""
    k = 1.0 - 0.32 * min(1.0, max(0.0, 1.0 - co.z / 0.8)) ** 1.5
    if abs(co.x) >= W / 2 - 0.01 and co.z < WALL1:
        k *= 1.0 - 0.25 * max(0.0, 1.0 - (WALL1 - co.z) / 0.8)
    return k, k, k


# --- Shared shapes -------------------------------------------------------------------------------

def tube(name, points, radii, mat, sides=10, smooth=True, cap=True):
    """A tube through points (parallel-transported rings), radii per point."""
    secs = []
    side = None
    pts = [Vector(p) for p in points]
    for i, (pt, r) in enumerate(zip(pts, radii)):
        tan = (pts[min(i + 1, len(pts) - 1)] - pts[max(i - 1, 0)]).normalized()
        if side is None:
            side = tan.cross(Vector((1, 0, 0)))
            if side.length < 1e-3:
                side = tan.cross(Vector((0, 1, 0)))
        side = (side - tan * side.dot(tan)).normalized()
        up = side.cross(tan).normalized()
        secs.append(sh.ring(pt, side, up, r, r, sides))
    bm = sh.loft(secs, cap_start=cap, cap_end=cap)
    return env.mesh(name, bm, mat, smooth=smooth)


def lump(name, centre, radius, mat, seed, scale=(1, 1, 1), rough=0.25, subdiv=3, flat_bottom=None):
    """A noisy sphere: rocks, canopy clumps, bushes."""
    from mathutils.noise import noise as perlin
    rng = random.Random(seed)
    off = Vector((rng.random() * 50, rng.random() * 50, rng.random() * 50))
    bm = bmesh.new()
    bmesh.ops.create_icosphere(bm, subdivisions=subdiv, radius=1.0)
    for v in bm.verts:
        d = v.co.normalized()
        k = 1.0 + rough * perlin(d * 1.6 + off) + rough * 0.45 * perlin(d * 4.0 + off * 1.7)
        p = Vector((d.x * scale[0], d.y * scale[1], d.z * scale[2])) * radius * k
        if flat_bottom is not None and p.z < -flat_bottom * radius:
            p.z = -flat_bottom * radius + (p.z + flat_bottom * radius) * 0.15
        v.co = p + Vector(centre)
    ob = env.mesh(name, bm, mat, smooth=True)
    if mat.get("metres"):
        env.box_uvs(ob)
    return ob


def double_sided(mat):
    mat.use_backface_culling = False
    return mat


# --- The shed --------------------------------------------------------------------------------------

def shed():
    parts = []
    stone = env.surface("stone")[0]
    boards = env.surface("planks")[0]
    dark = env.surface("planks_dark")[0]
    roofm = env.surface("roof_tiles")[0]
    w, d, h, rise = 8.4, 7.0, 3.1, 2.2
    parts.append(env.box("footing", (0, 0, -0.1), (w + 0.4, d + 0.4, 0.6), stone, bevel=0.03))
    # Board walls on three sides; the front (-Y) is open between two posts, under a beam.
    for y in (d / 2 - 0.08,):
        parts.append(timber("back", (0, y, 0.2 + h / 2), (w, 0.16, h), boards))
    for x in (-w / 2 + 0.08, w / 2 - 0.08):
        parts.append(timber("side", (x, 0, 0.2 + h / 2), (0.16, d, h), boards))
        parts.append(env.prism("gable", [(x, -d / 2, 0.2 + h), (x, d / 2, 0.2 + h), (x, 0, 0.2 + h + rise)], 0.16, (1, 0, 0), boards))
    for x in (-w / 2 + 1.2, w / 2 - 1.2):
        parts.append(timber("front_wall", (x * 1.0 + (-0.6 if x < 0 else 0.6), -d / 2 + 0.08, 0.2 + h / 2), (1.2, 0.16, h), boards))
    for x in (-w / 2, -1.6, 1.6, w / 2):
        parts.append(timber("post", (x, -d / 2, 0.2 + h / 2), (0.24, 0.24, h), dark))
    parts.append(timber("beam", (0, -d / 2, 0.2 + h - 0.12), (w + 0.2, 0.26, 0.26), dark))
    parts.append(timber("beam", (0, d / 2, 0.2 + h - 0.12), (w + 0.2, 0.26, 0.26), dark))
    # Roof: ridge along x, slopes front and back.
    over = 0.5
    for side in (-1, 1):
        run = d / 2 + over
        slope = rise / (d / 2)
        top = (0.0, 0.2 + h + rise + 0.07)
        eave = (side * run, 0.2 + h - over * slope + 0.07)
        pts = [(-w / 2 - 0.4, top[0], top[1]), (-w / 2 - 0.4, eave[0], eave[1])]
        ob = env.prism("roof", [(0.0, top[0], top[1]), (0.0, eave[0], eave[1]), (0.0, eave[0], eave[1] - 0.14),
                               (0.0, top[0], top[1] - 0.14)], w + 0.8, (1, 0, 0), roofm)
        parts.append(ob)
    # Inside: hay, a cart wheel against the wall, sacks.
    hay = env.surface("thatch")[0]
    for k, (x, y, s_) in enumerate(((-2.4, 1.8, 1.0), (-1.2, 2.2, 0.8), (2.6, 2.0, 0.9))):
        parts.append(lump("hay", (x, y, 0.35 * s_ + 0.2), 0.75 * s_, hay, 40 + k, scale=(1.2, 0.9, 0.6), rough=0.18, flat_bottom=0.55))
    wheel = tube("wheel", [(1.0 + 0.9 * math.cos(a), 3.25, 0.95 + 0.9 * math.sin(a)) for a in [i * math.pi / 8 for i in range(17)]], [0.06] * 17, dark, sides=6)
    parts.append(wheel)
    for a in range(0, 8):
        ang = a * math.pi / 4
        parts.append(brace("spoke", (1.0, 3.25, 0.95), (1.0 + 0.86 * math.cos(ang), 3.25, 0.95 + 0.86 * math.sin(ang)), 0.05, dark))
    sack = env.flat("sack", (0.42, 0.34, 0.22), rough=0.95)
    for k, x in enumerate((3.0, 3.35)):
        parts.append(lump("sack", (x, -0.6 + k * 0.4, 0.55), 0.35, sack, 60 + k, scale=(0.8, 0.7, 1.0), rough=0.1, flat_bottom=0.6))
    return [finish(parts, "shed", eaves=(0.2 + h, 0.8))]


# --- The windmill ----------------------------------------------------------------------------------

def windmill():
    parts = []
    plaster = env.surface("plaster")[0]
    stone = env.surface("stone")[0]
    roofm = env.surface("thatch")[0]
    dark = env.surface("planks_dark")[0]
    # A tapering tower: stone footing, plastered above.
    prof = [(2.45, -0.3), (2.45, 0.9), (2.3, 1.0), (1.75, 6.0), (1.45, 8.9)]
    bm = sh.lathe([(r, z) for r, z in prof], 20)
    tower = env.mesh("tower", bm, plaster)
    parts.append(tower)
    ring = sh.lathe([(2.55, -0.3), (2.55, 0.95), (2.3, 1.05)], 20)
    parts.append(env.mesh("footing", ring, stone))
    roof = sh.lathe([(1.95, 8.75), (1.85, 9.05), (1.1, 10.2), (0.0, 11.0)], 20)
    parts.append(env.mesh("cap", roof, roofm, smooth=True))
    # The door (front, -Y) and small windows up the tower.
    front = (0, -1, 0)
    planks = env.surface("planks")[0]
    parts.append(_oriented_box("door", (0, -2.3, 1.0 + 0.95), 1.0, 1.9, 0.12, front, planks, beam=True))
    parts.append(_oriented_box("lintel", (0, -2.32, 2.97), 1.4, 0.16, 0.14, front, dark, beam=True))
    for z, ang in ((4.2, 0.6), (6.4, -0.5)):
        r = 2.3 - (2.3 - 1.45) * (z - 1.0) / 7.9
        out = (math.sin(ang), -math.cos(ang), 0)
        window(parts, out[0] * r, out[1] * r, z, 0.45, 0.6, out, dark, (0.2, 0.25, 0.15), box=False, shutters=False)
    # The axle housing where the sails mount.
    parts.append(tube("axle", [(0, -1.2, 7.2), (0, -1.75, 7.2)], [0.28, 0.25], dark, sides=10))
    return [finish(parts, "windmill")]


def sails():
    """Four sails turning about the axle (Blender -Y, glTF +Z), hub at the origin."""
    parts = []
    dark = env.surface("planks_dark")[0]
    cloth = double_sided(env.flat("sailcloth", (0.85, 0.8, 0.68), rough=0.95))
    for i in range(4):
        rot = Matrix.Rotation(i * math.pi / 2, 4, "Y")
        stock = timber("stock", (0, 0, 2.9), (0.14, 0.14, 5.8), dark)
        stock.matrix_world = rot @ stock.matrix_world
        parts.append(stock)
        for z in [1.1 + 0.42 * k for k in range(11)]:
            bar = timber("lath", (0.55, 0.02, z), (1.1, 0.05, 0.05), dark)
            bar.matrix_world = rot @ bar.matrix_world
            parts.append(bar)
        bm = bmesh.new()
        vs = [bm.verts.new(v) for v in ((0.1, 0.06, 1.05), (1.08, 0.06, 1.25), (1.0, 0.06, 5.5), (0.1, 0.06, 5.6))]
        bm.faces.new(vs)
        sail = env.mesh("sail", bm, cloth)
        sail.matrix_world = rot @ sail.matrix_world
        parts.append(sail)
    hub = lump("hub", (0, 0, 0), 0.3, dark, 9, rough=0.02, subdiv=2)
    parts.append(hub)
    ob = finish(parts, "windmill.sails", ground_grime=False)
    return [ob]


# --- The bridge ------------------------------------------------------------------------------------

def deck_z(y):
    """The walkable deck from `world::Valley::ground` (y is along the path, across the river)."""
    return 1.4 + 1.6 * (1.0 - (y / 9.0) ** 2)


def bridge():
    parts = []
    stone = env.surface("stone")[0]
    cobbles = env.surface("cobbles")[0]
    half_w, rail_w, rail_h = 2.3, 0.36, 0.8
    span, crown = 6.2, 1.9
    ys = [-9.4 + 18.8 * i / 64 for i in range(65)]

    def under(y):
        if abs(y) < span:
            return -0.9 + (crown + 0.9) * math.sqrt(max(0.0, 1 - (y / span) ** 2))
        return -2.6

    # The body: a loft along y, between the arch underside and the deck.
    bm = bmesh.new()
    rings = []
    for y in ys:
        top = deck_z(min(abs(y), 9.1) * (1 if y >= 0 else -1))
        b = under(y)
        xo = half_w + rail_w
        rings.append([bm.verts.new(v) for v in ((-xo, y, b), (xo, y, b), (xo, y, top), (-xo, y, top))])
    for a, b in zip(rings, rings[1:]):
        for i in range(4):
            j = (i + 1) % 4
            bm.faces.new((a[i], a[j], b[j], b[i]))
    bm.faces.new(rings[0])
    bm.faces.new(list(reversed(rings[-1])))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    body = env.mesh("bridge", bm, stone)
    # The deck's top faces take cobbles.
    cob = len(body.data.materials)
    body.data.materials.append(cobbles)
    for p in body.data.polygons:
        if p.normal.z > 0.6:
            p.material_index = cob
    parts.append(body)
    # Parapets following the deck, with cap stones.
    for side in (-1, 1):
        bm = bmesh.new()
        rings = []
        for y in ys:
            top = deck_z(min(abs(y), 9.1) * (1 if y >= 0 else -1))
            xi, xo = side * half_w, side * (half_w + rail_w)
            rings.append([bm.verts.new(v) for v in ((xi, y, top - 0.05), (xo, y, top - 0.05), (xo, y, top + rail_h), (xi, y, top + rail_h))])
        for a, b in zip(rings, rings[1:]):
            for i in range(4):
                j = (i + 1) % 4
                bm.faces.new((a[i], a[j], b[j], b[i]))
        bm.faces.new(rings[0])
        bm.faces.new(list(reversed(rings[-1])))
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        parts.append(env.mesh("parapet", bm, stone))
        rng = random.Random(7 + side)
        y = -9.3
        while y < 9.3:
            ln = rng.uniform(0.6, 0.9)
            yc = min(y + ln / 2, 9.3)
            top = deck_z(max(-9.1, min(9.1, yc)))
            slope = math.atan2(deck_z(max(-9.1, min(9.1, yc + 0.05))) - deck_z(max(-9.1, min(9.1, yc - 0.05))), 0.1)
            rot = Matrix.Rotation(-slope, 3, "X") if True else None
            parts.append(env.box("capstone", (side * (half_w + rail_w / 2), yc, top + rail_h + 0.06), (rail_w + 0.1, ln - 0.04, 0.14), stone,
                                 bevel=0.02, rot=Matrix.Rotation(slope, 3, "X")))
            y += ln
    # Voussoirs: a ring of larger stones around the arch on both faces.
    for side in (-1, 1):
        n = 21
        for k in range(n):
            a0 = math.pi * k / n
            a1 = math.pi * (k + 1) / n
            mid = (a0 + a1) / 2
            yi, zi = -span * math.cos(mid), -0.9 + (crown + 0.9) * math.sin(mid)
            out = Vector((0, -math.cos(mid) * (crown + 0.9) / span, math.sin(mid))).normalized()
            centre = Vector((side * (half_w + rail_w + 0.03), yi, zi)) + out * 0.28
            rot = Matrix.Rotation(math.atan2(out.y, out.z), 3, "X").inverted()
            parts.append(env.box("voussoir", centre, (0.12, 0.62, 0.55), stone, bevel=0.03, rot=Matrix.Rotation(-math.atan2(out.y, out.z), 3, "X")))
    return [finish(parts, "bridge.stone", ground_grime=False)]


# --- Trees -----------------------------------------------------------------------------------------

def broadleaf(seed, height=7.4):
    rng = random.Random(seed)
    parts = []
    barkm = env.surface("bark")[0]
    leaves = env.surface("foliage")[0]
    # Trunk with a gentle lean, roots flaring at the base.
    lean = Vector((rng.uniform(-0.25, 0.25), rng.uniform(-0.25, 0.25), 0))
    trunk_pts = [Vector((0, 0, -0.3)), Vector((0, 0, 0.6)), lean * 0.3 + Vector((0, 0, 2.2)), lean * 0.7 + Vector((0, 0, 3.8)),
                 lean + Vector((0, 0, height * 0.72))]
    trunk = tube("trunk", trunk_pts, [0.5, 0.36, 0.3, 0.24, 0.12], barkm, sides=14)
    parts.append(trunk)
    for k in range(6):
        a = k * 2.399 + rng.random()
        r = Vector((math.cos(a), math.sin(a), 0))
        parts.append(tube("root", [Vector((0, 0, 0.55)), r * 0.55 + Vector((0, 0, 0.12)), r * 1.0 + Vector((0, 0, -0.06))], [0.2, 0.13, 0.03], barkm, sides=7))
    crown_c = lean + Vector((0, 0, height * 0.78))
    tips = []
    for k in range(7):
        a = k * 2.399 + rng.random() * 0.5
        y0 = rng.uniform(2.6, 4.2)
        base = lean * (y0 / 4.6) + Vector((0, 0, y0))
        reach = rng.uniform(1.6, 2.6)
        tip = crown_c + Vector((math.cos(a) * reach, math.sin(a) * reach, rng.uniform(-0.8, 1.0)))
        mid = base.lerp(tip, 0.5) + Vector((0, 0, 0.4))
        parts.append(tube("branch", [base, mid, tip], [0.15, 0.09, 0.03], barkm, sides=7))
        tips.append(tip)
    # The crown: a cluster of leafy clumps over each branch tip and round the top.
    centres = tips + [crown_c + Vector((0, 0, 1.0)), crown_c + Vector((0.8, 0.4, 0.3)), crown_c + Vector((-0.7, -0.5, 0.5))]
    radii = [rng.uniform(1.25, 1.8) if k < len(tips) else rng.uniform(1.6, 2.0) for k in range(len(centres))]
    crown = canopy("crown", centres, radii, leaves, seed)
    env.soften_normals(crown, crown_c, 0.6)
    env.shade(crown, lambda co, n: _leaf_shade(co, n, crown_c, height))
    env.wind(crown, lambda co: 0.45 + 0.55 * min(1.0, (co - crown_c).length / 3.2))
    wood = env.join(parts, "wood")
    env.world_uvs(wood)
    env.shade(wood, env.grime(0.0, 0.6, 0.3))
    env.wind(wood, lambda co: 0.25 * max(0.0, co.z / height) ** 2)
    return [env.join([wood, crown], f"tree")]


def canopy(name, centres, radii, mat, seed, step=0.07, target=5200):
    """Foliage as a field: round each centre a core and a ring of smaller clumps, mostly on top,
    joined softly and ruffled, so the silhouette breaks into clumps of leaves."""
    rng = np.random.default_rng(seed)
    parts = []
    for c, R in zip(centres, radii):
        c = np.array(c)
        parts.append(sdf.Ellipsoid(c, np.array((0.8, 0.8, 0.62)) * R))
        for _ in range(8):
            d = rng.normal(size=3) + np.array((0, 0, 0.45))
            d /= np.linalg.norm(d)
            rr = R * rng.uniform(0.34, 0.52)
            parts.append(sdf.Ellipsoid(c + d * R * np.array((0.66, 0.66, 0.5)), np.array((rr, rr, rr * 0.85))))
    field = sdf.Union(parts, k=0.2 * max(radii) / 1.6)
    s1 = int(rng.integers(1, 9999))
    field = sdf.Displace(field, lambda p: 0.07 * (2 * fur.fbm(p, 0.24, s1, octaves=2) - 1), reach=0.08)
    ob = sdf.mesh(field, step, name, relax_iters=2)
    tris = sum(len(p.vertices) - 2 for p in ob.data.polygons)
    if tris > target:
        dec = ob.modifiers.new("decimate", "DECIMATE")
        dec.ratio = target / tris
        sc.activate(ob)
        bpy.ops.object.modifier_apply(modifier=dec.name)
    for p in ob.data.polygons:
        p.use_smooth = True
    ob.data.materials.append(mat)
    env.box_uvs(ob)
    return ob


def _leaf_shade(co, n, centre, height):
    """Lighter on top and outside, darker beneath and within."""
    up = max(0.0, n.z) * 0.25
    out = min(1.0, (co - centre).length / 3.0)
    k = 0.62 + up + 0.25 * out
    return k * 0.95, k, k * 0.85


def pine(seed, height=9.0):
    rng = random.Random(seed)
    barkm = env.surface("bark")[0]
    needles = env.surface("needles")[0]
    trunk = tube("trunk", [(0, 0, -0.3), (0, 0, 1.0), (0, 0, height * 0.5), (0, 0, height * 0.95)], [0.38, 0.3, 0.2, 0.05], barkm, sides=12)
    tiers = []
    n = 6
    for k in range(n):
        t = k / (n - 1)
        z = 2.0 + t * (height - 2.6)
        r = 2.4 * (1 - t) ** 0.9 + 0.45
        bm = bmesh.new()
        segs = 14
        ring_lo = []
        ring_hi = []
        for i in range(segs):
            a = 2 * math.pi * i / segs + rng.random() * 0.2
            rr = r * (0.85 + 0.3 * rng.random())
            ring_lo.append(bm.verts.new((math.cos(a) * rr, math.sin(a) * rr, z - 0.35 * r * (0.8 + 0.4 * rng.random()))))
            ring_hi.append(bm.verts.new((math.cos(a) * rr * 0.55, math.sin(a) * rr * 0.55, z + 0.1)))
        apex = bm.verts.new((0, 0, z + r * 1.05))
        under = bm.verts.new((0, 0, z - 0.2 * r))
        for i in range(segs):
            j = (i + 1) % segs
            bm.faces.new((ring_lo[i], ring_lo[j], ring_hi[j], ring_hi[i]))
            bm.faces.new((ring_hi[i], ring_hi[j], apex))
            bm.faces.new((ring_lo[j], ring_lo[i], under))
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        tiers.append(env.mesh("tier", bm, needles, smooth=True))
    crown = env.join(tiers, "crown")
    env.world_uvs(crown)
    centre = Vector((0, 0, height * 0.55))
    env.soften_normals(crown, centre, 0.5)
    env.shade(crown, lambda co, n: _leaf_shade(co, n, centre, height))
    env.wind(crown, lambda co: 0.3 + 0.7 * min(1.0, co.z / height))
    env.world_uvs(trunk)
    env.shade(trunk, env.grime(0.0, 0.6, 0.3))
    env.wind(trunk, lambda co: 0.2 * max(0.0, co.z / height) ** 2)
    return [env.join([trunk, crown], "tree")]


def bush(seed):
    rng = random.Random(seed)
    leaves = env.surface("foliage")[0]
    centres = [Vector((rng.uniform(-0.4, 0.4), rng.uniform(-0.4, 0.4), 0.45 + rng.uniform(0, 0.25))) for _ in range(4)]
    ob = canopy("bush", centres, [rng.uniform(0.55, 0.8) for _ in centres], leaves, seed, step=0.035, target=2600)
    # Sit on the ground: nothing below it.
    for v in ob.data.vertices:
        v.co.z = max(v.co.z, 0.02)
    c = Vector((0, 0, 0.4))
    env.soften_normals(ob, c, 0.5)
    env.shade(ob, lambda co, n: _leaf_shade(co, n, c, 1.2))
    env.wind(ob, lambda co: min(1.0, co.z / 1.2))
    return [ob]


# --- Rocks and plants -------------------------------------------------------------------------------

def rock(seed, scale=(1.3, 1.0, 0.8), moss=0.0):
    """A boulder sunk into the ground: hewn faces, weathering and cracks (the golem's stones,
    bigger), painted with its own mottled stone, lichen and, if `moss`, moss on top."""
    rx, ry, rz = scale
    cz = rz * 0.42
    field, _ = golem.stone((0, -ry, cz), (0, ry, cz), rx, seed, flat=rz / rx, cuts=5, crack=1.0, lump=0.04, warp=0.07,
                           depth=(0.72, 0.9))
    field = sdf.Intersect(field, sdf.Plane((0, 0, -0.1), (0, 0, -1)))
    ob = sdf.mesh(field, 0.012, "rock", relax_iters=3)
    tris = sum(len(p.vertices) - 2 for p in ob.data.polygons)
    if tris > 3600:
        dec = ob.modifiers.new("decimate", "DECIMATE")
        dec.ratio = 3600 / tris
        sc.activate(ob)
        bpy.ops.object.modifier_apply(modifier=dec.name)
    for p in ob.data.polygons:
        p.use_smooth = True
    ob.data.set_sharp_from_angle(angle=math.radians(40))
    golem.paint(ob, [], env.TEX, f"rock_{seed}", size=1024, seed=seed, base=(0.2, 0.193, 0.18), moss_amount=moss, ground=0.0,
                lichen_amount=0.45, streaks=1.0)
    return [ob]


def grass(seed, blades=9):
    rng = random.Random(seed)
    mat = double_sided(env.flat("grass", (0.36, 0.52, 0.16), rough=0.85))
    bm = bmesh.new()
    col = bm.loops.layers.color.new("Col")
    for b in range(blades):
        a = rng.uniform(0, 2 * math.pi)
        lean = rng.uniform(0.15, 0.45)
        length = rng.uniform(0.35, 0.7)
        d = Vector((math.cos(a), math.sin(a), 0))
        base = d * rng.uniform(0.0, 0.12)
        side = Vector((-d.y, d.x, 0))
        rows = []
        for i in range(5):
            t = i / 4
            w = 0.03 * (1 - t) + 0.002
            c = base + d * (lean * length * t * t) + Vector((0, 0, length * t * (1 - 0.15 * t)))
            rows.append((bm.verts.new(c - side * w), bm.verts.new(c + side * w), t))
        for (a0, b0, t0), (a1, b1, t1) in zip(rows, rows[1:]):
            f = bm.faces.new((a0, b0, b1, a1))
            for loop in f.loops:
                t = t0 if loop.vert in (a0, b0) else t1
                k = 0.45 + 0.65 * t
                loop[col] = (k * 0.95, k, k * 0.8, 1.0)
    ob = env.mesh("grass", bm, mat, smooth=True)
    ob.data.uv_layers.new(name="UVMap")
    env.wind(ob, lambda co: min(1.0, (max(0.0, co.z) / 0.6) ** 1.5))
    return [ob]


def flower(seed, petal):
    """A small clump of flowers: leaves at the base, short stems, broad petals."""
    rng = random.Random(seed)
    stem_m = env.flat("stem", (0.18, 0.34, 0.1), rough=0.85)
    leaf_m = double_sided(env.flat("flower_leaf", (0.16, 0.32, 0.09), rough=0.8))
    petal_m = double_sided(env.flat(f"petal_{seed}", petal, rough=0.6))
    heart = env.flat("flower_heart", (0.85, 0.65, 0.15), rough=0.6)
    parts = []
    for k in range(5):
        a = k * 2.4 + rng.random()
        d = Vector((math.cos(a), math.sin(a), 0))
        s_ = Vector((-d.y, d.x, 0))
        bm = bmesh.new()
        vs = [bm.verts.new(v) for v in (Vector((0, 0, 0.01)), d * 0.09 + s_ * 0.035 + Vector((0, 0, 0.06)), d * 0.17 + Vector((0, 0, 0.05)),
                                        d * 0.09 - s_ * 0.035 + Vector((0, 0, 0.06)))]
        bm.faces.new(vs)
        parts.append(env.mesh("leaf", bm, leaf_m, smooth=True))
    for k in range(4):
        a = rng.uniform(0, 6.28)
        h = rng.uniform(0.2, 0.34)
        base = Vector((math.cos(a) * 0.04, math.sin(a) * 0.04, 0))
        top = base + Vector((math.cos(a) * 0.05, math.sin(a) * 0.05, h))
        parts.append(tube("stem", [base, base.lerp(top, 0.5) + Vector((0.01, 0, 0)), top], [0.01, 0.008, 0.006], stem_m, sides=5))
        tilt = Vector((math.cos(a) * 0.35, math.sin(a) * 0.35, 1)).normalized()
        u = tilt.cross(Vector((0, 0, 1)))
        u = u.normalized() if u.length > 1e-3 else Vector((1, 0, 0))
        v = tilt.cross(u).normalized()
        for j in range(6):
            pa = j * math.pi / 3 + rng.random() * 0.2
            d = u * math.cos(pa) + v * math.sin(pa)
            sd = tilt.cross(d).normalized()
            bm = bmesh.new()
            vs = [bm.verts.new(top + w) for w in (tilt * 0.005, d * 0.04 + sd * 0.022 + tilt * 0.008, d * 0.07 + tilt * 0.004,
                                                   d * 0.04 - sd * 0.022 + tilt * 0.008)]
            bm.faces.new(vs)
            parts.append(env.mesh("petal", bm, petal_m, smooth=True))
        parts.append(lump("heart", top + tilt * 0.012, 0.016, heart, seed + k, rough=0.05, subdiv=1))
    for ob in parts:
        if not ob.data.uv_layers:
            ob.data.uv_layers.new(name="UVMap")
    ob = env.join(parts, "flowers")
    env.shade(ob, lambda co, n: (0.75 + 0.25 * min(1.0, co.z / 0.3),) * 3)
    env.wind(ob, lambda co: min(1.0, max(0.0, co.z) / 0.32))
    return [ob]


def reeds(seed):
    rng = random.Random(seed)
    mat = double_sided(env.flat("reed", (0.42, 0.5, 0.2), rough=0.85))
    head = env.flat("reed_head", (0.3, 0.2, 0.1), rough=0.9)
    parts = []
    for k in range(14):
        a = rng.uniform(0, 6.28)
        base = Vector((math.cos(a), math.sin(a), 0)) * rng.uniform(0, 0.35)
        h = rng.uniform(0.9, 1.6)
        bend = Vector((rng.uniform(-0.15, 0.15), rng.uniform(-0.15, 0.15), 0))
        pts = [base, base + bend * 0.3 + Vector((0, 0, h * 0.5)), base + bend + Vector((0, 0, h))]
        parts.append(tube("reed", pts, [0.012, 0.009, 0.004], mat, sides=4))
        if k % 3 == 0:
            parts.append(tube("head", [pts[2] - Vector((0, 0, 0.22)), pts[2] - Vector((0, 0, 0.04))], [0.025, 0.022], head, sides=6))
    for ob in parts:
        ob.data.uv_layers.new(name="UVMap")
    ob = env.join(parts, "reeds")
    env.shade(ob, lambda co, n: (0.7 + 0.3 * min(1, co.z), 0.7 + 0.3 * min(1, co.z), 0.7 + 0.3 * min(1, co.z)))
    env.wind(ob, lambda co: min(1.0, max(0.0, co.z) / 1.4) ** 1.3)
    return [ob]


# --- Village props --------------------------------------------------------------------------------

def fence():
    """A 6 m run of post-and-rail fence."""
    dark = env.surface("planks")[0]
    parts = []
    for x in (-3.0, 0.0, 3.0):
        parts.append(timber("post", (x, 0, 0.6), (0.16, 0.16, 1.4), dark))
    for z in (0.5, 0.95):
        parts.append(timber("rail", (0, 0, z), (6.2, 0.08, 0.12), dark))
    return [finish(parts, "fence", ground_grime=True)]


def lamp_post():
    dark = env.surface("planks_dark")[0]
    iron = env.flat("iron", (0.08, 0.08, 0.085), rough=0.5, metal=1.0)
    glow = env.flat("lamp_glow", (1.0, 0.72, 0.38), rough=0.4, glow=3.0)
    parts = [timber("post", (0, 0, 1.7), (0.16, 0.16, 3.4), dark), timber("arm", (0.32, 0, 3.25), (0.7, 0.1, 0.1), dark),
             brace("strut", (0.0, 0, 2.85), (0.45, 0, 3.22), 0.07, dark)]
    parts.append(env.box("lamp", (0.6, 0, 2.86), (0.28, 0.28, 0.4), glow))
    parts.append(env.box("lamp_cap", (0.6, 0, 3.1), (0.36, 0.36, 0.08), iron))
    parts.append(env.box("lamp_base", (0.6, 0, 2.64), (0.32, 0.32, 0.05), iron))
    return [finish(parts, "lamp.post")]


def market_stall():
    wood = env.surface("planks")[0]
    parts = []
    for x in (-2.0, 2.0):
        for y in (-1.0, 1.0):
            parts.append(timber("post", (x, y, 1.5), (0.13, 0.13, 3.0 if y > 0 else 2.6), wood))
    parts.append(timber("counter", (0, -0.1, 0.9), (4.3, 2.0, 0.12), wood))
    parts.append(timber("front", (0, -1.02, 0.47), (4.2, 0.08, 0.86), wood))
    stripes = [double_sided(env.flat("awning_a", (0.8, 0.75, 0.6), rough=0.9)), double_sided(env.flat("awning_b", (0.42, 0.52, 0.32), rough=0.9))]
    for i in range(8):
        x = i * 0.55 - 2.2
        bm = bmesh.new()
        vs = [bm.verts.new(v) for v in ((x, -1.35, 2.65), (x, 1.15, 3.05), (x + 0.55, 1.15, 3.05), (x + 0.55, -1.35, 2.65))]
        bm.faces.new(vs)
        parts.append(env.mesh("awning", bm, stripes[i % 2]))
    rng = random.Random(5)
    produce = [env.flat("apple", (0.6, 0.1, 0.06), rough=0.5), env.flat("pear", (0.55, 0.62, 0.2), rough=0.6),
               env.flat("squash", (0.8, 0.45, 0.1), rough=0.6)]
    for i in range(24):
        x = (i % 8) * 0.5 - 1.75
        y = (i // 8) * 0.42 - 0.6
        parts.append(lump("produce", (x + rng.uniform(-0.05, 0.05), y, 1.08), 0.13 + rng.uniform(0, 0.05), produce[(i // 3) % 3], i, rough=0.06, subdiv=2))
    for ob in parts:
        if not ob.data.uv_layers and not ob.material_slots[0].material.get("metres"):
            ob.data.uv_layers.new(name="UVMap")
    return [finish(parts, "market.stall")]


def well():
    stone = env.surface("stone")[0]
    dark = env.surface("planks_dark")[0]
    roofm = env.surface("roof_tiles")[0]
    parts = [env.mesh("ring", sh.lathe([(0.0, -0.2), (0.9, -0.2), (0.9, 0.85), (0.7, 0.85), (0.7, 0.1), (0.0, 0.1)], 20), stone)]
    for x in (-0.85, 0.85):
        parts.append(timber("post", (x, 0, 1.4), (0.14, 0.14, 1.9), dark))
    parts.append(timber("axle", (0, 0, 1.85), (1.9, 0.1, 0.1), dark))
    for side in (-1, 1):
        parts.append(env.prism("roof", [(0, 0, 2.75), (0, side * 0.9, 2.2), (0, side * 0.9, 2.12), (0, 0, 2.66)], 2.2, (1, 0, 0), roofm))
    rope = env.flat("rope", (0.45, 0.38, 0.25), rough=0.9)
    parts.append(tube("rope", [(0, 0, 1.82), (0, 0, 1.0)], [0.015, 0.015], rope, sides=5))
    bucket = env.surface("planks")[0]
    parts.append(env.mesh("bucket", sh.lathe([(0.0, 0.75), (0.16, 0.75), (0.19, 1.0), (0.17, 1.0), (0.14, 0.78), (0.0, 0.78)], 12), bucket))
    return [finish(parts, "well")]


def barrel():
    wood = env.surface("planks")[0]
    iron = env.flat("iron", (0.08, 0.08, 0.085), rough=0.5, metal=1.0)
    prof = [(0.0, 0.0), (0.3, 0.0), (0.36, 0.25), (0.38, 0.45), (0.36, 0.65), (0.3, 0.9), (0.0, 0.9)]
    parts = [env.mesh("barrel", sh.lathe(prof, 16), wood)]
    for z, r in ((0.12, 0.335), (0.78, 0.335), (0.3, 0.37), (0.6, 0.37)):
        parts.append(env.mesh("hoop", sh.lathe([(r, z - 0.025), (r + 0.012, z - 0.02), (r + 0.012, z + 0.02), (r, z + 0.025)], 16), iron))
    return [finish(parts, "barrel")]


def crate():
    wood = env.surface("planks")[0]
    dark = env.surface("planks_dark")[0]
    parts = [env.box("crate", (0, 0, 0.4), (0.8, 0.8, 0.8), wood, bevel=0.01)]
    for x in (-0.38, 0.38):
        for y in (-0.38, 0.38):
            parts.append(timber("edge", (x, y, 0.4), (0.07, 0.07, 0.82), dark))
    for z in (0.03, 0.77):
        for y in (-0.38, 0.38):
            parts.append(timber("edge", (0, y, z), (0.82, 0.07, 0.07), dark))
        for x in (-0.38, 0.38):
            parts.append(timber("edge", (x, 0, z), (0.07, 0.82, 0.07), dark))
    for ob in parts:
        if not ob.data.uv_layers:
            env.beam_uvs(ob)
    return [finish(parts, "crate")]


def hay_bale():
    hay = env.surface("thatch")[0]
    ob = lump("bale", (0, 0, 0.45), 0.6, hay, 3, scale=(1.3, 0.85, 0.75), rough=0.08, subdiv=3, flat_bottom=0.65)
    return [finish([ob], "hay.bale")]


def signpost():
    wood = env.surface("planks")[0]
    parts = [timber("post", (0, 0, 1.2), (0.14, 0.14, 2.4), wood)]
    for z, ang in ((2.0, 0.4), (1.65, -0.5)):
        b = timber("sign", (0.45, 0, z), (0.95, 0.05, 0.22), wood)
        b.matrix_world = Matrix.Rotation(ang, 4, "Z") @ b.matrix_world
        parts.append(b)
    return [finish(parts, "signpost")]


def bench():
    wood = env.surface("planks")[0]
    parts = [timber("seat", (0, 0, 0.45), (1.6, 0.38, 0.06), wood)]
    for x in (-0.65, 0.65):
        parts.append(timber("leg", (x, 0, 0.21), (0.08, 0.34, 0.42), wood))
    parts.append(timber("back", (0, 0.17, 0.75), (1.6, 0.05, 0.22), wood))
    for x in (-0.65, 0.65):
        parts.append(timber("upright", (x, 0.17, 0.6), (0.06, 0.05, 0.38), wood))
    return [finish(parts, "bench")]


def woodpile():
    barkm = env.surface("bark")[0]
    end = env.flat("end_grain", (0.52, 0.38, 0.22), rough=0.8)
    parts = []
    rng = random.Random(8)
    for row in range(3):
        for k in range(6 - row):
            y = (k - (5 - row) / 2) * 0.26
            z = 0.13 + row * 0.23
            log = tube("log", [(-0.55, y, z), (0.55, y, z)], [0.12 + rng.uniform(-0.01, 0.02)] * 2, barkm, sides=8)
            for p in log.data.polygons:
                if abs(p.normal.x) > 0.9:
                    p.material_index = 0
            parts.append(log)
    return [finish(parts, "woodpile")]


# --- Building and the catalogue ------------------------------------------------------------------

PROPS = []


def prop(pid, label, kind, **info):
    def deco(fn):
        PROPS.append(dict(id=pid, label=label, kind=kind, fn=fn, **info))
        return fn
    return deco


# Cottages record the blocks and grips `world` climbs them by, as masonry does, for the same check.
for _cid, _spec in COTTAGES.items():
    _world = next(k for k in WORLD_COTTAGES["kinds"] if k["prop"] == _cid)
    prop(_cid, _spec["label"], "house", footprint=[7.0, 6.2], blocks=_world["blocks"], grips=_world["grips"])(
        lambda cid=_cid: [finish(cottage(cid), cid, shade=cottage_shade)])
prop("shed", "Shed", "landmark", footprint=[8.8, 7.4])(shed)
prop("windmill", "Windmill", "landmark", footprint=[5.0, 5.0],
     parts=[{"file": "environment/windmill.sails.gltf", "at": [0.0, 7.2, 1.9], "spin": {"axis": [0.0, 0.0, 1.0], "rpm": 5.0}}])(windmill)
prop("windmill.sails", "Windmill sails", "part")(sails)
prop("bridge.stone", "Stone bridge", "bridge", footprint=[5.3, 18.8])(bridge)
for _k in range(3):
    prop(f"tree.oak.{_k + 1}", "Oak", "tree", wind=True)(lambda k=_k: broadleaf(11 + k * 7, height=7.0 + k * 0.6))
for _k in range(2):
    prop(f"tree.pine.{_k + 1}", "Pine", "tree", wind=True)(lambda k=_k: pine(5 + k * 3, height=9.0 + k * 1.5))
for _k in range(2):
    prop(f"bush.{_k + 1}", "Bush", "plant", wind=True)(lambda k=_k: bush(21 + k))
for _k, _scale in enumerate(((1.3, 1.0, 0.8), (1.0, 1.1, 0.7), (1.6, 0.9, 0.5), (0.8, 0.7, 0.9))):
    prop(f"rock.{_k + 1}", "Rock", "rock")(lambda k=_k, s=_scale: rock(70 + k, s, moss=0.9 if k % 2 == 0 else 0.25))
for _k in range(3):
    prop(f"grass.{_k + 1}", "Grass", "grass", wind=True)(lambda k=_k: grass(90 + k, blades=7 + 3 * k))
for _k, _col in enumerate(((0.9, 0.88, 0.8), (0.85, 0.3, 0.25), (0.55, 0.45, 0.85), (0.95, 0.75, 0.2))):
    prop(f"flowers.{_k + 1}", "Flowers", "flower", wind=True)(lambda k=_k, c=_col: flower(100 + k, c))
prop("reeds", "Reeds", "plant", wind=True)(lambda: reeds(3))
prop("fence", "Fence", "prop", footprint=[6.2, 0.2])(fence)
prop("lamp.post", "Lamp post", "prop")(lamp_post)
prop("market.stall", "Market stall", "prop", footprint=[4.4, 2.6])(market_stall)
prop("well", "Well", "prop", footprint=[1.9, 1.9])(well)
prop("barrel", "Barrel", "prop")(barrel)
prop("crate", "Crate", "prop")(crate)
prop("hay.bale", "Hay bale", "prop")(hay_bale)
prop("signpost", "Signpost", "prop")(signpost)
prop("bench", "Bench", "prop")(bench)
prop("woodpile", "Woodpile", "prop")(woodpile)

# Masonry stands where the `world` crate puts its solids (crates/world/src/masonry.rs), each piece
# built to fill its blocks; they build their own levels of detail (aldermoor_art/masonry.py).
MASONRY = masonry.pieces()
HEADS = masonry.heads(MASONRY)
for _id, _piece in MASONRY.items():
    _blocks = [{k: b[k] for k in ("part", "center", "half", "yaw", "top", "rise")} for b in _piece["blocks"]]
    _grips = [{k: g[k] for k in ("part", "a", "b", "out")} for g in _piece.get("grips", [])]
    if _id.startswith("wall.field."):
        prop(_id, "Dry-stone wall", "masonry", blocks=_blocks, grips=_grips, levels=True)(
            lambda lod, piece=_piece, pid=_id: masonry.field_wall(piece, HEADS[pid], lod, seed=sum(map(ord, pid))))
    elif _id == "ruin.hall":
        prop(_id, "Ruined hall", "masonry", blocks=_blocks, grips=_grips, levels=True)(lambda lod, piece=_piece: masonry.ruin_hall(piece, lod, seed=7))
    elif _id.startswith("terrace.ledges."):
        prop(_id, "Cliff ledges", "masonry", blocks=_blocks, grips=_grips, levels=True)(
            lambda lod, piece=_piece, pid=_id: masonry.terrace_ledges(piece, lod, seed=sum(map(ord, pid))))


def triangles(objs):
    return sum(sum(len(p.vertices) - 2 for p in o.data.polygons) for o in objs if o.type == "MESH")


def build(p):
    path = env.OUT / f"{p['id']}.gltf"
    if p.get("levels"):
        # Builds each level of detail itself, LOD 0 last so its objects are the ones reviewed.
        for lod in (2, 1, 0):
            sc.reset()
            objs = p["fn"](lod)
            export.rigid(export.lod_path(path, lod) if lod else path, objs, lods=False)
    else:
        sc.reset()
        objs = p["fn"]()
        export.rigid(path, objs)
    pts = [o.matrix_world @ v.co for o in objs for v in o.data.vertices]
    entry = {k: v for k, v in p.items() if k not in ("fn", "levels")}
    entry["file"] = str(path.relative_to(sc.OUT))
    entry["height"] = round(max(q.z for q in pts), 3)
    entry["triangles"] = triangles(objs)
    if "--review" in ARGS:
        REVIEW.mkdir(parents=True, exist_ok=True)
        for yaw in (30, 200) if p["id"] != "bridge.stone" else (90, 35):
            review.render(REVIEW / f"{p['id']}-{yaw}.png", objs, yaw=yaw, size=(900, 700), pad=1.1)
    print("BUILT", p["id"], entry["triangles"], "triangles")
    return entry


def main():
    old = json.loads(CAT_PATH.read_text()) if CAT_PATH.exists() else {"version": 1, "props": []}
    keep = [e for e in old.get("props", []) if ONLY and not any(e["id"].startswith(o) for o in ONLY)]
    built = [build(p) for p in PROPS if not ONLY or any(p["id"].startswith(o) for o in ONLY)]
    props = sorted(keep + built, key=lambda e: e["id"])
    cat = {"version": 1, "props": props, "placements": PLACEMENTS}
    CAT_PATH.write_text(json.dumps(cat, indent=1))
    print("WROTE", CAT_PATH, len(props), "props")


# Which props stand where the `world` crate places things (variants by placement index).
PLACEMENTS = {
    # `world::Valley::houses[i]` and `trees[i]` take the i-th entry, wrapping round.
    "houses": list(COTTAGES),
    "trees": ["tree.oak.1", "tree.oak.2", "tree.oak.3"],
    # Placed at world::BRIDGE_X across the river, at height 0: the deck is `Valley::ground`.
    "bridge": "bridge.stone",
    # Fixed landmarks (x, z on the ground, yaw), matching the obstacles in `world`.
    "landmarks": [{"prop": "windmill", "at": [49.0, -84.0], "yaw": 0.0}, {"prop": "shed", "at": [-26.0, -48.0], "yaw": 0.0}],
    # Decoration the game walks through, from the Swift village layout.
    "dressing": [{"prop": "market.stall", "at": [-4.0, -26.0], "yaw": 0.1}]
    + [{"prop": "fence", "at": [x, -47.0], "yaw": 0.0} for x in (-28.0, -22.0, -16.0, 18.0, 24.0, 30.0, 36.0)]
    + [{"prop": "fence", "at": [-8.0, z], "yaw": 1.5708} for z in (12.0, 6.0, 0.0, -6.0)]
    + [{"prop": "lamp.post", "at": list(p), "yaw": 0.0} for p in ((9.0, 1.0), (-3.0, -24.0), (9.0, -40.0), (9.0, -70.0))]
    + [{"prop": "well", "at": [6.0, -30.0], "yaw": 0.0}, {"prop": "woodpile", "at": [-17.5, -9.5], "yaw": 1.75},
       {"prop": "barrel", "at": [-1.6, -24.6], "yaw": 0.0}, {"prop": "crate", "at": [-6.6, -24.9], "yaw": 0.3},
       {"prop": "hay.bale", "at": [-22.0, -44.0], "yaw": 0.4}, {"prop": "signpost", "at": [4.0, -50.0], "yaw": 0.6},
       {"prop": "bench", "at": [-9.0, -16.5], "yaw": 0.18}],
    # Kinds the renderer scatters itself (nothing collides with them).
    "scatter": {"rocks": ["rock.1", "rock.2", "rock.3", "rock.4"], "grass": ["grass.1", "grass.2", "grass.3"],
                "flowers": ["flowers.1", "flowers.2", "flowers.3", "flowers.4"], "bushes": ["bush.1", "bush.2"],
                "riverbank": ["reeds"], "forest": ["tree.pine.1", "tree.pine.2"]},
}

main()
