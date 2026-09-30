//! Vehicles in the world (docs/architecture.md, Vehicles): kinds whose motion is a car's, stepped by the physics
//! layer (`sim-physics`) every tick after the rules. Rules and players write intent (throttle, brake, steer, or
//! hand the car to the engine's autopilot); the physics writes the car's state back as props. The full-precision
//! physics state lives in hidden props (`_x`, `_vx`, ...), so the world is still just entities and props: hashes,
//! snapshots and replays cover it without anything new.
//!
//! Units: vehicle worlds use 1 m cells, so fine units are millimetres. A track (the surface cars drive on) is laid
//! out from `origin` (mm) in the world.

use crate::world::{FINE, World};
use serde::{Deserialize, Serialize};
use sim_physics::{Angle, Fleet, Fx, Params, Pilot, Plan, Track, TrackDef, VehicleDef, sit_on};
use std::collections::BTreeMap;

/// What the engine writes on a vehicle (readable by rules), then its hidden state (engine only).
pub const VEHICLE_PROPS: [&str; 17] = [
    "yaw",
    "speed",
    "g_long",
    "g_lat",
    "track_s",
    "track_off",
    "_on",
    "_x",
    "_y",
    "_yaw",
    "_vx",
    "_vy",
    "_r",
    "_ax",
    "_lat",
    "_seg",
    "_slip",
];

/// Intent a vehicle reads (a rule, an action or the autopilot writes it): throttle and brake 0..1000, steer
/// -1000 (right)..1000 (left); `pilot` 1 hands the car to the autopilot, driving `line` mm left of the centreline
/// at `pace` ‰ of the plan (1000 by default); `grid` places the car on the starting grid (slot 1, 2, ...).
pub const VEHICLE_INPUTS: [&str; 7] = ["throttle", "brake", "steer", "pilot", "line", "pace", "grid"];

/// The starting grid, behind the start line: rows `spacing` apart, `columns` side by side `gap` apart (mm).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grid {
    #[serde(default = "spacing")]
    pub spacing: i64,
    #[serde(default = "columns")]
    pub columns: i64,
    #[serde(default = "gap")]
    pub gap: i64,
}

fn spacing() -> i64 {
    9000
}

fn columns() -> i64 {
    2
}

fn gap() -> i64 {
    5000
}

impl Default for Grid {
    fn default() -> Grid {
        Grid { spacing: spacing(), columns: columns(), gap: gap() }
    }
}

/// The ground vehicles drive on: a track laid from `origin` (mm), and its starting grid.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Surface {
    pub track: TrackDef,
    pub origin: (i64, i64),
    #[serde(default)]
    pub grid: Grid,
}

/// Derived from the vehicle and surface data (not hashed, rebuilt after a restore).
#[derive(Clone, Debug, Default)]
pub(crate) struct Cache {
    params: BTreeMap<String, Params>,
    track: Option<Track>,
    /// (kind, line mm) → the plan the autopilot follows.
    plans: BTreeMap<(String, i64), Plan>,
}

/// Metres (physics) from millimetres (world), and back.
fn metres(mm: i64) -> Fx {
    Fx::ratio(mm, 1000)
}

fn mm(m: Fx) -> i64 {
    m.to_units(1000)
}

impl World {
    /// Which kinds are vehicles (and their cars), the surface they drive on, and the game's tick rate.
    pub fn set_vehicles(&mut self, kinds: BTreeMap<String, VehicleDef>, surface: Option<Surface>, tick_rate: i64) {
        self.vehicles = kinds;
        self.surface = surface;
        self.tick_rate = tick_rate.max(1);
        self.drive_cache = None;
    }

    /// The car a vehicle kind drives, if it is one.
    pub fn vehicle(&self, kind: &str) -> Option<&VehicleDef> {
        self.vehicles.get(kind)
    }

    pub fn surface(&self) -> Option<&Surface> {
        self.surface.as_ref()
    }

    fn cache(&mut self) -> Cache {
        if let Some(c) = self.drive_cache.take() {
            return *c;
        }
        Cache {
            params: self.vehicles.iter().map(|(k, d)| (k.clone(), Params::new(d))).collect(),
            track: self.surface.as_ref().and_then(|s| Track::new(&s.track).ok()),
            plans: BTreeMap::new(),
        }
    }

