//! Deterministic numbers for physics: the same bits on every machine, compiler and core count.
//!
//! [`Fx`] is a Q48.16 fixed-point number (the format Photon Quantum uses for deterministic lockstep): 48 integer
//! bits, 16 fractional bits, products and quotients through `i128` so they cannot overflow midway. [`Angle`] is a
//! binary angle: a full turn is 2^32, so turning past a full circle wraps for free. Sine and cosine read a
//! quarter-wave table built at compile time with integer maths only (a Taylor series in Q62), interpolated
//! between entries. No floats anywhere: `docs/research/driving-physics.md` §3.

use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

/// Fractional bits of [`Fx`].
pub const FRAC: u32 = 16;

/// A Q48.16 fixed-point number. Arithmetic rounds toward negative infinity (a right shift), the same way everywhere.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fx(pub i64);

impl Fx {
    pub const ZERO: Fx = Fx(0);
    pub const ONE: Fx = Fx(1 << FRAC);
    pub const HALF: Fx = Fx(1 << (FRAC - 1));

    pub const fn int(n: i64) -> Fx {
        Fx(n << FRAC)
    }

    /// `n / d` as a fixed-point number (a spec-sheet value in thousandths: `Fx::ratio(3910, 1000)`).
    pub const fn ratio(n: i64, d: i64) -> Fx {
        Fx((((n as i128) << FRAC) / d as i128) as i64)
    }

    /// The whole part, rounded toward negative infinity.
    pub const fn floor(self) -> i64 {
        self.0 >> FRAC
    }

    /// The nearest whole number (halves round up).
    pub const fn round(self) -> i64 {
        (self.0 + (1 << (FRAC - 1))) >> FRAC
    }

    /// `self × scale`, rounded to a whole number: back to the engine's integer units (`FINE`, thousandths).
    pub const fn to_units(self, scale: i64) -> i64 {
        Fx((self.0 as i128 * scale as i128) as i64).round()
    }

    pub const fn abs(self) -> Fx {
        Fx(self.0.abs())
    }

    pub const fn signum(self) -> i64 {
        self.0.signum()
    }

    pub fn min(self, o: Fx) -> Fx {
        Ord::min(self, o)
    }

    pub fn max(self, o: Fx) -> Fx {
        Ord::max(self, o)
    }

    pub fn clamp(self, lo: Fx, hi: Fx) -> Fx {
        Ord::clamp(self, lo, hi)
    }

    /// The square root (integer Newton iteration, exact to the last bit: the largest `r` with `r × r <= self`).
    /// Negative numbers have none; asked for one, it answers zero.
    pub fn sqrt(self) -> Fx {
        if self.0 <= 0 {
            debug_assert!(self.0 == 0, "sqrt of a negative number: {self:?}");
            return Fx::ZERO;
        }
        // The standard library's integer root is exact by definition; 64 bits when the shifted value fits (2.5×
        // faster than 128, measured), 128 otherwise.
        if self.0 < 1 << (63 - FRAC) { Fx(((self.0 as u64) << FRAC).isqrt() as i64) } else { Fx(((self.0 as u128) << FRAC).isqrt() as i64) }
    }
}

impl Add for Fx {
    type Output = Fx;
    fn add(self, o: Fx) -> Fx {
        Fx(self.0 + o.0)
    }
}

impl Sub for Fx {
    type Output = Fx;
    fn sub(self, o: Fx) -> Fx {
        Fx(self.0 - o.0)
    }
}

impl AddAssign for Fx {
    fn add_assign(&mut self, o: Fx) {
        self.0 += o.0;
    }
}

impl SubAssign for Fx {
    fn sub_assign(&mut self, o: Fx) {
        self.0 -= o.0;
    }
}

impl Neg for Fx {
    type Output = Fx;
    fn neg(self) -> Fx {
        Fx(-self.0)
    }
}

