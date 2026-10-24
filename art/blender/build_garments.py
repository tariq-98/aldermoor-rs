"""Builds cloth garments for every dressable body type: robes, a tunic, a coat, an apron and hats.

    art/blender.sh -P art/blender/build_garments.py [-- --review]

Needs the humanoid blends (build_humanoid.py, build_races.py, build_creatures.py). Each garment
is fitted per body type over the clothing it may cover (aldermoor_art/garments.py) and added to
assets/catalog/humanoid.json as pieces with a dye mask.
"""

import json
import math
import sys
from pathlib import Path

import bmesh
import bpy
from mathutils import Matrix, Vector

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aldermoor_art import armour as ar, export, fit, garments as gm, regions as rg, review, scene as sc, shapes as sh  # noqa: E402

OUT = sc.OUT / "humanoid"
TEX = OUT / "textures"
CAT_PATH = sc.OUT / "catalog" / "humanoid.json"
ARGS = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []

# Linear RGB colours.
GARMENTS = [
    dict(id="robe.mage", label="Mage's robe", kind="robe", slot="torso_under", layer=1.25, hem="ankle",
         base=(0.035, 0.06, 0.2), trim=(0.55, 0.38, 0.1), bands=[(0.0, 0.035), (0.93, 1.0)], flare=0.55,
         sleeves=dict(t1=1.95, bell=0.9), collar=0.02),
    dict(id="robe.necro", label="Gravecaller's robe", kind="robe", slot="torso_under", layer=1.25, hem="ankle",
         base=(0.03, 0.025, 0.035), trim=(0.22, 0.06, 0.3), bands=[(0.0, 0.025), (0.95, 1.0)], flare=0.45,
         sleeves=dict(t1=1.95, bell=1.1), collar=0.07, tatter=0.05),
    dict(id="robe.healer", label="Sunward vestments", kind="robe", slot="torso_under", layer=1.25, hem="ankle",
         base=(0.6, 0.55, 0.45), trim=(0.6, 0.42, 0.1), bands=[(0.0, 0.03), (0.9, 0.95)], flare=0.4,
         sleeves=dict(t1=1.9, bell=0.5), collar=0.01, tabard=(0.55, 0.38, 0.09)),
    dict(id="tunic.villager", label="Belted tunic", kind="tunic", slot="torso_under", layer=1.15, hem="knee",
         base=(0.2, 0.12, 0.05), trim=(0.45, 0.4, 0.3), bands=[(0.0, 0.04), (0.96, 1.0)], flare=0.25,
         sleeves=dict(t1=1.55, bell=0.15), belt=True),
    dict(id="coat.traveller", label="Traveller's coat", kind="coat", slot="torso", layer=2.05, hem="knee",
         base=(0.12, 0.07, 0.035), trim=(0.06, 0.035, 0.02), bands=[(0.0, 0.03)], flare=0.3, open_front=1.0,
         sleeves=dict(t1=1.92, bell=0.2), collar=0.04),
    dict(id="apron.smith", label="Smith's apron", kind="apron", slot="torso", layer=2.05, hem="knee",
         base=(0.1, 0.055, 0.025), trim=(0.05, 0.03, 0.015), bands=[(0.0, 0.03)]),
]
HATS = [
    dict(id="hat.wizard", label="Wizard's hat", base=(0.035, 0.06, 0.2), trim=(0.55, 0.38, 0.1), kind="wizard"),
    dict(id="hat.straw", label="Straw hat", base=(0.5, 0.38, 0.16), trim=(0.25, 0.08, 0.04), kind="straw"),
]


def clothing(slots):
    return [o for o in bpy.data.objects if o.type == "MESH" and o.get("slot") in slots]


def plate_cloth(ob, thickness=0.003, centre=None):
    return ar.plate(ob, thickness, centre)


