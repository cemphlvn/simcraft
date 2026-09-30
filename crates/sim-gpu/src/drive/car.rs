//! A stock car, as geometry in its own frame (x right, y up, z forward, the origin on the ground under its middle;
//! a Next Gen Cup car's proportions: 4.9 m long, 2.0 m wide, 1.3 m high, 2.87 m wheelbase): the body others see,
//! and the cockpit you sit in (dash, digital display with shift lights, switch panel, steering wheel and hands,
//! roll cage, A-pillars, the window net, the rear-view mirror, the windshield with its sun strip).

use super::geom::{Builder, FONT, Frame, MIRROR, NET, char_w, rgb, rgba, text3};
use super::photos::{Found, Photos};
use super::{Cockpit, MirrorLook};
use crate::math::{Eye, V3};
use crate::stage::{BLOB, WHITE, Wrap};

pub const WHEELBASE: f32 = 2.87;
const TRACK_W: f32 = 0.86;
const WHEEL_R: f32 = 0.33;

/// What a car looks like: paint, an accent, its number.
#[derive(Clone, Debug)]
pub struct CarLook {
    pub paint: [f32; 3],
    pub accent: [f32; 3],
    pub number: i64,
}

/// Body sections along the car: (z, half width, bottom, top).
const SECTIONS: [(f32, f32, f32, f32); 7] = [
    (-2.45, 0.93, 0.18, 0.96),
    (-2.1, 0.97, 0.15, 0.98),
    (-1.05, 0.98, 0.15, 0.98),
    (0.95, 0.98, 0.15, 0.90),
    (1.7, 0.98, 0.15, 0.80),
    (2.2, 0.92, 0.13, 0.68),
    (2.45, 0.80, 0.12, 0.56),
];

/// The front wheels' steering angle (radians) for a steer of -1..1: a road car's lock, about 30°.
pub fn road_wheel_angle(steer: f32) -> f32 {
    steer.clamp(-1.0, 1.0) * 0.5
}

