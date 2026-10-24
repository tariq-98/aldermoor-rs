"""Skinned back items built against each body: capes on spring-bone chains, backpacks with straps.

Both are generated from the body type's rest pose and the outer surface of its clothing (the
"envelope"), so they sit on that body with a set clearance:
- A cape's collar copies the body's own skin weights, so it can't come away from the shoulders.
  Its hanging cloth is driven by spring-bone chains rooted on the upper spine.
- A backpack's bag is rigid on the upper spine. Its straps follow the body over the shoulders,
  with the body's weights.
"""

import math

import bmesh
import bpy
from mathutils import Matrix, Vector
from mathutils.bvhtree import BVHTree

from . import scene as sc, shapes as sh

BODY_BONES = ("spine_01", "spine_02", "spine_03", "neck_01", "clavicle_l", "clavicle_r", "upperarm_l", "upperarm_r", "pelvis")


class Envelope:
    """Ray queries against the body plus its clothing, in rest-pose world space."""

    def __init__(self, objects, body):
        dg = bpy.context.evaluated_depsgraph_get()
        verts, polys = [], []
        for ob in objects:
            ev = ob.evaluated_get(dg)
            me = ev.to_mesh()
            base = len(verts)
            verts += [ev.matrix_world @ v.co for v in me.vertices]
            polys += [[base + i for i in p.vertices] for p in me.polygons]
            ev.to_mesh_clear()
        self.tree = BVHTree.FromPolygons(verts, polys)
        self.body = body
        self.body_world = [body.matrix_world @ v.co for v in body.data.vertices]
        self.body_tree = BVHTree.FromPolygons(self.body_world, [list(p.vertices) for p in body.data.polygons])
        self.groups = {g.index: g.name for g in body.vertex_groups}

    def last_hit(self, origin, direction, max_dist=2.0):
        """The furthest surface along a ray (the outside of whatever stacks up)."""
        d = direction.normalized()
        hit = self.tree.ray_cast(origin, d, max_dist)
        last = None
        while hit[0] is not None:
            last = hit
            hit = self.tree.ray_cast(hit[0] + d * 1e-4, d, max_dist)
        return last

    def first_hit(self, origin, direction, max_dist=2.0):
        hit = self.tree.ray_cast(origin, direction.normalized(), max_dist)
        return hit if hit[0] is not None else None

    def back_y(self, x, z):
        """Largest y (the back is +Y) of the envelope at (x, z), or None."""
        hit = self.first_hit(Vector((x, 1.5, z)), Vector((0, -1, 0)))
        return hit[0].y if hit else None

    def weights_at(self, p, allowed=BODY_BONES, k=6):
        """Skin weights of the nearest body vertices, limited to `allowed` bones."""
        loc, _, face, _ = self.body_tree.find_nearest(p)
        if loc is None:
            return {"spine_03": 1.0}
        poly = self.body.data.polygons[face]
        acc = {}
        for vi in poly.vertices:
            w_v = 1.0 / (1e-4 + (self.body_world[vi] - loc).length)
            for g in self.body.data.vertices[vi].groups:
                n = self.groups.get(g.group)
                if n in allowed:
                    acc[n] = acc.get(n, 0.0) + g.weight * w_v
        total = sum(acc.values())
        if total <= 0:
            return {"spine_03": 1.0}
        return {n: w / total for n, w in acc.items()}


def _add_bones(skel, bones):
    """bones: [(name, parent, head, tail)] in armature space."""
    sc.activate(skel, "EDIT")
    ebs = skel.data.edit_bones
    for name, parent, head, tail in bones:
        eb = ebs.get(name) or ebs.new(name)
        eb.head, eb.tail = head, tail
        eb.parent = ebs[parent]
        eb.use_connect = False
        eb.use_deform = True
        eb.roll = 0.0
    bpy.ops.object.mode_set(mode="OBJECT")


def _assign(ob, weights):
    """weights: per-vertex dict bone -> weight."""
    for i, wd in enumerate(weights):
        for bone, w in wd.items():
            if w <= 1e-4:
                continue
            vg = ob.vertex_groups.get(bone) or ob.vertex_groups.new(name=bone)
            vg.add([i], w, "REPLACE")


def _normalise(wd, limit=4):
    top = sorted(wd.items(), key=lambda kv: -kv[1])[:limit]
    s = sum(w for _, w in top)
    return {n: w / s for n, w in top} if s > 0 else {"spine_03": 1.0}


