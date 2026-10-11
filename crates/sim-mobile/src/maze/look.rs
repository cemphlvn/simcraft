//! How MAZE looks: a maple board in a walnut box, walnut walls, black holes, a green goal and a steel marble, seen
//! from above by a camera that turns with the board's tilt; and the HUD over it. Plain functions of the game's
//! state: drawing never changes it.

use sim_physics::rigid::{Shape, V3};

use super::{Button, Kind, Maze, lin};
use crate::draw3d::{Camera, Inst3, Mesh, Scene3};
use crate::font;
use crate::gesture::Px;
use crate::layer::{Color, Frame, Layer, Rect, Shape as Shape2};

const MAPLE: u32 = 0xe9c48c;
const WALNUT: u32 = 0x7a4a2a;
const BOX: u32 = 0x5a341c;
const STEEL: u32 = 0xc9d2dc;
const GOAL: u32 = 0x35d07f;
const INK: u32 = 0x1d2a4a;

/// Screen space the HUD takes at the top (points), kept clear of the board.
const HUD_TOP: f32 = 120.0;

/// The camera: straight above the board (leaning back a little), as far away as the whole board needs to fit the
/// screen under the HUD, turned with the board's tilt so on screen the board tips.
pub fn camera(m: &Maze) -> Camera {
    let c = &m.t.camera;
    let fov = c.fov.to_radians();
    let aspect = (m.screen.0 / m.screen.1).max(0.1);
    let (hw, hd) = m.half_extent();
    let (hw, hd) = (hw + c.margin, hd + c.margin);
    let tan = (fov / 2.0).tan();
    // The board fills the screen below the HUD: the part of the height that is left, and the look point moved up
    // the board so the board's centre sits in the middle of what is left.
    let hud = (HUD_TOP * m.scale / m.screen.1).clamp(0.0, 0.4);
    let usable = 1.0 - hud;
    let dist = (hw / (tan * aspect)).max(hd / (tan * usable));
    let shift = hud * dist * tan;
    let look = V3::new(0.0, 0.0, -shift);
    let lean = c.lean.to_radians();
    let eye = look + V3::new(0.0, lean.cos(), lean.sin()) * dist;
    let up = V3::new(0.0, lean.sin(), -lean.cos());
    // Turning the camera back by the tilt shows the board turned by it.
    let k = c.tilt_shown;
    let q = Maze::board_rot(m.tilt.0 * k, m.tilt.1 * k);
    Camera { eye: look + q.unrotate(eye - look), target: look, up: q.unrotate(up), fov_y: fov, near: 0.5, far: dist * 4.0 }
}

