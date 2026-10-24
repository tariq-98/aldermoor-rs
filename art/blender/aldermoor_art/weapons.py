"""Weapons, shields, bows and quivers, modelled in code.

Every item is authored around its attachment point: up is +Z (blade tip, shaft end, shield top)
and front is -Y (cutting edge, bow back, shield face). Each builder returns a list of objects.
"""

import math

from mathutils import Matrix, Vector

from . import shapes as sh
from .holds import NOCK_Z


def _blade(length, base_z, w0, w_mid, t0, t1, tip=0.18, n=28, name="blade", mat="steel", fuller=True):
    """A hexagonal-section blade: edges along +-Y, flats facing +-X."""
    sections = []
    for i in range(n + 1):
        t = i / n
        z = base_z + t * length
        if t < 1 - tip:
            w = w0 + (w_mid - w0) * (t / (1 - tip))
        else:
            k = (t - (1 - tip)) / tip
            w = w_mid * (1 - k ** 1.35)
        th = t0 + (t1 - t0) * t
        if w < 1e-4:
            sections.append([Vector((0, 0, z))])
            continue
        flat = 0.42 if fuller and t < 0.7 else 0.3
        sections.append([
            Vector((0, -w, z)), Vector((th * 0.55, -w * 0.62, z)), Vector((th, -w * flat * 0.5, z)),
            Vector((th, w * flat * 0.5, z)), Vector((th * 0.55, w * 0.62, z)), Vector((0, w, z)),
            Vector((-th * 0.55, w * 0.62, z)), Vector((-th, w * flat * 0.5, z)), Vector((-th, -w * flat * 0.5, z)),
            Vector((-th * 0.55, -w * 0.62, z)),
        ])
    bm = sh.loft(sections)
    ob = sh.obj(name, bm, mat, smooth=False)
    return sh.finish(ob, bevel=0.0004, segments=1, angle=50)


def _regrip(parts, at):
    """Moves everything but the grip wrap so the grip sits `at` (0..1) of the way up the item.

    Poles are held near their balance, so a staff can stand on the ground at the hand's height
    and rides the back centred.
    """
    zs = []
    for ob in parts:
        if ob.name.startswith("grip"):
            continue
        zs += [(ob.matrix_basis @ v.co).z for v in ob.data.vertices]
    lo, hi = min(zs), max(zs)
    dz = -(lo + at * (hi - lo))
    for ob in parts:
        if not ob.name.startswith("grip"):
            ob.location.z += dz
    return parts


def _grip(z0, z1, r, ridges=9, mat="leather", name="grip"):
    prof = []
    steps = ridges * 4
    for i in range(steps + 1):
        z = z0 + (z1 - z0) * i / steps
        bulge = 1 + 0.06 * math.sin(math.pi * i / steps)
        ridge = 0.08 * abs(math.sin(math.pi * ridges * i / steps))
        prof.append((r * (bulge + ridge * 0.5), z))
    prof = [(r * 0.6, z0 - 0.002)] + prof + [(r * 0.6, z1 + 0.002)]
    ob = sh.obj(name, sh.lathe(prof, 16), mat)
    return sh.finish(ob, bevel=0)


def _guard(half, z, thick, drop=0.012, mat="steel", name="guard", ends=True):
    path = []
    n = 14
    for i in range(n + 1):
        y = -half + 2 * half * i / n
        path.append(Vector((0, y, z + drop * (y / half) ** 2)))
    ob = sh.obj(name, sh.sweep(path, lambda t: thick * (1.0 - 0.35 * abs(2 * t - 1) ** 1.5), n=10, power=3.0,
                               ratio=0.8, up=Vector((1, 0, 0))), mat)
    parts = [sh.finish(ob, bevel=0.0008, angle=40)]
    if ends:
        for s in (-1, 1):
            ball = sh.obj(f"{name}_end", sh.lathe([(0, -1), (0.6, -0.8), (1, 0), (0.6, 0.8), (0, 1)], 12), mat)
            ball.scale = (thick * 1.25,) * 3
            ball.location = (0, s * (half + thick * 0.6), z + drop)
            parts.append(ball)
    centre = sh.obj(f"{name}_block", sh.loft([
        sh.ring(Vector((0, 0, z - thick * 0.6)), Vector((1, 0, 0)), Vector((0, 1, 0)), thick * 0.75, thick * 1.6, 12, 3),
        sh.ring(Vector((0, 0, z + thick * 1.1)), Vector((1, 0, 0)), Vector((0, 1, 0)), thick * 0.6, thick * 1.1, 12, 3),
    ]), mat)
    parts.append(sh.finish(centre, bevel=0.0008))
    return parts


def _knob(z_top, height, r, mat="steel", name="pommel"):
    prof = [(r * 0.45, z_top), (r * 0.8, z_top - height * 0.12), (r, z_top - height * 0.4),
            (r * 0.95, z_top - height * 0.7), (r * 0.6, z_top - height * 0.92), (0, z_top - height)]
    return sh.finish(sh.obj(name, sh.lathe(prof, 18), mat), bevel=0)


def arming_sword():
    parts = [_blade(0.76, 0.066, 0.024, 0.019, 0.0036, 0.0018)]
    parts += [_grip(-0.052, 0.052, 0.0145)]
    parts += _guard(0.095, 0.058, 0.0085)
    parts += [_knob(-0.052, 0.04, 0.022)]
    return parts


