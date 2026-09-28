//! The native renderer: diffs frames and writes only what changed, inside a synchronized update.

use std::io::{self, Write};

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor};
use crossterm::terminal::{
    BeginSynchronizedUpdate, EndSynchronizedUpdate, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{execute, queue};

use crate::canvas::{Canvas, Rgb};

fn color(c: Rgb) -> Color {
    Color::Rgb { r: c.0, g: c.1, b: c.2 }
}

/// Keeps the previous frame; `present` writes the difference.
pub struct Renderer {
    prev: Option<Canvas>,
    /// Cells written by the last `present` (for measuring).
    pub last_writes: usize,
}

impl Default for Renderer {
    fn default() -> Self {
        Renderer::new()
    }
}

impl Renderer {
    pub fn new() -> Renderer {
        Renderer { prev: None, last_writes: 0 }
    }

    /// Forget the previous frame (after a resize): the next frame is written in full.
    pub fn invalidate(&mut self) {
        self.prev = None;
    }

    /// Writes `frame` to `out`: only cells that differ from the previous frame, with cursor moves and colour
    /// changes only when needed, all inside one synchronized update (the terminal shows it at once).
    pub fn present(&mut self, frame: &Canvas, out: &mut impl Write) -> io::Result<()> {
        let full = self.prev.as_ref().is_none_or(|p| p.w != frame.w || p.h != frame.h);
        queue!(out, BeginSynchronizedUpdate)?;
        let (mut fg, mut bg) = (None, None);
        let mut cursor: Option<(u16, u16)> = None;
        let mut writes = 0;
        for y in 0..frame.h {
            for x in 0..frame.w {
                let cell = frame.get(x, y).copied().unwrap_or_default();
                if !full && self.prev.as_ref().and_then(|p| p.get(x, y)) == Some(&cell) {
                    continue;
                }
                if cursor != Some((x, y)) {
                    queue!(out, MoveTo(x, y))?;
                }
                if fg != Some(cell.fg) {
                    queue!(out, SetForegroundColor(color(cell.fg)))?;
                    fg = Some(cell.fg);
                }
                if bg != Some(cell.bg) {
                    queue!(out, SetBackgroundColor(color(cell.bg)))?;
                    bg = Some(cell.bg);
                }
                queue!(out, Print(cell.ch))?;
                cursor = Some((x + 1, y));
                writes += 1;
            }
        }
        queue!(out, ResetColor, EndSynchronizedUpdate)?;
        out.flush()?;
        self.prev = Some(frame.clone());
        self.last_writes = writes;
        Ok(())
    }
}

/// Raw mode + alternate screen + hidden cursor while alive; restored on drop and on panic.
pub struct TerminalGuard;

impl TerminalGuard {
    pub fn enter() -> io::Result<TerminalGuard> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, Hide, EnableMouseCapture)?;
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            default(info);
        }));
        Ok(TerminalGuard)
    }
}

fn restore() {
    // Also drop any pixel images we placed (kitty graphics protocol; ignored elsewhere).
    let _ = write!(io::stdout(), "\x1b_Ga=d,d=A,q=2\x1b\\");
    let _ = execute!(io::stdout(), DisableMouseCapture, ResetColor, Show, LeaveAlternateScreen);
    let _ = disable_raw_mode();
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::Cell;

    #[test]
    fn only_changed_cells_are_written() {
        let mut r = Renderer::new();
        let mut a = Canvas::new(40, 10);
        let mut sink = Vec::new();
        r.present(&a, &mut sink).unwrap();
        assert_eq!(r.last_writes, 400, "first frame: everything");
        sink.clear();
        r.present(&a, &mut sink).unwrap();
        assert_eq!(r.last_writes, 0, "same frame: nothing");
        a.put(3, 4, Cell { ch: 'a', fg: Rgb(255, 0, 0), bg: Rgb::BLACK });
        sink.clear();
        r.present(&a, &mut sink).unwrap();
        assert_eq!(r.last_writes, 1);
        let text = String::from_utf8_lossy(&sink);
        assert!(text.contains('a') && text.contains("\u{1b}[?2026h"), "synchronized update around the change");
    }
}