/// The body: what the others (and the chase camera) see. `hood_only`: from inside, only what shows through the
/// windshield and the rear window (the hood, the fenders, the deck and spoiler).
pub fn body(b: &mut Builder, text: &mut Builder, fr: &Frame, look: &CarLook, steer: f32, hood_only: bool) {
    let paint = look.paint;
    let dark = [0.07, 0.07, 0.08];
    let glass = [0.06, 0.07, 0.09];
    let p = |x: f32, y: f32, z: f32| fr.at(V3(x, y, z));
    let uv = [[0.0; 2]; 4];
    for w in SECTIONS.windows(2) {
        let ((z0, w0, _, t0), (z1, w1, _, t1)) = (w[0], w[1]);
        let rear_deck = z1 <= -1.05 + 1e-3;
        let front = z0 >= 0.95 - 1e-3;
        if hood_only && !(rear_deck || front) {
            continue;
        }
        // Top (deck, hood), and the sides from the shoulder down to the wheel arches.
        b.quad([p(-w0, t0, z0), p(w0, t0, z0), p(w1, t1, z1), p(-w1, t1, z1)], uv, paint, 1.0);
        for s in [-1.0f32, 1.0] {
            b.quad([p(s * w0, 0.36, z0), p(s * w1, 0.36, z1), p(s * w1, t1, z1), p(s * w0, t0, z0)], uv, paint, 1.0);
            // A stripe along the shoulder, in the accent colour.
            let (m0, m1) = (t0 - 0.12, t1 - 0.12);
            b.quad(
                [
                    p(s * (w0 + 0.004), m0, z0),
                    p(s * (w1 + 0.004), m1, z1),
                    p(s * (w1 + 0.004), m1 + 0.05, z1),
                    p(s * (w0 + 0.004), m0 + 0.05, z0),
                ],
                uv,
                look.accent,
                1.0,
            );
        }
    }
    if !hood_only {
        // Skirts between the wheels and the bumpers below the arches.
        for s in [-1.0f32, 1.0] {
            for (za, zb) in [(-0.95, 0.95), (-2.45, -1.9), (1.9, 2.45)] {
                b.quad(
                    [p(s * 0.97, 0.13, za), p(s * 0.97, 0.13, zb), p(s * 0.97, 0.36, zb), p(s * 0.97, 0.36, za)],
                    uv,
                    paint.map(|v| v * 0.85),
                    1.0,
                );
            }
        }
        // Tail and nose faces.
        let (zr, wr, br, tr) = SECTIONS[0];
        b.quad([p(wr, br, zr), p(-wr, br, zr), p(-wr, tr, zr), p(wr, tr, zr)], uv, paint.map(|v| v * 0.8), 1.0);
        b.glow(
            [p(-0.8, 0.8, zr - 0.004), p(-0.45, 0.8, zr - 0.004), p(-0.45, 0.88, zr - 0.004), p(-0.8, 0.88, zr - 0.004)],
            uv,
            [0.7, 0.05, 0.05, 1.0],
        );
        b.glow(
            [p(0.45, 0.8, zr - 0.004), p(0.8, 0.8, zr - 0.004), p(0.8, 0.88, zr - 0.004), p(0.45, 0.88, zr - 0.004)],
            uv,
            [0.7, 0.05, 0.05, 1.0],
        );
        let (zn, wn, bn, tn) = SECTIONS[6];
        b.quad([p(-wn, bn, zn), p(wn, bn, zn), p(wn, tn, zn), p(-wn, tn, zn)], uv, dark, 1.0);
        // Greenhouse: windshield, roof, rear window, side glass.
        let (wb, wt) = (0.9, 0.68);
        b.quad([p(-wb, 0.90, 0.95), p(wb, 0.90, 0.95), p(wt, 1.27, 0.2), p(-wt, 1.27, 0.2)], uv, glass, 1.0);
        b.quad([p(-wt, 1.27, 0.2), p(wt, 1.27, 0.2), p(wt, 1.27, -0.55), p(-wt, 1.27, -0.55)], uv, paint, 1.0);
        b.quad([p(-wt, 1.27, -0.55), p(wt, 1.27, -0.55), p(wb, 0.98, -1.05), p(-wb, 0.98, -1.05)], uv, glass, 1.0);
        for s in [-1.0f32, 1.0] {
            b.quad([p(s * wb, 0.90, 0.95), p(s * wt, 1.27, 0.2), p(s * wt, 1.27, -0.55), p(s * wb, 0.98, -1.05)], uv, glass, 1.0);
        }
        // Headlight decals on the nose.
        let (za, zb) = (2.2, 2.42);
        for s in [-1.0f32, 1.0] {
            let (x0, x1) = (s * 0.45, s * 0.78);
            b.glow([p(x0, 0.69, za), p(x1, 0.66, za), p(x1 * 0.95, 0.6, zb), p(x0, 0.6, zb)], uv, [0.95, 0.92, 0.7, 1.0]);
        }
        // Wheels: the tyre (a short tube) and its face; the fronts turn with the steering.
        let delta = road_wheel_angle(steer);
        for (x, z, front) in [(-TRACK_W, 1.435, true), (TRACK_W, 1.435, true), (-TRACK_W, -1.435, false), (TRACK_W, -1.435, false)] {
            let a = if front { delta } else { 0.0 };
            let axle = V3(a.cos(), 0.0, a.sin()).scale(0.15);
            let c = V3(x, WHEEL_R, z);
            b.tube(fr, c - axle, c + axle, WHEEL_R, 12, [0.1, 0.1, 0.1]);
            let side = if x > 0.0 { 1.0 } else { -1.0 };
            b.disc(fr, c + axle.scale(side), axle.scale(side), 0.22, 10, [0.55, 0.5, 0.42]);
        }
        // The spoiler across the tail.
        b.cuboid(fr, V3(0.0, 1.03, -2.42), V3(0.86, 0.05, 0.01), dark);
        // Splitter.
        b.cuboid(fr, V3(0.0, 0.1, 2.45), V3(0.9, 0.012, 0.12), dark);
        // Numbers: on both doors and on the roof (read from the right, where the grandstands are).
        let num = look.number.to_string();
        let h = 0.42;
        let wdt = char_w(h) * num.len() as f32;
        let white = [0.97, 0.97, 0.97, 1.0];
        let r = fr.dir(V3(0.0, 0.0, char_w(h)));
        let down = fr.dir(V3(0.0, -h, 0.0));
        text3(text, &num, p(0.984, 0.8, -0.05 - wdt / 2.0), r, down, white);
        let r_left = fr.dir(V3(0.0, 0.0, -char_w(h)));
        text3(text, &num, p(-0.984, 0.8, -0.05 + wdt / 2.0), r_left, down, white);
        let rh = 0.5;
        let rw = char_w(rh) * num.len() as f32;
        text3(text, &num, p(-rh / 2.0, 1.275, -0.18 - rw / 2.0), fr.dir(V3(0.0, 0.0, char_w(rh))), fr.dir(V3(rh, 0.0, 0.0)), white);
    }
    if hood_only {
        // The hood's louvers and pins, seen through the windshield.
        for s in [-1.0f32, 1.0] {
            b.quad([p(s * 0.18, 0.875, 1.25), p(s * 0.5, 0.875, 1.25), p(s * 0.5, 0.83, 1.6), p(s * 0.18, 0.83, 1.6)], uv, dark, 1.0);
        }
        b.cuboid(fr, V3(0.0, 1.03, -2.42), V3(0.86, 0.05, 0.01), dark);
    }
}

