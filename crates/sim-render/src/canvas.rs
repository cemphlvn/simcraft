//! A grid of cells: what one frame looks like. No terminal here.

/// 24-bit colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const BLACK: Rgb = Rgb(0, 0, 0);
    pub const WHITE: Rgb = Rgb(230, 230, 230);
    pub const DIM: Rgb = Rgb(110, 110, 120);

    /// Scale brightness by `pct` (0..=100+).
    pub fn shade(self, pct: u32) -> Rgb {
        let f = |c: u8| (c as u32 * pct / 100).min(255) as u8;
        Rgb(f(self.0), f(self.1), f(self.2))
    }

    /// A stable colour for a name (kinds, series).
    pub fn of(name: &str) -> Rgb {
        let h = name.bytes().fold(0x811c_9dc5_u32, |h, b| (h ^ b as u32).wrapping_mul(0x0100_0193));
        let hue = (h % 360) as f32;
        hsv(hue, 0.55, 0.95)
    }
}

/// HSV (hue in degrees) → RGB.
pub fn hsv(h: f32, s: f32, v: f32) -> Rgb {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let to = |u: f32| ((u + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgb(to(r), to(g), to(b))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: Rgb,
    pub bg: Rgb,
}

impl Default for Cell {
    fn default() -> Self {
        Cell { ch: ' ', fg: Rgb::WHITE, bg: Rgb::BLACK }
    }
}

/// A rectangle on the canvas (cells).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl Rect {
    pub fn new(x: u16, y: u16, w: u16, h: u16) -> Rect {
        Rect { x, y, w, h }
    }

    /// Shrink by `n` cells on every side.
    pub fn inner(self, n: u16) -> Rect {
        Rect { x: self.x + n, y: self.y + n, w: self.w.saturating_sub(2 * n), h: self.h.saturating_sub(2 * n) }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Canvas {
    pub w: u16,
    pub h: u16,
    cells: Vec<Cell>,
}

impl Canvas {
    pub fn new(w: u16, h: u16) -> Canvas {
        Canvas { w, h, cells: vec![Cell::default(); w as usize * h as usize] }
    }

    pub fn area(&self) -> Rect {
        Rect::new(0, 0, self.w, self.h)
    }

    pub fn clear(&mut self, bg: Rgb) {
        self.cells.iter_mut().for_each(|c| *c = Cell { ch: ' ', fg: Rgb::WHITE, bg });
    }

    pub fn get(&self, x: u16, y: u16) -> Option<&Cell> {
        (x < self.w && y < self.h).then(|| &self.cells[y as usize * self.w as usize + x as usize])
    }

    pub fn put(&mut self, x: u16, y: u16, cell: Cell) {
        if x < self.w && y < self.h {
            self.cells[y as usize * self.w as usize + x as usize] = cell;
        }
    }

    /// Set a character and foreground, keeping the background.
    pub fn glyph(&mut self, x: u16, y: u16, ch: char, fg: Rgb) {
        if let Some(bg) = self.get(x, y).map(|c| c.bg) {
            self.put(x, y, Cell { ch, fg, bg });
        }
    }

    /// Text clipped to `r`, starting at its top-left corner plus (dx, dy).
    pub fn text(&mut self, r: Rect, dx: u16, dy: u16, s: &str, fg: Rgb) {
        if dy >= r.h {
            return;
        }
        for (i, ch) in s.chars().enumerate() {
            let x = dx as usize + i;
            if x >= r.w as usize {
                break;
            }
            self.glyph(r.x + x as u16, r.y + dy, ch, fg);
        }
    }

    pub fn fill(&mut self, r: Rect, cell: Cell) {
        for y in r.y..r.y.saturating_add(r.h).min(self.h) {
            for x in r.x..r.x.saturating_add(r.w).min(self.w) {
                self.put(x, y, cell);
            }
        }
    }

    /// Two vertical pixels in one cell (upper half block): doubles the vertical resolution of pictures.
    pub fn pixels(&mut self, x: u16, y: u16, top: Rgb, bottom: Rgb) {
        self.put(x, y, Cell { ch: '▀', fg: top, bg: bottom });
    }

    pub fn row_text(&self, y: u16) -> String {
        (0..self.w).filter_map(|x| self.get(x, y).map(|c| c.ch)).collect()
    }
}
