//! How SMASH looks: a sunny beach, a pedestal, the tower, the slingshot with its bands, the dotted flight, debris;
//! and the HUD over it (progress, buttons, a hint, the clear banner). Plain functions of the game's state: drawing
//! never changes it.

use sim_physics::rigid::{Quat, Shape, V3};

use super::{Button, Kind, Smash, lin};
use crate::draw3d::{Inst3, Mesh, Scene3};
use crate::font;
use crate::gesture::Px;
use crate::layer::{Color, Frame, Layer, Rect, Shape as Shape2};

/// The rotation taking +y to the unit vector `d`.
pub fn y_to(d: V3) -> Quat {
    let c = V3::Y.dot(d);
    if c > 0.9999 {
        return Quat::IDENTITY;
    }
    if c < -0.9999 {
        return Quat::axis_angle(V3::X, std::f32::consts::PI);
    }
    Quat::axis_angle(V3::Y.cross(d), c.clamp(-1.0, 1.0).acos())
}

/// A round rod from `a` to `b`.
pub fn rod(a: V3, b: V3, r: f32, color: [f32; 3]) -> Inst3 {
    let d = b - a;
    let len = d.length().max(1e-4);
    Inst3::new(Mesh::Cylinder, (a + b) * 0.5, V3::new(r, len / 2.0, r), color).rot(y_to(d * (1.0 / len)))
}

fn palm(out: &mut Vec<Inst3>, base: V3, lean: f32, height: f32) {
    let trunk = lin(0x9a6b3f);
    let leaf = lin(0x2e9e4f);
    let mut p = base;
    let segs = 5;
    for i in 0..segs {
        let t = (i + 1) as f32 / segs as f32;
        let q = base + V3::new(lean * t * t, height * t, 0.0);
        out.push(rod(p, q, 0.16 - 0.02 * t, if i % 2 == 0 { trunk } else { lin(0x8a5d36) }));
        p = q;
    }
    for k in 0..6 {
        let a = k as f32 * std::f32::consts::TAU / 6.0 + 0.3;
        let dir = V3::new(a.cos(), -0.35, a.sin()).normalized();
        let c = p + dir * 0.9;
        out.push(
            Inst3::new(Mesh::Sphere, c, V3::new(1.0, 0.07, 0.32), leaf).rot(Quat::axis_angle(V3::Y, -a) * Quat::axis_angle(V3::Z, -0.35)),
        );
    }
    out.push(Inst3::new(Mesh::Sphere, p, V3::splat(0.22), lin(0x6b4a2a)));
}

fn umbrella(out: &mut Vec<Inst3>, base: V3, a: u32, b: u32) {
    out.push(rod(base, base + V3::new(0.0, 2.1, 0.0), 0.04, lin(0xf5f0e6)));
    for k in 0..8 {
        let ang = k as f32 * std::f32::consts::TAU / 8.0;
        let col = if k % 2 == 0 { lin(a) } else { lin(b) };
        let c = base + V3::new(ang.cos() * 0.55, 1.98, ang.sin() * 0.55);
        out.push(
            Inst3::new(Mesh::Cone, c, V3::new(0.55, 0.22, 0.32), col).rot(Quat::axis_angle(V3::Y, -ang) * Quat::axis_angle(V3::Z, 1.2)),
        );
    }
}