def arming_sword_sheath():
    """A scabbard around the arming sword's blade, in the sword's own frame."""
    sections = []
    n = 24
    for i in range(n + 1):
        t = i / n
        z = 0.068 + t * 0.775
        w = 0.0285 - 0.006 * t - (0.016 * ((t - 0.86) / 0.14) ** 1.6 if t > 0.86 else 0)
        th = 0.0085 - 0.002 * t
        sections.append(sh.ring(Vector((0, 0, z)), Vector((1, 0, 0)), Vector((0, 1, 0)), th, w, 14, 2.6))
    body = sh.finish(sh.obj("scabbard", sh.loft(sections), "leather_dark"), bevel=0)
    parts = [body]
    for z0, z1, mat in ((0.068, 0.105, "brass"), (0.33, 0.345, "brass"), (0.78, 0.845, "brass")):
        secs = []
        for z in (z0, z1):
            t = (z - 0.068) / 0.775
            w = 0.0285 - 0.006 * t - (0.016 * ((t - 0.86) / 0.14) ** 1.6 if t > 0.86 else 0) + 0.0012
            th = 0.0085 - 0.002 * t + 0.0012
            secs.append(sh.ring(Vector((0, 0, z)), Vector((1, 0, 0)), Vector((0, 1, 0)), th, w, 14, 2.6))
        parts.append(sh.finish(sh.obj("fitting", sh.loft(secs), mat), bevel=0.0006, angle=30))
    # A tip cap and two belt rings on the outer side.
    parts.append(_knob(0.852, 0.02, 0.006, "brass", "chape_tip"))
    for z in (0.12, 0.25):
        loop = sh.obj("ring", sh.sweep([Vector((0.0095, math.cos(a) * 0.012, z + math.sin(a) * 0.012))
                                         for a in (2 * math.pi * i / 16 for i in range(17))],
                                        lambda t: 0.0022, n=6, up=Vector((1, 0, 0))), "brass")
        parts.append(loop)
    return parts


def dagger():
    parts = [_blade(0.23, 0.04, 0.019, 0.015, 0.0034, 0.002, tip=0.3)]
    parts += [_grip(-0.045, 0.034, 0.0125, ridges=6, mat="leather_dark")]
    parts += _guard(0.045, 0.036, 0.0065, drop=-0.006, mat="brass")
    parts += [_knob(-0.045, 0.026, 0.016, "brass")]
    return parts


def dagger_sheath():
    sections = []
    n = 14
    for i in range(n + 1):
        t = i / n
        z = 0.04 + t * 0.245
        w = 0.024 - 0.004 * t - (0.016 * ((t - 0.75) / 0.25) ** 1.5 if t > 0.75 else 0)
        sections.append(sh.ring(Vector((0, 0, z)), Vector((1, 0, 0)), Vector((0, 1, 0)), 0.0075, max(w, 0.002), 12, 2.6))
    return [sh.finish(sh.obj("dagger_sheath", sh.loft(sections), "leather_tan"), bevel=0),
            _knob(0.29, 0.015, 0.005, "brass", "dagger_chape")]


def round_shield(face="wood_light", paint="paint_red"):
    """A domed round shield. Its back plane sits on the origin; it bulges towards -Y."""
    R = 0.33
    prof = []
    for i in range(13):
        r = R * i / 12
        prof.append((r, -0.022 - 0.03 * (1 - (r / R) ** 2)))
    # Lathe builds around Z; turn so the dome axis is Y, bulging to -Y.
    face_bm = sh.lathe([(0, prof[0][1])] + prof[1:] + [(R, -0.008), (R * 0.97, 0.0), (0, 0.004)], 40)
    sh.transform(face_bm, Matrix.Rotation(math.radians(-90), 4, "X"))
    disc = sh.finish(sh.obj("shield_face", face_bm, face), bevel=0)
    # A painted band across the face, as a thin raised layer.
    parts = [disc]
    band = []
    for i in range(13):
        r = R * 0.96 * i / 12
        band.append((r, -0.0236 - 0.03 * (1 - (r / R) ** 2)))
    rim_path = [Vector((math.cos(a) * R, 0, math.sin(a) * R)) for a in (2 * math.pi * i / 48 for i in range(49))]
    rim = sh.obj("shield_rim", sh.sweep([p + Vector((0, -0.012, 0)) for p in rim_path], lambda t: 0.012, n=10, ratio=0.75,
                                         up=Vector((0, 1, 0))), "iron")
    parts.append(rim)
    boss = sh.lathe([(0.085, 0.0), (0.083, -0.006), (0.07, -0.016), (0.055, -0.03), (0.035, -0.042), (0.015, -0.047), (0, -0.048)], 28)
    sh.transform(boss, Matrix.Translation((0, -0.05, 0)) @ Matrix.Rotation(math.radians(-90), 4, "X"))
    parts.append(sh.finish(sh.obj("shield_boss", boss, "steel"), bevel=0))
    # Painted cross-band (vertical and horizontal), sitting proud of the face.
    for axis in ("v", "h"):
        sections = []
        for i in range(17):
            u = -R * 0.93 + 2 * R * 0.93 * i / 16
            r = abs(u)
            y = -0.0225 - 0.03 * (1 - (r / R) ** 2) - 0.0012
            c = Vector((0, y, u)) if axis == "v" else Vector((u, y, 0))
            side = Vector((1, 0, 0)) if axis == "v" else Vector((0, 0, 1))
            sections.append([c - side * 0.045 + Vector((0, 0.001, 0)), c - side * 0.045, c + side * 0.045, c + side * 0.045 + Vector((0, 0.001, 0))])
        parts.append(sh.obj(f"shield_band_{axis}", sh.loft(sections, closed=True), paint, smooth=True))
    # Rivets around the rim.
    for i in range(16):
        a = 2 * math.pi * (i + 0.5) / 16
        rv = sh.obj("rivet", sh.lathe([(0.0001, 0.0), (0.006, -0.001), (0.005, -0.004), (0, -0.0055)], 8), "iron")
        rv.rotation_euler = (math.radians(-90), 0, 0)
        rv.location = (math.cos(a) * R * 0.88, -0.022 - 0.03 * (1 - 0.88 ** 2), math.sin(a) * R * 0.88)
        parts.append(rv)
    # Straps on the back for the forearm, and a grip bar.
    for z in (-0.1, 0.09):
        strap = sh.obj("strap", sh.sweep([Vector((x, 0.004 + 0.03 * math.sin(math.pi * (x + 0.07) / 0.14), z))
                                           for x in [(-0.07 + 0.14 * i / 10) for i in range(11)]],
                                          lambda t: 0.012, n=8, power=4, ratio=0.25, up=Vector((0, 1, 0))), "leather")
        parts.append(strap)
    return parts