/// A soft dark patch on the ground under a car (contact shadow).
pub fn shadow(b: &mut Builder, fr: &Frame) {
    let lift = V3(0.0, 0.03, 0.0);
    let p = |x: f32, z: f32| fr.at(V3(x, 0.0, z)) + lift;
    b.glow(
        [p(-1.25, -2.9), p(1.25, -2.9), p(1.25, 2.9), p(-1.25, 2.9)],
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        [0.0, 0.0, 0.0, 0.55],
    );
}

/// What the dash shows.
#[derive(Clone, Debug, Default)]
pub struct Dash {
    pub speed: f32,
    pub rpm: f32,
    pub gear: i64,
    /// -1..1 (positive: left).
    pub steer: f32,
    pub position: usize,
    pub cars: usize,
    pub lap: i64,
    pub time: f32,
    /// Speed as shown (mph or km/h) and its unit.
    pub speed_shown: f32,
    pub unit: &'static str,
}

/// The steering wheel's rotation (degrees, counterclockwise as the driver sees it) for a steer of -1..1.
pub fn wheel_angle(steer: f32, lock: f32) -> f32 {
    steer.clamp(-1.0, 1.0) * lock
}

/// The shift lights lit (of `n`) at `rpm`, and whether they flash (at or past the shift point).
pub fn shift_lights(rpm: f32, from: f32, at: f32, n: usize) -> (usize, bool) {
    if rpm >= at {
        return (n, true);
    }
    let t = ((rpm - from) / (at - from)).clamp(0.0, 1.0);
    ((t * n as f32).floor() as usize, false)
}

/// The mirror: where it hangs (car frame) and the camera that sees what it reflects (the driver's eye mirrored in
/// its glass). `eye` is the driver's eye in the car frame.
pub fn mirror_eye(fr: &Frame, m: &MirrorLook, eye: V3) -> Eye {
    let at = V3(m.at.0, m.at.1, m.at.2);
    // Aimed so the driver sees over the spoiler, down the track behind.
    let to_eye = (eye - at).norm();
    let to_rear = (V3(0.0, 1.2, -9.0) - at).norm();
    let n = (to_eye + to_rear).norm();
    let virt = eye - n.scale(2.0 * (eye - at).dot(n));
    let d = at - eye;
    let look = d - n.scale(2.0 * d.dot(n));
    let dist = (at - eye).dot(at - eye).sqrt();
    let (wp, lp) = (fr.at(virt), fr.at(virt + look.norm()));
    let fov = 2.0 * (m.size.1 / 2.0 / dist).atan().to_degrees();
    let mut e = Eye { pos: wp, target: lp, roll: 0.0, fov, near: dist, far: 2500.0 };
    // Keep the car's up (its roll on the banking).
    let (r0, u0, _) = Eye { roll: 0.0, ..e }.basis();
    e.roll = (-fr.u.dot(r0)).atan2(fr.u.dot(u0)).to_degrees();
    e
}