/// The beach around the pedestal: drawn every frame, never simulated.
fn scenery(s: &Smash, out: &mut Vec<Inst3>) {
    let p = &s.t.pedestal;
    out.push(Inst3::new(Mesh::Cube, V3::new(0.0, -0.5, 0.0), V3::new(80.0, 0.5, 80.0), lin(0xf4d7a1)).no_shadow());
    // Wet sand and the sea, beyond the play area.
    out.push(Inst3::new(Mesh::Cube, V3::new(0.0, -0.49, -30.0), V3::new(80.0, 0.5, 4.0), lin(0xd9b77e)).no_shadow());
    out.push(Inst3::new(Mesh::Cube, V3::new(0.0, -0.47, -80.0), V3::new(120.0, 0.5, 47.0), lin(0x29b6e8)).glow(0.25).no_shadow());
    out.push(Inst3::new(Mesh::Cube, V3::new(0.0, -0.475, -33.6), V3::new(80.0, 0.5, 0.35), lin(0xffffff)).glow(0.6).no_shadow());
    // The pedestal: a round top on a post with a foot, like a fairground stand.
    let top = Inst3::new(
        Mesh::Cylinder,
        V3::new(0.0, p.top - p.thickness / 2.0, 0.0),
        V3::new(p.radius, p.thickness / 2.0, p.radius),
        lin(0xb8743a),
    );
    out.push(top);
    out.push(Inst3::new(
        Mesh::Cylinder,
        V3::new(0.0, p.top - p.thickness - 0.03, 0.0),
        V3::new(p.radius * 0.97, 0.03, p.radius * 0.97),
        lin(0x3b7fd9),
    ));
    let post = (p.top - p.thickness) / 2.0;
    out.push(Inst3::new(Mesh::Cylinder, V3::new(0.0, post, 0.0), V3::new(0.16, post, 0.16), lin(0xe27b2e)));
    for k in 0..3 {
        let y = post * (0.5 + 0.5 * k as f32);
        out.push(Inst3::new(Mesh::Cylinder, V3::new(0.0, y, 0.0), V3::new(0.175, 0.035, 0.175), lin(0x3b7fd9)));
    }
    out.push(Inst3::new(Mesh::Cylinder, V3::new(0.0, 0.05, 0.0), V3::new(0.55, 0.05, 0.55), lin(0x3b7fd9)));
    palm(out, V3::new(-6.5, 0.0, -9.0), 0.8, 6.0);
    palm(out, V3::new(7.5, 0.0, -13.0), -1.0, 7.0);
    palm(out, V3::new(-11.0, 0.0, -20.0), 1.2, 7.5);
    palm(out, V3::new(12.0, 0.0, -24.0), -0.6, 6.5);
    palm(out, V3::new(-4.5, 0.0, -26.0), 0.5, 6.0);
    umbrella(out, V3::new(4.8, 0.0, -6.0), 0xff5a5a, 0xffffff);
    umbrella(out, V3::new(-8.0, 0.0, -14.0), 0xffc93c, 0x3b7fd9);
    // Pebbles and shells scattered on the sand, for depth near the player.
    let mut r = super::fx::Rng::new(99);
    for _ in 0..26 {
        let at = V3::new(r.signed() * 7.0, 0.0, r.signed() * 6.0 + 3.0);
        if at.x.abs() < 1.8 && at.z > -2.0 && at.z < 9.5 {
            continue;
        }
        let sz = 0.05 + r.unit() * 0.07;
        let col = if r.unit() < 0.5 { lin(0xfff3e0) } else { lin(0xe0a37a) };
        out.push(Inst3::new(Mesh::Sphere, at, V3::new(sz * 1.3, sz * 0.5, sz), col));
    }
}

