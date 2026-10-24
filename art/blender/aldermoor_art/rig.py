"""The shared humanoid skeleton: one set of bone frames, many proportions.

Every humanoid skeleton we export uses the Universal Animation Library's rest orientations, and
only joint positions differ between bodies, races and builds. A clip's local rotations then play
unchanged on any of them. The engine takes translations from the skeleton's own rest pose, except
the pelvis, whose clip translation it scales by leg length.
"""

import bpy
from mathutils import Matrix, Vector

from . import scene


def bone_order(bones):
    """Parents before children."""
    out = []

    def visit(b):
        out.append(b.name)
        for c in b.children:
            visit(c)

    for b in bones:
        if b.parent is None:
            visit(b)
    return out


def rest_frames(arm):
    """Each bone's rest rotation relative to its parent (armature space for roots)."""
    frames = {}
    for b in arm.data.bones:
        local = b.parent.matrix_local.inverted() @ b.matrix_local if b.parent else b.matrix_local
        frames[b.name] = local.to_quaternion()
    return frames


def rest_heads(arm):
    return {b.name: b.head_local.copy() for b in arm.data.bones}


def normalise_frames(arm, frames):
    """Re-orients every bone to the reference frames while keeping its joint position and length.

    Meshes bound to the armature don't move: the armature modifier deforms relative to the rest
    pose, whatever its orientation.
    """
    scene.activate(arm, "EDIT")
    ebs = arm.data.edit_bones
    for eb in ebs:
        eb.use_connect = False
    heads = {eb.name: eb.head.copy() for eb in ebs}
    lengths = {eb.name: eb.length for eb in ebs}
    world = {}
    for name in bone_order([eb for eb in ebs]):
        eb = ebs[name]
        if name not in frames:
            world[name] = eb.matrix.to_3x3()
            continue
        r = frames[name].to_matrix()
        world[name] = world[eb.parent.name] @ r if eb.parent else r
        m = world[name].to_4x4()
        m.translation = heads[name]
        eb.matrix = m
        eb.length = lengths[name]
    bpy.ops.object.mode_set(mode="OBJECT")


def pose_to_heads(arm, targets):
    """Poses the armature so each bone's head sits on the target position, keeping rest rotations."""
    scene.activate(arm, "POSE")
    for name in bone_order(arm.data.bones):
        pb = arm.pose.bones[name]
        if name not in targets:
            continue
        rest = arm.data.bones[name].matrix_local
        m = rest.to_3x3().to_4x4()
        m.translation = targets[name]
        pb.matrix = m
        bpy.context.view_layer.update()
    bpy.ops.object.mode_set(mode="OBJECT")


def apply_armature(mesh):
    """Bakes the current pose into the mesh and removes its armature modifier."""
    scene.activate(mesh)
    for m in [m for m in mesh.modifiers if m.type == "ARMATURE"]:
        if mesh.data.shape_keys:
            raise RuntimeError(f"{mesh.name} has shape keys; refitting would drop them")
        bpy.ops.object.modifier_apply(modifier=m.name)


def refit(meshes, src_arm, dst_arm):
    """Moves meshes skinned to src_arm onto dst_arm's joint positions, then binds them to dst_arm."""
    targets = rest_heads(dst_arm)
    pose_to_heads(src_arm, targets)
    for me in meshes:
        apply_armature(me)
        scene.bind(me, dst_arm)
    for pb in src_arm.pose.bones:
        pb.matrix_basis = Matrix()


# Sockets: bones that carry equipment. Items are authored in Blender with the attachment point at
# the origin, "up" along +Z and "front" along -Y. A socket's local Y is where the item's up points
# and its local Z is where the item's front points (blade and edge for weapons).

def _frame(up, front, origin):
    y = up.normalized()
    z = (front - y * front.dot(y)).normalized()
    x = y.cross(z)
    m = Matrix((x, y, z)).transposed().to_4x4()
    m.translation = origin
    return m


