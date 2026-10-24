"""Builds the plate armour set for every body type and adds it to assets/catalog/humanoid.json.

    art/blender.sh -P art/blender/build_armour.py [-- --review]

Needs art/out/humanoid-<body type>.blend from build_humanoid.py. Each piece is lofted from the
body's own silhouette over the clothing it may cover (see aldermoor_art/armour.py), so it fits
both frames with clearance to spare.
"""

import json
import math
import sys
from pathlib import Path

import bmesh
import bpy
from mathutils import Matrix, Vector

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aldermoor_art import armour as ar, export, fit, metal, regions as rg, review, scene as sc, shapes as sh  # noqa: E402

OUT = sc.OUT / "humanoid"
REVIEW = sc.ROOT / "art" / "out" / "review" / "armour"
ARGS = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
CAT_PATH = sc.OUT / "catalog" / "humanoid.json"

TORSO = ["chest", "belly", "hips"]
STEEL, TRIM, STRAP = "steel", "brass", "leather_dark"


def heads(skel):
    return {b.name: skel.matrix_world @ b.head_local for b in skel.data.bones}


def clothing(slots):
    return [o for o in bpy.data.objects if o.type == "MESH" and o.get("slot") in slots]


def torso_centre(layer, spine_pt):
    """The middle of the torso's cross-section through a point on the spine."""
    f = layer.outer(spine_pt, Vector((0, -1, 0))) or 0.15
    b = layer.outer(spine_pt, Vector((0, 1, 0))) or 0.05
    l = layer.outer(spine_pt, Vector((1, 0, 0))) or 0.15
    r = layer.outer(spine_pt, Vector((-1, 0, 0))) or 0.15
    return spine_pt + Vector(((l - r) / 2, (b - f) / 2, 0))


def spine_at(h, z):
    chain = [h["pelvis"], h["spine_01"], h["spine_02"], h["spine_03"], h["neck_01"]]
    for a, b in zip(chain, chain[1:]):
        if a.z <= z <= b.z:
            t = (z - a.z) / max(b.z - a.z, 1e-6)
            return a.lerp(b, t)
    return chain[0] if z < chain[0].z else chain[-1]


