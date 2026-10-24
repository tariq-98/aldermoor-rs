"""Races as proportions: per-bone scaling of the rest pose, applied to the body and everything on it.

Each bone gets a length scale (along the bone) and a thickness scale (across it); heads, hands
and feet scale uniformly. A vertex moves by the blend of its bones' affine transforms (the same
linear blend skinning the renderer does), so skin and clothing move together and keep their fit.
Joint positions follow the parent bone. Bone orientations never change, so every animation in
the shared library still plays.
"""

import bpy
import numpy as np
from mathutils import Matrix, Vector

from . import scene as sc

# (length, thickness) per group; "uniform" groups take one factor.
RACES = {
    "elf": {
        "label": "Elf",
        "legs": (1.08, 0.92), "spine": (1.05, 0.93), "neck": (1.14, 0.9), "clavicle": (0.97, 0.95),
        "upperarm": (1.05, 0.9), "lowerarm": (1.06, 0.9), "hands": 0.98, "feet": 0.97, "head": 0.97,
        "ears": "elf", "gear_scale": 1.0,
    },
    "dwarf": {
        "label": "Dwarf",
        "legs": (0.7, 1.2), "spine": (0.9, 1.2), "neck": (0.75, 1.2), "clavicle": (1.12, 1.15),
        "upperarm": (0.86, 1.22), "lowerarm": (0.86, 1.22), "hands": 1.12, "feet": 1.12, "head": 1.07,
        "ears": None, "gear_scale": 0.92,
    },
    "foxfolk": {
        "label": "Foxfolk",
        "legs": (0.98, 0.9), "spine": (0.97, 0.9), "neck": (1.12, 0.86), "clavicle": (0.95, 0.9),
        "upperarm": (0.98, 0.88), "lowerarm": (1.0, 0.86), "hands": 0.95, "feet": 0.98, "head": 1.0,
        "ears": None, "fox": True, "gear_scale": 0.97,
    },
    "goblin": {
        "label": "Goblin",
        "legs": (0.6, 0.86), "spine": (0.78, 0.9), "neck": (0.8, 0.85), "clavicle": (0.85, 0.85),
        "upperarm": (0.92, 0.82), "lowerarm": (1.0, 0.82), "hands": 1.08, "feet": 1.1, "head": 1.22,
        "ears": "goblin", "nose": 1.45, "npc_only": True, "gear_scale": 0.8,
    },
}

GROUPS = {
    "legs": ("thigh_", "calf_"),
    "spine": ("pelvis", "spine_"),
    "neck": ("neck_",),
    "clavicle": ("clavicle_",),
    "upperarm": ("upperarm_",),
    "lowerarm": ("lowerarm_",),
    "hands": ("hand_", "thumb_", "index_", "middle_", "ring_", "pinky_"),
    "feet": ("foot_", "ball_"),
    "head": ("Head",),
}


def bone_scale(race, name):
    """(length scale, thickness scale) for a bone."""
    for group, prefixes in GROUPS.items():
        if any(name.startswith(p) for p in prefixes):
            v = race[group]
            return (v, v) if isinstance(v, (int, float)) else v
    return (1.0, 1.0)


def transforms(skel, race):
    """Per-bone affine maps (rest, armature space) and the new joint positions."""
    bones = skel.data.bones
    rest = {b.name: b.matrix_local.copy() for b in bones}
    heads = {b.name: b.head_local.copy() for b in bones}
    A, new_head = {}, {}

    def visit(b):
        sl, st = bone_scale(race, b.name)
        if b.parent is None:
            new_head[b.name] = heads[b.name].copy()
        else:
            new_head[b.name] = apply(A[b.parent.name], heads[b.name])
        R = rest[b.name].to_3x3()
        S = Matrix.Diagonal((st, sl, st))
        lin = R @ S @ R.transposed()
        A[b.name] = (lin, heads[b.name], new_head[b.name])
        for c in b.children:
            visit(c)

    for b in bones:
        if b.parent is None:
            visit(b)
    return A, new_head


