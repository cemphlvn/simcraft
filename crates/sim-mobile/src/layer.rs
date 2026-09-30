//! Screen layers: where things go on a phone screen and in which order they are drawn.
//!
//! A frame is a list of shapes, each on a [`Layer`] (back to front: backdrop, board, pieces, fx, hud, overlay) with
//! an order inside it. HUD nodes are placed by [`Anchor`] inside the safe area (the part no notch, island or home
//! bar covers), and the board is fitted into it whole, whatever the screen's shape. After AutoGameUI's model of a game
//! screen (a tree of nodes with anchors, render order and meaning; `docs/research/mobile-types.md` §4). Drawing is
//! the host's job, so this is plain `f32` pixels; nothing here reaches the simulation.

use sim_physics::fixed::Fx;
use sim_physics::verlet::V2;

use crate::gesture::Px;

/// The layers of a screen, back to front.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    Backdrop,
    Board,
    Pieces,
    Fx,
    Hud,
    Overlay,
}

/// A rectangle in pixels, y down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn center(&self) -> Px {
        Px::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

/// What covers the screen's edges (pixels).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
}

impl Insets {
    /// A portrait phone's usual cover when the platform does not say: a status bar or an island on top (59 points on
    /// current iPhones), a home indicator below (34 points). `scale` is pixels per point.
    pub fn phone(scale: f32) -> Insets {
        Insets { top: 59.0 * scale, bottom: 34.0 * scale, left: 0.0, right: 0.0 }
    }

    /// The screen minus the cover.
    pub fn safe(&self, w: f32, h: f32) -> Rect {
        Rect::new(self.left, self.top, (w - self.left - self.right).max(0.0), (h - self.top - self.bottom).max(0.0))
    }
}

/// Where a node sits in its area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Top,
    Bottom,
    Center,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// A node of `size` placed by `anchor` in `area`, then moved by `offset` (towards the area's inside: a positive
/// offset from a top anchor moves down, from a bottom anchor up).
pub fn place(anchor: Anchor, size: (f32, f32), offset: (f32, f32), area: Rect) -> Rect {
    let (w, h) = size;
    let (dx, dy) = offset;
    let left = area.x + dx;
    let right = area.x + area.w - w - dx;
    let midx = area.x + (area.w - w) / 2.0 + dx;
    let top = area.y + dy;
    let bottom = area.y + area.h - h - dy;
    let midy = area.y + (area.h - h) / 2.0 + dy;
    let (x, y) = match anchor {
        Anchor::Top => (midx, top),
        Anchor::Bottom => (midx, bottom),
        Anchor::Center => (midx, midy),
        Anchor::TopLeft => (left, top),
        Anchor::TopRight => (right, top),
        Anchor::BottomLeft => (left, bottom),
        Anchor::BottomRight => (right, bottom),
    };
    Rect::new(x, y, w, h)
}

/// A board of `w` × `h` world units (y up) fitted whole into a rectangle of the screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    /// Pixels per world unit.
    pub scale: f32,
    /// Where the board lands on the screen.
    pub rect: Rect,
}

impl Fit {
    /// The largest board that fits `area`, centred.
    pub fn new(w: f32, h: f32, area: Rect) -> Fit {
        let scale = (area.w / w).min(area.h / h);
        let (bw, bh) = (w * scale, h * scale);
        Fit { scale, rect: Rect::new(area.x + (area.w - bw) / 2.0, area.y + (area.h - bh) / 2.0, bw, bh) }
    }

    /// A world point (floats, for drawing) on the screen.
    pub fn px(&self, x: f32, y: f32) -> Px {
        Px::new(self.rect.x + x * self.scale, self.rect.y + self.rect.h - y * self.scale)
    }

    /// A simulation point on the screen.
    pub fn px_of(&self, p: V2) -> Px {
        self.px(fx_f32(p.x), fx_f32(p.y))
    }

    /// A screen point in the world, rounded to the simulation's numbers: a finger enters the simulation as integers.
    pub fn world(&self, p: Px) -> V2 {
        let x = (p.x - self.rect.x) / self.scale;
        let y = (self.rect.y + self.rect.h - p.y) / self.scale;
        V2::new(f32_fx(x), f32_fx(y))
    }
}

pub fn fx_f32(v: Fx) -> f32 {
    v.0 as f32 / Fx::ONE.0 as f32
}

pub fn f32_fx(v: f32) -> Fx {
    Fx((v * Fx::ONE.0 as f32).round() as i64)
}

/// A colour, linear and premultiplied, ready for the GPU.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Color(pub [f32; 4]);

impl Color {
    /// An sRGB hex colour (`0xRRGGBB`), opaque.
    pub fn hex(rgb: u32) -> Color {
        Color::hexa(rgb, 1.0)
    }