impl Mul for Fx {
    type Output = Fx;
    fn mul(self, o: Fx) -> Fx {
        Fx(((self.0 as i128 * o.0 as i128) >> FRAC) as i64)
    }
}

/// Scaling by a whole number is exact.
impl Mul<i64> for Fx {
    type Output = Fx;
    fn mul(self, k: i64) -> Fx {
        Fx(self.0 * k)
    }
}

/// Division rounds toward negative infinity, like the shift in `Mul`. Dividing by zero is a bug in the caller
/// (a physics step guards its denominators); in a release build it answers zero instead of stopping the game.
impl Div for Fx {
    type Output = Fx;
    fn div(self, o: Fx) -> Fx {
        if o.0 == 0 {
            debug_assert!(false, "{self:?} / 0");
            return Fx::ZERO;
        }
        // A 64-bit divide when the shifted dividend fits (4× faster than i128's library call, measured); the
        // same answer either way.
        if self.0.unsigned_abs() < 1 << (62 - FRAC) {
            Fx((self.0 << FRAC).div_euclid(o.0))
        } else {
            Fx(((self.0 as i128) << FRAC).div_euclid(o.0 as i128) as i64)
        }
    }
}

impl Div<i64> for Fx {
    type Output = Fx;
    fn div(self, k: i64) -> Fx {
        if k == 0 {
            debug_assert!(false, "{self:?} / 0");
            return Fx::ZERO;
        }
        Fx(self.0.div_euclid(k))
    }
}

/// Bits of a full turn in an [`Angle`].
pub const TURN_BITS: u32 = 32;
/// A full turn.
pub const TURN: i64 = 1 << TURN_BITS;

/// A binary angle: `TURN` units a full circle, counterclockwise. Any value is valid; sin and cos wrap it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Angle(pub i64);

impl Angle {
    pub const ZERO: Angle = Angle(0);
    pub const QUARTER: Angle = Angle(TURN / 4);
    pub const HALF: Angle = Angle(TURN / 2);

    /// `n / d` of a full turn (`Angle::turns(1, 4)` is a right angle).
    pub const fn turns(n: i64, d: i64) -> Angle {
        Angle(((n as i128 * TURN as i128) / d as i128) as i64)
    }

    /// An angle in hundredths of a degree (spec sheets: a steering lock of 48.00° is 4800).
    pub const fn centidegrees(c: i64) -> Angle {
        Angle::turns(c, 36000)
    }

    /// An angle in radians.
    pub fn radians(r: Fx) -> Angle {
        Angle(((r.0 as i128 * TURN as i128) / TWO_PI.0 as i128) as i64)
    }

    /// This angle scaled by `k` (a steering input of 0.25 turns the wheels a quarter of the way to full lock).
    pub fn times(self, k: Fx) -> Angle {
        Angle(((self.0 as i128 * k.0 as i128) >> FRAC) as i64)
    }

    /// The direction of the vector (x, y): CORDIC in vectoring mode, shifts and adds only (Volder 1959). Exact to a
    /// few units of `TURN` (under a millionth of a degree); (0, 0) has no direction and answers zero.
    pub fn atan2(y: Fx, x: Fx) -> Angle {
        let (mut x, mut y) = (x.0, y.0);
        if x == 0 && y == 0 {
            return Angle::ZERO;
        }
        // Scale up to 60 bits so the shifts below keep their precision (the rotation grows it by ~1.65, still
        // inside i64).
        let big = x.unsigned_abs().max(y.unsigned_abs());
        let up = (big.leading_zeros() as i32 - 4).max(0) as u32;
        let down = (4 - big.leading_zeros() as i32).max(0) as u32;
        x = (x << up) >> down;
        y = (y << up) >> down;
        // Into the right half-plane first: CORDIC turns at most ~100° either way.
        let mut z = 0i64;
        if x < 0 {
            x = -x;
            y = -y;
            z = TURN / 2;
        }
        for (i, &a) in ATAN_TABLE.iter().enumerate() {
            let (dx, dy) = (y >> i, x >> i);
            if y > 0 {
                x += dx;
                y -= dy;
                z += a;
            } else {
                x -= dx;
                y += dy;
                z -= a;
            }
        }
        Angle(z)
    }