/// The slingshot: a forked stick, two bands to the pouch, the stone in it.
fn slingshot(s: &Smash, alpha: f32, out: &mut Vec<Inst3>) {
    let t = &s.t.sling;
    let rest = V3::new(t.at.0, t.at.1, t.at.2);
    let wood = lin(0x8a5a2b);
    let dark = lin(0x6e4520);
    let tip_l = rest + V3::new(-0.34, 0.02, 0.0);
    let tip_r = rest + V3::new(0.34, 0.02, 0.0);
    let fork = rest + V3::new(0.0, -0.42, 0.03);
    let foot = rest + V3::new(0.0, -1.25, 0.06);
    out.push(rod(foot, fork, 0.055, wood));
    out.push(rod(fork, tip_l, 0.045, wood));
    out.push(rod(fork, tip_r, 0.045, wood));
    out.push(Inst3::new(Mesh::Sphere, fork, V3::splat(0.07), dark));
    for tip in [tip_l, tip_r] {
        out.push(Inst3::new(Mesh::Cylinder, tip, V3::new(0.055, 0.035, 0.055), dark));
    }
    let pouch = if s.drag.is_some() { s.aim.from } else { rest + s.band.off };
    let _ = alpha;
    // A band thins as it stretches (its volume stays).
    let band = lin(0xd8342c);
    for tip in [tip_l, tip_r] {
        let side = if tip.x < 0.0 { -1.0 } else { 1.0 };
        let end = pouch + V3::new(side * 0.07, 0.0, 0.0);
        let len = (end - tip).length();
        let r = (0.022 * (0.34 / len.max(0.05)).sqrt()).clamp(0.009, 0.026);
        out.push(rod(tip, end, r, band));
    }
    out.push(Inst3::new(Mesh::Cube, pouch, V3::new(0.1, 0.05, 0.07), lin(0x5a3a22)).bevel(0.02));
    if s.loaded {
        // The new stone pops into the pouch.
        let since = (-s.reload).max(0.0);
        let pop = 1.0 + 0.25 * (-since * 9.0).exp() * (since * 30.0).sin();
        let r = s.t.stone.radius * pop.max(0.5);
        out.push(Inst3::new(Mesh::Sphere, pouch + V3::new(0.0, r * 0.6, 0.0), V3::splat(r), lin(0x2f3440)));
    }
}

fn piece(out: &mut Vec<Inst3>, kind: Kind, shape: Shape, pos: V3, rot: Quat, colour: u32) {
    let col = lin(colour);
    match (kind, shape) {
        (Kind::Can, Shape::Prism { radius, half_height, .. }) => {
            out.push(Inst3::new(Mesh::Cylinder, pos, V3::new(radius, half_height, radius), col).rot(rot));
            out.push(Inst3::new(Mesh::Cylinder, pos, V3::new(radius * 1.02, half_height * 0.38, radius * 1.02), lin(0x2f6fe0)).rot(rot));
            for y in [-1.0f32, 1.0] {
                let rim = pos + rot.rotate(V3::new(0.0, y * half_height * 0.97, 0.0));
                out.push(
                    Inst3::new(Mesh::Cylinder, rim, V3::new(radius * 0.94, half_height * 0.04, radius * 0.94), lin(0xb9c2d0)).rot(rot),
                );
            }
        }
        (Kind::Glass, Shape::Prism { radius, half_height, .. }) => {
            out.push(Inst3::new(Mesh::Cylinder, pos, V3::new(radius, half_height * 0.86, radius), col).rot(rot).glow(0.18));
            let lid = pos + rot.rotate(V3::new(0.0, half_height * 0.9, 0.0));
            out.push(Inst3::new(Mesh::Cylinder, lid, V3::new(radius * 0.8, half_height * 0.1, radius * 0.8), lin(0xff5fa2)).rot(rot));
            let shine = pos + rot.rotate(V3::new(-radius * 0.55, half_height * 0.1, radius * 0.55));
            out.push(
                Inst3::new(Mesh::Cube, shine, V3::new(0.012, half_height * 0.5, 0.012), [1.0, 1.0, 1.0]).rot(rot).glow(1.0).no_shadow(),
            );
        }
        (_, Shape::Box { half }) => {
            let bevel = (half.x.min(half.y).min(half.z) * 0.22).min(0.04);
            let mut i = Inst3::new(Mesh::Cube, pos, half, col).rot(rot).bevel(bevel);
            if kind == Kind::Shard {
                i = i.glow(0.25);
            }
            out.push(i);
        }
        (_, Shape::Prism { radius, half_height, .. }) => {
            out.push(Inst3::new(Mesh::Cylinder, pos, V3::new(radius, half_height, radius), col).rot(rot))
        }
        (_, Shape::Sphere { radius }) => out.push(Inst3::new(Mesh::Sphere, pos, V3::splat(radius), col).rot(rot)),
    }
}