def socket_frames(arm, body):
    """Socket rest matrices (armature space) for this skeleton, measured from its bones and body."""
    from mathutils.bvhtree import BVHTree

    h = rest_heads(arm)
    dg = bpy.context.evaluated_depsgraph_get()
    bvh = BVHTree.FromObject(body, dg)

    def surface(start, direction, extra):
        hit = bvh.ray_cast(start, direction.normalized(), 1.0)
        assert hit[0] is not None, f"no body surface from {start} towards {direction}"
        # The last surface along the ray, so arms or clothing in front don't stop it.
        point = hit[0]
        while True:
            nxt = bvh.ray_cast(point + direction.normalized() * 1e-3, direction.normalized(), 1.0)
            if nxt[0] is None:
                break
            point = nxt[0]
        return point + direction.normalized() * extra

    out = {}
    for side, sign in (("r", -1.0), ("l", 1.0)):
        wrist, index, pinky, middle, thumb = (h[f"{n}_{side}"] for n in ("hand", "index_01", "pinky_01", "middle_01", "thumb_02"))
        blade = index - pinky
        edge = middle - wrist
        knuckles = (index + pinky) / 2
        # The palm faces the side the thumb hangs towards.
        palm = edge.cross(blade).normalized()
        if palm.dot(thumb - knuckles) < 0:
            palm = -palm
        grip = knuckles + palm * 0.026 + (middle - wrist).normalized() * 0.004
        out[f"socket_hand_{side}"] = ("hand_" + side, _frame(blade, edge, grip))

    # Shield on the outer left forearm: face out of the back of the hand, top towards the thumb.
    elbow, wrist = h["lowerarm_l"], h["hand_l"]
    along = wrist - elbow
    mount = elbow + along * 0.55
    back_of_hand = Vector((0, 0, 1))
    # About 4 cm of forearm and 3.5 cm of straps and padding under the shield's back.
    out["socket_shield_l"] = ("lowerarm_l", _frame(Vector((0, -1, 0)), back_of_hand, mount + back_of_hand * 0.075))

    up = Vector((0, 0, 1))
    front = Vector((0, -1, 0))
    back = Vector((0, 1, 0))
    chest = h["spine_03"] + Vector((0, 0, 0.05))
    back_point = surface(chest, back, 0.0)
    out["socket_back"] = ("spine_03", _frame(up, front, back_point))
    # Everything on the back can be drawn over a shoulder (docs/ASSETS.md, "Drawing and
    # sheathing"). Blades hang hilt-up behind a shoulder, the grip where that hand reaches:
    # socket_back_1 over the right shoulder, socket_back_2 over the left.
    shoulder_z = (h["upperarm_l"].z + h["clavicle_l"].z) / 2
    for name, sign in (("socket_back_1", -1.0), ("socket_back_2", 1.0)):
        u = Vector((-sign * 0.55, 0.06, -0.83)).normalized()  # Item up = blade tip, down and across.
        e = back.cross(u) * -sign  # Mirror images, so a mirrored draw meets the item as it lies.
        grip = surface(Vector((sign * 0.11, h["spine_03"].y, shoulder_z - 0.03)), back, 0.07)
        out[name] = ("spine_03", _frame(u, e, grip))
    # Poles (staves, spears, tools) cross the back head-up over the right shoulder; bows the
    # same way over the left. Their grips sit mid-back, and the hand takes the shaft higher up.
    mid_back = surface(Vector((0, h["spine_03"].y, (h["spine_02"].z + h["spine_03"].z) / 2)), back, 0.07)
    for name, sign in (("socket_back_pole", -1.0), ("socket_back_bow", 1.0)):
        u = Vector((sign * 0.42, 0.04, 0.91)).normalized()
        out[name] = ("spine_03", _frame(u, back.cross(u) * -sign, mid_back))
    # Shields: upright in the middle of the back, face out, over anything slung beneath.
    out["socket_back_shield"] = ("spine_03", _frame(Vector((0, 0.08, 1)), back, back_point + back * 0.11 + Vector((0, 0, -0.06))))
    # With a travel pack on: a shield strapped to its outer face, long items down its sides
    # (bows on the left, for the left hand; everything else on the right).
    out["socket_pack"] = ("spine_03", _frame(up, back, back_point + Vector((0, 0.235, -0.05))))
    out["socket_pack_side"] = ("spine_03", _frame(Vector((0.12, 0.08, 1)), Vector((1, 0, 0)), back_point + Vector((0.205, 0.11, -0.04))))
    out["socket_pack_side_r"] = ("spine_03", _frame(Vector((-0.12, 0.08, 1)), Vector((-1, 0, 0)), back_point + Vector((-0.205, 0.11, -0.04))))
    # Blades ride the pack's right side tip-down, the hilt up behind the right shoulder.
    out["socket_pack_blade"] = ("spine_03", _frame(Vector((0.12, 0.08, -1)), Vector((-1, 0, 0)), back_point + Vector((-0.205, 0.11, 0.09))))
    # Quiver: arrows over the right shoulder.
    out["socket_quiver"] = ("spine_03", _frame(Vector((-0.35, 0.12, 1)), front, back_point + back * 0.05 + Vector((0.04, 0, -0.06))))
    # Sheathed one-handers on the hips, hilt forward and up, tip back and down.
    hips = h["pelvis"]
    for side, sign in (("l", 1.0), ("r", -1.0)):
        side_dir = Vector((sign, 0, 0))
        p = surface(hips + Vector((0, -0.03, 0.03)), side_dir, 0.05)
        tip = Vector((0, 0.85, -1)).normalized()
        e = Vector((0, -1, -0.85)).normalized()
        out[f"socket_hip_{side}"] = ("pelvis", _frame(tip, e, p))
    # A hip quiver on the right, for when a pack takes the back: arrows up, leaning back.
    p = surface(hips + Vector((0, 0.02, -0.04)), Vector((-1, 0, 0)), 0.035)
    out["socket_quiver_hip"] = ("pelvis", _frame(Vector((0, 0.3, 1)), Vector((1, 0, 0)), p))
    # Hats and rigid helmets: the head's rest frame, upright and facing front.
    out["socket_head"] = ("Head", _frame(up, front, h["Head"]))
    return out


def add_sockets(arm, frames):
    scene.activate(arm, "EDIT")
    ebs = arm.data.edit_bones
    for name, (parent, m) in frames.items():
        eb = ebs.get(name) or ebs.new(name)
        eb.parent = ebs[parent]
        eb.use_connect = False
        eb.use_deform = False
        # The matrix setter keeps the current length, and a zero-length bone has no direction.
        eb.head, eb.tail = (0, 0, 0), (0, 0.05, 0)
        eb.matrix = m
        eb.length = 0.05
    bpy.ops.object.mode_set(mode="OBJECT")
