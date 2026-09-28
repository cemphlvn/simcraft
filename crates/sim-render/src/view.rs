//! Views as data: `games/<name>/view.ron` picks a theme and asset packs, defines composite components and lays out
//! components with props, the way game.ron defines the game.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::canvas::{Canvas, Rect};
use crate::component::{Ctx, Registry, Ui};
use crate::layout::{Size, cols, rows};
use crate::projection::{Camera, Projection};
use crate::scene::Scene;
use crate::style::{Style, Theme};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "View", deny_unknown_fields)]
pub struct View {
    /// A built-in theme (`dark`, `light`); a game's theme.ron overrides its tokens.
    #[serde(default = "dark")]
    pub theme: String,
    /// Asset packs (`assets/<pack>.ron`), later ones win.
    #[serde(default)]
    pub assets: Vec<String>,
    /// Composite components: a name for a layout of other components.
    #[serde(default)]
    pub components: BTreeMap<String, Node>,
    pub layout: Node,
}

fn dark() -> String {
    "dark".into()
}

#[derive(Clone, Debug, Deserialize)]
pub enum Node {
    Rows(Vec<(Size, Node)>),
    Cols(Vec<(Size, Node)>),
    /// A component: built in, registered from Rust, or composite (from `components`).
    C {
        name: String,
        #[serde(default = "unit")]
        props: ron::Value,
        /// A theme class applied to this component and everything inside it.
        #[serde(default)]
        class: Option<String>,
        /// Filled at load: which live projection a `World` uses.
        #[serde(skip)]
        slot: Option<usize>,
    },
}

fn unit() -> ron::Value {
    ron::Value::Unit
}

/// A component node in code: `c("Series")`.
pub fn c(name: &str) -> Node {
    Node::C { name: name.into(), props: ron::Value::Unit, class: None, slot: None }
}

/// A component with props written as RON: `cp("Series", r#"(names: ["nest.food"])"#)`.
pub fn cp(name: &str, props: &str) -> Node {
    Node::C { name: name.into(), props: ron::from_str(props).unwrap_or(ron::Value::Unit), class: None, slot: None }
}

#[derive(Deserialize)]
struct WorldProps {
    projection: crate::projection::ProjectionSpec,
}

impl View {
    /// The view used when a game has no view.ron: a 3D world if it has levels, else a map, plus panels.
    pub fn default_for(depth: i64) -> View {
        let world = if depth > 1 { r#"(projection: (dim: "3D"))"# } else { r#"(projection: (dim: "2D", level: 0))"# };
        View {
            theme: dark(),
            assets: Vec::new(),
            components: BTreeMap::new(),
            layout: Node::Rows(vec![
                (Size::Fixed(1), c("Title")),
                (
                    Size::Fill,
                    Node::Cols(vec![
                        (Size::Percent(62), cp("World", world)),
                        (
                            Size::Fill,
                            Node::Rows(vec![
                                (Size::Fixed(5), c("Env")),
                                (Size::Fill, c("Inspector")),
                                (Size::Fixed(9), c("Counts")),
                            ]),
                        ),
                    ]),
                ),
                (Size::Fixed(2), c("Legend")),
                (Size::Fixed(6), c("Events")),
                (Size::Fixed(1), c("Help")),
            ]),
        }
    }

    /// Expands composites, checks every component name and props, and returns the live projections
    /// (one per `World`, in layout order).
    /// `depth`: the world's levels (projections are checked against it).
    pub fn resolve(&mut self, registry: &Registry, depth: i64) -> Result<Vec<Projection>, Vec<String>> {
        let mut errs = Vec::new();
        let mut worlds = Vec::new();
        let components = self.components.clone();
        #[allow(clippy::too_many_arguments)]
        fn walk(
            n: &mut Node,
            components: &BTreeMap<String, Node>,
            registry: &Registry,
            depth: i64,
            path: &mut Vec<String>,
            worlds: &mut Vec<Projection>,
            errs: &mut Vec<String>,
        ) {
            match n {
                Node::Rows(v) | Node::Cols(v) => {
                    v.iter_mut().for_each(|(_, child)| walk(child, components, registry, depth, path, worlds, errs))
                }
                Node::C { name, props, class, slot } => {
                    if let Some(body) = components.get(name.as_str()) {
                        if path.contains(name) {
                            errs.push(format!("component '{name}' contains itself ({} -> {name})", path.join(" -> ")));
                            return;
                        }
                        path.push(name.clone());
                        let mut inner = body.clone();
                        walk(&mut inner, components, registry, depth, path, worlds, errs);
                        path.pop();
                        // A composite used with a class: wrap by giving its root the class if it has none.
                        if let (Some(cl), Node::C { class: inner_class @ None, .. }) = (class.clone(), &mut inner) {
                            *inner_class = Some(cl);
                        }
                        *n = inner;
                        return;
                    }
                    if registry.get(name).is_none() {
                        let known: Vec<&str> = registry.names().chain(components.keys().map(String::as_str)).collect();
                        errs.push(format!("unknown component '{name}' (known: {})", known.join(", ")));
                        return;
                    }
                    if name == "World" {
                        match props.clone().into_rust::<WorldProps>().map_err(|e| e.to_string()).and_then(|p| p.projection.build(depth)) {
                            Ok(p) => {
                                *slot = Some(worlds.len());
                                worlds.push(p);
                            }
                            Err(e) => errs.push(format!("World props: {e} (e.g. `(projection: (dim: \"3D\", yaw: 30))`)")),
                        }
                    }
                }
            }
        }
        walk(&mut self.layout, &components, registry, depth, &mut Vec::new(), &mut worlds, &mut errs);
        if errs.is_empty() { Ok(worlds) } else { Err(errs) }
    }

    pub fn draw(&self, registry: &Registry, theme: &Theme, scene: &Scene, ui: &Ui, canvas: &mut Canvas) {
        let style = Style::new(theme);
        canvas.clear(style.color("bg"));
        let area = canvas.area();
        node(&self.layout, registry, theme, &style, scene, ui, canvas, area);
    }
}

#[allow(clippy::too_many_arguments)]
fn node(n: &Node, registry: &Registry, theme: &Theme, style: &Style, scene: &Scene, ui: &Ui, canvas: &mut Canvas, r: Rect) {
    match n {
        Node::Rows(v) | Node::Cols(v) => {
            let sizes: Vec<Size> = v.iter().map(|(s, _)| *s).collect();
            let areas = if matches!(n, Node::Rows(_)) { rows(r, &sizes) } else { cols(r, &sizes) };
            for ((_, child), area) in v.iter().zip(areas) {
                node(child, registry, theme, style, scene, ui, canvas, area);
            }
        }
        Node::C { name, props, class, slot } => {
            let style = style.with_class(theme, class.as_deref());
            let Some(comp) = registry.get(name) else { return };
            let mut ctx = Ctx { canvas, scene, style, ui };
            if let Err(e) = comp.draw(&mut ctx, props, *slot, r) {
                ctx.text(r, 0, 0, &format!("{name}: {e}"), "trend_down");
            }
        }
    }
}

/// The default camera, for code that builds views.
pub fn camera() -> Camera {
    Camera::default()
}