    /// One tick of every vehicle: read intent and state from props, step the physics, write state back.
    pub(crate) fn drive_vehicles(&mut self) {
        if self.vehicles.is_empty() {
            return;
        }
        let mut cache = self.cache();
        let origin = self.surface.as_ref().map_or((0, 0), |s| s.origin);
        let grid = self.surface.as_ref().map(|s| s.grid.clone()).unwrap_or_default();
        let ids: Vec<_> = self.vehicles.keys().filter_map(|k| self.by_kind.get(k)).flatten().copied().collect();
        let mut fleet = Fleet::default();
        let mut pilots = Vec::new();
        for &id in &ids {
            let e = &self.entities[&id];
            let get = |k: &str| e.props.get(k).copied().unwrap_or(0);
            let p = cache.params[&e.kind].clone();
            let i = if get("_on") == 0 {
                // First tick: from the grid slot, or from where it was spawned; facing along the track under it.
                let (mut x, mut y) = (metres(get("px") - origin.0), metres(get("py") - origin.1));
                let mut yaw = Angle(get("yaw") << 16);
                if let Some(track) = &cache.track {
                    let slot = get("grid");
                    let pose = if slot > 0 {
                        let (row, col) = ((slot - 1) / grid.columns.max(1), (slot - 1) % grid.columns.max(1));
                        let across = (col * 2 - (grid.columns - 1)) * grid.gap / 2;
                        track.pose(-metres((row + 1) * grid.spacing), -metres(across))
                    } else {
                        let at = track.locate(x, y, None);
                        track.pose(at.s, at.offset)
                    };
                    (x, y, yaw) = (pose.x, pose.y, pose.heading);
                }
                fleet.add(p, x, y, yaw)
            } else {
                let i = fleet.add(p, Fx(get("_x")), Fx(get("_y")), Angle(get("_yaw")));
                (fleet.vx[i], fleet.vy[i], fleet.yaw_rate[i]) = (Fx(get("_vx")), Fx(get("_vy")), Fx(get("_r")));
                (fleet.accel_long[i], fleet.tyre_lat[i]) = (Fx(get("_ax")), Fx(get("_lat")));
                i
            };
            fleet.throttle[i] = Fx::ratio(get("throttle").clamp(0, 1000), 1000);
            fleet.brake[i] = Fx::ratio(get("brake").clamp(0, 1000), 1000);
            fleet.steer[i] = Fx::ratio(get("steer").clamp(-1000, 1000), 1000);
            let hint = usize::try_from(get("_seg") - 1).ok();
            if let Some(track) = &cache.track {
                let place = sit_on(&mut fleet, i, track, hint);
                if get("pilot") > 0 {
                    let (line, pace) = (get("line"), if e.props.contains_key("pace") { get("pace") } else { 1000 });
                    pilots.push((i, e.kind.clone(), line, pace, place));
                }
            }
        }
        if let Some(track) = &cache.track {
            for (i, kind, line, pace, place) in pilots {
                let plan = cache.plans.entry((kind, line)).or_insert_with_key(|(k, l)| Plan::new(track, &cache.params[k], metres(*l)));
                let pilot = Pilot { pace: Fx::ratio(pace.clamp(0, 2000), 1000), ..Pilot::default() };
                pilot.drive(&mut fleet, i, track, plan, place);
            }
        }
        fleet.step(self.tick_rate);
        let (w, h) = (self.width * FINE - 1, self.height * FINE - 1);
        for (i, &id) in ids.iter().enumerate() {
            let place = cache.track.as_ref().map(|t| t.locate(fleet.x[i], fleet.y[i], None));
            let e = self.entities.get_mut(&id).expect("vehicle");
            let p = &mut e.props;
            let old = (p.get("px").copied().unwrap_or(0), p.get("py").copied().unwrap_or(0));
            let (px, py) = ((origin.0 + mm(fleet.x[i])).clamp(0, w), (origin.1 + mm(fleet.y[i])).clamp(0, h));
            let outputs = [
                ("px", px),
                ("py", py),
                ("vx", px - old.0),
                ("vy", py - old.1),
                ("yaw", (fleet.yaw[i].0 >> 16) & 0xffff),
                ("speed", mm(fleet.vx[i])),
                ("g_long", mm(fleet.accel_long[i])),
                ("g_lat", mm(fleet.accel_lat[i])),
                ("track_s", place.map_or(0, |pl| mm(pl.s))),
                ("track_off", place.map_or(0, |pl| mm(pl.offset))),
                ("throttle", mm(fleet.throttle[i])),
                ("brake", mm(fleet.brake[i])),
                ("steer", mm(fleet.steer[i])),
                ("_on", 1),
                ("_x", fleet.x[i].0),
                ("_y", fleet.y[i].0),
                ("_yaw", fleet.yaw[i].0),
                ("_vx", fleet.vx[i].0),
                ("_vy", fleet.vy[i].0),
                ("_r", fleet.yaw_rate[i].0),
                ("_ax", fleet.accel_long[i].0),
                ("_lat", fleet.tyre_lat[i].0),
                ("_seg", place.map_or(0, |pl| pl.seg as i64 + 1)),
                ("_slip", mm(fleet.slip_rear[i].abs().max(fleet.slip_front[i].abs()))),
            ];
            for (k, v) in outputs {
                p.insert(k.into(), v);
            }
            let (cx, cy, z) = (px.div_euclid(FINE), py.div_euclid(FINE), e.z);
            if (cx, cy) != (e.x, e.y) {
                self.relocate(id, cx, cy, z, false);
            }
        }
        self.drive_cache = Some(Box::new(cache));
    }
}