ARROW_SIDE = 0.012  # The arrow rests on the bow's left (+X) side, for a right-handed archer.


def _tag(ob, group):
    vg = ob.vertex_groups.new(name=group)
    vg.add(list(range(len(ob.data.vertices))), 1.0, "REPLACE")
    return ob


def _bow(half=0.705, brace=0.17, recurve=0.05, width=0.017, wood="wood", riser_wood="wood_dark"):
    """A bow held at its grip. The string side is +Y (towards the archer).

    The string has a middle ring at the nocking point and a nocked arrow is built in place, so
    `bow_morphs` can turn them into the `draw` and `arrow` morph targets after the join.
    """
    reach = half - 0.085
    path_up = []
    for i in range(21):
        t = i / 20
        z = 0.085 + t * reach
        bend = brace * 0.55 * t ** 1.7
        rc = -recurve * max(0.0, (t - 0.8) / 0.2) ** 2
        path_up.append(Vector((0, bend + rc, z)))
    limbs = []
    for s in (1, -1):
        path = [Vector((p.x, p.y, p.z * s)) for p in path_up]
        limb = sh.obj("limb", sh.sweep(path, lambda t: width * (1 - 0.55 * t), n=10, power=2.4, ratio=0.45,
                                        up=Vector((0, 1, 0))), wood)
        limbs.append(_tag(sh.finish(limb, bevel=0), "limb"))
    riser_path = [Vector((0, -0.006 * math.sin(math.pi * i / 10), -0.09 + 0.18 * i / 10)) for i in range(11)]
    riser = sh.finish(sh.obj("riser", sh.sweep(riser_path, lambda t: 0.019 - 0.004 * abs(2 * t - 1), n=12, power=2.2,
                                                 ratio=0.8, up=Vector((0, 1, 0))), riser_wood), bevel=0)
    wrap = _grip(-0.055, 0.055, 0.0205, ridges=10, mat="leather")
    tip = path_up[-1]
    tips = [Vector((0, tip.y, tip.z)), Vector((0, tip.y, -tip.z))]
    string = _tag(sh.obj("string", sh.sweep([tips[0], Vector((0, tip.y, NOCK_Z)), tips[1]], lambda t: 0.0013, n=6,
                                              up=Vector((1, 0, 0))), "cord"), "string")
    nocks = []
    for p in tips:
        k = sh.obj("nock", sh.lathe([(0, -0.012), (0.0075, -0.006), (0.0075, 0.006), (0, 0.012)], 10), "bone")
        k.location = p
        nocks.append(_tag(k, "limb"))
    shafts = []
    for piece in arrow():
        # Nocked: the arrow points at the target (-Y) from the string.
        piece.rotation_euler = (math.pi / 2, 0, 0)
        piece.location = (ARROW_SIDE, tip.y, NOCK_Z)
        shafts.append(_tag(piece, "arrow"))
    return limbs + [riser, wrap, string] + nocks + shafts


def hunting_bow():
    """A short recurve."""
    return _bow()


def longbow():
    """A tall, straight self bow."""
    return _bow(half=0.9, brace=0.15, recurve=0.0, width=0.0195, wood="wood_light")


def bow_morphs(ob, draw):
    """Turns a joined bow's tagged parts into two morph targets.

    `arrow` shows the nocked arrow (it is folded into the grip otherwise). `draw` pulls the
    string's middle back by `draw` metres towards the archer, bends the limbs and carries the
    arrow with the string. Returns the string's rest point and pull, in item space.
    """
    me = ob.data
    groups = {g.name: g.index for g in ob.vertex_groups}

    def members(name):
        gi = groups[name]
        return {v.index for v in me.vertices if any(g.group == gi and g.weight > 0.5 for g in v.groups)}

    limb, string, arrow_v = members("limb"), members("string"), members("arrow")
    co = [v.co.copy() for v in me.vertices]
    z1 = max(abs(co[i].z) for i in limb)
    z0 = 0.085
    bend, shrink = 0.11 * z1 / 0.705, 0.035 * z1 / 0.705

    def limb_delta(p):
        t = max(0.0, min(1.0, (abs(p.z) - z0) / (z1 - z0)))
        return Vector((0, bend * t * t, -math.copysign(shrink * t * t, p.z)))

    middle = [co[i] for i in string if abs(co[i].z - NOCK_Z) < 0.05]
    rest = sum(middle, Vector()) / len(middle)
    rest = Vector((0, rest.y, NOCK_Z))
    pull = Vector((0, draw, 0))
    fold = Vector((0, 0, NOCK_Z))
    basis = ob.shape_key_add(name="Basis", from_mix=False)
    k_arrow = ob.shape_key_add(name="arrow", from_mix=False)
    k_draw = ob.shape_key_add(name="draw", from_mix=False)
    for i, p in enumerate(co):
        if i in arrow_v:
            basis.data[i].co = fold
            me.vertices[i].co = fold
            k_arrow.data[i].co = p
            k_draw.data[i].co = fold + pull
        elif i in string:
            if p.z > z1 * 0.5:
                d = limb_delta(Vector((0, 0, z1)))
            elif p.z < -z1 * 0.5:
                d = limb_delta(Vector((0, 0, -z1)))
            else:
                d = pull
            k_draw.data[i].co = p + d
        elif i in limb:
            k_draw.data[i].co = p + limb_delta(p)
    for g in list(ob.vertex_groups):
        ob.vertex_groups.remove(g)
    return rest, pull