/// The dotted flight and a ring where it lands, flowing towards the target.
fn flight(s: &Smash, out: &mut Vec<Inst3>) {
    let pts = &s.preview.points;
    if pts.len() < 2 {
        return;
    }
    let spacing = 0.42;
    // No dots in the first metre: they would sit on the pouch, in front of the lens.
    let skip = 1.0;
    let flow = (s.tick as f32 / 60.0 * 1.4) % spacing;
    let mut walked = 0.0;
    let mut next = spacing - flow;
    let total: f32 = pts.windows(2).map(|w| (w[1] - w[0]).length()).sum();
    for w in pts.windows(2) {
        let seg = (w[1] - w[0]).length();
        while next <= walked + seg && seg > 1e-6 {
            let p = w[0].lerp(w[1], (next - walked) / seg);
            if next < skip {
                next += spacing;
                continue;
            }
            // Dots shrink and fade towards the end, so the arc reads as a direction.
            let k = 1.0 - (next / total.max(1e-3)) * 0.55;
            out.push(Inst3::new(Mesh::Sphere, p, V3::splat(0.036 * k), [1.0, 1.0, 1.0]).glow(0.85 * k + 0.1).no_shadow());
            next += spacing;
        }
        walked += seg;
    }
    if let Some((at, n)) = s.preview.hit {
        let pulse = 1.0 + 0.12 * (s.tick as f32 / 60.0 * 9.0).sin();
        let r = s.t.stone.radius * 1.5 * pulse;
        let c = at - n * (s.t.stone.radius - 0.01);
        out.push(Inst3::new(Mesh::Cylinder, c, V3::new(r, 0.006, r), [1.0, 0.95, 0.4]).rot(y_to(n)).glow(1.0).no_shadow());
        out.push(
            Inst3::new(Mesh::Cylinder, c + n * 0.004, V3::new(r * 0.7, 0.007, r * 0.7), lin(0xff5a3c)).rot(y_to(n)).glow(1.0).no_shadow(),
        );
    }
}

/// The 3D picture at `alpha` between the last two ticks.
pub fn scene(s: &Smash, alpha: f32) -> Scene3 {
    let mut out = Vec::with_capacity(400);
    scenery(s, &mut out);
    for (_, b) in s.world.iter() {
        let kind = Kind::of_user(b.user);
        if matches!(kind, Kind::Ground | Kind::Pedestal) {
            continue;
        }
        let (pos, rot) = b.pose_at(alpha);
        let colour = if kind == Kind::Ball { 0x2f3440 } else { s.material(kind).colour };
        piece(&mut out, kind, b.shape, pos, rot, colour);
    }
    // A thin glint trail behind flying stones, once they are clear of the lens (near the camera it is a blob).
    let pouch = V3::new(s.t.sling.at.0, s.t.sling.at.1, s.t.sling.at.2);
    for st in &s.stones {
        if let Some(b) = s.world.get(st.id).filter(|_| st.hit.is_none()) {
            let (pos, _) = b.pose_at(alpha);
            let away = ((pouch - pos).length() - 1.5).clamp(0.0, 1.5) / 1.5;
            for k in 1..7 {
                let p = pos - b.vel * (k as f32 * 0.01);
                let r = s.t.stone.radius * 0.45 * (1.0 - k as f32 * 0.13) * away;
                if r > 0.005 {
                    out.push(Inst3::new(Mesh::Sphere, p, V3::splat(r), [1.0, 0.96, 0.88]).glow(0.9).no_shadow());
                }
            }
        }
    }
    for c in &s.chips.list {
        let pos = c.prev.lerp(c.pos, alpha);
        let sz = super::fx::Chips::scale(c);
        let mut i = Inst3::new(Mesh::Cube, pos, V3::splat(sz), c.color).rot(c.rot);
        if c.spark {
            i = i.glow(1.0).no_shadow();
        }
        out.push(i);
    }
    slingshot(s, alpha, &mut out);
    flight(s, &mut out);
    let sun = V3::new(-0.62, 0.68, 0.38).normalized();
    Scene3 {
        camera: s.rig.camera(alpha),
        sun,
        sun_color: [1.25, 1.12, 0.95],
        sky_top: lin(0x3d9be9),
        sky_horizon: lin(0xbfe6ff),
        ground: lin(0xd9b98a),
        fog: lin(0xcdeaff),
        fog_near: 30.0,
        fog_far: 110.0,
        shadow_center: V3::new(0.0, 1.6, 0.5),
        shadow_radius: 4.2,
        flash: s.flash,
        instances: out,
    }
}

