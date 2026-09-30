#!/usr/bin/env python3
"""Lay out a 1.5-mile quad-oval from Charlotte Motor Speedway's published figures and write tracks/charlotte.ron.

Published (charlottemotorspeedway.com, Track Facts): 1.5 mi (7,920 ft); frontstretch 1,980 ft, backstretch 1,500 ft;
turns 1-2 2,400 ft long with a 685 ft radius, turns 3-4 2,040 ft with 625 ft; turns banked 24°, straights 5°.

A turn is longer than a circular arc of its radius would be for the angle it turns: real turns ease in and out
with transition spirals (clothoids), the published radius being the tightest part. An eased turn of radius R,
angle θ and spiral length Ls is θ·R + Ls long, so each spiral is Ls = length − θ·R: the published length and
radius fix it (turns 1-2: ~138 m, 3-4: ~80 m). Banking rises along the spirals. Not published: the shape of the
quad-oval's dogleg; here one eased bend of radius DOGLEG_R with DOGLEG_SPIRAL spirals, its angle and the
straights either side solved so the lap closes with the published frontstretch length. The lap starts just past
the dogleg (the real start/finish line is at its apex, ~70 m earlier). Charlotte's lengths, radii and banking on
an approximate plan, not a survey of the real track.

    games/race/gen_track.py            # writes games/race/tracks/charlotte.ron and prints its extent
"""

import math
from pathlib import Path

FT = 0.3048
R12, R34 = 685 * FT, 625 * FT
L12, L34 = 2400 * FT, 2040 * FT
BACK, FRONT = 1500 * FT, 1980 * FT
DOGLEG_R, DOGLEG_SPIRAL = 500.0, 60.0
WIDTH = 18.0  # m, assumed (not published)
TURN_BANK, STRAIGHT_BANK = 24.0, 5.0


def eased_turned(r, ls, length, t):
    """Heading turned t metres into an eased turn (the engine's own formula)."""
    t = min(max(t, 0.0), length)
    f = lambda u: u * u / (2 * ls * r)
    if ls == 0:
        return t / r
    if t < ls:
        return f(t)
    if t <= length - ls:
        return f(ls) + (t - ls) / r
    return (length - ls) / r - f(length - t)


def walk(segs, x=0.0, y=0.0, h=0.0, step=0.02):
    """End point of ("S", len) and ("E", radius, angle, spiral) pieces laid from (x, y) heading h."""
    for s in segs:
        if s[0] == "S":
            x, y = x + s[1] * math.cos(h), y + s[1] * math.sin(h)
            continue
        _, r, ang, ls = s
        length = ang * r + ls
        t = 0.0
        while t < length:
            dt = min(step, length - t)
            hd = h + eased_turned(r, ls, length, t + dt / 2)
            x, y = x + dt * math.cos(hd), y + dt * math.sin(hd)
            t += dt
        h += ang
    return x, y, h


def pieces(a, b, k):
    turn = math.pi - k
    return [("S", b), ("E", R12, turn, L12 - turn * R12), ("S", BACK), ("E", R34, turn, L34 - turn * R34), ("S", a),
            ("E", DOGLEG_R, 2 * k, DOGLEG_SPIRAL)]


def straights(k, rounded=None):
    """The straights a (before the dogleg) and b (after it) that close the lap: the end point is linear in them."""
    ps = rounded or pieces(0.0, 0.0, k)
    ps = [("S", 0.0) if p[0] == "S" and i in (0, 4) else p for i, p in enumerate(ps)]
    x0, y0, _ = walk(ps)
    ha = sum(p[2] for p in ps[:4] if p[0] == "E")  # heading along a
    # b runs at heading 0, a at heading ha: x0 + b + a·cos ha = 0, y0 + a·sin ha = 0.
    a = -y0 / math.sin(ha)
    b = -x0 - a * math.cos(ha)
    return a, b


def front(k):
    a, b = straights(k)
    return a + b + (2 * k * DOGLEG_R + DOGLEG_SPIRAL)


