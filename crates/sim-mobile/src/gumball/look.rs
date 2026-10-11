//! How GUMBALL looks: a glass jar on a red gumball-machine base, seen from the front, the gumballs in sugar
//! colours, the next one hanging in the dropper over the finger; and the HUD (score, best, the one after next, the
//! jar-full banner). Plain functions of the game's state: drawing never changes it.

use sim_physics::rigid::V3;

use super::{Button, Gumball, lin};
use crate::draw3d::{Camera, Inst3, Mesh, Scene3};
use crate::font;
use crate::gesture::Px;
use crate::layer::{Color, Frame, Layer, Rect, Shape as Shape2};

const INK: u32 = 0x24203a;
const BASE: u32 = 0xd8342c;
const GLASS: u32 = 0xcdeefc;
const CHROME: u32 = 0xdfe6ee;

/// Screen space the HUD takes at the top (points), kept clear of the jar.
const HUD_TOP: f32 = 130.0;

/// From the front, as far back as the jar and the dropper need to fit under the HUD.
pub fn camera(g: &Gumball) -> Camera {
    let c = &g.t.camera;
    let fov = c.fov.to_radians();
    let tan = (fov / 2.0).tan();
    let aspect = (g.screen.0 / g.screen.1).max(0.1);
    let hw = g.t.jar.width / 2.0 + g.t.jar.wall + c.margin;
    let (bottom, top) = (-0.7, g.drop_y() + g.radius(g.t.balls.drop_sizes - 1) + c.margin);
    let hh = (top - bottom) / 2.0;
    let hud = (HUD_TOP * g.scale / g.screen.1).clamp(0.0, 0.4);
    let dist = (hw / (tan * aspect)).max(hh / (tan * (1.0 - hud)));
    let look = V3::new(0.0, (top + bottom) / 2.0 + hud * dist * tan, 0.0);
    Camera { eye: look + V3::new(0.0, 0.25, dist), target: look, up: V3::Y, fov_y: fov, near: 0.5, far: dist * 4.0 }
}

fn ball(out: &mut Vec<Inst3>, at: V3, r: f32, colour: u32) {
    out.push(Inst3::new(Mesh::Sphere, at, V3::splat(r), lin(colour)));
    // A sugar shine towards the light.
    out.push(Inst3::new(Mesh::Sphere, at + V3::new(-0.32, 0.42, 0.78) * r, V3::splat(r * 0.22), [1.0, 1.0, 1.0]).glow(0.85).no_shadow());
}

/// The 3D picture at `alpha` between the last two ticks.
pub fn scene(g: &Gumball, alpha: f32) -> Scene3 {
    let mut out = Vec::with_capacity(160);
    let j = &g.t.jar;
    let (hw, wall) = (j.width / 2.0, j.wall);
    let back = -(g.radius(g.t.balls.sizes - 1) + 0.15);
    // The machine's red base and the jar's floor.
    out.push(Inst3::new(Mesh::Cube, V3::new(0.0, -0.4, -0.1), V3::new(hw + wall + 0.2, 0.4, 0.75), lin(BASE)).bevel(0.06));
    out.push(
        Inst3::new(Mesh::Cube, V3::new(0.0, -0.01, back * 0.5), V3::new(hw + wall, 0.01, -back * 0.5 + 0.1), lin(0xf3d9c4)).no_shadow(),
    );
    // The glass: the back pane lights itself (it is larger than the shadow map, which left lit patches on it), the
    // sides are thick panes, a chrome lid on top.
    out.push(
        Inst3::new(Mesh::Cube, V3::new(0.0, j.height / 2.0, back), V3::new(hw + wall, j.height / 2.0, 0.02), lin(0xefe8f7))
            .glow(1.0)
            .no_shadow(),
    );
    for side in [-1.0, 1.0] {
        let x = side * (hw + wall / 2.0);
        out.push(
            Inst3::new(
                Mesh::Cube,
                V3::new(x, j.height / 2.0, back * 0.5),
                V3::new(wall / 2.0, j.height / 2.0, -back * 0.5 + 0.1),
                lin(GLASS),
            )
            .glow(0.25)
            .bevel(0.03),
        );
    }
    out.push(
        Inst3::new(Mesh::Cube, V3::new(0.0, j.height + 0.06, back * 0.5), V3::new(hw + wall + 0.06, 0.06, -back * 0.5 + 0.14), lin(CHROME))
            .bevel(0.03),
    );
    // The rim line: red, and pulsing while a ball sits over it.
    let danger = g.balls.iter().map(|b| b.over).fold(0.0f32, f32::max) / j.over_time.max(0.01);
    let pulse = if danger > 0.0 { 0.6 + 0.4 * (g.tick as f32 / 60.0 * 12.0).sin() } else { 0.35 };
    out.push(Inst3::new(Mesh::Cube, V3::new(0.0, j.rim, back + 0.03), V3::new(hw, 0.012, 0.01), lin(0xff3b30)).glow(pulse).no_shadow());
    for b in &g.balls {
        if let Some(body) = g.world.get(b.id) {
            let (pos, _) = body.pose_at(alpha);
            ball(&mut out, pos, g.radius(b.size), g.t.balls.colours[b.size]);
        }
    }
    // The dropper: the next ball over the finger, and a guide down while aiming.
    if g.lost.is_none() {
        let x = g.clamp_x(g.hand_x);
        let y = g.drop_y();
        if g.reload <= 0.0 {
            ball(&mut out, V3::new(x, y, 0.0), g.radius(g.next), g.t.balls.colours[g.next]);
        }
        if g.aiming {
            let len = (y - 0.02) / 2.0;
            out.push(
                Inst3::new(Mesh::Cylinder, V3::new(x, len, back + 0.05), V3::new(0.01, len, 0.01), [1.0, 1.0, 1.0]).glow(0.6).no_shadow(),
            );
        }
    }
    Scene3 {
        camera: camera(g),
        sun: V3::new(-0.35, 0.75, 0.55).normalized(),
        sun_color: [1.2, 1.12, 1.0],
        sky_top: lin(0x5b3d8f),
        sky_horizon: lin(0xf6a6c1),
        ground: lin(0xffd2b0),
        fog: lin(0x2b2140),
        fog_near: 80.0,
        fog_far: 200.0,
        shadow_center: V3::new(0.0, j.height / 2.0, 0.0),
        shadow_radius: j.height * 0.75,
        flash: 0.0,
        instances: out,
    }
}