/// The HUD's buttons (laid out on the safe area): reset and the way back to the lab at the top corners, and the
/// level switch either side of the level's name.
pub fn buttons(s: &Smash) -> [Button; 4] {
    let sa = s.safe;
    let u = s.scale;
    let (w, h) = (74.0 * u, 34.0 * u);
    let m = 14.0 * u;
    let row = level_row(s);
    let k = row.h;
    [
        Button { rect: Rect::new(sa.x + m, sa.y + m, w, h), label: "RESET" },
        Button { rect: Rect::new(sa.x + sa.w - m - w, sa.y + m, w, h), label: "LAB" },
        Button { rect: Rect::new(row.x - k - 8.0 * u, row.y, k, k), label: "-" },
        Button { rect: Rect::new(row.x + row.w + 8.0 * u, row.y, k, k), label: "+" },
    ]
}

/// Where the level's name sits: under the progress bar.
fn level_row(s: &Smash) -> Rect {
    let sa = s.safe;
    let u = s.scale;
    let w = sa.w * 0.5;
    Rect::new(sa.x + (sa.w - w) / 2.0, sa.y + 52.0 * u, w, 30.0 * u)
}

/// A banner over the middle of the screen that pops in.
fn banner(s: &Smash, since: u64, text: &str, sub: &str, colour: u32, frame: &mut Frame) {
    let u = s.scale;
    let px = 2.0 * u;
    let t = since as f32 / 60.0;
    let pop = if t < 0.35 { 0.6 + (t / 0.35) * 0.55 } else { 1.0 + 0.15 * (-(t - 0.35) * 6.0).exp() };
    let c = Px::new(s.screen.0 / 2.0, s.screen.1 * 0.36);
    let tp = px * 2.0 * pop;
    let w = font::width(text, tp).max(font::width(sub, px * 1.2)) + 44.0 * u;
    let rect = Rect::new(c.x - w / 2.0, c.y - 46.0 * u * pop, w, 92.0 * u * pop);
    frame.push(Layer::Overlay, 0, Shape2::Box { rect, r: 30.0 * u * pop, color: Color::hex(colour) });
    let ink = Color::hex(0x1d2a4a);
    font::centered(frame, Layer::Overlay, 1, Px::new(c.x, c.y - 14.0 * u * pop), tp, ink, text);
    font::centered(frame, Layer::Overlay, 1, Px::new(c.x, c.y + 24.0 * u * pop), px * 1.2, ink, sub);
}