def arrow(length=0.76, fletch="feather", head="iron"):
    shaft = sh.obj("shaft", sh.lathe([(0.0001, 0.0), (0.0042, 0.003), (0.0042, length - 0.02), (0.003, length)], 8), "wood_light")
    point = sh.obj("head", sh.loft([
        [Vector((0.0015, 0, length - 0.005)), Vector((0, -0.011, length + 0.005)), Vector((-0.0015, 0, length - 0.005)), Vector((0, 0.011, length + 0.005))],
        [Vector((0, 0, length + 0.05))],
    ]), head, smooth=False)
    vanes = []
    for k in range(3):
        a = 2 * math.pi * k / 3
        d = Vector((math.cos(a), math.sin(a), 0))
        v = sh.obj("vane", sh.loft([
            [Vector((0, 0, 0.015)), d * 0.012 + Vector((0, 0, 0.03)), d * 0.012 + Vector((0, 0, 0.11)), Vector((0, 0, 0.13))],
            [Vector((0, 0, 0.015)) + d.cross(Vector((0, 0, 1))) * 0.0006, d * 0.012 + Vector((0, 0, 0.03)) + d.cross(Vector((0, 0, 1))) * 0.0006,
             d * 0.012 + Vector((0, 0, 0.11)) + d.cross(Vector((0, 0, 1))) * 0.0006, Vector((0, 0, 0.13)) + d.cross(Vector((0, 0, 1))) * 0.0006],
        ], closed=False), fletch, smooth=False)
        vanes.append(v)
    return [shaft, point] + vanes


def quiver(arrows=7):
    """A back quiver: the contact point is the origin, the body sits behind it (+Y)."""
    r, h = 0.052, 0.5
    base = Vector((0, r + 0.012, -0.23))
    body = sh.obj("quiver", sh.lathe([(0, 0), (r * 0.92, 0.004), (r, 0.03), (r * 1.02, h * 0.6), (r * 1.08, h), (r * 0.98, h + 0.004)], 20), "leather")
    body = sh.finish(body, bevel=0)
    bands = []
    for z in (0.05, h * 0.55, h - 0.03):
        b = sh.obj("band", sh.lathe([(r * 1.09 + (0.004 if z > h * 0.9 else 0), z - 0.012), (r * 1.12, z - 0.008), (r * 1.12, z + 0.008), (r * 1.09, z + 0.012)], 20), "leather_tan")
        bands.append(b)
    parts = [body] + bands
    for p in parts:
        p.location = base
    sh_arrows = []
    for i in range(arrows):
        a = 2 * math.pi * i / arrows
        rr = r * 0.55 * (0.4 + 0.6 * ((i * 37) % 5) / 4)
        for piece in arrow(fletch="feather_red" if i % 3 == 0 else "feather"):
            # Heads down inside the quiver, fletching standing out of the top.
            piece.location = base + Vector((math.cos(a) * rr, math.sin(a) * rr, 0.83 + 0.02 * (i % 3)))
            piece.rotation_euler = (math.pi, 0, a)
            sh_arrows.append(piece)
    strap = sh.obj("quiver_strap", sh.sweep([base + Vector((0, -r - 0.004, z)) for z in (0.05, 0.2, 0.35, 0.47)],
                                            lambda t: 0.014, n=6, power=4, ratio=0.25, up=Vector((0, 1, 0))), "leather_dark")
    return parts + sh_arrows + [strap]


def battle_staff(crystal="crystal_blue"):
    """A wizard's staff held 60% of the way up; the head is at +Z."""
    path = []
    for i in range(41):
        t = i / 40
        z = -0.62 + t * 1.62
        path.append(Vector((0.006 * math.sin(t * 9.0), 0.005 * math.sin(t * 6.0 + 1.0), z)))
    shaft = sh.finish(sh.obj("shaft", sh.sweep(path, lambda t: 0.0155 + 0.004 * t + 0.002 * math.sin(t * 40) ** 2, n=12,
                                               up=Vector((1, 0, 0))), "wood"), bevel=0)
    parts = [shaft, _grip(-0.1, 0.12, 0.0185, ridges=8, mat="leather")]
    for z in (-0.6, 0.14, 0.86):
        band = sh.obj("band", sh.lathe([(0.017, z - 0.012), (0.0215, z - 0.006), (0.0215, z + 0.006), (0.017, z + 0.012)], 14), "brass")
        parts.append(band)
    # Three curling prongs cradle a crystal at the head.
    top = path[-1]
    for k in range(3):
        a = 2 * math.pi * k / 3
        prong = []
        for i in range(13):
            t = i / 12
            rr = 0.035 * math.sin(math.pi * t * 0.92)
            ang = a + t * 1.4
            prong.append(top + Vector((math.cos(ang) * rr, math.sin(ang) * rr, t * 0.16 - 0.02)))
        parts.append(sh.finish(sh.obj("prong", sh.sweep(prong, lambda t: 0.0085 * (1 - 0.7 * t), n=8, up=Vector((1, 0, 0))), "wood_dark"), bevel=0))
    gem = sh.obj("crystal", sh.loft([
        [top + Vector((0, 0, 0.035))],
        sh.ring(top + Vector((0, 0, 0.075)), Vector((1, 0, 0)), Vector((0, 1, 0)), 0.024, 0.024, 6),
        sh.ring(top + Vector((0, 0, 0.11)), Vector((1, 0, 0)), Vector((0, 1, 0)), 0.02, 0.02, 6),
        [top + Vector((0, 0, 0.15))],
    ]), crystal, smooth=False)
    parts.append(gem)
    foot = sh.obj("ferrule", sh.lathe([(0.017, -0.6), (0.016, -0.625), (0.01, -0.64), (0, -0.645)], 12), "iron")
    parts.append(foot)
    return _regrip(parts, 0.6)


