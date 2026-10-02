//! The camera: a pose (eye, look-at point, field of view) that follows a target pose on a damped spring, plus shake.
//!
//! Shake is trauma-based (Eiserloh, "Juicing your cameras with math", GDC 2016): a hit adds trauma (0..1), trauma
//! decays linearly, and the shake is trauma² so small hits barely move the picture while big ones really do. The
//! offsets come from smooth noise (sums of sines at unrelated frequencies), not random jumps, so the picture sways
//! instead of jittering, and the same run always shakes the same way.

use sim_physics::rigid::V3;

use crate::draw3d::Camera;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub eye: V3,
    pub look: V3,
    /// Vertical field of view (degrees).
    pub fov: f32,
}

impl Pose {
    pub fn lerp(self, o: Pose, t: f32) -> Pose {
        Pose { eye: self.eye.lerp(o.eye, t), look: self.look.lerp(o.look, t), fov: self.fov + (o.fov - self.fov) * t }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Rig {
    pub pose: Pose,
    /// The target after smoothing: the spring follows this, never the raw target, so a target that jumps (aim →
    /// chase on release, → the impact on a hit) bends the camera's path instead of kicking it (bounded jerk).
    goal: Pose,
    pub prev: Pose,
    vel: (V3, V3, f32),
    pub trauma: f32,
    /// Seconds of shake time (advances with real time, not game time: a hit-stop still shakes).
    time: f32,
    /// Shake as of the last tick and the one before (angles in degrees: yaw, pitch, roll; then a move in metres).
    shake: [f32; 4],
    prev_shake: [f32; 4],
}

/// A smooth wobble in −1..1 for channel `k` at time `t`.
fn wobble(k: u32, t: f32) -> f32 {
    let k = k as f32;
    let a = (t * (13.1 + k * 3.7) + k * 1.3).sin();
    let b = (t * (23.3 + k * 5.1) + k * 2.9).sin();
    let c = (t * (7.7 + k * 1.9) + k * 0.7).sin();
    (a * 0.5 + b * 0.3 + c * 0.2).clamp(-1.0, 1.0)
}

impl Rig {
    pub fn new(pose: Pose) -> Rig {
        Rig { pose, goal: pose, prev: pose, vel: (V3::ZERO, V3::ZERO, 0.0), trauma: 0.0, time: 0.0, shake: [0.0; 4], prev_shake: [0.0; 4] }
    }

    /// Jumps to `pose` with no motion (a reset).
    pub fn snap(&mut self, pose: Pose) {
        *self = Rig::new(pose);
    }

    pub fn add_trauma(&mut self, t: f32) {
        self.trauma = (self.trauma + t).min(1.0);
    }

    /// One tick of `dt` seconds towards `target`: a spring of `hz` with damping ratio `zeta` (1 = no overshoot).
    /// The shake's largest turn is `angle` degrees and its largest move `moves` metres; trauma loses `decay` a second.
    #[allow(clippy::too_many_arguments)]
    pub fn step(&mut self, target: Pose, dt: f32, hz: f32, zeta: f32, decay: f32, angle: f32, moves: f32) {
        self.prev = self.pose;
        self.prev_shake = self.shake;
        let w = std::f32::consts::TAU * hz;
        // First stage: the goal glides towards the target (exponential, twice the spring's rate).
        let k = 1.0 - (-2.0 * w * dt).exp();
        self.goal = self.goal.lerp(target, k);
        let target = self.goal;
        // Semi-implicit: the velocity first, then the position with the new velocity (stable at any frame rate here).
        let spring = |x: f32, v: &mut f32, to: f32| {
            *v += (w * w * (to - x) - 2.0 * zeta * w * *v) * dt;
            x + *v * dt
        };
        let (ev, lv, fv) = &mut self.vel;
        let p = self.pose;
        self.pose.eye = V3::new(
            spring(p.eye.x, &mut ev.x, target.eye.x),
            spring(p.eye.y, &mut ev.y, target.eye.y),
            spring(p.eye.z, &mut ev.z, target.eye.z),
        );
        self.pose.look = V3::new(
            spring(p.look.x, &mut lv.x, target.look.x),
            spring(p.look.y, &mut lv.y, target.look.y),
            spring(p.look.z, &mut lv.z, target.look.z),
        );
        self.pose.fov = spring(p.fov, fv, target.fov);
        self.time += dt;
        self.trauma = (self.trauma - decay * dt).max(0.0);
        let s = self.trauma * self.trauma;
        self.shake =
            [angle * s * wobble(0, self.time), angle * s * wobble(1, self.time), angle * 0.6 * s * wobble(2, self.time), moves * s];
    }

    /// The camera `alpha` of the way from the last tick to this one, shake included.
    pub fn camera(&self, alpha: f32) -> Camera {
        let p = self.prev.lerp(self.pose, alpha);
        let mut sh = [0.0; 4];
        for (i, v) in sh.iter_mut().enumerate() {
            *v = self.prev_shake[i] + (self.shake[i] - self.prev_shake[i]) * alpha;
        }
        let fwd = (p.look - p.eye).normalized();
        let right = fwd.cross(V3::Y).normalized();
        let up = right.cross(fwd);
        let (yaw, pitch, roll) = (sh[0].to_radians(), sh[1].to_radians(), sh[2].to_radians());
        let dist = (p.look - p.eye).length();
        // Turning the look point is turning the head; the move is a small translation of the whole camera.
        let look = p.look + right * (yaw.tan() * dist) + up * (pitch.tan() * dist);
        let t = self.time;
        let offset = right * (sh[3] * wobble(3, t)) + up * (sh[3] * wobble(4, t));
        Camera {
            eye: p.eye + offset,
            target: look + offset,
            up: (up + right * roll.sin()).normalized(),
            fov_y: p.fov.to_radians(),
            near: 0.1,
            far: 300.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pose(z: f32) -> Pose {
        Pose { eye: V3::new(0.0, 2.0, z), look: V3::ZERO, fov: 45.0 }
    }

    #[test]
    fn a_critically_damped_spring_arrives_without_overshoot() {
        let mut r = Rig::new(pose(10.0));
        let mut min_z = f32::MAX;
        for _ in 0..240 {
            r.step(pose(5.0), 1.0 / 60.0, 2.0, 1.0, 1.0, 0.0, 0.0);
            min_z = min_z.min(r.pose.eye.z);
        }
        assert!((r.pose.eye.z - 5.0).abs() < 0.01, "arrived: {}", r.pose.eye.z);
        assert!(min_z > 4.999, "no overshoot: {min_z}");
    }

    #[test]
    fn trauma_shakes_then_decays_to_stillness() {
        let mut r = Rig::new(pose(10.0));
        r.add_trauma(1.0);
        r.step(pose(10.0), 1.0 / 60.0, 2.0, 1.0, 2.0, 2.0, 0.1);
        let shaken = r.camera(1.0);
        assert!(shaken.target != V3::ZERO, "a hit moves the picture");
        for _ in 0..60 {
            r.step(pose(10.0), 1.0 / 60.0, 2.0, 1.0, 2.0, 2.0, 0.1);
        }
        assert_eq!(r.trauma, 0.0);
        let c = r.camera(1.0);
        assert!((c.target - V3::ZERO).length() < 1e-4, "still again: {:?}", c.target);
    }
}
