//! simcraft renderer: a component library for game interfaces, with its own terminal renderer.
//! See docs/architecture.md, "Renderer".

pub mod canvas;
pub mod component;
pub mod layered;
pub mod layout;
pub mod projection;
pub mod scene;
pub mod style;
pub mod term;
pub mod view;

pub use canvas::{Canvas, Cell, Rect, Rgb};
pub use component::{Component, Ctx, Registry, Ui, props};
pub use layout::{Size, cols, rows};
pub use projection::{Camera, Projection};
pub use scene::Scene;
pub use style::{Assets, Border, Look, Style, Theme};
pub use term::{Renderer, TerminalGuard};
pub use view::{Node, View, c, cp};