def wand(crystal="crystal_violet"):
    prof = [(0.0001, -0.07), (0.009, -0.068), (0.0115, -0.05), (0.0105, -0.02), (0.012, 0.0), (0.0105, 0.03),
            (0.0085, 0.05), (0.0065, 0.15), (0.0048, 0.25), (0.0042, 0.27)]
    body = sh.finish(sh.obj("wand", sh.lathe(prof, 12), "wood_dark"), bevel=0)
    collar = sh.obj("collar", sh.lathe([(0.009, 0.045), (0.0105, 0.05), (0.0105, 0.058), (0.008, 0.062)], 12), "silver")
    tip = sh.obj("tip", sh.loft([[Vector((0, 0, 0.265))],
                                 sh.ring(Vector((0, 0, 0.278)), Vector((1, 0, 0)), Vector((0, 1, 0)), 0.008, 0.008, 6),
                                 [Vector((0, 0, 0.3))]]), crystal, smooth=False)
    return [body, collar, tip]


# --- More weapon families ----------------------------------------------------------------------

def _haft(z0, z1, r0, r1, mat="wood", name="haft", wobble=0.0):
    path = [Vector((wobble * math.sin(i * 0.9), 0, z0 + (z1 - z0) * i / 20)) for i in range(21)]
    return sh.finish(sh.obj(name, sh.sweep(path, lambda t: r0 + (r1 - r0) * t, n=12, up=Vector((1, 0, 0))), mat), bevel=0)


def _flat_blade(outline, thickness, mat="steel", name="head", bevel=0.0006):
    """Extrudes a 2D outline in the YZ plane (edge towards -Y) by `thickness` along X."""
    import bmesh
    bm = bmesh.new()
    front = [bm.verts.new(Vector((thickness / 2, y, z))) for y, z in outline]
    back = [bm.verts.new(Vector((-thickness / 2, y, z))) for y, z in outline]
    n = len(outline)
    bm.faces.new(front)
    bm.faces.new(list(reversed(back)))
    for i in range(n):
        j = (i + 1) % n
        bm.faces.new((front[i], back[i], back[j], front[j]))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    ob = sh.obj(name, bm, mat, smooth=False)
    return sh.finish(ob, bevel=bevel, segments=1, angle=30)


def hand_axe():
    """A bearded axe: the edge faces front (-Y), the haft runs along Z."""
    parts = [_haft(-0.17, 0.44, 0.015, 0.013, "wood"), _grip(-0.16, -0.04, 0.0165, ridges=6, mat="leather")]
    outline = [(-0.02, 0.36), (-0.06, 0.39), (-0.115, 0.43), (-0.13, 0.36), (-0.125, 0.29), (-0.1, 0.25),
               (-0.07, 0.29), (-0.03, 0.33), (0.018, 0.33), (0.018, 0.42), (-0.01, 0.43)]
    parts.append(_flat_blade(outline, 0.012))
    edge = _flat_blade([(-0.112, 0.425), (-0.13, 0.36), (-0.125, 0.295), (-0.118, 0.3), (-0.123, 0.36), (-0.106, 0.418)], 0.0125, "silver", "edge")
    parts.append(edge)
    parts.append(_knob(-0.17, 0.02, 0.016, "iron", "cap"))
    return parts


def mace():
    parts = [_haft(-0.16, 0.42, 0.014, 0.016, "wood_dark"), _grip(-0.15, -0.02, 0.016, ridges=6, mat="leather")]
    for k in range(7):
        a = 2 * math.pi * k / 7
        d = Vector((math.cos(a), math.sin(a), 0))
        flange = _flat_blade([(0.0, 0.38), (0.045, 0.40), (0.05, 0.44), (0.036, 0.49), (0.0, 0.51)], 0.008, "dark_steel", "flange")
        flange.rotation_euler = (0, 0, a + math.pi / 2)
        parts.append(flange)
    parts.append(sh.obj("core", sh.lathe([(0.012, 0.37), (0.024, 0.39), (0.026, 0.45), (0.02, 0.5), (0.0, 0.525)], 14), "iron"))
    parts.append(_knob(-0.16, 0.025, 0.018, "iron", "cap"))
    return parts


