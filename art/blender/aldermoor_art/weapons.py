"""Weapons, shields, bows and quivers, modelled in code.

Every item is authored around its attachment point: up is +Z (blade tip, shaft end, shield top)
and front is -Y (cutting edge, bow back, shield face). Each builder returns a list of objects.
"""

import math

from mathutils import Matrix, Vector

from . import shapes as sh


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


def hunting_bow():
    """A recurve bow held at its grip. The string side is +Y (towards the archer)."""
    brace = 0.17
    path_up = []
    for i in range(21):
        t = i / 20
        z = 0.085 + t * 0.62
        bend = brace * 0.55 * t ** 1.7
        recurve = -0.05 * max(0.0, (t - 0.8) / 0.2) ** 2
        path_up.append(Vector((0, bend + recurve, z)))
    limbs = []
    for s in (1, -1):
        path = [Vector((p.x, p.y, p.z * s)) for p in path_up]
        limb = sh.obj("limb", sh.sweep(path, lambda t: 0.017 * (1 - 0.55 * t), n=10, power=2.4, ratio=0.45,
                                        up=Vector((0, 1, 0))), "wood")
        limbs.append(sh.finish(limb, bevel=0))
    riser_path = [Vector((0, -0.006 * math.sin(math.pi * i / 10), -0.09 + 0.18 * i / 10)) for i in range(11)]
    riser = sh.finish(sh.obj("riser", sh.sweep(riser_path, lambda t: 0.019 - 0.004 * abs(2 * t - 1), n=12, power=2.2,
                                                 ratio=0.8, up=Vector((0, 1, 0))), "wood_dark"), bevel=0)
    wrap = _grip(-0.055, 0.055, 0.0205, ridges=10, mat="leather")
    tips = [Vector((0, path_up[-1].y, path_up[-1].z)), Vector((0, path_up[-1].y, -path_up[-1].z))]
    string = sh.obj("string", sh.sweep([tips[0], tips[1]], lambda t: 0.0011, n=6, up=Vector((1, 0, 0))), "cord")
    nocks = []
    for p in tips:
        k = sh.obj("nock", sh.lathe([(0, -0.012), (0.0075, -0.006), (0.0075, 0.006), (0, 0.012)], 10), "bone")
        k.location = p
        nocks.append(k)
    return limbs + [riser, wrap, string] + nocks


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
    """A wizard's staff held a third of the way up; the head is at +Z."""
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
    return parts


def wand(crystal="crystal_violet"):
    prof = [(0.0001, -0.07), (0.009, -0.068), (0.0115, -0.05), (0.0105, -0.02), (0.012, 0.0), (0.0105, 0.03),
            (0.0085, 0.05), (0.0065, 0.15), (0.0048, 0.25), (0.0042, 0.27)]
    body = sh.finish(sh.obj("wand", sh.lathe(prof, 12), "wood_dark"), bevel=0)
    collar = sh.obj("collar", sh.lathe([(0.009, 0.045), (0.0105, 0.05), (0.0105, 0.058), (0.008, 0.062)], 12), "silver")
    tip = sh.obj("tip", sh.loft([[Vector((0, 0, 0.265))],
                                 sh.ring(Vector((0, 0, 0.278)), Vector((1, 0, 0)), Vector((0, 1, 0)), 0.008, 0.008, 6),
                                 [Vector((0, 0, 0.3))]]), crystal, smooth=False)
    return [body, collar, tip]
