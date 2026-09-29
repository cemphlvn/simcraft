//! A body that walks a voxel world in first person: the feel of moving, as data (`WalkFeel`) and pure logic (no
//! GPU, no window), so it is tested and measured headless (`feel_probe`).
//!
//! Space: X right, Y up, Z forward; one voxel is one unit. `solid(ix, iy, iz)` answers for voxel indices.
//! Speeds glide: velocity eases toward what the keys ask (`accel`), so starting and stopping take a moment;
//! the view eases toward where the mouse points (`look_smooth`), so turning glides instead of snapping.
//! Walking into a wall climbs it (crawlers do); space jumps from a floor or off a wall.

use serde::Deserialize;

use crate::math::{Eye, V3};

/// How moving feels. Every number is the designer's (`roam.ron` → `feel`).
#[derive(Clone, Copy, Debug, serde::Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WalkFeel {
    /// Eye height above the feet (voxels).
    pub eye: f32,
    /// Body half-width and height (voxels): what bumps into walls.
    pub radius: f32,
    pub height: f32,
    /// Speeds (voxels per second): walking, and with the run key held.
    pub walk: f32,
    pub run: f32,
    /// How fast velocity eases toward the wanted one, on a surface and in the air (per second; higher = snappier).
    pub accel: f32,
    pub air: f32,
    /// Climbing speed up a wall you walk into (voxels per second).
    pub climb: f32,
    /// Jump take-off speed and gravity (voxels per second, per second²).
    pub jump: f32,
    pub gravity: f32,
    /// Degrees of turn per pixel of mouse movement; how fast the view eases after the mouse (per second).
    pub sensitivity: f32,
    pub look_smooth: f32,
    /// Head bob: height (voxels) and steps per voxel travelled.
    pub bob: f32,
    pub bob_rate: f32,
    /// Field of view (degrees) walking, and at full run.
    pub fov: f32,
    pub run_fov: f32,
    /// Degrees the view rolls into a sideways move; how deep a landing dips the eye (per unit of fall speed).
    pub lean: f32,
    pub land_dip: f32,
    /// How fast the eye glides after the body's height (per second; 0 = bolted to the body): steps, hops, climbs
    /// and landings become a glide instead of a jolt. The bob fades in and out at the same rate.
    pub eye_glide: f32,
}

impl Default for WalkFeel {
    fn default() -> Self {
        WalkFeel {
            eye: 0.35,
            radius: 0.3,
            height: 0.45,
            walk: 2.5,
            run: 5.0,
            accel: 9.0,
            air: 2.5,
            climb: 2.0,
            jump: 4.5,
            gravity: 14.0,
            sensitivity: 0.12,
            look_smooth: 16.0,
            bob: 0.025,
            bob_rate: 1.6,
            fov: 72.0,
            run_fov: 82.0,
            lean: 2.5,
            land_dip: 0.012,
            eye_glide: 0.0,
        }
    }
}

/// What the player asks this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Input {
    /// -1 (back) .. 1 (forward), -1 (left) .. 1 (right).
    pub forward: f32,
    pub strafe: f32,
    pub run: bool,
    /// Held; a jump happens when it goes down.
    pub jump: bool,
    /// Mouse movement since the last frame (pixels, right and down positive).
    pub look: (f32, f32),
}

/// The whole body and camera state (serialisable: a spike report carries it, so the frame can be rebuilt exactly).
#[derive(Clone, Debug, serde::Serialize, Deserialize)]
pub struct Walker {
    pub feel: WalkFeel,
    /// Feet (centre of the body's base).
    pub pos: V3,
    pub vel: V3,
    /// Where the mouse points (degrees; yaw 0 looks along +Z, pitch up positive).
    pub yaw: f32,
    pub pitch: f32,
    /// Where the camera looks: eases after `yaw`/`pitch`.
    pub view_yaw: f32,
    pub view_pitch: f32,
    pub grounded: bool,
    pub climbing: bool,
    jump_held: bool,
    bob_phase: f32,
    /// The landing dip: a spring on the eye height (offset, velocity).
    dip: (f32, f32),
    roll: f32,
    fov: f32,
    /// The eye's height when it glides (see `WalkFeel::eye_glide`), and how much of the bob shows (0..1).
    eye_y: f32,
    bob_amp: f32,
}