def main():
    lo, hi = 0.05, 0.6
    for _ in range(80):
        mid = (lo + hi) / 2
        if front(mid) < FRONT:
            lo = mid
        else:
            hi = mid
    k = (lo + hi) / 2
    a, b = straights(k)
    assert a > 0 and b > 0, (a, b, k)
    # Round as the file stores (mm, hundredths of a degree), angles summing to exactly 360.00°, then solve the
    # straights again for the rounded pieces.
    ps = pieces(a, b, k)
    cds = [round(math.degrees(p[2]) * 100) if p[0] == "E" else None for p in ps]
    turns = [i for i, c in enumerate(cds) if c is not None]
    cds[max(turns, key=lambda i: cds[i])] += 36000 - sum(cds[i] for i in turns)
    ps = [("E", round(p[1] * 1000) / 1000, math.radians(c / 100), round(p[3] * 1000) / 1000) if c is not None else p
          for p, c in zip(ps, cds)]
    a, b = straights(k, ps)
    ps[0], ps[4] = ("S", round(b * 1000) / 1000), ("S", round(a * 1000) / 1000)
    x, y, _ = walk(ps)
    assert math.hypot(x, y) < 0.005, f"closes {math.hypot(x, y) * 1000:.1f} mm off"
    names = ["Frontstretch", "Turns 1-2", "Backstretch", "Turns 3-4", "Frontstretch", "Tri-oval"]
    lines = []
    for p, c, name in zip(ps, cds, names):
        if p[0] == "S":
            lines.append(f'        (shape: Straight({round(p[1] * 1000)}), bank: {round(STRAIGHT_BANK * 100)}, name: "{name}"),')
        else:
            bank = TURN_BANK if name.startswith("Turns") else STRAIGHT_BANK
            lines.append(f'        (shape: EasedLeft({round(p[1] * 1000)}, {c}, {round(p[3] * 1000)}), '
                         f'bank: {round(bank * 100)}, name: "{name}"),')
    out = Path(__file__).parent / "tracks" / "charlotte.ron"
    out.write_text(
        "// Generated by games/race/gen_track.py: Charlotte Motor Speedway's published length, radii, turn lengths and\n"
        "// banking; turns eased by transition spirals; the dogleg's shape approximate. mm, hundredths of a degree.\n"
        "(\n"
        '    name: "1.5-mile quad-oval (Charlotte dimensions)",\n'
        f"    width: {round(WIDTH * 1000)},\n"
        "    start: (0, 0, 0),\n"
        "    // Inside: the apron, then the infield grass (70 % grip, no wall). Outside: the SAFER barrier at the edge.\n"
        "    left: (width: 20000, grip: 700, wall: false),\n"
        "    right: (width: 0, wall: true),\n"
        "    segments: [\n" + "\n".join(lines) + "\n    ],\n)\n"
    )
    # Extent (centreline ± 40 m), to place the track in a world.
    xs, ys = [], []
    X = Y = H = 0.0
    for p in ps:
        n = 200
        for j in range(n):
            if p[0] == "S":
                X, Y = X + p[1] / n * math.cos(H), Y + p[1] / n * math.sin(H)
            else:
                X, Y, H2 = walk([("E", p[1], p[2] / n, 0.0)], X, Y, H)
                H = H2
            xs.append(X)
            ys.append(Y)
    lap = sum(p[1] if p[0] == "S" else p[2] * p[1] + p[3] for p in ps)
    print(f"lap {lap:.1f} m; spirals {ps[1][3]:.1f} m (turns 1-2), {ps[3][3]:.1f} m (turns 3-4); dogleg "
          f"{math.degrees(2 * k):.1f}°; straights a {a:.1f} b {b:.1f}; extent x {min(xs) - 40:.0f}..{max(xs) + 40:.0f}, "
          f"y {min(ys) - 40:.0f}..{max(ys) + 40:.0f} m; wrote {out}")


if __name__ == "__main__":
    main()