def build_garment(spec, skel, body):
    h = gm.heads(skel)
    layer = ar.Layer([body] + clothing(("torso_under", "legs_under", "feet")),
                     ["chest", "belly", "hips", "neck", "thigh_hi", "thigh_lo", "shin_hi", "shin_lo", "foot"])
    bw = gm.BodyWeights(body)
    neck = h["neck_01"]
    hip = (h["thigh_l"].z + h["thigh_r"].z) / 2
    knee = (h["calf_l"].z + h["calf_r"].z) / 2
    ankle = (h["foot_l"].z + h["foot_r"].z) / 2
    z_top = neck.z - 0.005
    z_bottom = {"ankle": ankle + 0.04, "knee": knee - 0.06}[spec["hem"]]
    if spec["kind"] == "apron":
        z_top = h["spine_03"].z + 0.02
    parts = []
    tex_path = TEX / spec["id"].replace(".", "_")
    if not Path(f"{tex_path}.jpg").exists() or True:
        image = gm.cloth_texture(tex_path, 512, spec["base"], spec["trim"], spec["bands"], seed=len(spec["id"]))
    mat = gm.cloth_material(f"cloth.{spec['id']}", image)
    rows = 22
    cols = 40
    clearance = 0.012 if spec["kind"] != "coat" else 0.018
    # Aprons start from a closed ring and keep only its front-facing columns (below).
    open_front = spec.get("open_front", 0.0) if spec["kind"] != "apron" else 0.0
    # Skirts flare from mid-thigh, below where the hands hang.
    thigh_mid = (hip + knee) / 2
    grid, levels, closed = gm.body_grid(layer, h, z_top, z_bottom, rows, cols, clearance,
                                        flare_from=thigh_mid, flare=spec.get("flare", 0.0), open_front=open_front)
    if spec["kind"] == "apron":
        # Only the front panel: the columns facing forwards.
        grid = [[p for p, a in zip(row, [2 * math.pi * i / cols + math.radians(270) for i in range(cols)])
                 if math.cos(a - math.radians(90)) > 0.45] for row in grid]
        closed = False
    if spec.get("tatter"):
        rnd = __import__("random").Random(5)
        for i in range(len(grid[-1])):
            grid[-1][i] = grid[-1][i] + Vector((0, 0, spec["tatter"] * rnd.random()))
            grid[-2][i] = grid[-2][i] + Vector((0, 0, spec["tatter"] * 0.4 * rnd.random()))
    ob, _ = ar.grid_mesh(spec["id"], grid, closed=closed, mat=None)
    # UVs run v from the hem (0) to the collar (1).
    for loop in ob.data.uv_layers.active.data:
        loop.uv = (loop.uv[0], 1 - loop.uv[1])
    ob.data.materials.append(mat)
    plate_cloth(ob, 0.003, centre=gm.torso_centre(layer, gm.spine_at(h, (z_top + hip) / 2)))
    weights = []
    for v in ob.data.vertices:
        p = ob.matrix_world @ v.co
        w = gm.skirt_weights(h, p, hip) if spec["kind"] != "apron" or p.z < hip else None
        weights.append(w if w else bw.at(p))
    gm.assign(ob, weights)
    parts.append(ob)
    # Sleeves.
    sl = spec.get("sleeves")
    if sl:
        for side in (1, -1):
            g = gm.sleeve_grid(ar.Layer([body] + clothing(("torso_under",)), ["upperarm_hi", "upperarm_lo", "forearm_hi", "forearm_lo", "chest"], side=side),
                               h, side, -0.05, sl["t1"], 14, clearance * 0.9, bell=sl.get("bell", 0.0))
            so, _ = ar.grid_mesh(f"{spec['id']}_sleeve", g, closed=True, mat=None)
            so.data.materials.append(mat)
            for loop in so.data.uv_layers.active.data:
                loop.uv = (loop.uv[0], 1 - loop.uv[1])
            plate_cloth(so, 0.003, centre=h[f"lowerarm_{'l' if side > 0 else 'r'}"])
            gm.assign(so, [bw.at(so.matrix_world @ v.co) for v in so.data.vertices])
            parts.append(so)
    # Collar: rings rising round the neck.
    if spec.get("collar"):
        rise = spec["collar"]
        cl = ar.Layer([body], ["neck"])
        g = ar.radial_grid(cl, lambda z: Vector((neck.x, neck.y + 0.005, z)), lambda z: (Vector((1, 0, 0)), Vector((0, -1, 0))),
                           [neck.z - 0.01, neck.z + rise * 0.5, neck.z + rise], [2 * math.pi * i / 32 for i in range(32)], 0.012 + rise * 0.15,
                           smooth=(4, 1), passes=2)
        co, _ = ar.grid_mesh(f"{spec['id']}_collar", g, closed=True, mat=None)
        co.data.materials.append(mat)
        plate_cloth(co, 0.003, centre=neck)
        gm.assign(co, [bw.at(co.matrix_world @ v.co) for v in co.data.vertices])
        parts.append(co)
    # A belt for tunics.
    if spec.get("belt"):
        z = h["spine_01"].z - 0.02
        ring = [gm.torso_centre(layer, gm.spine_at(h, z)) for _ in range(1)][0]
        g = ar.radial_grid(layer, lambda zz: gm.torso_centre(layer, gm.spine_at(h, zz)), lambda zz: (Vector((1, 0, 0)), Vector((0, -1, 0))),
                           [z + 0.022, z - 0.022], [2 * math.pi * i / 40 for i in range(40)], clearance + 0.007, smooth=(4, 0), passes=2)
        bo, _ = ar.grid_mesh(f"{spec['id']}_belt", g, closed=True, mat="leather_dark")
        plate_cloth(bo, 0.004, centre=ring)
        gm.assign(bo, [bw.at(bo.matrix_world @ v.co) for v in bo.data.vertices])
        parts.append(bo)
        # The buckle is built in place: skinned meshes ignore their object's own transform.
        front = (g[0][10] + g[1][10]) / 2
        bmb = sh.lathe([(0.0, -0.016), (0.018, -0.012), (0.018, 0.012), (0.0, 0.016)], 4)
        sh.transform(bmb, Matrix.Translation(front + Vector((0, -0.007, 0))) @ Matrix.Rotation(math.radians(90), 4, "X")
                     @ Matrix.Rotation(math.radians(45), 4, "Z"))
        buckle = sh.obj("buckle", bmb, "brass")
        gm.assign(buckle, [{"spine_01": 1.0}] * len(buckle.data.vertices))
        parts.append(buckle)
    # A tabard over the vestments: front and back panels from the shoulders.
    if spec.get("tabard"):
        tmat = gm.cloth_material(f"cloth.{spec['id']}.tabard", gm.cloth_texture(TEX / f"{spec['id'].replace('.', '_')}_tabard", 256,
                                                                                spec["tabard"], spec["base"], [(0.0, 0.06), (0.94, 1.0)], seed=9))
        from mathutils.bvhtree import BVHTree
        robe = parts[0]
        dg = bpy.context.evaluated_depsgraph_get()
        robe_tree = BVHTree.FromObject(robe, dg)
        for sgn in (-1, 1):
            rows_t = []
            for j in range(24):
                t = j / 23
                z = z_top - 0.02 + (knee - 0.02 - (z_top - 0.02)) * t
                c = gm.torso_centre(layer, gm.spine_at(h, z))
                row = []
                for x in [-0.11 + 0.22 * k / 9 for k in range(10)]:
                    o = Vector((c.x + x, c.y + sgn * 0.6, z))
                    hit = robe_tree.ray_cast(o, Vector((0, -sgn, 0)), 1.0)
                    y = hit[0].y + sgn * 0.012 if hit[0] is not None else c.y + sgn * 0.2
                    row.append(Vector((c.x + x, y, z)))
                rows_t.append(row)
            to, _ = ar.grid_mesh("tabard", rows_t, closed=False, mat=None)
            to.data.materials.append(tmat)
            plate_cloth(to, 0.003, centre=gm.torso_centre(layer, gm.spine_at(h, hip)))
            gm.assign(to, [(gm.skirt_weights(h, to.matrix_world @ v.co, hip) or bw.at(to.matrix_world @ v.co)) for v in to.data.vertices])
            parts.append(to)
    for p in parts:
        sc.bind(p, skel)
        rg.tag(p, skel)
    return parts