fn floor(v: f32) -> i64 {
    v.floor() as i64
}

impl Walker {
    pub fn new(feel: WalkFeel, pos: V3, yaw: f32) -> Walker {
        Walker {
            feel,
            pos,
            vel: V3(0.0, 0.0, 0.0),
            yaw,
            pitch: -8.0,
            view_yaw: yaw,
            view_pitch: -8.0,
            grounded: false,
            climbing: false,
            jump_held: false,
            bob_phase: 0.0,
            dip: (0.0, 0.0),
            roll: 0.0,
            fov: feel.fov,
            eye_y: pos.1 + feel.eye,
            bob_amp: 0.0,
        }
    }

    /// Does the body at feet `p` overlap a solid voxel?
    fn overlaps(&self, p: V3, solid: &impl Fn(i64, i64, i64) -> bool) -> bool {
        let (r, h, e) = (self.feel.radius, self.feel.height, 1e-4);
        for ix in floor(p.0 - r + e)..=floor(p.0 + r - e) {
            for iy in floor(p.1 + e)..=floor(p.1 + h - e) {
                for iz in floor(p.2 - r + e)..=floor(p.2 + r - e) {
                    if solid(ix, iy, iz) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Moves along one axis (0 x, 1 y, 2 z); stops flush against a solid voxel. True if it was stopped.
    fn move_axis(&mut self, axis: usize, d: f32, solid: &impl Fn(i64, i64, i64) -> bool) -> bool {
        if d == 0.0 {
            return false;
        }
        let mut p = self.pos;
        let moved = get(p, axis) + d;
        set(&mut p, axis, moved);
        if !self.overlaps(p, solid) {
            self.pos = p;
            return false;
        }
        // Flush with the voxel boundary we ran into.
        let (r, h, e) = (self.feel.radius, self.feel.height, 1e-3);
        let (lo, hi) = if axis == 1 { (0.0, h) } else { (-r, r) };
        let c = get(p, axis);
        let flush = if d > 0.0 { (c + hi).floor() - hi - e } else { (c + lo).floor() + 1.0 - lo + e };
        set(&mut p, axis, flush);
        if !self.overlaps(p, solid) && (flush - get(self.pos, axis)) * d >= 0.0 {
            self.pos = p;
        }
        true
    }

    /// Is there a wall right ahead in horizontal direction `dir` (unit), at foot or chest height?
    fn wall_ahead(&self, dir: V3, solid: &impl Fn(i64, i64, i64) -> bool) -> bool {
        let reach = self.feel.radius + 0.08;
        [0.05, self.feel.height * 0.6].iter().any(|&up| {
            let q = V3(self.pos.0 + dir.0 * reach, self.pos.1 + up, self.pos.2 + dir.2 * reach);
            solid(floor(q.0), floor(q.1), floor(q.2))
        })
    }

    /// Unit vectors along the ground: forward and right for the view's yaw.
    pub fn flat_axes(&self) -> (V3, V3) {
        let (s, c) = self.view_yaw.to_radians().sin_cos();
        (V3(s, 0.0, c), V3(c, 0.0, -s))
    }

    /// Where the view looks (unit).
    pub fn look_dir(&self) -> V3 {
        let (sy, cy) = self.view_yaw.to_radians().sin_cos();
        let (sp, cp) = self.view_pitch.to_radians().sin_cos();
        V3(sy * cp, sp, cy * cp)
    }

    /// Where the eye would be if it were bolted to the body.
    fn eye_target(&self) -> f32 {
        let bob = if self.feel.eye_glide > 0.0 {
            self.bob_phase.sin() * self.feel.bob * self.bob_amp
        } else if self.grounded {
            self.bob_phase.sin() * self.feel.bob
        } else {
            0.0
        };
        self.pos.1 + self.feel.eye + bob + self.dip.0
    }

    pub fn eye_pos(&self) -> V3 {
        let y = if self.feel.eye_glide > 0.0 { self.eye_y } else { self.eye_target() };
        V3(self.pos.0, y, self.pos.2)
    }

    pub fn eye(&self) -> Eye {
        let p = self.eye_pos();
        Eye { pos: p, target: p + self.look_dir(), roll: self.roll, fov: self.fov, near: 0.05, far: 200.0 }
    }

    /// Horizontal speed (voxels per second).
    pub fn speed(&self) -> f32 {
        (self.vel.0 * self.vel.0 + self.vel.2 * self.vel.2).sqrt()
    }

    /// One frame. Long frames are cut into short steps, so a slow frame never tunnels through a wall.
    pub fn step(&mut self, dt: f32, input: Input, solid: &impl Fn(i64, i64, i64) -> bool) {
        let f = self.feel;
        self.yaw += input.look.0 * f.sensitivity;
        self.pitch = (self.pitch - input.look.1 * f.sensitivity).clamp(-85.0, 85.0);
        let jump = input.jump && !self.jump_held;
        self.jump_held = input.jump;
        let n = (dt / (1.0 / 120.0)).ceil().clamp(1.0, 16.0) as usize;
        let h = dt / n as f32;
        for i in 0..n {
            self.substep(h, input, jump && i == 0, solid);
        }
    }

    fn substep(&mut self, dt: f32, input: Input, jump: bool, solid: &impl Fn(i64, i64, i64) -> bool) {
        let f = self.feel;
        let ease = |rate: f32| 1.0 - (-rate * dt).exp();
        self.view_yaw += (self.yaw - self.view_yaw) * ease(f.look_smooth);
        self.view_pitch += (self.pitch - self.view_pitch) * ease(f.look_smooth);

        // Pushed out of anything that appeared around the body (mud dropped on you): up, onto it.
        for _ in 0..3 {
            if self.overlaps(self.pos, solid) {
                self.pos.1 = self.pos.1.floor() + 1.0 + 1e-3;
            }
        }

        let (fwd, right) = self.flat_axes();
        let mut wish = fwd.scale(input.forward) + right.scale(input.strafe);
        let len = wish.dot(wish).sqrt();
        if len > 1.0 {
            wish = wish.scale(1.0 / len);
        }
        let top = if input.run { f.run } else { f.walk };
        let wish = wish.scale(top);
        let on_something = self.grounded || self.climbing;
        let k = ease(if on_something { f.accel } else { f.air });
        self.vel.0 += (wish.0 - self.vel.0) * k;
        self.vel.2 += (wish.2 - self.vel.2) * k;

        // Walking into a wall climbs it; jumping leaves it.
        let pushing = len > 0.1;
        self.climbing = pushing && self.wall_ahead(wish.norm(), solid);
        if jump && on_something {
            self.vel.1 = f.jump;
            self.climbing = false;
        } else if self.climbing {
            self.vel.1 += (f.climb - self.vel.1) * ease(12.0);
        } else {
            self.vel.1 -= f.gravity * dt;
        }

        self.move_axis(0, self.vel.0 * dt, solid);
        self.move_axis(2, self.vel.2 * dt, solid);
        let falling = self.vel.1;
        let was_grounded = self.grounded;
        let stopped = self.move_axis(1, self.vel.1 * dt, solid);
        self.grounded =
            stopped && falling < 0.0 || (self.vel.1 <= 0.0 && solid(floor(self.pos.0), floor(self.pos.1 - 0.02), floor(self.pos.2)));
        if stopped {
            if falling < 0.0 && !was_grounded {
                // Landing: the eye dips with the fall speed and springs back.
                self.dip.1 += falling * f.land_dip * 10.0;
            }
            self.vel.1 = 0.0;
        }
        if self.grounded && !self.climbing {
            self.vel.1 = self.vel.1.max(0.0);
        }

        // The dip is a critically damped spring back to 0.
        let (w, x, v) = (14.0, self.dip.0, self.dip.1);
        let acc = -w * w * x - 2.0 * w * v;
        self.dip.1 = v + acc * dt;
        self.dip.0 = x + self.dip.1 * dt;

        let speed = self.speed();
        if self.grounded {
            self.bob_phase += speed * f.bob_rate * std::f32::consts::TAU * dt;
        }
        let run_share = ((speed - f.walk) / (f.run - f.walk).max(0.01)).clamp(0.0, 1.0);
        self.fov += (f.fov + (f.run_fov - f.fov) * run_share - self.fov) * ease(6.0);
        let side = self.vel.dot(right) / f.run.max(0.01);
        self.roll += (side * f.lean - self.roll) * ease(8.0);
        if f.eye_glide > 0.0 {
            self.bob_amp += ((self.grounded as u8 as f32) - self.bob_amp) * ease(f.eye_glide * 0.5);
            let target = self.eye_target();
            self.eye_y += (target - self.eye_y) * ease(f.eye_glide);
        }
    }
}

fn get(p: V3, axis: usize) -> f32 {
    match axis {
        0 => p.0,
        1 => p.1,
        _ => p.2,
    }
}

fn set(p: &mut V3, axis: usize, v: f32) {
    match axis {
        0 => p.0 = v,
        1 => p.1 = v,
        _ => p.2 = v,
    }
}

/// What a look ray meets within `reach`: the first solid voxel, and the open voxel just before it (where a
/// dropped ball goes). Voxel traversal (Amanatides and Woo): every voxel the ray passes, in order.
pub fn ray(from: V3, dir: V3, reach: f32, solid: &impl Fn(i64, i64, i64) -> bool) -> Option<([i64; 3], [i64; 3])> {
    let p = [from.0, from.1, from.2];
    let d = [dir.0, dir.1, dir.2];
    let mut cell = [floor(p[0]), floor(p[1]), floor(p[2])];
    let step: [i64; 3] = std::array::from_fn(|i| if d[i] > 0.0 { 1 } else { -1 });
    let mut t_max: [f32; 3] = std::array::from_fn(|i| {
        if d[i].abs() < 1e-9 {
            f32::INFINITY
        } else {
            let edge = if d[i] > 0.0 { cell[i] as f32 + 1.0 } else { cell[i] as f32 };
            (edge - p[i]) / d[i]
        }
    });
    let t_delta: [f32; 3] = std::array::from_fn(|i| if d[i].abs() < 1e-9 { f32::INFINITY } else { 1.0 / d[i].abs() });
    let mut before = cell;
    loop {
        let axis = (0..3).min_by(|&a, &b| t_max[a].total_cmp(&t_max[b])).unwrap_or(0);
        if t_max[axis] > reach {
            return None;
        }
        before = if solid(cell[0], cell[1], cell[2]) { before } else { cell };
        cell[axis] += step[axis];
        t_max[axis] += t_delta[axis];
        if solid(cell[0], cell[1], cell[2]) {
            return Some((cell, before));
        }
    }
}

/// How a walk feels, measured: a scripted player (walk, run, turn, stop, jump) on flat ground with a wall, at
/// 60 frames a second. Pure numbers: the feel eval (`simcraft-play <game> --feel`, log in `FEEL.md`).
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct FeelReport {
    /// Seconds from standing to 90% of walking speed, and from walking to a stop (below 5%).
    pub start_s: f32,
    pub stop_s: f32,
    /// Largest change of the view's turning speed between frames (degrees per frame²): lower = smoother turns.
    pub look_jerk: f32,
    /// How far the view trails the mouse, at most (degrees).
    pub look_lag: f32,
    /// Largest eye jolt between frames while walking and landing (voxels per frame²): lurches.
    pub eye_jerk: f32,
    /// Height of a jump (voxels), and seconds in the air.
    pub jump_h: f32,
    pub air_s: f32,
    /// Seconds to climb a wall 3 voxels high by walking into it.
    pub climb_s: f32,
}

pub fn feel_probe(feel: WalkFeel) -> FeelReport {
    // Flat floor at y < 0, a wall 3 high from z = 8 on, open above it.
    let solid = |_x: i64, y: i64, z: i64| y < 0 || (z >= 8 && y < 3);
    let dt = 1.0 / 60.0;
    let mut w = Walker::new(feel, V3(0.5, 0.0, 0.5), 0.0);
    let mut out = FeelReport::default();
    let mut t = 0.0f32;
    let hold = |forward: f32| Input { forward, ..Input::default() };
    // Start: forward until 90% of walking speed.
    while w.speed() < 0.9 * feel.walk && t < 5.0 {
        w.step(dt, hold(1.0), &solid);
        t += dt;
    }
    out.start_s = t;
    // Stop.
    t = 0.0;
    while w.speed() > 0.05 * feel.walk && t < 5.0 {
        w.step(dt, hold(0.0), &solid);
        t += dt;
    }
    out.stop_s = t;
    // Turn: the mouse sweeps 600 px right over 0.5 s, then rests; watch the view follow.
    let (mut last_rate, mut last_yaw) = (0.0f32, w.view_yaw);
    for i in 0..90 {
        let look = if i < 30 { (20.0, 0.0) } else { (0.0, 0.0) };
        w.step(dt, Input { look, ..Input::default() }, &solid);
        let rate = w.view_yaw - last_yaw;
        out.look_jerk = out.look_jerk.max((rate - last_rate).abs());
        out.look_lag = out.look_lag.max((w.yaw - w.view_yaw).abs());
        (last_rate, last_yaw) = (rate, w.view_yaw);
    }
    // Walk (eye jolts from the bob), then a jump and its landing.
    let (mut prev, mut prev_v) = (w.eye_pos().1, 0.0f32);
    let mut jolt = |w: &Walker, out: &mut FeelReport| {
        let y = w.eye_pos().1;
        let v = y - prev;
        out.eye_jerk = out.eye_jerk.max((v - prev_v).abs());
        (prev, prev_v) = (y, v);
    };
    w = Walker::new(feel, V3(0.5, 0.0, 0.5), 90.0);
    for _ in 0..60 {
        w.step(dt, hold(1.0), &solid);
        jolt(&w, &mut out);
    }
    let base = w.pos.1;
    let (mut air, mut top) = (0.0f32, base);
    w.step(dt, Input { forward: 1.0, jump: true, ..Input::default() }, &solid);
    while !w.grounded && air < 3.0 {
        w.step(dt, hold(1.0), &solid);
        jolt(&w, &mut out);
        air += dt;
        top = top.max(w.pos.1);
    }
    out.jump_h = top - base;
    out.air_s = air + dt;
    for _ in 0..30 {
        w.step(dt, hold(1.0), &solid);
        jolt(&w, &mut out);
    }
    // Climb: walk at the wall until the feet are on top of it.
    w = Walker::new(feel, V3(0.5, 0.0, 6.0), 0.0);
    t = 0.0;
    while w.pos.1 < 3.0 && t < 10.0 {
        w.step(dt, hold(1.0), &solid);
        t += dt;
    }
    out.climb_s = t;
    let r = |v: f32| (v * 1000.0).round() / 1000.0;
    FeelReport {
        start_s: r(out.start_s),
        stop_s: r(out.stop_s),
        look_jerk: r(out.look_jerk),
        look_lag: r(out.look_lag),
        eye_jerk: r(out.eye_jerk),
        jump_h: r(out.jump_h),
        air_s: r(out.air_s),
        climb_s: r(out.climb_s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floor_only(_x: i64, y: i64, _z: i64) -> bool {
        y < 0
    }

    #[test]
    fn a_walker_glides_to_speed_and_glides_to_a_stop() {
        let feel = WalkFeel::default();
        let mut w = Walker::new(feel, V3(0.5, 0.0, 0.5), 0.0);
        w.step(1.0 / 60.0, Input { forward: 1.0, ..Input::default() }, &floor_only);
        assert!(w.speed() > 0.0 && w.speed() < feel.walk * 0.3, "no instant top speed: {}", w.speed());
        for _ in 0..120 {
            w.step(1.0 / 60.0, Input { forward: 1.0, ..Input::default() }, &floor_only);
        }
        assert!((w.speed() - feel.walk).abs() < 0.05, "reaches walking speed");
        assert!(w.grounded && w.pos.1.abs() < 1e-2, "stands on the floor");
        w.step(1.0 / 60.0, Input::default(), &floor_only);
        assert!(w.speed() > feel.walk * 0.5, "keeps gliding a moment after the key is let go");
    }

    #[test]
    fn walls_stop_the_body_and_walking_into_one_climbs_it() {
        let wall = |_x: i64, y: i64, z: i64| y < 0 || (z >= 3 && y < 2);
        let mut w = Walker::new(WalkFeel::default(), V3(0.5, 0.0, 0.5), 0.0);
        let mut max_z = 0.0f32;
        for _ in 0..600 {
            w.step(1.0 / 60.0, Input { forward: 1.0, ..Input::default() }, &wall);
            if w.pos.1 < 1.9 {
                max_z = max_z.max(w.pos.2);
            }
            if w.pos.1 >= 2.0 && w.pos.2 > 3.3 {
                break;
            }
        }
        assert!(max_z <= 3.0 - w.feel.radius + 1e-2, "never inside the wall: {max_z}");
        assert!(w.pos.1 >= 2.0 - 1e-2 && w.pos.2 > 3.3, "climbed onto it: {:?}", w.pos);
    }

    #[test]
    fn a_jump_goes_up_and_lands_again() {
        let mut w = Walker::new(WalkFeel::default(), V3(0.5, 0.0, 0.5), 0.0);
        w.step(1.0 / 60.0, Input::default(), &floor_only);
        w.step(1.0 / 60.0, Input { jump: true, ..Input::default() }, &floor_only);
        let mut top = 0.0f32;
        for _ in 0..120 {
            w.step(1.0 / 60.0, Input { jump: true, ..Input::default() }, &floor_only);
            top = top.max(w.pos.1);
        }
        assert!(top > 0.5, "went up {top}");
        assert!(w.grounded && w.pos.1.abs() < 1e-2, "landed; holding space does not jump again");
    }

    #[test]
    fn the_view_eases_after_the_mouse() {
        let mut w = Walker::new(WalkFeel::default(), V3(0.5, 0.0, 0.5), 0.0);
        w.step(1.0 / 60.0, Input { look: (100.0, 0.0), ..Input::default() }, &floor_only);
        assert!(w.yaw > 11.9 && w.view_yaw > 0.0 && w.view_yaw < w.yaw, "the view trails the mouse");
        for _ in 0..60 {
            w.step(1.0 / 60.0, Input::default(), &floor_only);
        }
        assert!((w.view_yaw - w.yaw).abs() < 0.01, "and arrives");
    }

    #[test]
    fn a_ray_finds_the_face_it_looks_at() {
        let solid = |x: i64, y: i64, z: i64| y < 0 || (x, y, z) == (0, 0, 3);
        let hit = ray(V3(0.5, 0.4, 0.5), V3(0.0, 0.0, 1.0), 4.0, &solid);
        assert_eq!(hit, Some(([0, 0, 3], [0, 0, 2])));
        let down = ray(V3(0.5, 0.4, 0.5), V3(0.0, -1.0, 0.0), 4.0, &solid);
        assert_eq!(down, Some(([0, -1, 0], [0, 0, 0])));
        assert_eq!(ray(V3(0.5, 0.4, 0.5), V3(0.0, 1.0, 0.0), 4.0, &solid), None, "nothing within reach");
    }
}