def cape(skel, env, style):
    """A cape for this body: returns (mesh objects, spring chains, bone list).

    style: dict(cloth=material, trim=material, length=hem height above the floor,
                columns, rows, chains, links, clearance)
    """
    h = {b.name: skel.matrix_world @ b.head_local for b in skel.data.bones}
    neck = h["neck_01"]
    shoulder_x = abs(h["upperarm_l"].x) + 0.03
    clearance = style.get("clearance", 0.022)
    hem_z = style["length"]
    NC, NR = style.get("columns", 25), style.get("rows", 34)

    # The collar line runs over the shoulders and round the back of the neck.
    top = []
    for i in range(NC):
        s = i / (NC - 1)
        theta = math.radians(-100 + 200 * s)
        x = neck.x + shoulder_x * math.sin(theta) * (1.0 if abs(theta) < math.pi / 2 else 1.0)
        y = neck.y + 0.075 * math.cos(theta) + 0.02
        hit = env.first_hit(Vector((x, y, neck.z + 0.5)), Vector((0, 0, -1)))
        z = hit[0].z + clearance * 0.6 if hit else neck.z - 0.05
        # The collar sits a little out from whatever it rests on.
        top.append(Vector((x, y + clearance * 0.5 * max(0.0, math.cos(theta)), z)))
    # Rows hang from the collar: each column follows the back down, then falls straight.
    rows = []
    for j in range(NR):
        t = j / (NR - 1)
        row = []
        for i in range(NC):
            s = i / (NC - 1)
            T = top[i]
            # A hem a little longer at the back's centre than at the sides.
            hem = hem_z - style.get("hem_curve", 0.05) * (1 - (2 * s - 1) ** 2)
            z = T.z + (hem - T.z) * (t ** 1.15)
            spread = 1.0 + 0.32 * min(1.0, t * 1.6)
            x = T.x * spread if abs(T.x) > 1e-6 else 0.0
            # Back surface from the collar down to this height; the cloth never comes back in.
            y = T.y
            zz = T.z
            steps = 10
            for k in range(1, steps + 1):
                zz = T.z + (z - T.z) * k / steps
                by = env.back_y(x, zz)
                if by is not None:
                    y = max(y, by + clearance)
            # A gentle outward billow towards the hem, and pleats that deepen as the cloth falls.
            theta = math.radians(-100 + 200 * s)
            y += 0.045 * t ** 1.5 * (math.cos(theta) ** 0.5 if abs(theta) < math.pi / 2 else 0.4)
            folds = style.get("folds", 5)
            depth = 0.016 * min(1.0, t * 1.4) ** 1.2
            y += depth * (0.5 + 0.5 * math.cos(2 * math.pi * folds * s))
            x += 0.006 * min(1.0, t * 1.4) * math.sin(2 * math.pi * folds * s)
            row.append(Vector((x, y, z)))
        rows.append(row)

    # Chains: evenly across the cloth, rooted a little below the collar.
    n_chains = style.get("chains", 5)
    links = style.get("links", 5)
    root_row = max(2, int(NR * 0.12))
    chain_cols = [round((c + 0.5) / n_chains * (NC - 1)) for c in range(n_chains)]
    bones, chains = [], []
    for c, col in enumerate(chain_cols):
        joints = []
        for k in range(links + 1):
            r = root_row + round((NR - 1 - root_row) * k / links)
            joints.append(rows[r][col])
        names = []
        for k in range(links):
            name = f"cape_{c}_{k}"
            parent = "spine_03" if k == 0 else f"cape_{c}_{k - 1}"
            bones.append((name, parent, joints[k], joints[k + 1]))
            names.append(name)
        chains.append(names)
    _add_bones(skel, bones)

    # The cloth as a grid, then given thickness.
    bm = bmesh.new()
    vgrid = [[bm.verts.new(rows[j][i]) for i in range(NC)] for j in range(NR)]
    for j in range(NR - 1):
        for i in range(NC - 1):
            bm.faces.new((vgrid[j][i], vgrid[j][i + 1], vgrid[j + 1][i + 1], vgrid[j + 1][i]))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    uv = bm.loops.layers.uv.new("UVMap")
    grid_pos = {vgrid[j][i]: (i, j) for j in range(NR) for i in range(NC)}
    for f in bm.faces:
        for loop in f.loops:
            i, j = grid_pos[loop.vert]
            loop[uv].uv = (i / (NC - 1), 1 - j / (NR - 1))
    cloth = sh.obj("cape", bm, style["cloth"])
    # Weights per grid vertex: body weights at the collar, chains below, blended in between.
    weights = []
    for j in range(NR):
        for i in range(NC):
            p = rows[j][i]
            body_w = env.weights_at(p)
            # Position among the chain columns.
            ci = 0
            while ci < len(chain_cols) - 1 and i > chain_cols[ci + 1]:
                ci += 1
            if i <= chain_cols[0]:
                pairs = [(0, 1.0)]
            elif i >= chain_cols[-1]:
                pairs = [(len(chain_cols) - 1, 1.0)]
            else:
                a = (i - chain_cols[ci]) / (chain_cols[ci + 1] - chain_cols[ci])
                pairs = [(ci, 1 - a), (ci + 1, a)]
            # Position along the chain.
            if j <= root_row:
                chain_w = {}
                blend = 0.0
            else:
                f = (j - root_row) / (NR - 1 - root_row) * links
                k = min(links - 1, int(f))
                chain_w = {}
                for c, wc in pairs:
                    chain_w[f"cape_{c}_{k}"] = chain_w.get(f"cape_{c}_{k}", 0) + wc
                blend = min(1.0, (j - root_row) / 3.0)
            wd = {n: w * (1 - blend) for n, w in body_w.items()}
            for n, w in chain_w.items():
                wd[n] = wd.get(n, 0) + w * blend
            weights.append(_normalise(wd))
    _assign(cloth, weights)
    mod = cloth.modifiers.new("thick", "SOLIDIFY")
    mod.thickness = 0.005
    mod.offset = 1.0
    mod.use_even_offset = False
    mod.use_quality_normals = True
    sc.activate(cloth)
    bpy.ops.object.modifier_apply(modifier=mod.name)

    # A rolled collar along the top edge, and brooches at the front of the shoulders.
    parts = [cloth]
    path = [p + Vector((0, 0, 0.006)) for p in top]
    roll = sh.obj("cape_collar", sh.sweep(path, lambda t: 0.013 * (0.7 + 0.3 * math.sin(math.pi * t)), n=10, ratio=0.8,
                                          up=Vector((0, 0, 1))), style["trim"])
    _assign(roll, [_normalise(env.weights_at(v.co)) for v in roll.data.vertices])
    parts.append(roll)
    for end in (top[0], top[-1]):
        b = sh.lathe([(0, 0.006), (0.016, 0.004), (0.02, 0.0), (0.017, -0.004), (0, -0.005)], 18)
        n = (end - neck)
        n.z = 0
        n = (n.normalized() + Vector((0, -0.6, 0.6))).normalized()
        sh.transform(b, Matrix.Translation(end + n * 0.012) @ n.to_track_quat("Z", "Y").to_matrix().to_4x4())
        brooch = sh.obj("cape_brooch", b, style.get("metal", "brass"))
        _assign(brooch, [_normalise(env.weights_at(v.co)) for v in brooch.data.vertices])
        parts.append(brooch)
    for p in parts:
        sc.bind(p, skel)
    return parts, chains


