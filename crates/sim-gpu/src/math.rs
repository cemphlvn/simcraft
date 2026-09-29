//! The little 3D math the renderer needs: vectors and one view-projection matrix (column-major, for WGSL).

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct V3(pub f32, pub f32, pub f32);

impl std::ops::Add for V3 {
    type Output = V3;
    fn add(self, o: V3) -> V3 {
        V3(self.0 + o.0, self.1 + o.1, self.2 + o.2)
    }
}

impl std::ops::Sub for V3 {
    type Output = V3;
    fn sub(self, o: V3) -> V3 {
        V3(self.0 - o.0, self.1 - o.1, self.2 - o.2)
    }
}

impl V3 {
    pub fn scale(self, k: f32) -> V3 {
        V3(self.0 * k, self.1 * k, self.2 * k)
    }
    pub fn dot(self, o: V3) -> f32 {
        self.0 * o.0 + self.1 * o.1 + self.2 * o.2
    }
    pub fn cross(self, o: V3) -> V3 {
        V3(self.1 * o.2 - self.2 * o.1, self.2 * o.0 - self.0 * o.2, self.0 * o.1 - self.1 * o.0)
    }
    pub fn norm(self) -> V3 {
        let l = self.dot(self).sqrt().max(1e-6);
        self.scale(1.0 / l)
    }
}

/// A camera: where it is, what it looks at, how it is rolled, how wide it sees.
#[derive(Clone, Copy, Debug)]
pub struct Eye {
    pub pos: V3,
    pub target: V3,
    /// Degrees, positive = the view tilts clockwise (banking into a left turn).
    pub roll: f32,
    /// Vertical field of view, degrees.
    pub fov: f32,
    pub near: f32,
    pub far: f32,
}

impl Eye {
    /// Right, up and forward axes (left-handed: x right, y up, z forward).
    pub fn basis(&self) -> (V3, V3, V3) {
        let f = (self.target - self.pos).norm();
        let r = V3(0.0, 1.0, 0.0).cross(f).norm();
        let u = f.cross(r);
        let (s, c) = self.roll.to_radians().sin_cos();
        (r.scale(c) + u.scale(s), u.scale(c) - r.scale(s), f)
    }

    /// View × projection for a `w × h` target, column-major (`mat4x4<f32>` in WGSL), depth 0..1 (wgpu).
    pub fn view_proj(&self, w: f32, h: f32) -> [[f32; 4]; 4] {
        let (r, u, f) = self.basis();
        let sy = 1.0 / (self.fov.to_radians() / 2.0).tan();
        let sx = sy / (w / h.max(1.0));
        let (n, fa) = (self.near, self.far);
        let a = fa / (fa - n);
        let b = -n * fa / (fa - n);
        let e = self.pos;
        // Rows of the combined matrix: clip = P · V · p.
        let rows = [
            [r.0 * sx, r.1 * sx, r.2 * sx, -r.dot(e) * sx],
            [u.0 * sy, u.1 * sy, u.2 * sy, -u.dot(e) * sy],
            [f.0 * a, f.1 * a, f.2 * a, -f.dot(e) * a + b],
            [f.0, f.1, f.2, -f.dot(e)],
        ];
        std::array::from_fn(|c| std::array::from_fn(|r| rows[r][c]))
    }

    /// Screen pixel of a world point, or `None` behind the camera (for tests and 2D overlays).
    pub fn project(&self, p: V3, w: f32, h: f32) -> Option<(f32, f32)> {
        let m = self.view_proj(w, h);
        let v = [p.0, p.1, p.2, 1.0];
        let clip: [f32; 4] = std::array::from_fn(|r| (0..4).map(|c| m[c][r] * v[c]).sum());
        (clip[3] > 1e-4).then(|| ((clip[0] / clip[3] * 0.5 + 0.5) * w, (0.5 - clip[1] / clip[3] * 0.5) * h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eye() -> Eye {
        Eye { pos: V3(0.0, 1.0, 0.0), target: V3(0.0, 1.0, 10.0), roll: 0.0, fov: 70.0, near: 0.05, far: 200.0 }
    }

    #[test]
    fn straight_ahead_is_the_centre_and_far_things_meet_the_horizon() {
        let e = eye();
        let (x, y) = e.project(V3(0.0, 1.0, 5.0), 1600.0, 900.0).unwrap();
        assert!((x - 800.0).abs() < 0.01 && (y - 450.0).abs() < 0.01);
        let near = e.project(V3(1.0, 0.0, 2.0), 1600.0, 900.0).unwrap();
        let far = e.project(V3(1.0, 0.0, 100.0), 1600.0, 900.0).unwrap();
        assert!(near.0 > far.0 && near.1 > far.1, "the road narrows and rises to the horizon");
        assert!(e.project(V3(0.0, 1.0, -1.0), 1600.0, 900.0).is_none(), "behind the camera");
    }

    #[test]
    fn right_is_right_and_roll_tilts() {
        let e = eye();
        assert!(e.project(V3(1.0, 1.0, 5.0), 1600.0, 900.0).unwrap().0 > 800.0);
        let rolled = Eye { roll: 10.0, ..eye() };
        let p = rolled.project(V3(2.0, 1.0, 5.0), 1600.0, 900.0).unwrap();
        assert!((p.1 - 450.0).abs() > 1.0, "a roll moves an off-centre point vertically");
    }
}