def build_hat(spec, skel, body):
    h = gm.heads(skel)
    head = h["Head"]
    pts = [body.matrix_world @ v.co for v in body.data.vertices if (body.matrix_world @ v.co).z > head.z]
    top = max(p.z for p in pts) if pts else head.z + 0.22
    c = Vector((head.x, head.y + 0.005, top - 0.075))
    ring_r = max(((p - c) * Vector((1, 1, 0))).length for p in pts if abs(p.z - c.z) < 0.01) + 0.012 if pts else 0.1
    tex = gm.cloth_texture(TEX / spec["id"].replace(".", "_"), 256, spec["base"], spec["trim"], [(0.1, 0.22)], weave=0.12, seed=3)
    mat = gm.cloth_material(f"cloth.{spec['id']}", tex)
    if spec["kind"] == "wizard":
        prof = [(ring_r * 1.9, -0.005), (ring_r * 1.85, 0.002), (ring_r * 1.02, 0.01), (ring_r * 0.95, 0.05),
                (ring_r * 0.7, 0.15), (ring_r * 0.42, 0.25), (ring_r * 0.15, 0.33), (0.0, 0.37)]
    else:
        prof = [(ring_r * 2.6, -0.02), (ring_r * 2.55, -0.012), (ring_r * 1.05, 0.0), (ring_r * 1.02, 0.06),
                (ring_r * 0.8, 0.085), (0.0, 0.09)]
    bm = sh.lathe(prof, 32)
    sh.transform(bm, Matrix.Translation(c))
    ob = sh.obj(spec["id"], bm, None)
    # A floppy tip for the wizard's hat: bend the top backwards.
    if spec["kind"] == "wizard":
        for v in ob.data.vertices:
            k = max(0.0, (v.co.z - c.z - 0.15) / 0.22)
            v.co.y += 0.09 * k ** 2
            v.co.z -= 0.03 * k ** 2
    uv = ob.data.uv_layers.new(name="UVMap")
    for poly in ob.data.polygons:
        for li in poly.loop_indices:
            co = ob.data.vertices[ob.data.loops[li].vertex_index].co
            uv.data[li].uv = (math.atan2(co.y - c.y, co.x - c.x) / (2 * math.pi) + 0.5, (co.z - c.z + 0.02) / 0.4)
    ob.data.materials.append(mat)
    ar.plate(ob, 0.004, centre=c + Vector((0, 0, 0.03)))
    gm.assign(ob, [{"Head": 1.0}] * len(ob.data.vertices))
    sc.bind(ob, skel)
    rg.tag(ob, skel)
    return [ob]