def spear():
    """Held at its middle; the point is at +Z."""
    parts = [_haft(-0.62, 1.32, 0.0145, 0.0135, "wood_light")]
    parts.append(_grip(-0.1, 0.1, 0.0165, ridges=7, mat="leather"))
    leaf = []
    n = 16
    for i in range(n + 1):
        t = i / n
        z = 1.3 + t * 0.32
        w = 0.036 * math.sin(math.pi * t ** 0.6) * (1 - t) ** 0.15
        th = 0.006 * (1 - t) + 0.001
        leaf.append([Vector((0, -w, z)), Vector((th, 0, z)), Vector((0, w, z)), Vector((-th, 0, z))] if w > 1e-4 else [Vector((0, 0, z))])
    parts.append(sh.finish(sh.obj("point", sh.loft(leaf), "steel", smooth=False), bevel=0))
    parts.append(sh.obj("socket", sh.lathe([(0.015, 1.24), (0.019, 1.27), (0.017, 1.31), (0.009, 1.33)], 12), "iron"))
    parts.append(sh.obj("butt", sh.lathe([(0.015, -0.6), (0.016, -0.63), (0.009, -0.65), (0.0, -0.66)], 12), "iron"))
    return _regrip(parts, 0.5)


def greatsword():
    parts = [_blade(1.05, 0.12, 0.028, 0.022, 0.0045, 0.002, tip=0.14)]
    parts += [_grip(-0.15, 0.1, 0.016, ridges=14, mat="leather_dark")]
    parts += _guard(0.15, 0.11, 0.011, drop=0.018)
    parts += [_knob(-0.15, 0.05, 0.026, "steel")]
    ricasso = sh.obj("ricasso", sh.lathe([(0.012, 0.12), (0.014, 0.15), (0.012, 0.18)], 10), "leather")
    parts.append(ricasso)
    return parts


def kite_shield(face="paint_blue", charge="paint_cream"):
    """A tall kite shield, curved across its width. Back plane on the origin; it bulges to -Y."""
    import bmesh
    H, W = 0.9, 0.44
    bm = bmesh.new()
    rows, cols = 18, 13
    grid = []
    for j in range(rows + 1):
        v = j / rows
        z = 0.3 - H * v
        half = W / 2 * (math.sin(math.pi * min(1.0, (1 - v) * 1.25) / 2) if v > 0.2 else 1.0) * (1 - 0.12 * max(0.0, 0.2 - v) / 0.2)
        if v >= 0.999:
            half = 0.0
        row = []
        for i in range(cols + 1):
            u = -1 + 2 * i / cols
            x = u * max(half, 1e-4)
            y = -0.012 - 0.045 * (1 - u * u) * max(0.2, 1 - v * 0.6)
            row.append(bm.verts.new(Vector((x, y, z + 0.035 * (1 - u * u) * (1 - v)))))
        grid.append(row)
    for j in range(rows):
        for i in range(cols):
            bm.faces.new((grid[j][i], grid[j][i + 1], grid[j + 1][i + 1], grid[j + 1][i]))
    # The outline, read before the tip's vertices are merged.
    outline = [grid[0][i].co.copy() for i in range(cols + 1)] + [grid[j][-1].co.copy() for j in range(1, rows + 1)]
    outline += [grid[j][0].co.copy() for j in range(rows - 1, 0, -1)]
    bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-5)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    ob = sh.obj("kite", bm, face)
    mod = ob.modifiers.new("t", "SOLIDIFY")
    mod.thickness = 0.014
    mod.offset = 1
    sh.finish(ob, bevel=0)
    parts = [ob]
    # Rim along the outline.
    parts.append(sh.obj("kite_rim", sh.sweep(outline + [outline[0]], lambda t: 0.009, n=8, up=Vector((0, 1, 0))), "iron"))
    # A pale chevron across the face.
    chev = []
    for t in (0.0, 0.5, 1.0):
        x = -W * 0.42 + W * 0.84 * t
        z = 0.02 - 0.16 * (1 - abs(2 * t - 1))
        chev.append(Vector((x, -0.06 + 0.04 * abs(2 * t - 1) ** 2, z)))
    parts.append(sh.obj("chevron", sh.ribbon(chev, [Vector((0, -1, 0))] * 3, 0.07, 0.002), charge))
    boss = sh.lathe([(0.04, 0.0), (0.036, -0.012), (0.02, -0.024), (0.0, -0.028)], 18)
    sh.transform(boss, Matrix.Translation((0, -0.057, 0.09)) @ Matrix.Rotation(math.radians(-90), 4, "X"))
    parts.append(sh.obj("kite_boss", boss, "brass"))
    for z in (-0.12, 0.1):
        parts.append(sh.obj("strap", sh.sweep([Vector((x, 0.006 + 0.03 * math.sin(math.pi * (x + 0.07) / 0.14), z))
                                                for x in [(-0.07 + 0.14 * i / 10) for i in range(11)]],
                                               lambda t: 0.012, n=8, power=4, ratio=0.25, up=Vector((0, 1, 0))), "leather"))
    return parts


def skull(scale=1.0, mat="bone"):
    """A stylised skull: cranium, brow, cheekbones and a jaw, with dark sockets."""
    cran = sh.obj("skull", sh.lathe([(0.0, 0.07), (0.03, 0.065), (0.048, 0.045), (0.054, 0.015), (0.05, -0.01),
                                     (0.038, -0.03), (0.03, -0.04), (0.0, -0.042)], 18), mat)
    cran.scale = (0.9 * scale, 1.05 * scale, scale)
    jaw = sh.obj("jaw", sh.lathe([(0.0, -0.035), (0.03, -0.038), (0.033, -0.055), (0.024, -0.07), (0.0, -0.072)], 14), mat)
    jaw.scale = (0.8 * scale, 0.85 * scale, scale)
    jaw.location = (0, -0.012 * scale, 0)
    parts = [cran, jaw]
    for sx in (-1, 1):
        eye = sh.obj("socket", sh.lathe([(0.0, -0.012), (0.012, -0.006), (0.013, 0.006), (0.0, 0.012)], 10), "leather_dark")
        eye.location = (sx * 0.02 * scale, -0.047 * scale, 0.004 * scale)
        parts.append(eye)
    nose = sh.obj("nose", sh.lathe([(0.0, -0.008), (0.007, 0.0), (0.0, 0.008)], 6), "leather_dark")
    nose.location = (0, -0.05 * scale, -0.018 * scale)
    parts.append(nose)
    return parts