def apply(t, p):
    lin, old, new = t
    return new + lin @ (p - old)


def deform(mesh, A, dz):
    """Moves a skinned mesh's vertices by its bones' blended affine maps (rest pose)."""
    me = mesh.data
    mw = mesh.matrix_world
    inv = mw.inverted()
    groups = {g.index: g.name for g in mesh.vertex_groups}
    for v in me.vertices:
        p = mw @ v.co
        acc, wsum = Vector(), 0.0
        for g in v.groups:
            name = groups.get(g.group)
            if name in A and g.weight > 0:
                acc += apply(A[name], p) * g.weight
                wsum += g.weight
        q = acc / wsum if wsum > 0 else p
        q.z -= dz
        v.co = inv @ q
    me.update()


def rebuild_skeleton(skel, race, A, new_head, dz):
    sc.activate(skel, "EDIT")
    for eb in skel.data.edit_bones:
        sl, _ = bone_scale(race, eb.name)
        m = eb.matrix.copy()
        length = eb.length * (sl if not eb.name.startswith("socket_") else 1.0)
        h = new_head[eb.name] - Vector((0, 0, dz))
        m.translation = h
        eb.matrix = m
        eb.length = length
    bpy.ops.object.mode_set(mode="OBJECT")


def ground_offset(meshes):
    """How far the deformed feet sit above the floor (their lowest point should be z = 0)."""
    lo = min((m.matrix_world @ v.co).z for m in meshes for v in m.data.vertices)
    return lo


def pointed_ears(body, regions, ears, kind, head):
    """Stretches the 'ears' region into points: elegant for elves, broad for goblins."""
    import math
    from . import regions as rg
    me = body.data
    mw = body.matrix_world
    inv = mw.inverted()
    idx = rg.INDEX["ears"]
    ear_verts = set()
    for poly, r in zip(me.polygons, regions):
        if r == idx:
            ear_verts.update(poly.vertices)
    if not ear_verts:
        return 0
    for side in (1, -1):
        vs = [i for i in ear_verts if (mw @ me.vertices[i].co).x * side > 0]
        if not vs:
            continue
        pts = [mw @ me.vertices[i].co for i in vs]
        zmin = min(p.z for p in pts)
        zmax = max(p.z for p in pts)
        root = sum(pts, Vector()) / len(pts)
        root.x = head.x + (root.x - head.x) * 0.8
        for i, p in zip(vs, pts):
            t = (p.z - zmin) / max(zmax - zmin, 1e-6)
            if kind == "elf":
                k = max(0.0, t - 0.35) / 0.65
                d = Vector((side * 0.35, 0.55, 1.0)).normalized() * (0.045 * k ** 1.6)
                q = p + d
            else:
                out = (p - root)
                out.z *= 1.15
                q = root + Vector((out.x * 1.9, out.y * 1.4, out.z * 1.3))
                k = max(0.0, t - 0.2) / 0.8
                q += Vector((side * 0.06, 0.03, 0.035)) * k ** 1.4
            me.vertices[i].co = inv @ q
    me.update()
    return len(ear_verts)


def bigger_nose(body, regions, factor, head):
    """Scales the face's frontmost bump (the nose) about its root."""
    from . import regions as rg
    me = body.data
    mw = body.matrix_world
    inv = mw.inverted()
    idx = rg.INDEX["face"]
    face = set()
    for poly, r in zip(me.polygons, regions):
        if r == idx:
            face.update(poly.vertices)
    pts = {i: mw @ me.vertices[i].co for i in face}
    tip = min(pts.items(), key=lambda kv: kv[1].y)[1]
    root = tip + Vector((0, 0.03, 0.012))
    for i, p in pts.items():
        d = (p - tip).length
        if d < 0.035:
            w = (1 - d / 0.035) ** 1.5
            q = root + (p - root) * (1 + (factor - 1) * w)
            me.vertices[i].co = inv @ q
    me.update()