def colliders(skel, body, extra=()):
    """Capsules for cloth collision, in each bone's local frame (= the glTF node frame).

    Radii come from the body's own vertices around each segment, plus room for clothing.
    `extra` adds (bone, next bone or None for the bone's own tail, room) segments: a tail.
    """
    specs = [
        ("pelvis", "spine_01", 0.03), ("spine_01", "spine_02", 0.03), ("spine_02", "spine_03", 0.03),
        ("spine_03", "neck_01", 0.025),
        ("thigh_l", "calf_l", 0.02), ("thigh_r", "calf_r", 0.02),
        ("calf_l", "foot_l", 0.015), ("calf_r", "foot_r", 0.015),
        ("upperarm_l", "lowerarm_l", 0.02), ("upperarm_r", "lowerarm_r", 0.02),
    ] + list(extra)
    groups = {g.index: g.name for g in body.vertex_groups}
    world = [body.matrix_world @ v.co for v in body.data.vertices]
    by_bone = {}
    for v, p in zip(body.data.vertices, world):
        if v.groups:
            g = max(v.groups, key=lambda g: g.weight)
            by_bone.setdefault(groups[g.group], []).append(p)
    out = []
    for bone, nxt, extra in specs:
        b = skel.data.bones[bone]
        m = skel.matrix_world @ b.matrix_local
        inv = m.inverted()
        a = skel.matrix_world @ b.head_local
        e = skel.matrix_world @ (skel.data.bones[nxt].head_local if nxt else b.tail_local)
        seg = e - a
        dists = []
        for p in by_bone.get(bone, []):
            t = max(0.0, min(1.0, (p - a).dot(seg) / seg.length_squared))
            dists.append((p - (a + seg * t)).length)
        dists.sort()
        r = (dists[int(len(dists) * 0.6)] if dists else 0.06) + extra
        la, lb = inv @ a, inv @ e
        out.append({"bone": bone, "radius": round(r, 4), "a": [round(c, 4) for c in la], "b": [round(c, 4) for c in lb]})
    return out