    /// An sRGB hex colour with alpha.
    pub fn hexa(rgb: u32, a: f32) -> Color {
        let lin = |c: u32| {
            let s = c as f32 / 255.0;
            if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
        };
        Color([lin(rgb >> 16 & 0xff) * a, lin(rgb >> 8 & 0xff) * a, lin(rgb & 0xff) * a, a])
    }
}

/// What can be drawn: a capsule (a segment with a radius; a circle when both ends meet) or a rounded box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Capsule { a: Px, b: Px, r: f32, color: Color },
    Box { rect: Rect, r: f32, color: Color },
}

impl Shape {
    pub fn circle(at: Px, r: f32, color: Color) -> Shape {
        Shape::Capsule { a: at, b: at, r, color }
    }
}

/// One frame: shapes on layers. Drawn by layer, then by order, then as added.
#[derive(Clone, Debug, Default)]
pub struct Frame {
    items: Vec<(Layer, i32, Shape)>,
}

impl Frame {
    pub fn push(&mut self, layer: Layer, order: i32, shape: Shape) {
        self.items.push((layer, order, shape));
    }

    /// The shapes back to front.
    pub fn sorted(mut self) -> Vec<Shape> {
        self.items.sort_by_key(|&(layer, order, _)| (layer, order));
        self.items.into_iter().map(|(_, _, s)| s).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_board_fits_whole_and_centred_on_any_screen() {
        // 9 × 16 into a tall phone's safe area: width-bound, centred vertically.
        let f = Fit::new(9.0, 16.0, Rect::new(0.0, 100.0, 900.0, 2000.0));
        assert_eq!(f.scale, 100.0);
        assert_eq!(f.rect, Rect::new(0.0, 300.0, 900.0, 1600.0));
        // Into a tablet: height-bound, centred horizontally.
        let f = Fit::new(9.0, 16.0, Rect::new(0.0, 0.0, 2000.0, 1600.0));
        assert_eq!(f.scale, 100.0);
        assert_eq!(f.rect.x, 550.0);
    }

    #[test]
    fn world_and_screen_round_trip() {
        let f = Fit::new(9.0, 16.0, Rect::new(0.0, 0.0, 900.0, 1600.0));
        assert_eq!(f.px(0.0, 0.0), Px::new(0.0, 1600.0), "the world's origin is the board's bottom left");
        let w = f.world(Px::new(450.0, 800.0));
        assert_eq!(w, V2::new(Fx::ratio(9, 2), Fx::int(8)));
        assert_eq!(f.px_of(w), Px::new(450.0, 800.0));
    }

    #[test]
    fn anchors_keep_hud_inside_the_safe_area() {
        let safe = Insets::phone(3.0).safe(1179.0, 2556.0);
        let top = place(Anchor::Top, (300.0, 60.0), (0.0, 12.0), safe);
        assert_eq!(top.y, 59.0 * 3.0 + 12.0, "below the island");
        assert_eq!(top.x, (1179.0 - 300.0) / 2.0);
        let bottom = place(Anchor::BottomRight, (100.0, 100.0), (20.0, 20.0), safe);
        assert_eq!(bottom.y + bottom.h, 2556.0 - 34.0 * 3.0 - 20.0, "above the home bar");
        assert_eq!(bottom.x + bottom.w, 1179.0 - 20.0);
    }

    #[test]
    fn a_frame_draws_by_layer_then_order_then_as_added() {
        let dot = |x: f32| Shape::circle(Px::new(x, 0.0), 1.0, Color::hex(0));
        let mut f = Frame::default();
        f.push(Layer::Hud, 0, dot(1.0));
        f.push(Layer::Backdrop, 5, dot(2.0));
        f.push(Layer::Pieces, 1, dot(3.0));
        f.push(Layer::Pieces, 0, dot(4.0));
        f.push(Layer::Pieces, 0, dot(5.0));
        let xs: Vec<f32> = f.sorted().iter().map(|s| if let Shape::Capsule { a, .. } = s { a.x } else { -1.0 }).collect();
        assert_eq!(xs, vec![2.0, 4.0, 5.0, 3.0, 1.0]);
    }

    #[test]
    fn colours_are_linear_and_premultiplied() {
        assert_eq!(Color::hex(0xffffff).0, [1.0, 1.0, 1.0, 1.0]);
        let half = Color::hexa(0xffffff, 0.5).0;
        assert_eq!(half, [0.5, 0.5, 0.5, 0.5]);
        assert!((Color::hex(0x808080).0[0] - 0.2158).abs() < 0.001, "sRGB mid-grey is darker in linear light");
    }
}