/// The HUD: buttons, how much of the tower is left, the level and its stones, a hint before the first shot, and
/// the banners for a cleared or a lost level.
pub fn hud(s: &Smash, _alpha: f32, frame: &mut Frame) {
    let u = s.scale;
    let px = 2.0 * u;
    let ink = Color::hex(0x1d2a4a);
    let white = Color::hex(0xffffff);
    for b in buttons(s) {
        frame.push(Layer::Hud, 0, Shape2::Box { rect: b.rect, r: b.rect.h / 2.0, color: Color::hexa(0xffffff, 0.85) });
        font::centered(frame, Layer::Hud, 1, b.rect.center(), px * 0.9, ink, b.label);
    }
    // Progress: the share of the tower knocked off.
    let total = s.total().max(1);
    let done = total - s.left().min(total);
    let sa = s.safe;
    let bw = sa.w * 0.36;
    let bar = Rect::new(sa.x + (sa.w - bw) / 2.0, sa.y + 22.0 * u, bw, 20.0 * u);
    frame.push(Layer::Hud, 0, Shape2::Box { rect: bar, r: bar.h / 2.0, color: Color::hexa(0x1d2a4a, 0.55) });
    let k = done as f32 / total as f32;
    if k > 0.0 {
        let fill = Rect::new(bar.x + 3.0 * u, bar.y + 3.0 * u, (bar.w - 6.0 * u) * k, bar.h - 6.0 * u);
        frame.push(Layer::Hud, 1, Shape2::Box { rect: fill, r: fill.h / 2.0, color: Color::hex(0xffc93c) });
    }
    font::centered(frame, Layer::Hud, 2, bar.center(), px * 0.75, white, &format!("{done}/{total}"));
    // The level, and its stones as dots: full for each one left.
    let row = level_row(s);
    frame.push(Layer::Hud, 0, Shape2::Box { rect: row, r: row.h / 2.0, color: Color::hexa(0x1d2a4a, 0.45) });
    let name = format!("{} {}", s.level + 1, s.level().name);
    font::centered(frame, Layer::Hud, 1, row.center(), px * 0.8, white, &name);
    let n = s.level().stones;
    let r = 7.0 * u;
    let gap = 22.0 * u;
    let y = row.y + row.h + 18.0 * u;
    let x0 = sa.x + sa.w / 2.0 - gap * (n as f32 - 1.0) / 2.0;
    for i in 0..n {
        let c = Px::new(x0 + gap * i as f32, y);
        let left = i < s.stones_left + u32::from(s.loaded && s.stones_left == 0);
        frame.push(Layer::Hud, 0, Shape2::circle(c, r + 2.0 * u, Color::hexa(0xffffff, 0.8)));
        frame.push(Layer::Hud, 1, Shape2::circle(c, r, if left { Color::hex(0x2f3440) } else { Color::hexa(0x2f3440, 0.15) }));
    }
    if s.shots == 0 && s.drag.is_none() && s.level == 0 {
        let y = sa.y + sa.h - 70.0 * u;
        let wob = (s.tick as f32 / 60.0 * 3.0).sin() * 6.0 * u;
        font::centered(frame, Layer::Hud, 1, Px::new(sa.x + sa.w / 2.0, y + wob), px, white, "PULL DOWN TO AIM");
    }
    if let Some(won) = s.won {
        let stars = s.stars();
        let sub = format!("{} OF 3 STARS", stars);
        banner(s, s.tick - won, "LEVEL CLEAR", &sub, 0xffc93c, frame);
        // The stars themselves, popping in one after another.
        let t = (s.tick - won) as f32 / 60.0;
        for i in 0..3u32 {
            let shown = t > 0.35 + i as f32 * 0.18;
            let c = Px::new(s.screen.0 / 2.0 + (i as f32 - 1.0) * 46.0 * u, s.screen.1 * 0.36 + 80.0 * u);
            let on = i < stars && shown;
            let size = if on { 15.0 * u } else { 11.0 * u };
            frame.push(Layer::Overlay, 2, Shape2::circle(c, size + 3.0 * u, Color::hex(0x1d2a4a)));
            frame.push(Layer::Overlay, 3, Shape2::circle(c, size, if on { Color::hex(0xfff3a0) } else { Color::hexa(0xffffff, 0.35) }));
        }
    } else if let Some(lost) = s.lost {
        banner(s, s.tick - lost, "OUT OF STONES", "TRY AGAIN", 0xff8a7a, frame);
    }
}