def cuirass(skel, h):
    """Breastplate and backplate, overlapping at the sides below the arm holes."""
    layer = ar.Layer([bpy.data.objects["body"]] + clothing(("torso_under", "legs_under")), TORSO + ["neck"])
    z0 = h["spine_01"].z - 0.02
    z_arm = h["spine_03"].z + 0.03
    z_top = (h["clavicle_l"].z + h["spine_03"].z) / 2 + 0.035
    N = 14
    levels = [z0 + (z_top - z0) * (i / (N - 1)) ** 0.9 for i in range(N)]
    centres = {lv: torso_centre(layer, spine_at(h, lv)) for lv in levels}
    parts = []
    for name, mid, extra, half_lo, half_hi in (("breastplate", 90, 0.0, 100, 46), ("backplate", 270, -0.003, 96, 52)):
        rows = None
        # Build per-level angle spans: wide around the ribs, narrowing for the arm holes.
        cols = 40
        allrows = []
        for lv in levels:
            k = max(0.0, (lv - z_arm) / max(z_top - z_arm, 1e-6))
            half = half_lo + (half_hi - half_lo) * (0.5 - 0.5 * math.cos(math.pi * k))
            angs = [math.radians(mid - half + 2 * half * i / (cols - 1)) for i in range(cols)]
            grid = ar.radial_grid(layer, lambda _l, c=centres[lv]: c, lambda _l: (Vector((1, 0, 0)), Vector((0, -1, 0))),
                                  [lv], angs, 0.018 + extra + (0.003 if name == "breastplate" else 0), smooth=(4, 0), passes=3)
            allrows.append(grid[0])
        # Smooth between levels while keeping the clearance (rebuild from radii).
        for _ in range(3):
            for j in range(1, len(allrows) - 1):
                for i in range(cols):
                    a, b, c = allrows[j - 1][i], allrows[j][i], allrows[j + 1][i]
                    allrows[j][i] = b.lerp((a + c) / 2, 0.35)
        ob, _ = ar.grid_mesh(name, allrows, closed=False, mat=STEEL)
        ar.plate(ob, 0.0045, centre=centres[levels[len(levels) // 2]])
        parts.append(ob)
        loops = ar.boundary_loops(allrows, closed=False)
        parts += ar.rim_tubes(name, loops, False, 0.005, STEEL)
        # A medial ridge down the breastplate.
        if name == "breastplate":
            mid_col = [row[cols // 2] for row in allrows]
            parts.append(sh.obj("ridge", sh.sweep([p + (p - centres[levels[0]]).normalized() * 0.0 for p in mid_col],
                                                  lambda t: 0.004, n=6, up=Vector((0, -1, 0))), STEEL))
            # Brass rivets along the top edge.
            top = allrows[-1]
            parts += ar.rivets([(top[i] + Vector((0, 0, -0.012)), (top[i] - centres[levels[-1]]).normalized())
                                for i in range(3, cols - 3, 6)], 0.005, TRIM)
    chain = ["pelvis", "spine_01", "spine_02", "spine_03"]
    for p in parts:
        ar.assign(p, lambda q: ar.chain_weights(h, chain, q))
    # Shoulder straps joining the plates.
    for side in (1, -1):
        sname = "l" if side > 0 else "r"
        a = h[f"clavicle_{sname}"] + Vector((side * 0.07, -0.09, 0.0))
        b = h[f"clavicle_{sname}"] + Vector((side * 0.08, 0.0, 0.05))
        c = h[f"clavicle_{sname}"] + Vector((side * 0.07, 0.12, 0.0))
        path = [a.lerp(b, t / 5) for t in range(5)] + [b.lerp(c, t / 5) for t in range(6)]
        strap = sh.obj("strap", sh.ribbon(path, [Vector((0, 0, 1))] * len(path), 0.035, 0.005), STRAP)
        ar.assign(strap, lambda q: {"spine_03": 0.6, f"clavicle_{sname}": 0.4})
        parts.append(strap)
    return parts


def fauld(skel, h):
    """Three overlapping lames below the waist, flaring over the hips."""
    layer = ar.Layer([bpy.data.objects["body"]] + clothing(("torso_under", "legs_under")), ["hips", "belly", "thigh_hi"])
    z0 = h["spine_01"].z - 0.03
    parts = []
    angs = [2 * math.pi * i / 48 for i in range(48)]
    for k in range(3):
        top, bot = z0 - k * 0.055, z0 - k * 0.055 - 0.075
        levels = [top, (top + bot) / 2, bot]
        grid = ar.radial_grid(layer, lambda lv: torso_centre(layer, spine_at(h, lv)), lambda lv: (Vector((1, 0, 0)), Vector((0, -1, 0))),
                              levels, angs, 0.022 + 0.008 * k, smooth=(5, 1), passes=3)
        # Flare the bottom edge.
        c = torso_centre(layer, spine_at(h, bot))
        grid[-1] = [c + (p - c) * 1.06 for p in grid[-1]]
        ob, _ = ar.grid_mesh(f"fauld_{k}", grid, closed=True, mat=STEEL)
        ar.plate(ob, 0.004, centre=c)
        parts.append(ob)
        parts += ar.rim_tubes("fauld", [grid[-1]], True, 0.0045, STEEL)
    for p in parts:
        ar.assign(p, lambda q: {"pelvis": 0.75, "spine_01": 0.25})
    return parts


def pauldron(skel, h, side):
    sname = "l" if side > 0 else "r"
    layer = ar.Layer([bpy.data.objects["body"]] + clothing(("torso_under",)), ["upperarm_hi", "upperarm_lo", "chest"], side=side)
    joint = h[f"upperarm_{sname}"]
    elbow = h[f"lowerarm_{sname}"]
    axis = (elbow - joint).normalized()
    up = Vector((0, 0, 1))
    up = (up - axis * up.dot(axis)).normalized()
    fwd = axis.cross(up) * side
    parts = []
    # The cap: a dome over the shoulder, from above the joint out along the arm.
    cap_c = joint + up * 0.0 - axis * 0.02
    rings = []
    span = [math.radians(a) for a in range(-115, 116, 10)]
    for k, s in enumerate([-0.05, -0.02, 0.01, 0.04, 0.07, 0.1]):
        c = joint + axis * s
        row = []
        for a in span:
            d = (up * math.cos(a) + fwd * math.sin(a)).normalized()
            r = layer.outer(c, d) or 0.06
            row.append(r)
        rings.append((c, row))
    # Turn the raw radii into a smooth dome: one radius per ring, the largest needed, with a
    # rounded profile towards the inboard end.
    pts = []
    rmax = max(max(row) for _, row in rings) + 0.022
    for k, (c, row) in enumerate(rings):
        t = k / (len(rings) - 1)
        prof = 0.72 + 0.28 * math.sin(math.pi * min(1.0, t * 1.25) / 2)
        line = []
        for a, r in zip(span, row):
            rr = max(r + 0.02, rmax * prof)
            d = (up * math.cos(a) + fwd * math.sin(a)).normalized()
            line.append(c + d * rr)
        pts.append(line)
    ob, _ = ar.grid_mesh(f"pauldron_cap_{sname}", pts, closed=False, mat=STEEL)
    ar.plate(ob, 0.004, centre=joint)
    parts.append(ob)
    parts += ar.rim_tubes("pauldron", ar.boundary_loops(pts, False), False, 0.0048, STEEL)
    # Lames down the upper arm.
    for k in range(3):
        s0, s1 = 0.1 + k * 0.045, 0.1 + k * 0.045 + 0.06
        lame = []
        for s in (s0, (s0 + s1) / 2, s1):
            c = joint + axis * s
            line = []
            for a in [math.radians(x) for x in range(-110, 111, 10)]:
                d = (up * math.cos(a) + fwd * math.sin(a)).normalized()
                r = (layer.outer(c, d) or 0.05) + 0.018 + 0.004 * k
                line.append((c, d, r))
            lame.append(line)
        rmx = max(r for line in lame for _, _, r in line)
        grid = [[c + d * max(r, rmx * 0.96) for c, d, r in line] for line in lame]
        ob, _ = ar.grid_mesh(f"pauldron_lame_{sname}_{k}", grid, closed=False, mat=STEEL)
        ar.plate(ob, 0.0035, centre=joint + axis * s0)
        parts.append(ob)
        parts += ar.rim_tubes("lame", [grid[-1]], False, 0.004, STEEL)
    parts += ar.rivets([(pts[2][len(span) // 2] + up * 0.003, up), (pts[3][len(span) // 2] + up * 0.003, up)], 0.006, TRIM)
    for p in parts:
        ar.assign(p, lambda q: {f"upperarm_{sname}": 0.65 if (q - joint).dot(axis) < 0.05 else 1.0,
                                f"clavicle_{sname}": 0.35 if (q - joint).dot(axis) < 0.05 else 0.0})
    return parts


def gauntlet(skel, h, side):
    sname = "l" if side > 0 else "r"
    body = bpy.data.objects["body"]
    parts = []
    # A leather glove: the body's own hand surface, puffed out, with the body's own weights.
    regions = rg.read(body)
    keep = {rg.INDEX["hand"], rg.INDEX["forearm_lo"]}
    glove = body.copy()
    glove.data = body.data.copy()
    bpy.context.scene.collection.objects.link(glove)
    glove.modifiers.clear()
    bm = bmesh.new()
    bm.from_mesh(glove.data)
    bm.faces.ensure_lookup_table()
    wrist = h[f"hand_{sname}"]
    drop = [f for f, r in zip(bm.faces, regions) if r not in keep or (f.calc_center_median().x - 0.0) * side < 0]
    bmesh.ops.delete(bm, geom=drop, context="FACES")
    for v in bm.verts:
        v.co += v.normal * 0.0028
    bm.to_mesh(glove.data)
    bm.free()
    glove.name = f"glove_{sname}"
    glove.data.materials.clear()
    glove.data.materials.append(sh.material(STRAP))
    sc.bind(glove, skel)
    parts.append(glove)
    # The cuff: a flared steel tube from mid-forearm to past the wrist.
    layer = ar.Layer([body] + clothing(("torso_under",)), ["forearm_hi", "forearm_lo", "hand"], side=side)
    elbow = h[f"lowerarm_{sname}"]
    axis = (wrist - elbow).normalized()
    up = (Vector((0, 0, 1)) - axis * axis.z).normalized()
    fwd = axis.cross(up)
    levels = [0.45, 0.6, 0.75, 0.9, 1.02, 1.08]
    angs = [2 * math.pi * i / 32 for i in range(32)]
    grid = ar.radial_grid(layer, lambda t: elbow.lerp(wrist, t), lambda t: (up, fwd), levels, angs, 0.012, smooth=(4, 1), passes=3)
    for j, t in enumerate(levels):
        c = elbow.lerp(wrist, t)
        flare = 1.0 + 0.35 * max(0.0, (0.65 - t) / 0.2) ** 1.5
        grid[j] = [c + (p - c) * flare for p in grid[j]]
    ob, _ = ar.grid_mesh(f"cuff_{sname}", grid, closed=True, mat=STEEL)
    ar.plate(ob, 0.0035, centre=elbow.lerp(wrist, 0.75))
    parts.append(ob)
    parts += ar.rim_tubes("cuff", [grid[0]], True, 0.004, STEEL)
    for p in parts[1:]:
        ar.assign(p, lambda q: {f"lowerarm_{sname}": 1.0})
    # A plate over the back of the hand.
    knuckles = (h[f"index_01_{sname}"] + h[f"pinky_01_{sname}"]) / 2
    hand_axis = (knuckles - wrist).normalized()
    dorsal = Vector((0, 0, 1))
    across = hand_axis.cross(dorsal).normalized()
    hl = ar.Layer([body], ["hand"], side=side)
    rows = []
    for t in (0.05, 0.35, 0.7, 0.95):
        c = wrist.lerp(knuckles, t) - dorsal * 0.01
        row = []
        for a in [math.radians(x) for x in range(-60, 61, 12)]:
            d = (dorsal * math.cos(a) + across * math.sin(a)).normalized()
            r = (hl.outer(c, d, 0.12) or 0.02) + 0.009
            row.append(c + d * r)
        rows.append(row)
    ob, _ = ar.grid_mesh(f"handplate_{sname}", rows, closed=False, mat=STEEL)
    ar.plate(ob, 0.003, centre=wrist.lerp(knuckles, 0.5) - dorsal * 0.02)
    ar.assign(ob, lambda q: {f"hand_{sname}": 1.0})
    parts.append(ob)
    return parts


def legs(skel, h, side):
    sname = "l" if side > 0 else "r"
    body = bpy.data.objects["body"]
    layer = ar.Layer([body] + clothing(("legs_under",)), ["thigh_hi", "thigh_lo", "shin_hi", "shin_lo"], side=side)
    hip, knee, ankle = h[f"thigh_{sname}"], h[f"calf_{sname}"], h[f"foot_{sname}"]
    parts = []
    # Greave: a closed tube down the shin.
    axis = (ankle - knee).normalized()
    fwd = Vector((0, -1, 0))
    fwd = (fwd - axis * fwd.dot(axis)).normalized()
    left = axis.cross(fwd)
    angs = [2 * math.pi * i / 32 for i in range(32)]
    levels = [0.1, 0.25, 0.45, 0.65, 0.82, 0.92]
    grid = ar.radial_grid(layer, lambda t: knee.lerp(ankle, t), lambda t: (fwd, left), levels, angs, 0.014, smooth=(4, 1), passes=3)
    ob, _ = ar.grid_mesh(f"greave_{sname}", grid, closed=True, mat=STEEL)
    ar.plate(ob, 0.004, centre=knee.lerp(ankle, 0.5))
    parts.append(ob)
    parts += ar.rim_tubes("greave", [grid[0], grid[-1]], True, 0.0045, STEEL)
    for p in parts:
        ar.assign(p, lambda q: {f"calf_{sname}": 1.0})
    # Cuisse: the front of the thigh.
    axis_t = (knee - hip).normalized()
    fwd_t = Vector((0, -1, 0))
    fwd_t = (fwd_t - axis_t * fwd_t.dot(axis_t)).normalized()
    left_t = axis_t.cross(fwd_t)
    angs_t = [math.radians(a) for a in range(-100, 101, 10)]
    grid = ar.radial_grid(layer, lambda t: hip.lerp(knee, t), lambda t: (fwd_t, left_t), [0.22, 0.4, 0.6, 0.8, 0.93], angs_t,
                          0.016, smooth=(3, 1), passes=3)
    ob, _ = ar.grid_mesh(f"cuisse_{sname}", grid, closed=False, mat=STEEL)
    ar.plate(ob, 0.004, centre=hip.lerp(knee, 0.6))
    ar.assign(ob, lambda q: {f"thigh_{sname}": 1.0})
    parts.append(ob)
    rims = ar.rim_tubes("cuisse", ar.boundary_loops(grid, False), False, 0.0045, STEEL)
    for r in rims:
        ar.assign(r, lambda q: {f"thigh_{sname}": 1.0})
    parts += rims
    # Poleyn: a cop over the front of the knee.
    kc = knee
    front = [layer.outer(kc, (fwd * math.cos(math.radians(a)) + left * math.sin(math.radians(a))).normalized(), 0.15) for a in range(-50, 51, 10)]
    front = sorted(x for x in front if x)
    r_knee = (front[len(front) // 2] if front else 0.06) + 0.024
    elevs = [math.radians(x) for x in (-50, -30, -10, 10, 30, 50)]
    azims = [math.radians(x) for x in range(-75, 76, 10)]
    pts = []
    for e in elevs:
        line = []
        for a in azims:
            d = (fwd * math.cos(e) * math.cos(a) + left * math.cos(e) * math.sin(a) - axis * math.sin(e)).normalized()
            hit = layer.outer(kc, d, 0.15)
            r = max(r_knee * (1 - 0.12 * abs(math.sin(e))), (hit or 0) + 0.012)
            line.append(kc + d * min(r, r_knee * 1.2))
        pts.append(line)
    ob, _ = ar.grid_mesh(f"poleyn_{sname}", pts, closed=False, mat=STEEL)
    ar.plate(ob, 0.004, centre=kc)
    ar.assign(ob, lambda q: {f"calf_{sname}": 0.5, f"thigh_{sname}": 0.5})
    parts.append(ob)
    parts += ar.rivets([(pts[3][len(pts[3]) // 2] + fwd * 0.003, fwd)], 0.007, TRIM)
    ar.assign(parts[-1], lambda q: {f"calf_{sname}": 0.5, f"thigh_{sname}": 0.5})
    return parts


def helmet(skel, h, style):
    body = bpy.data.objects["body"]
    # Fitted to the skull, not the ears: helmets hide the ears, and foxfolk's stand tall.
    layer = ar.Layer([body], ["scalp", "face"])
    head = h["Head"]
    regions = rg.read(body)
    skull = {i for p, r in zip(body.data.polygons, regions) if r in (rg.INDEX["scalp"], rg.INDEX["face"]) for i in p.vertices}
    top_z = max((body.matrix_world @ body.data.vertices[i].co).z for i in skull)
    c = head + Vector((0, 0.005, (top_z - head.z) * 0.45))
    cols = 48
    rows = []
    elev = [math.radians(x) for x in (-30, -15, 0, 15, 30, 45, 60, 72, 82)]
    for j, e in enumerate(elev):
        row = []
        for i in range(cols):
            a = 2 * math.pi * i / cols
            d = Vector((math.cos(e) * math.cos(a), math.cos(e) * math.sin(a), math.sin(e)))
            r = (layer.outer(c, d, 0.3) or 0.1) + 0.016
            row.append(r)
        rows.append(row)
    # A clean dome: the largest radius at each elevation, eased between rings.
    radii = [max(r) * 0.94 + sum(r) / len(r) * 0.06 for r in rows]
    for k in range(len(rows)):
        rows[k] = [max(x, radii[k] * 0.97) for x in rows[k]]
        rows[k] = ar._smooth_ring(rows[k], 6)
    if style == "norman":
        # A conical crown.
        for k, e in enumerate(elev):
            if e > math.radians(40):
                rows[k] = [x * (1 + 0.08 * (math.degrees(e) - 40) / 42) for x in rows[k]]
    pts = []
    for e, row in zip(elev, rows):
        line = []
        for i, r in enumerate(row):
            a = 2 * math.pi * i / cols
            d = Vector((math.cos(e) * math.cos(a), math.cos(e) * math.sin(a), math.sin(e)))
            # The rim rises over the face (front is -Y, so angle 270 degrees).
            front = max(0.0, -math.sin(a)) ** 2
            lift = front * math.radians(32)
            line.append((e, a, d, r, lift))
        pts.append(line)
    # Re-sample each column from its own lower limit so the face opening has a smooth edge.
    grid = []
    for j in range(len(elev)):
        row = []
        for i in range(cols):
            e_min = elev[0] + pts[0][i][4]
            e = e_min + (elev[-1] - e_min) * j / (len(elev) - 1)
            k = min(len(elev) - 2, max(0, int((e - elev[0]) / (elev[1] - elev[0]))))
            t = (e - elev[k]) / (elev[k + 1] - elev[k])
            r = rows[k][i] * (1 - t) + rows[k + 1][i] * t
            a = 2 * math.pi * i / cols
            d = Vector((math.cos(e) * math.cos(a), math.cos(e) * math.sin(a), math.sin(e)))
            row.append(c + d * r)
        grid.append(row)
    apex = c + Vector((0, 0, max(rows[-1]) * (1.12 if style == "norman" else 1.02)))
    ob, _ = ar.grid_mesh(f"helm_{style}", grid, closed=True, mat=STEEL)
    bm = bmesh.new()
    bm.from_mesh(ob.data)
    bm.verts.ensure_lookup_table()
    top = [v for v in bm.verts if v.index >= len(bm.verts) - cols]
    pole = bm.verts.new(apex)
    for i in range(cols):
        bm.faces.new((top[i], top[(i + 1) % cols], pole))
    bm.to_mesh(ob.data)
    bm.free()
    ar.plate(ob, 0.004, centre=c)
    parts = [ob]
    parts += ar.rim_tubes("helm", [grid[0]], True, 0.006, TRIM if style == "kettle" else STEEL)
    if style == "norman":
        # A nasal guard down from the brow.
        i_front = int(cols * 0.75)
        brow = grid[0][i_front]
        nose = brow + Vector((0, -0.006, -0.075))
        parts.append(sh.obj("nasal", sh.ribbon([brow.lerp(nose, t / 6) + Vector((0, -0.004 * math.sin(math.pi * t / 6), 0)) for t in range(7)],
                                               [Vector((0, -1, 0.15))] * 7, 0.022, 0.004), STEEL))
        # A brow band with rivets.
        band = [p + (p - c).normalized() * 0.002 for p in grid[1]]
        parts.append(sh.obj("band", sh.sweep(band + [band[0]], lambda t: 0.0055, n=6, ratio=0.5, up=Vector((0, 0, 1))), TRIM))
    else:
        # A kettle hat's brim, sloping down and out.
        ring = grid[0]
        outer = [c + (p - c) * 1.0 + ((p - c) * Vector((1, 1, 0))).normalized() * 0.07 + Vector((0, 0, -0.03)) for p in ring]
        brim_rows = [ring, [a.lerp(b, 0.5) + Vector((0, 0, 0.004)) for a, b in zip(ring, outer)], outer]
        ob2, _ = ar.grid_mesh("brim", brim_rows, closed=True, mat=STEEL)
        ar.plate(ob2, 0.0035, centre=c + Vector((0, 0, 0.2)))
        parts.append(ob2)
        parts += ar.rim_tubes("brim", [outer], True, 0.0045, STEEL)
    for p in parts:
        ar.assign(p, lambda q: {"Head": 1.0})
    return parts


PIECES = [
    # id, label, slot, layer, builder
    ("plate.cuirass", "Plate cuirass", "torso", 2.0, lambda s, h: cuirass(s, h) + fauld(s, h)),
    ("plate.pauldrons", "Plate pauldrons", "shoulders", 3.0, lambda s, h: pauldron(s, h, 1) + pauldron(s, h, -1)),
    ("plate.gauntlets", "Plate gauntlets", "hands", 2.2, lambda s, h: gauntlet(s, h, 1) + gauntlet(s, h, -1)),
    ("plate.legs", "Plate legs", "legs", 2.1, lambda s, h: legs(s, h, 1) + legs(s, h, -1)),
    ("plate.helm_norman", "Nasal helm", "head", 2.5, lambda s, h: helmet(s, h, "norman")),
    ("plate.helm_kettle", "Kettle hat", "head", 2.5, lambda s, h: helmet(s, h, "kettle")),
]
DECLARED = {
    "plate.gauntlets": ["forearm_lo", "hand"],
    "plate.helm_norman": ["scalp", "ears"],
    "plate.helm_kettle": ["scalp", "ears"],
    "plate.legs": ["shin_hi", "shin_lo"],
}


def build(bt, cat):
    entries = []
    bpy.ops.wm.open_mainfile(filepath=str(sc.ROOT / "art" / "out" / f"humanoid-{bt}.blend"))
    skel = bpy.data.objects["skeleton"]
    body = bpy.data.objects["body"]
    h = heads(skel)
    body_regions = rg.read(body)
    lower = {}
    for o in bpy.data.objects:
        if o.type == "MESH" and o.get("piece"):
            lower.setdefault(o["piece"], []).append(o)
    built = {}
    clothes = [o for ms in lower.values() for o in ms]
    for pid, label, slot, layer, fn in PIECES:
        parts = fn(skel, h)
        for p in parts:
            if p.parent is None:
                sc.bind(p, skel)
            rg.tag(p, skel)
        # Hammered steel, brass and leather instead of flat colours, with occlusion from the
        # body, the clothes underneath and the plate itself.
        metal.work(parts, [body] + clothes)
        built[pid] = parts
        cov = fit.coverage(body, body_regions, parts, reach_out=0.08)
        hides = {"body": sorted(set(fit.hides(cov)) | set(DECLARED.get(pid, [])), key=rg.INDEX.get)}
        layers = {p["id"]: p["layer"] for p in cat["pieces"]}
        for qid, qmeshes in lower.items():
            if layers.get(qid, 9) >= layer:
                continue
            pair = []
            for qm in qmeshes:
                c2 = fit.coverage(qm, rg.read(qm), parts, reach_out=0.07, reach_in=0.01)
                pair += fit.hides(c2, 0.97)
            if slot == "hands" and qid.endswith(".top"):
                pair += ["forearm_lo", "hand"]
            if slot == "legs" and (qid.endswith(".boots") or qid.endswith(".shoes")):
                pair += ["shin_hi", "shin_lo"]
            pair = sorted(set(pair), key=rg.INDEX.get)
            if pair:
                hides[qid] = pair
        path = OUT / bt / "armour" / f"{pid}.gltf"
        export.piece(path, [skel] + parts)
        entries.append({"id": pid, "label": label, "slot": slot, "layer": layer, "file": str(path.relative_to(sc.OUT)),
                        "hides": hides, "covers_hair": slot == "head", "triangles": sh.triangles(parts),
                        "coverage": {k: round(v, 3) for k, v in cov.items() if v > 0.05}})
        print("BUILT", bt, pid, sh.triangles(parts), "tris; hides body", hides["body"])
    return entries


def review_set(bt):
    """Renders the plate set over the Ranger outfit, assembled the way the game does."""
    import importlib
    from aldermoor_art import looks
    importlib.reload(looks)
    REVIEW.mkdir(parents=True, exist_ok=True)
    files = []
    for clip, frame in (("Idle_Loop", 20), ("Sword_Idle", 12), ("Walk_Loop", 10), ("Sword_Regular_A", 7)):
        look = dict(body=bt, pieces=["ranger.top", "ranger.trousers", "ranger.boots", "plate.cuirass", "plate.pauldrons",
                                     "plate.gauntlets", "plate.legs", "plate.helm_norman"],
                    hair="hair.simple_parted", brows="brows.strong", tone="warm", hair_colour="chestnut", clip=clip, frame=frame)
        _, objs = looks.assemble(look)
        for yaw in (25, 160):
            f = REVIEW / f"{bt}-{clip}-{yaw}.png"
            review.render(f, objs, yaw=yaw, size=(560, 760))
            files.append(f)
    review.montage(REVIEW / f"{bt}-plate.png", files, 4)


def main():
    cat = json.loads(CAT_PATH.read_text())
    # `--only=foxfolk_male,foxfolk_female` refits just those body types.
    only = next((a.split("=", 1)[1].split(",") for a in ARGS if a.startswith("--only=")), None)
    merged = {}
    if only:
        for p in cat["pieces"]:
            if p.get("source") == "armour":
                merged[p["id"]] = {k: v for k, v in p.items() if k not in ("files", "hides", "coverage")} | {
                    "files": {k: v for k, v in p["files"].items() if k not in only},
                    "hides": {k: v for k, v in p["hides"].items() if k not in only},
                    "coverage": {k: v for k, v in p.get("coverage", {}).items() if k not in only}}
    for bt, info in cat["body_types"].items():
        if info.get("dress") is False or (only and bt not in only):
            continue  # Golems and other creatures that wear nothing.
        ents = build(bt, cat)
        for e in ents:
            m = merged.setdefault(e["id"], {k: v for k, v in e.items() if k not in ("file", "hides", "coverage", "triangles")}
                                  | {"files": {}, "hides": {}, "coverage": {}, "source": "armour"})
            m["files"][bt] = e["file"]
            m["hides"][bt] = e["hides"]
            m["coverage"][bt] = e["coverage"]
    cat = json.loads(CAT_PATH.read_text())
    cat["pieces"] = [p for p in cat["pieces"] if p.get("source") != "armour"] + list(merged.values())
    CAT_PATH.write_text(json.dumps(cat, indent=1))
    print("WROTE", CAT_PATH, len(merged), "armour pieces")
    if "--review" in ARGS:
        for bt in cat["body_types"]:
            review_set(bt)


main()