/// The 3D picture at `alpha` between the last two ticks.
pub fn scene(m: &Maze, alpha: f32) -> Scene3 {
    let mut out = Vec::with_capacity(200);
    let (hw, hd) = m.half_extent();
    let b = &m.t.board;
    // The box the board sits in, and the board.
    out.push(Inst3::new(Mesh::Cube, V3::new(0.0, -0.45, 0.0), V3::new(hw + 0.18, 0.25, hd + 0.18), lin(BOX)).bevel(0.04));
    out.push(Inst3::new(Mesh::Cube, V3::new(0.0, -0.1, 0.0), V3::new(hw, 0.1, hd), lin(MAPLE)).no_shadow());
    for (_, body) in m.world.iter() {
        if body.user != Kind::Wall as u32 {
            continue;
        }
        if let Shape::Box { half } = body.shape {
            out.push(Inst3::new(Mesh::Cube, body.pos, half, lin(WALNUT)).bevel(0.035));
        }
    }
    for h in &m.holes {
        let r = b.hole_radius;
        let rim = if h.goal { lin(GOAL) } else { lin(0x9a6a3c) };
        let glow = if h.goal { 0.55 + 0.25 * (m.age as f32 / 60.0 * 4.0).sin() } else { 0.0 };
        out.push(
            Inst3::new(Mesh::Cylinder, h.at + V3::new(0.0, 0.002, 0.0), V3::new(r * 1.22, 0.002, r * 1.22), rim).glow(glow).no_shadow(),
        );
        out.push(Inst3::new(Mesh::Cylinder, h.at + V3::new(0.0, 0.004, 0.0), V3::new(r, 0.003, r), lin(0x120d0a)).no_shadow());
    }
    // Where the marble starts: a ring burnt into the wood.
    out.push(Inst3::new(Mesh::Cylinder, m.start + V3::new(0.0, 0.002, 0.0), V3::new(0.19, 0.002, 0.19), lin(0xc99a5c)).no_shadow());
    if let Some(body) = m.marble.and_then(|id| m.world.get(id)) {
        let (pos, rot) = body.pose_at(alpha);
        let r = m.t.marble.radius;
        out.push(Inst3::new(Mesh::Sphere, pos, V3::splat(r), lin(STEEL)).rot(rot));
        // A highlight that stays towards the light while the marble rolls: it reads as polished steel.
        out.push(Inst3::new(Mesh::Sphere, pos + V3::new(-0.35, 0.6, -0.2) * r, V3::splat(r * 0.28), [1.0, 1.0, 1.0]).glow(0.9).no_shadow());
    }
    let sun = V3::new(-0.45, 0.85, 0.3).normalized();
    let radius = (hw * hw + hd * hd).sqrt();
    Scene3 {
        camera: camera(m),
        sun,
        sun_color: [1.2, 1.1, 0.95],
        sky_top: lin(0x2d3a5a),
        sky_horizon: lin(0x51607e),
        ground: lin(0x6b4a30),
        fog: lin(0x2d3a5a),
        fog_near: 60.0,
        fog_far: 160.0,
        shadow_center: V3::ZERO,
        shadow_radius: radius + 0.3,
        flash: 0.0,
        instances: out,
    }
}

/// The HUD's buttons: reset and the way to the next game at the top corners, the level switch either side of the
/// level's name.
pub fn buttons(m: &Maze) -> [Button; 4] {
    let sa = m.safe;
    let u = m.scale;
    let (w, h) = (74.0 * u, 34.0 * u);
    let mm = 14.0 * u;
    let row = level_row(m);
    let k = row.h;
    [
        Button { rect: Rect::new(sa.x + mm, sa.y + mm, w, h), label: "RESET" },
        Button { rect: Rect::new(sa.x + sa.w - mm - w, sa.y + mm, w, h), label: "SMASH" },
        Button { rect: Rect::new(row.x - k - 8.0 * u, row.y, k, k), label: "-" },
        Button { rect: Rect::new(row.x + row.w + 8.0 * u, row.y, k, k), label: "+" },
    ]
}

fn level_row(m: &Maze) -> Rect {
    let sa = m.safe;
    let u = m.scale;
    let w = sa.w * 0.5;
    Rect::new(sa.x + (sa.w - w) / 2.0, sa.y + 56.0 * u, w, 30.0 * u)
}

/// A banner over the middle of the screen that pops in.
fn banner(m: &Maze, since: u64, text: &str, sub: &str, colour: u32, frame: &mut Frame) {
    let u = m.scale;
    let px = 2.0 * u;
    let t = since as f32 / 60.0;
    let pop = if t < 0.35 { 0.6 + (t / 0.35) * 0.55 } else { 1.0 + 0.15 * (-(t - 0.35) * 6.0).exp() };
    let c = Px::new(m.screen.0 / 2.0, m.screen.1 * 0.42);
    let tp = px * 2.0 * pop;
    let w = font::width(text, tp).max(font::width(sub, px * 1.2)) + 44.0 * u;
    let rect = Rect::new(c.x - w / 2.0, c.y - 46.0 * u * pop, w, 92.0 * u * pop);
    frame.push(Layer::Overlay, 0, Shape2::Box { rect, r: 30.0 * u * pop, color: Color::hex(colour) });
    let ink = Color::hex(INK);
    font::centered(frame, Layer::Overlay, 1, Px::new(c.x, c.y - 14.0 * u * pop), tp, ink, text);
    font::centered(frame, Layer::Overlay, 1, Px::new(c.x, c.y + 24.0 * u * pop), px * 1.2, ink, sub);
}