def backpack(skel, env, style):
    """A travel pack: a rigid bag on the upper spine, plus straps that follow the shoulders."""
    h = {b.name: skel.matrix_world @ b.head_local for b in skel.data.bones}
    sock = skel.matrix_world @ skel.data.bones["socket_back"].matrix_local
    origin = sock.translation
    clearance = style.get("clearance", 0.02)
    W, H, D = 0.34, 0.42, 0.17
    centre = origin + Vector((0, clearance + D / 2, -0.06))
    parts = []
    body_secs = []
    for k in range(9):
        t = k / 8
        z = centre.z - H / 2 + H * t
        bulge = 1 + 0.08 * math.sin(math.pi * t)
        body_secs.append(sh.ring(Vector((centre.x, centre.y + 0.01 * math.sin(math.pi * t), z)), Vector((1, 0, 0)), Vector((0, 1, 0)),
                                 W / 2 * bulge * (0.92 if t > 0.85 else 1), D / 2 * bulge, 28, 4.0))
    bag = sh.finish(sh.obj("pack_bag", sh.loft(body_secs), style["cloth"]), bevel=0, subdivide=1)
    parts.append(bag)
    # Flap over the top and down the outside.
    top_z = centre.z + H / 2
    flap_path = [Vector((centre.x, centre.y - D / 2 + 0.01, top_z + 0.004)), Vector((centre.x, centre.y, top_z + 0.016)),
                 Vector((centre.x, centre.y + D / 2 + 0.006, top_z - 0.01)), Vector((centre.x, centre.y + D / 2 + 0.012, top_z - 0.08)),
                 Vector((centre.x, centre.y + D / 2 + 0.012, top_z - 0.15))]
    flap_n = [(p - centre).normalized() for p in flap_path]
    parts.append(sh.finish(sh.obj("pack_flap", sh.ribbon(flap_path, flap_n, W * 0.96, 0.007), style["leather"]), bevel=0.002, angle=30))
    # Front pocket on the outside face.
    pocket = []
    for k in range(5):
        t = k / 4
        z = centre.z - H * 0.42 + H * 0.45 * t
        pocket.append(sh.ring(Vector((centre.x, centre.y + D / 2 + 0.02, z)), Vector((1, 0, 0)), Vector((0, 1, 0)),
                              W * 0.32 * (1 + 0.05 * math.sin(math.pi * t)), 0.03, 20, 4.0))
    parts.append(sh.finish(sh.obj("pack_pocket", sh.loft(pocket), style["leather"]), bevel=0))
    # Bedroll strapped on top.
    roll = sh.lathe([(0.0, -W * 0.62), (0.055, -W * 0.6), (0.068, -W * 0.55), (0.068, W * 0.55), (0.055, W * 0.6), (0.0, W * 0.62)], 20)
    sh.transform(roll, Matrix.Translation(centre + Vector((0, 0.0, H / 2 + 0.075))) @ Matrix.Rotation(math.pi / 2, 4, "Y"))
    parts.append(sh.finish(sh.obj("pack_bedroll", roll, style["roll"]), bevel=0))
    for sx in (-0.16, 0.16):
        band = sh.lathe([(0.071, -0.012), (0.073, 0.0), (0.071, 0.012)], 20)
        sh.transform(band, Matrix.Translation(centre + Vector((sx, 0, H / 2 + 0.075))) @ Matrix.Rotation(math.pi / 2, 4, "Y"))
        parts.append(sh.obj("pack_band", band, style["leather"]))
    # A tin cup on the side.
    cup = sh.lathe([(0.0, 0.0), (0.04, 0.0), (0.045, 0.07), (0.043, 0.072), (0.0, 0.065)], 18)
    sh.transform(cup, Matrix.Translation(centre + Vector((W / 2 + 0.02, 0.02, -0.05))) @ Matrix.Rotation(math.pi / 2, 4, "Y"))
    parts.append(sh.obj("pack_cup", cup, "iron"))
    for p in parts:
        _assign(p, [{"spine_03": 1.0}] * len(p.data.vertices))
    # Straps: over each shoulder, down the chest, under the arm and back to the bag's base.
    for side in (1, -1):
        clav = h[f"clavicle_{'l' if side > 0 else 'r'}"]
        arm = h[f"upperarm_{'l' if side > 0 else 'r'}"]
        ctrl = [
            Vector((side * 0.08, centre.y - D / 2, centre.z + H / 2 - 0.03)),
            Vector((side * 0.10, h["neck_01"].y + 0.07, h["neck_01"].z - 0.02)),
            Vector((side * 0.12, (clav.y + arm.y) / 2, clav.z + 0.04)),
            Vector((side * 0.13, clav.y - 0.09, clav.z - 0.07)),
            Vector((side * 0.15, clav.y - 0.10, clav.z - 0.2)),
            Vector((side * 0.18, arm.y - 0.02, clav.z - 0.27)),
            Vector((side * 0.15, centre.y - D / 2 + 0.01, centre.z - H / 2 + 0.05)),
        ]
        path = []
        for a, b in zip(ctrl, ctrl[1:]):
            for k in range(6):
                path.append(a.lerp(b, k / 6))
        path.append(ctrl[-1])
        # Pull every point onto the envelope, just outside it.
        snapped = []
        for p in path:
            loc, nrm, _, d = env.tree.find_nearest(p)
            if loc is not None and d < 0.12:
                q = loc + nrm.normalized() * (clearance * 0.6)
                snapped.append(q if (p - centre).length > 0.12 else p)
            else:
                snapped.append(p)
        # Smooth the path.
        for _ in range(3):
            snapped = [snapped[0]] + [(snapped[i - 1] + snapped[i] * 2 + snapped[i + 1]) / 4 for i in range(1, len(snapped) - 1)] + [snapped[-1]]
        normals = []
        for q in snapped:
            loc, nrm, _, d = env.tree.find_nearest(q)
            normals.append(nrm if loc is not None and d < 0.12 else (q - centre).normalized())
        strap = sh.obj("pack_strap", sh.ribbon(snapped, normals, 0.042, 0.006), style["leather"])
        _assign(strap, [_normalise(env.weights_at(v.co)) for v in strap.data.vertices])
        parts.append(strap)
    for p in parts:
        sc.bind(p, skel)
    return parts