    /// The same direction, within `-TURN/2..TURN/2` (a difference of two headings, the short way round).
    pub const fn signed(self) -> Angle {
        Angle(((self.0 + TURN / 2) & (TURN - 1)) - TURN / 2)
    }

    /// The same direction, within `0..TURN`.
    pub const fn wrapped(self) -> Angle {
        Angle(self.0 & (TURN - 1))
    }

    /// The angle in radians.
    pub fn to_radians(self) -> Fx {
        Fx(((self.0 as i128 * TWO_PI.0 as i128) >> TURN_BITS) as i64)
    }

    pub fn sin(self) -> Fx {
        let a = self.0 & (TURN - 1);
        let quadrant = a >> (TURN_BITS - 2);
        let within = a & (TURN / 4 - 1);
        match quadrant {
            0 => quarter(within),
            1 => quarter(TURN / 4 - within),
            2 => -quarter(within),
            _ => -quarter(TURN / 4 - within),
        }
    }

    pub fn cos(self) -> Fx {
        (self + Angle::QUARTER).sin()
    }
}

impl Add for Angle {
    type Output = Angle;
    fn add(self, o: Angle) -> Angle {
        Angle(self.0 + o.0)
    }
}

impl Sub for Angle {
    type Output = Angle;
    fn sub(self, o: Angle) -> Angle {
        Angle(self.0 - o.0)
    }
}

impl Neg for Angle {
    type Output = Angle;
    fn neg(self) -> Angle {
        Angle(-self.0)
    }
}

/// 2π, Q48.16.
pub const TWO_PI: Fx = Fx(411775);

/// Entries in a quarter wave (a power of two), plus one for the end.
const QUARTER_BITS: u32 = 12;
const QUARTER_N: usize = 1 << QUARTER_BITS;
/// The table's own precision: Q30, far finer than `Fx`, so interpolating loses nothing a caller can see.
const TABLE_FRAC: u32 = 30;
static SIN_TABLE: [i64; QUARTER_N + 1] = sin_table();

/// sin over the first quarter (`0..=TURN/4`): interpolate the table, round to `Fx`.
fn quarter(a: i64) -> Fx {
    let step = TURN_BITS - 2 - QUARTER_BITS;
    let i = (a >> step) as usize;
    let f = a & ((1 << step) - 1);
    let lo = SIN_TABLE[i];
    let hi = SIN_TABLE[(i + 1).min(QUARTER_N)];
    let v = lo + (((hi - lo) as i128 * f as i128) >> step) as i64;
    Fx((v + (1 << (TABLE_FRAC - FRAC - 1))) >> (TABLE_FRAC - FRAC))
}

/// sin(k · π/2 / N) for k in 0..=N, in Q30, by a Taylor series in Q62 (`i128`, integers only).
const fn sin_table() -> [i64; QUARTER_N + 1] {
    // π/2 in Q62, from 60 decimal digits of π.
    const HALF_PI_Q62: i128 = 7_244_019_458_077_122_842;
    let mut t = [0i64; QUARTER_N + 1];
    let mut k = 0;
    while k <= QUARTER_N {
        let x = HALF_PI_Q62 * k as i128 / QUARTER_N as i128;
        let x2 = (x * x) >> 62;
        // sin x = x − x³/3! + x⁵/5! − …: each term is the last times −x²/((2n)(2n+1)).
        let mut term = x;
        let mut sum = x;
        let mut n = 1i128;
        while term != 0 {
            term = -((term * x2) >> 62) / ((2 * n) * (2 * n + 1));
            sum += term;
            n += 1;
        }
        // Q62 → Q30, rounded.
        t[k] = ((sum + (1 << 31)) >> 32) as i64;
        k += 1;
    }
    t
}

