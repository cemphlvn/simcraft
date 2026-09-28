//! Splitting a rectangle into rows or columns.

use serde::Deserialize;

use crate::canvas::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub enum Size {
    /// Exactly this many cells.
    Fixed(u16),
    /// This % of the space.
    Percent(u16),
    /// A share of what is left after Fixed and Percent (equal shares).
    Fill,
}

fn split(total: u16, sizes: &[Size]) -> Vec<u16> {
    let fixed: u16 = sizes
        .iter()
        .map(|s| match s {
            Size::Fixed(n) => *n,
            Size::Percent(p) => (total as u32 * *p as u32 / 100) as u16,
            Size::Fill => 0,
        })
        .fold(0u16, u16::saturating_add);
    let fills = sizes.iter().filter(|s| matches!(s, Size::Fill)).count() as u16;
    let rest = total.saturating_sub(fixed);
    let mut out: Vec<u16> = sizes
        .iter()
        .map(|s| match s {
            Size::Fixed(n) => *n,
            Size::Percent(p) => (total as u32 * *p as u32 / 100) as u16,
            Size::Fill => rest / fills.max(1),
        })
        .collect();
    // The last Fill takes the rounding remainder.
    if let Some(i) = sizes.iter().rposition(|s| matches!(s, Size::Fill)) {
        out[i] += rest - (rest / fills.max(1)) * fills;
    }
    out
}

pub fn rows(r: Rect, sizes: &[Size]) -> Vec<Rect> {
    let mut y = r.y;
    split(r.h, sizes)
        .into_iter()
        .map(|h| {
            let h = h.min(r.y + r.h - y.min(r.y + r.h));
            let out = Rect::new(r.x, y, r.w, h);
            y += h;
            out
        })
        .collect()
}

pub fn cols(r: Rect, sizes: &[Size]) -> Vec<Rect> {
    let mut x = r.x;
    split(r.w, sizes)
        .into_iter()
        .map(|w| {
            let w = w.min(r.x + r.w - x.min(r.x + r.w));
            let out = Rect::new(x, r.y, w, r.h);
            x += w;
            out
        })
        .collect()
}