def main():
    cat = json.loads(CAT_PATH.read_text())
    merged = {}
    only = ARGS[ARGS.index("--only") + 1].split(",") if "--only" in ARGS else None
    for bt, info in cat["body_types"].items():
        if info.get("creature") or (only and bt not in only):
            continue
        for spec in GARMENTS + HATS:
            bpy.ops.wm.open_mainfile(filepath=str(sc.ROOT / "art" / "out" / f"humanoid-{bt}.blend"))
            skel = bpy.data.objects["skeleton"]
            body = bpy.data.objects["body"]
            is_hat = spec in HATS
            parts = build_hat(spec, skel, body) if is_hat else build_garment(spec, skel, body)
            body_regions = rg.read(body)
            cov = fit.coverage(body, body_regions, parts, reach_out=0.09, reach_in=0.01)
            hides = {"body": fit.hides(cov, 0.9)}
            if is_hat:
                hides["body"] = sorted(set(hides["body"]) | {"scalp"}, key=rg.INDEX.get)
            layer = 2.5 if is_hat else spec["layer"]
            for o in bpy.data.objects:
                if o.type == "MESH" and o.get("piece") and o not in parts:
                    q_layer = next((p["layer"] for p in cat["pieces"] if p["id"] == o["piece"]), 9)
                    if q_layer >= layer:
                        continue
                    c2 = fit.coverage(o, rg.read(o), parts, reach_out=0.09, reach_in=0.01)
                    pair = fit.hides(c2, 0.95)
                    if pair:
                        hides.setdefault(o["piece"], [])
                        hides[o["piece"]] = sorted(set(hides[o["piece"]]) | set(pair), key=rg.INDEX.get)
            path = OUT / bt / "clothes" / f"{spec['id']}.gltf"
            export.piece(path, [skel] + parts)
            m = merged.setdefault(spec["id"], {"id": spec["id"], "label": spec["label"], "slot": "head" if is_hat else spec["slot"],
                                               "layer": layer, "files": {}, "hides": {}, "covers_hair": is_hat,
                                               "dye": f"humanoid/textures/{spec['id'].replace('.', '_')}_dye.png", "source": "garments",
                                               # What each dye channel was painted as (linear RGB): a dye recolours it.
                                               "dye_channels": [{"channel": "r", "name": "cloth", "reference": list(spec["base"])},
                                                                {"channel": "g", "name": "trim", "reference": list(spec["trim"])}]})
            m["files"][bt] = str(path.relative_to(sc.OUT))
            m["hides"][bt] = hides
            print("BUILT", bt, spec["id"], sh.triangles(parts), "tris; hides body", hides["body"])
    cat = json.loads(CAT_PATH.read_text())
    if only:
        for old in [p for p in cat["pieces"] if p.get("source") == "garments"]:
            m = merged.get(old["id"])
            if m:
                for k in ("files", "hides"):
                    m[k] = {**old[k], **m[k]}
    cat["pieces"] = [p for p in cat["pieces"] if p.get("source") != "garments"] + list(merged.values())
    CAT_PATH.write_text(json.dumps(cat, indent=1))
    print("WROTE", CAT_PATH, len(merged), "garments")
    if "--review" in ARGS:
        from aldermoor_art import looks
        looks.CAT = looks.catalog()
        files = []
        for bt, pieces, hair, tone, hc in (("male", ["robe.mage", "hat.wizard", "peasant.shoes"], "hair.long", "fair", "white"),
                                           ("female", ["robe.necro", "peasant.shoes"], "hair.long", "porcelain", "black"),
                                           ("elf_female", ["robe.healer", "peasant.shoes"], "hair.buns", "fair", "blonde"),
                                           ("dwarf_male", ["peasant.top", "peasant.trousers", "peasant.shoes", "apron.smith"], "hair.buzzed", "tan", "auburn"),
                                           ("male", ["peasant.top", "peasant.trousers", "ranger.boots", "coat.traveller", "hat.straw"], "hair.simple_parted", "olive", "dark brown"),
                                           ("goblin_female", ["tunic.villager", "peasant.trousers", "peasant.shoes"], "hair.buzzed_soft", "goblin green", "black")):
            for clip, frame in (("Idle_Loop", 20), ("Walk_Loop", 10)):
                look = dict(body=bt, pieces=pieces, hair=hair, brows="brows.fine", beard="beard.dwarven" if bt.startswith("dwarf") else None,
                            tone=tone, hair_colour=hc, clip=clip, frame=frame)
                _, objs = looks.assemble(look)
                f = sc.ROOT / "art" / "out" / "review" / "garments" / f"{bt}-{pieces[0]}-{clip}.png"
                review.render(f, objs, yaw=25 if clip == "Idle_Loop" else 160, size=(520, 760))
                files.append(f)
        review.montage(sc.ROOT / "art" / "out" / "review" / "garments" / "garments.png", files, 4)


main()