/// atan(2^-i) in `TURN` units, i = 0..31, by the Taylor series in Q62 (integers only), for CORDIC.
static ATAN_TABLE: [i64; 32] = atan_table();

const fn atan_table() -> [i64; 32] {
    const TWO_PI_Q62: i128 = 4 * 7_244_019_458_077_122_842;
    let mut t = [0i64; 32];
    t[0] = TURN / 8;
    let mut i = 1;
    while i < 32 {
        // atan x = x − x³/3 + x⁵/5 − …
        let x: i128 = 1 << (62 - i);
        let x2 = (x * x) >> 62;
        let (mut pow, mut sum, mut n) = (x, x, 1i128);
        while pow != 0 {
            pow = -((pow * x2) >> 62);
            sum += pow / (2 * n + 1);
            n += 1;
        }
        t[i] = ((sum * TURN as i128 + TWO_PI_Q62 / 2) / TWO_PI_Q62) as i64;
        i += 1;
    }
    t
}

/// A curve through points (x ascending), read by linear interpolation and held flat past either end: an engine's
/// torque against RPM, straight from a dyno sheet (`docs/research/driving-physics.md` §6.2).
pub fn curve(points: &[(i64, i64)], x: i64) -> i64 {
    let Some(&(x0, y0)) = points.first() else { return 0 };
    if x <= x0 {
        return y0;
    }
    for w in points.windows(2) {
        let ((xa, ya), (xb, yb)) = (w[0], w[1]);
        if x <= xb {
            return ya + ((yb - ya) as i128 * (x - xa) as i128 / (xb - xa).max(1) as i128) as i64;
        }
    }
    points[points.len() - 1].1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(x: Fx) -> f64 {
        x.0 as f64 / (1u64 << FRAC) as f64
    }

    #[test]
    fn arithmetic_rounds_the_same_way_everywhere() {
        assert_eq!(Fx::int(3) * Fx::ratio(1, 2), Fx::ratio(3, 2));
        assert_eq!((Fx::int(7) / Fx::int(2)).round(), 4);
        assert_eq!(Fx::int(-7) / Fx::int(2), Fx::ratio(-7, 2));
        // Rounding toward negative infinity, for both signs: the smallest step down stays a step down.
        assert_eq!(Fx(1) * Fx::HALF, Fx(0));
        assert_eq!(Fx(-1) * Fx::HALF, Fx(-1));
        assert_eq!(Fx(-1) / 2, Fx(-1));
        // No overflow midway: two big numbers multiply through i128.
        assert_eq!(Fx::int(1 << 30) * Fx::int(4), Fx::int(1 << 32));
        assert_eq!(Fx::ratio(2500, 1000).to_units(1000), 2500);
    }

    #[test]
    fn the_fast_paths_answer_what_the_wide_ones_do() {
        let edge = 1i64 << (62 - FRAC);
        for a in [1, -1, 12345, -98765, edge - 1, -(edge - 1), edge, -edge, edge * 5, i64::MAX >> 20] {
            for b in [1i64, -1, 3, -7, 65536, -65536 * 3 + 5, 1 << 40, -(1 << 33)] {
                let wide = ((a as i128) << FRAC).div_euclid(b as i128) as i64;
                assert_eq!((Fx(a) / Fx(b)).0, wide, "{a} / {b}");
            }
        }
    }

    #[test]
    fn square_roots_are_exact_to_the_last_bit() {
        for n in [0i64, 1, 2, 4, 9, 1000, 123_456, 1 << 40] {
            assert_eq!(Fx::int(n).sqrt().0, ((n as f64).sqrt() * 65536.0).floor() as i64, "sqrt {n}");
        }
        for raw in [1i64, 3, 17, 65535, 99_999_999, i64::MAX >> FRAC] {
            let r = Fx(raw).sqrt().0 as i128;
            let n = (raw as i128) << FRAC;
            assert!(r * r <= n && (r + 1) * (r + 1) > n, "sqrt raw {raw}");
        }
    }

    #[test]
    fn sine_and_cosine_match_the_real_ones_everywhere() {
        let mut worst = 0f64;
        for i in -3000i64..=3000 {
            let a = Angle(i * (TURN / 1000) + i * 7919);
            let rad = a.0 as f64 / TURN as f64 * std::f64::consts::TAU;
            worst = worst.max((f(a.sin()) - rad.sin()).abs()).max((f(a.cos()) - rad.cos()).abs());
        }
        // Within one step of Fx (1/65536 ≈ 1.5e-5).
        assert!(worst <= 1.0 / 65536.0, "worst error {worst}");
        assert_eq!(Angle::ZERO.sin(), Fx::ZERO);
        assert_eq!(Angle::QUARTER.sin(), Fx::ONE);
        assert_eq!(Angle::HALF.cos(), -Fx::ONE);
        assert_eq!(Angle::turns(1, 12).sin(), Fx::HALF);
        assert_eq!(Angle(-5).sin(), Angle(TURN - 5).sin());
        assert_eq!(Angle::QUARTER.to_radians().0, TWO_PI.0 / 4);
    }

    #[test]
    fn angles_convert_both_ways() {
        assert_eq!(Angle::centidegrees(9000), Angle::QUARTER);
        // Through radians and back: exact to one step of Fx (a radian in Q16 is coarser than a binary angle).
        let back = Angle::radians(Angle::QUARTER.to_radians());
        assert!((back.0 - Angle::QUARTER.0).abs() <= TURN / TWO_PI.0, "{back:?}");
        assert_eq!((Angle::HALF + Angle::HALF + Angle::QUARTER).wrapped(), Angle::QUARTER);
    }

    #[test]
    fn atan2_finds_every_direction() {
        for i in 0..2000i64 {
            let a = Angle(i * (TURN / 2000) + i * 104_729);
            for r in [Fx::ratio(1, 100), Fx::ONE, Fx::int(250), Fx::int(1 << 30)] {
                let (x, y) = (r * a.cos(), r * a.sin());
                let back = Angle::atan2(y, x);
                // The error comes from building (x, y): sin/cos are exact to one Fx step (1/65536 rad of direction),
                // and a tiny radius rounds its coordinates too. CORDIC itself adds a few units.
                let tol = (TURN / TWO_PI.0) * (2 + 2 * Fx::ONE.0 / r.0.max(1));
                let err = (back - a).signed().0.abs();
                assert!(err <= tol, "angle {a:?} radius {r:?}: {back:?}, off by {err}");
            }
        }
        // The axes, to CORDIC's own few units.
        let near = |a: Angle, b: i64| (a - Angle(b)).signed().0.abs() < 8;
        assert!(near(Angle::atan2(Fx::ZERO, Fx::ONE), 0));
        assert!(near(Angle::atan2(Fx::ONE, Fx::ZERO), TURN / 4));
        assert!(near(Angle::atan2(Fx::ZERO, -Fx::ONE), TURN / 2));
        assert!(near(Angle::atan2(-Fx::ONE, Fx::ZERO), -TURN / 4));
        assert_eq!(Angle::atan2(Fx::ZERO, Fx::ZERO), Angle::ZERO);
        assert_eq!(Angle(TURN - 10).signed(), Angle(-10));
    }

    #[test]
    fn a_curve_interpolates_and_holds_its_ends() {
        let torque = [(1000, 1200), (2000, 2100), (4000, 2500), (6500, 1900)];
        assert_eq!(curve(&torque, 500), 1200);
        assert_eq!(curve(&torque, 1500), 1650);
        assert_eq!(curve(&torque, 3000), 2300);
        assert_eq!(curve(&torque, 9000), 1900);
        assert_eq!(curve(&[], 10), 0);
    }
}