def settle(skel, chains, capsules, iterations=60, stiffness=0.3, particle=0.03):
    """Lets spring-bone chains hang under gravity in the current pose, against capsules.

    A static stand-in for the runtime solver, so review renders show a cape draped rather than
    held rigid. `capsules` are catalogue colliders (bone-local a, b and radius).
    """
    segs = []
    for c in capsules:
        M = skel.matrix_world @ skel.pose.bones[c["bone"]].matrix
        segs.append((M @ Vector(c["a"]), M @ Vector(c["b"]), c["radius"]))

    def push(p):
        for a, b, r in segs:
            ab = b - a
            t = max(0.0, min(1.0, (p - a).dot(ab) / max(ab.length_squared, 1e-9)))
            q = a + ab * t
            d = p - q
            lim = r + particle
            if d.length < lim:
                p = q + (d.normalized() if d.length > 1e-6 else Vector((0, 1, 0))) * lim
        return p

    down = Vector((0, 0, -1))
    for chain in chains:
        for name in chain:
            bpy.context.view_layer.update()
            pb = skel.pose.bones[name]
            M = skel.matrix_world @ pb.matrix
            head = M.translation.copy()
            length = pb.bone.length
            rigid = M @ Vector((0, length, 0))
            p = rigid.copy()
            for _ in range(iterations):
                p = p + down * 0.012
                p = p.lerp(rigid, stiffness * 0.06)
                p = head + (p - head).normalized() * length
                p = push(p)
                p = head + (p - head).normalized() * length
            q = (rigid - head).normalized().rotation_difference((p - head).normalized())
            new = Matrix.Translation(head) @ q.to_matrix().to_4x4() @ Matrix.Translation(-head) @ M
            pb.matrix = skel.matrix_world.inverted() @ new
    bpy.context.view_layer.update()
