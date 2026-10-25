"""Measures when the vendor attack clips' blows land, for build_clips.py's VENDOR_EVENTS:

    art/blender.sh --python art/blender/measure_strikes.py

For each clip, both hands' speed peaks (a slash lands at its fastest) and the moment each hand
is furthest in front of the chest (a punch or a shield bash lands at full stretch). Only the
peaks' timing matters; the speeds are in the clip library's own units.
"""

import sys
from pathlib import Path

import bpy
from mathutils import Vector

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aldermoor_art import scene as sc  # noqa: E402

CLIPS = ["Sword_Regular_A", "Sword_Regular_B", "Sword_Regular_C", "Sword_Regular_Combo", "Sword_Heavy_Combo",
         "Sword_Overhead", "Sword_Attack", "Sword_Dash", "Melee_Hook", "Punch_Jab", "Punch_Cross", "Shield_OneShot",
         "Zombie_Scratch", "Spell_Simple_Shoot", "OverhandThrow"]
RATE = 60
FORWARD = Vector((0, -1, 0))


def main():
    sc.reset()
    arm = sc.armature_of(sc.import_gltf(sc.OUT / "humanoid/anims.glb"))
    if not arm.animation_data:
        arm.animation_data_create()
    fps = bpy.context.scene.render.fps
    for name in CLIPS:
        act = bpy.data.actions.get(name)
        if act is None:
            print("MISSING", name)
            continue
        arm.animation_data.action = act
        if hasattr(arm.animation_data, "action_slot") and act.slots:
            arm.animation_data.action_slot = act.slots[0]
        f0, f1 = act.frame_range
        rows = []
        for k in range(int(round((f1 - f0) / fps * RATE)) + 1):
            t = k / RATE
            bpy.context.scene.frame_set(int(f0) + int(t * fps), subframe=(t * fps) % 1)
            bpy.context.view_layer.update()
            pb, mw = arm.pose.bones, arm.matrix_world
            rows.append((t, mw @ pb["hand_r"].tail, mw @ pb["hand_l"].tail, mw @ pb["spine_03"].head))
        report = []
        for side, i in (("right", 1), ("left", 2)):
            speed = [(rows[j][0], ((rows[j + 1][i] - rows[j - 1][i]) * RATE / 2).length) for j in range(1, len(rows) - 1)]
            top = max(v for _, v in speed)
            peaks = [round(t, 3) for j, (t, v) in enumerate(speed)
                     if 0 < j < len(speed) - 1 and v >= speed[j - 1][1] and v >= speed[j + 1][1] and v > 0.6 * top]
            reach = max(rows, key=lambda r: (r[i] - r[3]).dot(FORWARD))[0]
            report.append(f"{side} fastest at {peaks}, furthest at {round(reach, 3)}")
        print(f"STRIKE {name} ({round(rows[-1][0], 3)} s): " + "; ".join(report))


main()