/// The HUD: buttons, the level, the clock and the falls, a hint before the marble first moves, the clear banner.
pub fn hud(m: &Maze, _alpha: f32, frame: &mut Frame) {
    let u = m.scale;
    let px = 2.0 * u;
    let ink = Color::hex(INK);
    let white = Color::hex(0xffffff);
    for b in buttons(m) {
        frame.push(Layer::Hud, 0, Shape2::Box { rect: b.rect, r: b.rect.h / 2.0, color: Color::hexa(0xffffff, 0.88) });
        font::centered(frame, Layer::Hud, 1, b.rect.center(), px * 0.9, ink, b.label);
    }
    let sa = m.safe;
    // The clock between the corner buttons.
    let clock = Rect::new(sa.x + sa.w / 2.0 - 46.0 * u, sa.y + 14.0 * u, 92.0 * u, 34.0 * u);
    frame.push(Layer::Hud, 0, Shape2::Box { rect: clock, r: clock.h / 2.0, color: Color::hexa(INK, 0.55) });
    font::centered(frame, Layer::Hud, 1, clock.center(), px * 0.9, white, &format!("{:.1}", m.seconds()));
    let row = level_row(m);
    frame.push(Layer::Hud, 0, Shape2::Box { rect: row, r: row.h / 2.0, color: Color::hexa(INK, 0.45) });
    font::centered(frame, Layer::Hud, 1, row.center(), px * 0.8, white, &format!("{} {}", m.level + 1, m.level().name));
    if m.falls > 0 {
        let at = Px::new(sa.x + sa.w / 2.0, row.y + row.h + 16.0 * u);
        let text = if m.falls == 1 { "1 FALL".to_string() } else { format!("{} FALLS", m.falls) };
        font::centered(frame, Layer::Hud, 1, at, px * 0.75, Color::hex(0xffb4a8), &text);
    }
    let still = m.marble_vel().length() < 0.05 && m.tick < 600;
    if m.level == 0 && m.won.is_none() && still && m.falls == 0 {
        let hint = if m.sensor { "TILT TO ROLL" } else { "DRAG TO TILT" };
        let y = sa.y + sa.h - 46.0 * u + (m.age as f32 / 60.0 * 3.0).sin() * 5.0 * u;
        font::centered(frame, Layer::Hud, 1, Px::new(sa.x + sa.w / 2.0, y), px, white, hint);
        if m.sensor {
            font::centered(frame, Layer::Hud, 1, Px::new(sa.x + sa.w / 2.0, y + 24.0 * u), px * 0.7, white, "TAP TO LEVEL");
        }
    }
    if let Some(won) = m.won {
        let since = m.age - m.won_age;
        banner(m, since, "LEVEL CLEAR", &format!("{:.1} S  TAP FOR NEXT", won as f32 / 60.0), 0x9df2c0, frame);
        let t = since as f32 / 60.0;
        let stars = m.stars();
        for i in 0..3u32 {
            let shown = t > 0.35 + i as f32 * 0.18;
            let c = Px::new(m.screen.0 / 2.0 + (i as f32 - 1.0) * 46.0 * u, m.screen.1 * 0.42 + 80.0 * u);
            let on = i < stars && shown;
            let size = if on { 15.0 * u } else { 11.0 * u };
            frame.push(Layer::Overlay, 2, Shape2::circle(c, size + 3.0 * u, Color::hex(INK)));
            frame.push(Layer::Overlay, 3, Shape2::circle(c, size, if on { Color::hex(0xfff3a0) } else { Color::hexa(0xffffff, 0.35) }));
        }
    }
}