/// The cockpit, into the builders of `Parts` (drawn in their order): `solid` (untextured), the dash's and the
/// wheel's photographs when the game has them, `text` (the font), `net`, `mirror` (the mirror's picture), `glass`
/// (the windshield, last: it is see-through).
pub fn cockpit(parts: &mut Parts, fr: &Frame, c: &Cockpit, look: &CarLook, dash: &Dash) {
    let Parts { solid, text, net, mirror, glass, dash_photo, wheel_photo, .. } = parts;
    let p = |x: f32, y: f32, z: f32| fr.at(V3(x, y, z));
    let uv = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let black = [0.045, 0.045, 0.05];
    let flock = [0.07, 0.07, 0.075];
    let cage = rgb(c.cage);
    let ex = c.eye.0;
    // Dash top (flocked, so it does not reflect in the glass) and its face.
    solid.quad([p(-0.85, 0.86, 0.33), p(0.85, 0.86, 0.33), p(0.88, 0.90, 0.95), p(-0.88, 0.90, 0.95)], uv, flock, 1.0);
    solid.quad([p(-0.85, 0.55, 0.33), p(0.85, 0.55, 0.33), p(0.85, 0.86, 0.33), p(-0.85, 0.86, 0.33)], uv, black, 1.0);
    // The display in front of the driver: a black glass panel, the gear big in the middle, speed and rpm beside it;
    // on the dash's photograph, in the screen the picture has (`cockpit.display`), under its LEDs (`lights`).
    let (mut dw, mut dh, dz) = (0.27, 0.11, 0.326);
    let (mut dx0, mut dy0) = (ex - dw / 2.0, 0.835);
    let mut leds = (ex - 0.12, 0.0267, 0.848);
    match dash_photo {
        Some((b, f)) => {
            let (w, h, top) = (f.size, f.size / f.aspect(), c.dash_top);
            let x0 = ex - w / 2.0;
            b.quad([p(x0, top, 0.329), p(x0 + w, top, 0.329), p(x0 + w, top - h, 0.329), p(x0, top - h, 0.329)], uv, [1.0; 3], 1.0);
            let (u0, v0, u1, v1) = c.display;
            (dx0, dy0, dw, dh) = (x0 + w * u0, top - h * v0, w * (u1 - u0), h * (v1 - v0));
            let (l0, lv, l1) = c.lights;
            leds = (x0 + w * l0, w * (l1 - l0) / 9.0, top - h * lv);
        }
        None => solid.glow(
            [p(dx0, dy0, dz + 0.002), p(dx0 + dw, dy0, dz + 0.002), p(dx0 + dw, dy0 - dh, dz + 0.002), p(dx0, dy0 - dh, dz + 0.002)],
            uv,
            [0.01, 0.012, 0.015, 1.0],
        ),
    }
    let right = |h: f32| fr.dir(V3(char_w(h), 0.0, 0.0));
    let down = |h: f32| fr.dir(V3(0.0, -h, 0.0));
    let gear = match dash.gear {
        0 => "N".to_string(),
        g if g < 0 => "R".to_string(),
        g => g.to_string(),
    };
    let gh = 0.06;
    let mid = dx0 + dw / 2.0;
    text3(text, &gear, p(mid - char_w(gh) / 2.0, dy0 - dh / 2.0 + gh / 2.0, dz), right(gh), down(gh), [1.0, 1.0, 1.0, 1.0]);
    let sh = 0.024;
    let spd = format!("{:>3}", dash.speed_shown.round() as i64);
    text3(text, &spd, p(dx0 + 0.012, dy0 - 0.03, dz), right(sh), down(sh), [0.95, 0.95, 0.95, 1.0]);
    text3(text, dash.unit, p(dx0 + 0.012, dy0 - 0.062, dz), right(0.012), down(0.012), [0.6, 0.6, 0.6, 1.0]);
    let rpm = format!("{:>5}", (dash.rpm / 10.0).round() as i64 * 10);
    text3(text, &rpm, p(dx0 + dw - 0.012 - 5.0 * char_w(sh), dy0 - 0.03, dz), right(sh), down(sh), [0.95, 0.95, 0.95, 1.0]);
    text3(text, "RPM", p(dx0 + dw - 0.012 - 3.0 * char_w(0.012), dy0 - 0.062, dz), right(0.012), down(0.012), [0.6, 0.6, 0.6, 1.0]);
    let pos = format!("P{} L{}", dash.position, dash.lap.max(0));
    text3(text, &pos, p(dx0 + 0.012, dy0 - 0.09, dz), right(0.014), down(0.014), [0.9, 0.8, 0.3, 1.0]);
    // Shift lights above the display: green, yellow, red, then all flash at the shift point.
    let n = 10;
    let (lit, flash) = shift_lights(dash.rpm, c.shift_lights.0, c.shift_lights.1, n);
    let blink_on = (dash.time * 10.0).fract() < 0.5;
    for i in 0..n {
        let x = leds.0 + i as f32 * leds.1;
        let col = if i < 4 {
            [0.1, 0.95, 0.2]
        } else if i < 7 {
            [1.0, 0.8, 0.05]
        } else {
            [1.0, 0.1, 0.1]
        };
        let c4 = if flash {
            if blink_on { [0.2, 0.4, 1.0, 1.0] } else { [0.08, 0.08, 0.09, 1.0] }
        } else if i < lit {
            [col[0], col[1], col[2], 1.0]
        } else {
            [col[0] * 0.12, col[1] * 0.12, col[2] * 0.12, 1.0]
        };
        let (y0, s) = (leds.2, 0.006);
        solid.glow([p(x - s, y0 + s, 0.327), p(x + s, y0 + s, 0.327), p(x + s, y0 - s, 0.327), p(x - s, y0 - s, 0.327)], uv, c4);
    }
    // The switch panel right of centre: rows of toggles, two lit.
    for row in 0..3 {
        for col in 0..4 {
            let (x, y) = (0.06 + col as f32 * 0.06, 0.78 - row as f32 * 0.06);
            solid.cuboid(fr, V3(x, y, 0.32), V3(0.018, 0.013, 0.01), [0.12, 0.12, 0.13]);
            solid.tube(fr, V3(x, y, 0.31), V3(x, y + 0.01, 0.285), 0.004, 4, [0.7, 0.7, 0.7]);
        }
    }
    solid.glow([p(0.3, 0.8, 0.309), p(0.32, 0.8, 0.309), p(0.32, 0.78, 0.309), p(0.3, 0.78, 0.309)], uv, [0.1, 0.9, 0.2, 1.0]);
    solid.glow([p(0.3, 0.74, 0.309), p(0.32, 0.74, 0.309), p(0.32, 0.72, 0.309), p(0.3, 0.72, 0.309)], uv, [0.95, 0.2, 0.1, 1.0]);
    // A-pillars, the centre windshield post, the header and the roof lining.
    for s in [-1.0f32, 1.0] {
        let (b0, t0) = (V3(s * 0.88, 0.90, 0.95), V3(s * 0.69, 1.265, 0.2));
        let inward = V3(-s * 0.1, 0.0, -0.02);
        solid.quad([fr.at(b0), fr.at(t0), fr.at(t0 + inward), fr.at(b0 + inward)], uv, black, 1.0);
        solid.tube(fr, b0 + inward.scale(0.9), t0 + inward.scale(0.9), 0.022, 6, cage);
    }
    solid.tube(fr, V3(0.0, 0.905, 0.93), V3(0.0, 1.255, 0.22), 0.018, 6, black);
    solid.tube(fr, V3(-0.69, 1.245, 0.2), V3(0.69, 1.245, 0.2), 0.022, 6, cage);
    solid.quad([p(-0.72, 1.27, 0.21), p(0.72, 1.27, 0.21), p(0.72, 1.27, -1.05), p(-0.72, 1.27, -1.05)], uv, [0.1, 0.1, 0.105], 1.0);
    // The cage: door bars both sides (more on the driver's side), roof rails, the main hoop, a diagonal.
    for (x, ys) in [(-0.83, [0.46, 0.6, 0.74]), (0.83, [0.5, 0.64, 0.64])] {
        for y in ys {
            solid.tube(fr, V3(x, y, -0.95), V3(x, y, 0.78), 0.022, 6, cage);
        }
        solid.tube(fr, V3(x, 0.46, 0.78), V3(x * 0.95, 0.9, 0.9), 0.022, 6, cage);
        solid.tube(fr, V3(x * 0.84, 1.23, 0.2), V3(x * 0.84, 1.23, -0.95), 0.022, 6, cage);
    }
    solid.tube(fr, V3(0.8, 0.62, 0.7), V3(0.72, 1.2, -0.6), 0.022, 6, cage);
    for (a, b) in [
        ((-0.76, 0.3, -0.95), (-0.72, 1.22, -0.95)),
        ((-0.72, 1.22, -0.95), (0.72, 1.22, -0.95)),
        ((0.72, 1.22, -0.95), (0.76, 0.3, -0.95)),
    ] {
        solid.tube(fr, V3(a.0, a.1, a.2), V3(b.0, b.1, b.2), 0.028, 6, cage);
    }
    // The steering wheel: a suede rim with a marker at 12 o'clock, three spokes, the hub; it turns with the steer.
    let w = &c.wheel;
    let turn = wheel_angle(dash.steer, w.lock).to_radians();
    let tilt = w.tilt.to_radians();
    let centre = V3(ex, w.at.0, w.at.1);
    let (e_up, e_right) = (V3(0.0, tilt.cos(), tilt.sin()), V3(1.0, 0.0, 0.0));
    let axis = e_right.cross(e_up).scale(-1.0);
    let on_rim = |alpha: f32| {
        let a = alpha - turn;
        centre + e_up.scale(a.cos() * w.radius) + e_right.scale(a.sin() * w.radius)
    };
    match wheel_photo {
        // The wheel's photograph in the wheel's plane, turned with it (its top at 12 o'clock).
        Some((b, f)) => {
            let half = f.size / 2.0;
            let (st, ct) = turn.sin_cos();
            let (up, rt) = (e_up.scale(ct) - e_right.scale(st), e_right.scale(ct) + e_up.scale(st));
            let q = |x: f32, y: f32| fr.at(centre + rt.scale(x * half) + up.scale(y * half));
            let uvw = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
            b.quad([q(-1.0, 1.0), q(1.0, 1.0), q(1.0, -1.0), q(-1.0, -1.0)], uvw, [1.0; 3], 1.0);
        }
        None => {
            let segs = 28;
            for i in 0..segs {
                let (a0, a1) = (i as f32 / segs as f32 * std::f32::consts::TAU, (i + 1) as f32 / segs as f32 * std::f32::consts::TAU);
                let mid = (a0 + a1) / 2.0;
                let marker = !(0.2..std::f32::consts::TAU - 0.2).contains(&mid);
                let col = if marker { [1.0, 0.55, 0.05] } else { [0.06, 0.06, 0.065] };
                solid.tube(fr, on_rim(a0), on_rim(a1), 0.017, 6, col);
            }
            for alpha in [std::f32::consts::FRAC_PI_2, std::f32::consts::PI, 3.0 * std::f32::consts::FRAC_PI_2] {
                solid.tube(fr, centre, on_rim(alpha), 0.009, 4, [0.62, 0.63, 0.66]);
            }
            solid.tube(fr, centre - axis.scale(0.01), centre + axis.scale(0.045), 0.042, 8, [0.1, 0.1, 0.11]);
        }
    }
    solid.tube(fr, centre + axis.scale(0.045), centre + V3(0.0, -0.12, 0.38), 0.024, 6, [0.25, 0.25, 0.27]);
    // Gloved hands at a quarter to three, and the forearms back to the elbows.
    let suit = look.paint;
    for (alpha, elbow) in
        [(3.0 * std::f32::consts::FRAC_PI_2, V3(ex - 0.26, 0.72, -0.16)), (std::f32::consts::FRAC_PI_2, V3(ex + 0.26, 0.72, -0.16))]
    {
        // On the driver's side of the rim (a photographed wheel is a flat picture in its plane).
        let to_eye = (V3(c.eye.0, c.eye.1, c.eye.2) - centre).norm();
        let h = on_rim(alpha) + to_eye.scale(0.03);
        solid.tube(fr, h - e_up.scale(0.045), h + e_up.scale(0.05), 0.034, 6, [0.08, 0.08, 0.09]);
        solid.tube(fr, h + to_eye.scale(0.02), elbow, 0.042, 6, suit);
    }
    // The floor, the rear bulkhead under the rear window, and the seat's back (the mirror sees them).
    solid.quad([p(-0.9, 0.2, 0.9), p(0.9, 0.2, 0.9), p(0.9, 0.2, -1.1), p(-0.9, 0.2, -1.1)], uv, [0.12, 0.12, 0.12], 1.0);
    solid.quad([p(-0.92, 0.2, -1.07), p(0.92, 0.2, -1.07), p(0.92, 0.98, -1.07), p(-0.92, 0.98, -1.07)], uv, [0.2, 0.2, 0.21], 1.0);
    solid.cuboid(fr, V3(ex, 0.72, -0.55), V3(0.25, 0.4, 0.04), [0.08, 0.08, 0.09]);
    // The gear lever by the driver's right knee.
    solid.tube(fr, V3(ex + 0.33, 0.42, 0.08), V3(ex + 0.31, 0.64, -0.02), 0.012, 5, [0.6, 0.6, 0.62]);
    solid.cuboid(fr, V3(ex + 0.31, 0.655, -0.025), V3(0.022, 0.022, 0.022), [0.08, 0.08, 0.08]);
    // The window net on the driver's (left) side.
    if c.net {
        let (x, z0, z1, y0, y1) = (-0.86, -0.7, 0.72, 0.78, 1.2);
        net.glow(
            [p(x, y0, z0), p(x, y0, z1), p(x, y1, z1), p(x, y1, z0)],
            [[z0 / 0.26, y0 / 0.26], [z1 / 0.26, y0 / 0.26], [z1 / 0.26, y1 / 0.26], [z0 / 0.26, y1 / 0.26]],
            [1.0, 1.0, 1.0, 1.0],
        );
    }
    // The mirror: a black housing and the picture of what is behind (flipped: it is a mirror).
    if let Some(m) = &c.mirror {
        let (mx, my, mz) = m.at;
        let (hw, hh) = (m.size.0 / 2.0, m.size.1 / 2.0);
        solid.cuboid(fr, V3(mx, my, mz + 0.012), V3(hw + 0.012, hh + 0.012, 0.01), [0.03, 0.03, 0.03]);
        solid.tube(fr, V3(mx, my + hh, mz + 0.01), V3(mx, 1.265, mz + 0.03), 0.01, 4, [0.03, 0.03, 0.03]);
        mirror.glow(
            [p(mx - hw, my + hh, mz), p(mx + hw, my + hh, mz), p(mx + hw, my - hh, mz), p(mx - hw, my - hh, mz)],
            [[1.0, 0.0], [0.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
            [0.92, 0.92, 0.95, 1.0],
        );
    }
    // The windshield: nearly clear, with the dark sun strip across its top.
    let (wb, wt) = (0.88, 0.69);
    let lerp = |a: V3, b: V3, t: f32| a + (b - a).scale(t);
    let (bl, br, tl, tr) = (V3(-wb, 0.90, 0.95), V3(wb, 0.90, 0.95), V3(-wt, 1.265, 0.2), V3(wt, 1.265, 0.2));
    let k = 0.84;
    let (ml, mr) = (lerp(bl, tl, k), lerp(br, tr, k));
    glass.glow([fr.at(bl), fr.at(br), fr.at(mr), fr.at(ml)], uv, [0.03, 0.04, 0.05, 0.05]);
    glass.glow([fr.at(ml), fr.at(mr), fr.at(tr), fr.at(tl)], uv, [0.02, 0.05, 0.07, 0.62]);
}

/// Builders for one frame's moving things, in draw order.
pub struct Parts {
    pub shadows: Builder,
    pub bodies: Builder,
    pub solid: Builder,
    /// The dash's and the wheel's photographs (with the picture's size), when the game has them.
    pub dash_photo: Option<(Builder, Found)>,
    pub wheel_photo: Option<(Builder, Found)>,
    pub text: Builder,
    pub net: Builder,
    pub mirror: Builder,
    pub glass: Builder,
}

impl Parts {
    pub fn new(sun: V3, photos: &Photos) -> Parts {
        let photo = |k: &str| photos.get(k).map(|f| (Builder::new(&f.texture, Wrap::Clamp, sun), f.clone()));
        Parts {
            shadows: Builder::new(BLOB, Wrap::Clamp, sun),
            bodies: Builder::new(WHITE, Wrap::Clamp, sun),
            solid: Builder::new(WHITE, Wrap::Clamp, sun),
            dash_photo: photo("dash"),
            wheel_photo: photo("wheel"),
            text: Builder::new(FONT, Wrap::Clamp, sun),
            net: Builder::new(NET, Wrap::Repeat, sun),
            mirror: Builder::new(MIRROR, Wrap::Clamp, sun),
            glass: Builder::new(WHITE, Wrap::Clamp, sun),
        }
    }

    /// The meshes, and how many of them (from the start) the mirror's own picture may show (not the mirror itself,
    /// nor the glass in front of it).
    pub fn meshes(self) -> (Vec<crate::gpu::Mesh>, usize) {
        let mut all = vec![self.shadows, self.bodies, self.solid];
        all.extend(self.dash_photo.map(|d| d.0));
        all.extend(self.wheel_photo.map(|d| d.0));
        all.extend([self.text, self.net]);
        let seen = all.len();
        all.extend([self.mirror, self.glass]);
        (all.into_iter().map(Builder::mesh).collect(), seen)
    }
}

pub fn paint_of(colors: &[(u8, u8, u8)], i: usize) -> [f32; 3] {
    let c = colors.get(i % colors.len().max(1)).copied().unwrap_or((200, 30, 30));
    rgba(rgb(c), 1.0, 1.0)[..3].try_into().expect("three")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wheel_turns_with_the_steer_and_stops_at_its_lock() {
        assert_eq!(wheel_angle(0.0, 270.0), 0.0);
        assert_eq!(wheel_angle(0.5, 270.0), 135.0);
        assert_eq!(wheel_angle(-2.0, 270.0), -270.0, "clamped at the lock");
    }

    #[test]
    fn shift_lights_fill_up_then_flash() {
        assert_eq!(shift_lights(7000.0, 7800.0, 9200.0, 10), (0, false));
        assert_eq!(shift_lights(8500.0, 7800.0, 9200.0, 10), (5, false));
        assert_eq!(shift_lights(9300.0, 7800.0, 9200.0, 10), (10, true));
    }
}
