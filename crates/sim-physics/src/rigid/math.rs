//! Vectors, quaternions and 3 × 3 matrices in `f32` for the rigid-body solver and whoever draws it. Plain values,
//! no SIMD: the same binary gives the same numbers.

use std::ops::{Add, AddAssign, Mul, Neg, Sub, SubAssign};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl V3 {
    pub const ZERO: V3 = V3::new(0.0, 0.0, 0.0);
    pub const X: V3 = V3::new(1.0, 0.0, 0.0);
    pub const Y: V3 = V3::new(0.0, 1.0, 0.0);
    pub const Z: V3 = V3::new(0.0, 0.0, 1.0);
    pub const ONE: V3 = V3::new(1.0, 1.0, 1.0);

    pub const fn new(x: f32, y: f32, z: f32) -> V3 {
        V3 { x, y, z }
    }

    pub const fn splat(v: f32) -> V3 {
        V3::new(v, v, v)
    }

    pub fn dot(self, o: V3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: V3) -> V3 {
        V3::new(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }

    pub fn length_sq(self) -> f32 {
        self.dot(self)
    }

    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    /// The unit vector along `self`, or zero for (almost) zero.
    pub fn normalized(self) -> V3 {
        let l = self.length();
        if l > 1e-12 { self * (1.0 / l) } else { V3::ZERO }
    }

    /// Component-wise product.
    pub fn mul_each(self, o: V3) -> V3 {
        V3::new(self.x * o.x, self.y * o.y, self.z * o.z)
    }

    pub fn abs(self) -> V3 {
        V3::new(self.x.abs(), self.y.abs(), self.z.abs())
    }

    pub fn min(self, o: V3) -> V3 {
        V3::new(self.x.min(o.x), self.y.min(o.y), self.z.min(o.z))
    }

    pub fn max(self, o: V3) -> V3 {
        V3::new(self.x.max(o.x), self.y.max(o.y), self.z.max(o.z))
    }

    pub fn lerp(self, o: V3, t: f32) -> V3 {
        self + (o - self) * t
    }

    /// Component `i` (0 = x, 1 = y, 2 = z).
    pub fn get(self, i: usize) -> f32 {
        match i {
            0 => self.x,
            1 => self.y,
            _ => self.z,
        }
    }

    pub fn to_array(self) -> [f32; 3] {
        [self.x, self.y, self.z]
    }

    /// Two unit vectors that with `self` (a unit vector) make a right-handed frame.
    pub fn basis(self) -> (V3, V3) {
        let t = if self.x.abs() > 0.57 { V3::new(self.y, -self.x, 0.0) } else { V3::new(0.0, self.z, -self.y) }.normalized();
        (t, self.cross(t))
    }
}

impl Add for V3 {
    type Output = V3;
    fn add(self, o: V3) -> V3 {
        V3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl Sub for V3 {
    type Output = V3;
    fn sub(self, o: V3) -> V3 {
        V3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl Mul<f32> for V3 {
    type Output = V3;
    fn mul(self, k: f32) -> V3 {
        V3::new(self.x * k, self.y * k, self.z * k)
    }
}

impl Neg for V3 {
    type Output = V3;
    fn neg(self) -> V3 {
        V3::new(-self.x, -self.y, -self.z)
    }
}

impl AddAssign for V3 {
    fn add_assign(&mut self, o: V3) {
        *self = *self + o;
    }
}

impl SubAssign for V3 {
    fn sub_assign(&mut self, o: V3) {
        *self = *self - o;
    }
}

/// A rotation as a unit quaternion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Default for Quat {
    fn default() -> Quat {
        Quat::IDENTITY
    }
}

impl Quat {
    pub const IDENTITY: Quat = Quat { x: 0.0, y: 0.0, z: 0.0, w: 1.0 };

    /// A turn of `angle` radians about the unit vector `axis`.
    pub fn axis_angle(axis: V3, angle: f32) -> Quat {
        let (s, c) = (angle * 0.5).sin_cos();
        let a = axis.normalized() * s;
        Quat { x: a.x, y: a.y, z: a.z, w: c }
    }

    /// Turns about y (yaw), then x (pitch): a camera's or a launcher's aim.
    pub fn yaw_pitch(yaw: f32, pitch: f32) -> Quat {
        Quat::axis_angle(V3::Y, yaw) * Quat::axis_angle(V3::X, pitch)
    }

    pub fn conj(self) -> Quat {
        Quat { x: -self.x, y: -self.y, z: -self.z, w: self.w }
    }

    pub fn dot(self, o: Quat) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z + self.w * o.w
    }

    pub fn normalized(self) -> Quat {
        let l = self.dot(self).sqrt();
        if l > 1e-12 { Quat { x: self.x / l, y: self.y / l, z: self.z / l, w: self.w / l } } else { Quat::IDENTITY }
    }

    /// `v` turned by this rotation.
    pub fn rotate(self, v: V3) -> V3 {
        let q = V3::new(self.x, self.y, self.z);
        let t = q.cross(v) * 2.0;
        v + t * self.w + q.cross(t)
    }

    /// `v` turned back (by the inverse rotation).
    pub fn unrotate(self, v: V3) -> V3 {
        self.conj().rotate(v)
    }

    /// After turning at angular velocity `w` (radians a second, world frame) for `dt` seconds.
    pub fn integrate(self, w: V3, dt: f32) -> Quat {
        let h = dt * 0.5;
        let dq = Quat { x: w.x * h, y: w.y * h, z: w.z * h, w: 0.0 } * self;
        Quat { x: self.x + dq.x, y: self.y + dq.y, z: self.z + dq.z, w: self.w + dq.w }.normalized()
    }

    /// Normalised linear blend along the shorter arc: smooth enough between two ticks.
    pub fn nlerp(self, o: Quat, t: f32) -> Quat {
        let o = if self.dot(o) < 0.0 { Quat { x: -o.x, y: -o.y, z: -o.z, w: -o.w } } else { o };
        Quat {
            x: self.x + (o.x - self.x) * t,
            y: self.y + (o.y - self.y) * t,
            z: self.z + (o.z - self.z) * t,
            w: self.w + (o.w - self.w) * t,
        }
        .normalized()
    }

    /// The rotation as a matrix (its columns are the rotated x, y and z axes).
    pub fn mat(self) -> M3 {
        M3 { c: [self.rotate(V3::X), self.rotate(V3::Y), self.rotate(V3::Z)] }
    }
}

impl Mul for Quat {
    type Output = Quat;
    /// `self * o`: first `o`, then `self`.
    fn mul(self, o: Quat) -> Quat {
        Quat {
            w: self.w * o.w - self.x * o.x - self.y * o.y - self.z * o.z,
            x: self.w * o.x + self.x * o.w + self.y * o.z - self.z * o.y,
            y: self.w * o.y - self.x * o.z + self.y * o.w + self.z * o.x,
            z: self.w * o.z + self.x * o.y - self.y * o.x + self.z * o.w,
        }
    }
}

/// A 3 × 3 matrix by columns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct M3 {
    pub c: [V3; 3],
}

impl M3 {
    pub const ZERO: M3 = M3 { c: [V3::ZERO; 3] };

    pub fn diag(d: V3) -> M3 {
        M3 { c: [V3::new(d.x, 0.0, 0.0), V3::new(0.0, d.y, 0.0), V3::new(0.0, 0.0, d.z)] }
    }

    pub fn mul_v(&self, v: V3) -> V3 {
        self.c[0] * v.x + self.c[1] * v.y + self.c[2] * v.z
    }

    pub fn transpose(&self) -> M3 {
        let [a, b, c] = self.c;
        M3 { c: [V3::new(a.x, b.x, c.x), V3::new(a.y, b.y, c.y), V3::new(a.z, b.z, c.z)] }
    }

    pub fn mul(&self, o: &M3) -> M3 {
        M3 { c: [self.mul_v(o.c[0]), self.mul_v(o.c[1]), self.mul_v(o.c[2])] }
    }

    /// `R · diag(d) · Rᵀ`: a body's inverse inertia in the world, from its local diagonal and its rotation.
    pub fn sandwich(r: &M3, d: V3) -> M3 {
        r.mul(&M3::diag(d)).mul(&r.transpose())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: V3, b: V3) -> bool {
        (a - b).length() < 1e-5
    }

    #[test]
    fn a_quarter_turn_about_y_takes_x_to_minus_z() {
        let q = Quat::axis_angle(V3::Y, std::f32::consts::FRAC_PI_2);
        assert!(near(q.rotate(V3::X), V3::new(0.0, 0.0, -1.0)));
        assert!(near(q.unrotate(q.rotate(V3::new(1.0, 2.0, 3.0))), V3::new(1.0, 2.0, 3.0)));
    }

    #[test]
    fn integrating_a_spin_for_a_second_turns_by_its_rate() {
        let mut q = Quat::IDENTITY;
        for _ in 0..1000 {
            q = q.integrate(V3::new(0.0, std::f32::consts::FRAC_PI_2, 0.0), 0.001);
        }
        assert!(near(q.rotate(V3::X), V3::new(0.0, 0.0, -1.0)), "{:?}", q.rotate(V3::X));
    }

    #[test]
    fn products_compose_right_to_left_and_matrices_agree() {
        let a = Quat::axis_angle(V3::X, 0.3);
        let b = Quat::axis_angle(V3::Z, 1.1);
        let v = V3::new(0.2, -0.7, 1.3);
        assert!(near((a * b).rotate(v), a.rotate(b.rotate(v))));
        assert!(near((a * b).mat().mul_v(v), (a * b).rotate(v)));
    }

    #[test]
    fn a_basis_is_orthonormal() {
        for n in [V3::X, V3::Y, V3::Z, V3::new(0.6, 0.8, 0.0), V3::new(-0.3, 0.1, 0.9).normalized()] {
            let (t, b) = n.basis();
            assert!(t.dot(n).abs() < 1e-5 && b.dot(n).abs() < 1e-5 && t.dot(b).abs() < 1e-5);
            assert!((t.length() - 1.0).abs() < 1e-5 && (b.length() - 1.0).abs() < 1e-5);
        }
    }
}
