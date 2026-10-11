//! The autopilot: a player that looks before it drops, so the evals can say how the game plays out over a whole
//! jar (how long it lasts, how big the balls get, how hard the solver works). For each of a row of places across
//! the jar it works out where the ball in the hand would come to rest on the pile, and picks the lowest place,
//! preferring one where it lands on a ball of its own size (a merge).

use super::Gumball;

/// Where to drop the ball in the hand (m across the jar).
pub fn choose(g: &Gumball) -> f32 {
    let r = g.radius(g.next);
    let room = (g.t.jar.width / 2.0 - r - 0.005).max(0.0);
    let mut best = (f32::NEG_INFINITY, 0.0);
    for k in 0..=16 {
        let x = -room + 2.0 * room * k as f32 / 16.0;
        // The highest ball it would land on, and how high its centre would come to rest.
        let mut rest = r;
        let mut on = None;
        for b in &g.balls {
            let p = g.pos(b);
            let reach = g.radius(b.size) + r;
            let dx = (p.x - x).abs();
            if dx < reach {
                let y = p.y + (reach * reach - dx * dx).sqrt();
                if y > rest {
                    rest = y;
                    on = Some(b.size);
                }
            }
        }
        let mut score = -rest;
        if on == Some(g.next) {
            score += 1.5 + 0.3 * g.next as f32;
        }
        if rest + r > g.t.jar.rim - 0.1 {
            score -= 10.0;
        }
        if score > best.0 {
            best = (score, x);
        }
    }
    best.1
}