/// The HUD's buttons: start over, and the way on to the next game.
pub fn buttons(g: &Gumball) -> [Button; 2] {
    let sa = g.safe;
    let u = g.scale;
    let (w, h) = (74.0 * u, 34.0 * u);
    let m = 14.0 * u;
    [
        Button { rect: Rect::new(sa.x + m, sa.y + m, w, h), label: "RESET" },
        Button { rect: Rect::new(sa.x + sa.w - m - w, sa.y + m, w, h), label: "MAZE" },
    ]
}

/// The HUD: buttons, the score and the best, the ball after next, and the jar-full banner.
pub fn hud(g: &Gumball, _alpha: f32, frame: &mut Frame) {
    let u = g.scale;
    let px = 2.0 * u;
    let ink = Color::hex(INK);
    let white = Color::hex(0xffffff);
    for b in buttons(g) {
        frame.push(Layer::Hud, 0, Shape2::Box { rect: b.rect, r: b.rect.h / 2.0, color: Color::hexa(0xffffff, 0.9) });
        font::centered(frame, Layer::Hud, 1, b.rect.center(), px * 0.9, ink, b.label);
    }
    let sa = g.safe;
    let mid = sa.x + sa.w / 2.0;
    font::centered(frame, Layer::Hud, 1, Px::new(mid, sa.y + 32.0 * u), px * 1.6, white, &g.score.to_string());
    font::centered(frame, Layer::Hud, 1, Px::new(mid, sa.y + 62.0 * u), px * 0.7, Color::hexa(0xffffff, 0.75), &format!("BEST {}", g.best));
    // The ball after the one in the hand.
    let nx = sa.x + sa.w - 50.0 * u;
    let ny = sa.y + 88.0 * u;
    font::centered(frame, Layer::Hud, 1, Px::new(nx - 34.0 * u, ny), px * 0.65, white, "NEXT");
    let r = (8.0 + 2.5 * g.after as f32) * u;
    frame.push(Layer::Hud, 0, Shape2::circle(Px::new(nx, ny), r + 2.0 * u, Color::hexa(0xffffff, 0.85)));
    frame.push(Layer::Hud, 1, Shape2::circle(Px::new(nx, ny), r, Color::hex(g.t.balls.colours[g.after])));
    if g.drops == 0 && g.lost.is_none() {
        let y = sa.y + sa.h - 40.0 * u + (g.tick as f32 / 60.0 * 3.0).sin() * 5.0 * u;
        font::centered(frame, Layer::Hud, 1, Px::new(mid, y), px, white, "DRAG AND LET GO");
    }
    if let Some(lost) = g.lost {
        let t = (g.tick - lost) as f32 / 60.0;
        let pop = if t < 0.35 { 0.6 + (t / 0.35) * 0.55 } else { 1.0 + 0.15 * (-(t - 0.35) * 6.0).exp() };
        let c = Px::new(g.screen.0 / 2.0, g.screen.1 * 0.42);
        let sub = format!("SCORE {}  TAP TO PLAY", g.score);
        let w = font::width("JAR FULL", px * 2.0 * pop).max(font::width(&sub, px * 1.1)) + 44.0 * u;
        let rect = Rect::new(c.x - w / 2.0, c.y - 46.0 * u * pop, w, 92.0 * u * pop);
        frame.push(Layer::Overlay, 0, Shape2::Box { rect, r: 30.0 * u * pop, color: Color::hex(0xffd1e3) });
        font::centered(frame, Layer::Overlay, 1, Px::new(c.x, c.y - 14.0 * u * pop), px * 2.0 * pop, ink, "JAR FULL");
        font::centered(frame, Layer::Overlay, 1, Px::new(c.x, c.y + 24.0 * u * pop), px * 1.1, ink, &sub);
    }
}