def necro_staff():
    """Blackened wood with a skull, bone charms and a violet crystal. Necromancy, not horror."""
    parts = []
    path = [Vector((0.007 * math.sin(i * 0.5), 0.005 * math.cos(i * 0.4), -0.6 + 1.55 * i / 30)) for i in range(31)]
    parts.append(sh.finish(sh.obj("shaft", sh.sweep(path, lambda t: 0.015 + 0.004 * math.sin(t * 30) ** 2, n=10, up=Vector((1, 0, 0))), "wood_dark"), bevel=0))
    parts.append(_grip(-0.1, 0.12, 0.018, ridges=8, mat="leather_dark"))
    top = path[-1]
    for piece in skull(1.15):
        piece.location += top + Vector((0, 0, 0.05))
        parts.append(piece)
    gem = sh.obj("crystal", sh.loft([[top + Vector((0, 0, 0.115))],
                                     sh.ring(top + Vector((0, 0, 0.15)), Vector((1, 0, 0)), Vector((0, 1, 0)), 0.018, 0.018, 5),
                                     [top + Vector((0, 0, 0.2))]]), "crystal_violet", smooth=False)
    parts.append(gem)
    for k, z in enumerate((0.78, 0.7)):
        charm = sh.obj("charm", sh.lathe([(0.0, -0.02), (0.006, -0.012), (0.007, 0.0), (0.0, 0.012)], 8), "bone")
        charm.location = (0.03 * (1 if k else -1), 0.0, z)
        parts.append(charm)
        cord = sh.obj("cord", sh.sweep([Vector((0.0, 0, z + 0.06)), Vector((0.03 * (1 if k else -1), 0, z + 0.012))], lambda t: 0.0015, n=5, up=Vector((0, 1, 0))), "cord")
        parts.append(cord)
    return _regrip(parts, 0.6)


def healer_staff():
    """Pale ash with a sun ring cradling a green crystal, and a cloth ribbon."""
    parts = [_haft(-0.6, 0.96, 0.0155, 0.014, "wood_light"), _grip(-0.08, 0.12, 0.0175, ridges=8, mat="linen")]
    ring_path = [Vector((math.cos(a) * 0.075, 0, 1.06 + math.sin(a) * 0.075)) for a in (2 * math.pi * i / 32 for i in range(33))]
    parts.append(sh.obj("sun", sh.sweep(ring_path, lambda t: 0.007, n=8, up=Vector((0, 1, 0))), "gold"))
    for k in range(8):
        a = 2 * math.pi * k / 8
        ray = sh.obj("ray", sh.loft([[Vector((math.cos(a) * 0.08, 0.002, 1.06 + math.sin(a) * 0.08)), Vector((math.cos(a) * 0.08, -0.002, 1.06 + math.sin(a) * 0.08))],
                                     [Vector((math.cos(a) * 0.11, 0, 1.06 + math.sin(a) * 0.11))]], closed=False), "gold", smooth=False)
        parts.append(ray)
    gem = sh.obj("crystal", sh.loft([[Vector((0, 0, 1.01))],
                                     sh.ring(Vector((0, 0, 1.06)), Vector((1, 0, 0)), Vector((0, 1, 0)), 0.026, 0.026, 6),
                                     [Vector((0, 0, 1.11))]]), "crystal_green", smooth=False)
    parts.append(gem)
    parts.append(sh.obj("ribbon", sh.ribbon([Vector((0.012, 0, 0.94)), Vector((0.03, -0.01, 0.85)), Vector((0.025, 0.0, 0.74)), Vector((0.04, 0.01, 0.66))],
                                            [Vector((1, -0.3, 0))] * 4, 0.025, 0.002), "wool_cream"))
    return _regrip(parts, 0.6)


def spellbook():
    """A closed book held in the off hand: the grip is the middle of the spine."""
    W, H, T = 0.17, 0.23, 0.05
    cover = sh.obj("book", sh.loft([sh.ring(Vector((0, -W / 2 + 0.01, z)), Vector((1, 0, 0)), Vector((0, 1, 0)), T / 2, W / 2, 16, 6.0)
                                    for z in (-H / 2, H / 2)]), "leather")
    pages = sh.obj("pages", sh.loft([sh.ring(Vector((0, -W / 2 + 0.006, z)), Vector((1, 0, 0)), Vector((0, 1, 0)), T / 2 - 0.004, W / 2 - 0.004, 16, 8.0)
                                     for z in (-H / 2 + 0.006, H / 2 - 0.006)]), "linen")
    pages.location.y -= 0.003
    corners = []
    for sz in (-1, 1):
        c = sh.obj("corner", sh.lathe([(0.0, -0.01), (0.012, -0.006), (0.012, 0.006), (0.0, 0.01)], 8), "brass")
        c.location = (0, -W + 0.012, sz * (H / 2 - 0.012))
        c.scale = (T * 11, 1, 1)
        corners.append(c)
    clasp = sh.obj("clasp", sh.ribbon([Vector((T / 2, -W + 0.01, 0)), Vector((T / 2 + 0.003, -W - 0.004, 0)), Vector((-T / 2 - 0.003, -W - 0.004, 0)), Vector((-T / 2, -W + 0.01, 0))],
                                      [Vector((0, -1, 0))] * 4, 0.03, 0.003), "brass")
    gem = sh.obj("sigil", sh.lathe([(0.0, -0.004), (0.016, 0.0), (0.0, 0.004)], 6), "crystal_blue")
    gem.rotation_euler = (0, math.pi / 2, 0)
    gem.location = (T / 2 + 0.003, -W / 2, 0.02)
    return [cover, pages, clasp, gem] + corners


