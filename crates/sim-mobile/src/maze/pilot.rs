//! The autopilot: plays a level the way a steady hand would, so the evals can say every level can be won and how
//! it feels to win it (how long it takes, how often the marble knocks a wall). It plans the shortest way through
//! the cells (never over a hole), keeps only the corners, and tilts the board to roll towards the next corner at a
//! speed it can stop from by the time it gets there.

use std::collections::VecDeque;

use sim_physics::rigid::V3;

use super::Maze;
use super::tuning::{Cell, Level};

/// The shortest way from the start to a goal through floor cells (four-way), or none.
pub fn route(level: &Level) -> Option<Vec<(usize, usize)>> {
    let (w, h) = level.size();
    let start = *level.find(Cell::Start).first()?;
    let mut came: Vec<Option<(usize, usize)>> = vec![None; w * h];
    let mut seen = vec![false; w * h];
    let mut queue = VecDeque::from([start]);
    seen[start.1 * w + start.0] = true;
    while let Some((c, r)) = queue.pop_front() {
        if level.at(c, r) == Cell::Goal {
            let mut path = vec![(c, r)];
            let mut at = (c, r);
            while let Some(prev) = came[at.1 * w + at.0] {
                path.push(prev);
                at = prev;
            }
            path.reverse();
            return Some(path);
        }
        let next = [(c + 1, r), (c.wrapping_sub(1), r), (c, r + 1), (c, r.wrapping_sub(1))];
        for (nc, nr) in next {
            if nc >= w || nr >= h || seen[nr * w + nc] || matches!(level.at(nc, nr), Cell::Wall | Cell::Hole) {
                continue;
            }
            seen[nr * w + nc] = true;
            came[nr * w + nc] = Some((c, r));
            queue.push_back((nc, nr));
        }
    }
    None
}

/// Only the cells where the way turns, and the goal.
pub fn corners(path: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for i in 1..path.len() {
        let last = i + 1 == path.len();
        let turns = !last && {
            let (a, b, c) = (path[i - 1], path[i], path[i + 1]);
            (b.0 as i64 - a.0 as i64, b.1 as i64 - a.1 as i64) != (c.0 as i64 - b.0 as i64, c.1 as i64 - b.1 as i64)
        };
        if turns || last {
            out.push(path[i]);
        }
    }
    out
}

pub struct Pilot {
    corners: Vec<V3>,
    next: usize,
    /// Top speed on a straight (m/s), and how hard it brakes for a corner (m/s²).
    pub speed: f32,
    pub brake: f32,
}

impl Pilot {
    pub fn new(m: &Maze) -> Option<Pilot> {
        let path = route(m.level())?;
        let corners = corners(&path).into_iter().map(|(c, r)| m.cell_at(c, r)).collect();
        Some(Pilot { corners, next: 0, speed: 1.2, brake: 1.0 })
    }

    /// The tilt for this tick (and starts over from the first corner after a fall).
    pub fn tilt(&mut self, m: &Maze) -> (f32, f32) {
        if m.marble.is_none() || m.dropping.is_some() {
            self.next = 0;
            return (0.0, 0.0);
        }
        let (p, v) = (m.marble_pos(), m.marble_vel());
        let flat = |a: V3| V3::new(a.x, 0.0, a.z);
        while self.next + 1 < self.corners.len() && flat(self.corners[self.next] - p).length() < 0.08 {
            self.next += 1;
        }
        let to = flat(self.corners[self.next] - p);
        let d = to.length();
        // Fast enough to stop exactly at the corner, never over the top speed.
        let want = if d > 1e-4 { to * ((2.0 * self.brake * d).sqrt().min(self.speed) / d) } else { V3::ZERO };
        let acc = (want - flat(v)) * 5.0;
        // A rolling ball speeds up at 5/7 of the slope's pull.
        let g = m.t.physics.gravity * 5.0 / 7.0;
        (acc.x / g, -acc.z / g)
    }
}