def focus_orb(crystal="crystal_blue"):
    """An orb in a gilded cage, cupped in the off hand."""
    orb = sh.obj("orb", sh.lathe([(0.0, -0.045), (0.032, -0.032), (0.045, 0.0), (0.032, 0.032), (0.0, 0.045)], 18), crystal)
    orb.location = (0, -0.02, 0.07)
    parts = [orb]
    for k in range(4):
        a = math.pi * k / 4
        hoop = [Vector((math.cos(a) * math.cos(t) * 0.05, math.sin(a) * math.cos(t) * 0.05 - 0.02, 0.07 + math.sin(t) * 0.05))
                for t in (math.pi * (i / 16 - 0.5) for i in range(17))]
        parts.append(sh.obj("cage", sh.sweep(hoop, lambda t: 0.003, n=6, up=Vector((0, 0, 1))), "gold"))
    base = sh.obj("base", sh.lathe([(0.0, 0.0), (0.02, 0.004), (0.03, 0.02), (0.022, 0.028)], 14), "gold")
    base.location = (0, -0.02, 0.0)
    parts.append(base)
    return parts


# --- Work tools ----------------------------------------------------------------------------------

def smith_hammer():
    parts = [_haft(-0.12, 0.3, 0.014, 0.016, "wood"), _grip(-0.11, 0.0, 0.016, ridges=5, mat="leather")]
    head = sh.obj("head", sh.loft([sh.ring(Vector((0, y, 0.32)), Vector((1, 0, 0)), Vector((0, 0, 1)), 0.022, 0.024, 12, 4.0) for y in (-0.06, 0.05)]
                                  + [sh.ring(Vector((0, 0.075, 0.32)), Vector((1, 0, 0)), Vector((0, 0, 1)), 0.008, 0.016, 12, 2.0)]), "iron")
    parts.append(sh.finish(head, bevel=0.002))
    return parts


def hoe():
    parts = [_haft(-0.5, 0.95, 0.015, 0.014, "wood_light")]
    blade = _flat_blade([(-0.02, 0.92), (-0.13, 0.9), (-0.14, 0.97), (-0.02, 0.985)], 0.006, "iron", "blade")
    blade.rotation_euler = (0, 0, 0)
    parts.append(blade)
    parts.append(sh.obj("collar", sh.lathe([(0.016, 0.9), (0.019, 0.92), (0.016, 0.99)], 10), "iron"))
    return parts


def watering_can():
    body = sh.obj("can", sh.lathe([(0.0, -0.13), (0.08, -0.13), (0.085, -0.02), (0.07, 0.02), (0.0, 0.02)], 20), "brass")
    body.location = (0, -0.0, -0.12)
    spout = sh.obj("spout", sh.sweep([Vector((0, -0.07, -0.2)), Vector((0, -0.16, -0.12)), Vector((0, -0.23, -0.07))],
                                     lambda t: 0.012 - 0.004 * t, n=8, up=Vector((1, 0, 0))), "brass")
    rose = sh.obj("rose", sh.lathe([(0.0, 0.0), (0.02, 0.005), (0.022, 0.015), (0.0, 0.012)], 12), "brass")
    rose.location = (0, -0.235, -0.065)
    rose.rotation_euler = (math.radians(-60), 0, 0)
    handle = sh.obj("handle", sh.sweep([Vector((0, 0.05, -0.12)), Vector((0, 0.02, 0.0)), Vector((0, -0.03, 0.0)), Vector((0, -0.05, -0.06))],
                                       lambda t: 0.007, n=8, up=Vector((1, 0, 0))), "brass")
    return [body, spout, rose, handle]


def lantern():
    """Carried by its ring: the body hangs below the origin."""
    top = sh.obj("cap", sh.lathe([(0.0, -0.02), (0.012, -0.022), (0.05, -0.06), (0.054, -0.068), (0.0, -0.066)], 6), "dark_steel")
    base = sh.obj("base", sh.lathe([(0.0, -0.205), (0.056, -0.205), (0.056, -0.19), (0.048, -0.185), (0.0, -0.186)], 6), "dark_steel")
    parts = [top, base]
    for k in range(6):
        a = 2 * math.pi * (k + 0.5) / 6
        bar = sh.obj("bar", sh.sweep([Vector((math.cos(a) * 0.05, math.sin(a) * 0.05, -0.19)), Vector((math.cos(a) * 0.05, math.sin(a) * 0.05, -0.066))],
                                     lambda t: 0.0028, n=6, up=Vector((1, 0, 0))), "dark_steel")
        parts.append(bar)
    parts.append(sh.obj("glass", sh.lathe([(0.0, -0.186), (0.046, -0.186), (0.046, -0.07), (0.0, -0.07)], 6), "glass"))
    parts.append(sh.obj("candle", sh.lathe([(0.0, -0.186), (0.012, -0.186), (0.012, -0.15), (0.0, -0.15)], 10), "linen"))
    parts.append(sh.obj("flame", sh.lathe([(0.0, -0.15), (0.008, -0.138), (0.0, -0.115)], 10), "flame"))
    parts.append(sh.obj("ring", sh.sweep([Vector((math.cos(a) * 0.016, 0, -0.004 + math.sin(a) * 0.016)) for a in (2 * math.pi * i / 16 for i in range(17))],
                                         lambda t: 0.0028, n=6, up=Vector((0, 1, 0))), "dark_steel"))
    return parts
