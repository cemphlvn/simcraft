use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

pub type EntityId = u64;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    pub id: EntityId,
    pub kind: String,
    pub state: String,
    pub x: i64,
    pub y: i64,
    /// Level: 0 is the top; 2D worlds have only level 0.
    #[serde(default)]
    pub z: i64,
    pub props: BTreeMap<String, i64>,
    /// A learning agent's own weights (int8, one byte each): opaque to the core, inherited by its young.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub genome: Vec<i8>,
}

fn one() -> i64 {
    1
}

/// The whole world, in portable form. Grid and kind index are derived; not carried.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldSnapshot {
    pub seed: u64,
    pub tick: u64,
    pub width: i64,
    pub height: i64,
    #[serde(default = "one")]
    pub depth: i64,
    pub next_id: EntityId,
    pub solid: BTreeSet<String>,
    pub entities: Vec<Entity>,
    #[serde(default)]
    pub fields: BTreeMap<String, Vec<i64>>,
    #[serde(default)]
    pub terrain: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub cling: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub motion: BTreeMap<String, Motion>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub vehicles: BTreeMap<String, sim_physics::VehicleDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<crate::drive::Surface>,
    #[serde(default = "sixty")]
    pub tick_rate: i64,
}

fn sixty() -> i64 {
    60
}

/// Fine units in a cell: continuous positions are integers too (docs/architecture.md, Continuous motion).
pub const FINE: i64 = 1000;

/// The props the engine owns on a moving kind: centre, velocity, height and its speed, the entity it rides.
pub const MOTION_PROPS: [&str; 7] = ["px", "py", "vx", "vy", "ph", "vh", "mount"];

/// Returned by the footprint queries when nothing is there.
pub const FAR: i64 = 999_999;

/// How a kind moves continuously: its footprint (across, along; fine units) and how fast its height falls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Motion {
    pub size: (i64, i64),
    /// Fine units per tick² taken from the upward speed while in the air.
    #[serde(default)]
    pub gravity: i64,
}

/// All game state. BTreeMap: iteration order is fixed by id → determinism.
/// Mutation only through methods: the grid index always stays in sync with entities.
#[derive(Clone, Debug)]
pub struct World {
    pub seed: u64,
    pub tick: u64,
    pub width: i64,
    pub height: i64,
    /// Levels (z). 1 for 2D worlds.
    pub depth: i64,
    pub(crate) entities: BTreeMap<EntityId, Entity>,
    /// Entity ids per cell (ascending). Derived data; not part of the hash.
    grid: Vec<Vec<EntityId>>,
    /// Entity ids per kind. Makes lookups cheap for sparse kinds (8 wolves, 2000 sheep).
    pub(crate) by_kind: BTreeMap<String, BTreeSet<EntityId>>,
    /// A cell cannot hold a second solid (wall, tree, hero).
    solid: BTreeSet<String>,
    next_id: EntityId,
    /// Numbers per voxel (temperature, soil...), in voxel order. Part of the world and its hash.
    fields: BTreeMap<String, Vec<i64>>,
    /// A voxel whose value in this field is non-zero is solid for every mover (soil, rock).
    terrain: Option<String>,
    /// Kinds that crawl: they only enter voxels touching terrain (walls, floors, ceilings), so they never float.
    cling: BTreeSet<String>,
    /// Kinds that move continuously (fine positions, velocities), integrated every tick.
    motion: BTreeMap<String, Motion>,
    /// The broadphase for moving kinds (derived, not hashed): per kind and column, (py, id) sorted along the road.
    /// Built after a tick's motion; any change to the world drops it, and queries scan until it is built again.
    index: Option<MotionIndex>,
    /// Vehicle kinds and the car each drives (`drive.rs`), the surface they drive on, and ticks a second.
    pub(crate) vehicles: BTreeMap<String, sim_physics::VehicleDef>,
    pub(crate) surface: Option<crate::drive::Surface>,
    pub(crate) tick_rate: i64,
    /// Derived from the vehicle data (physics parameters, the built track, autopilot plans): not hashed.
    pub(crate) drive_cache: Option<Box<crate::drive::Cache>>,
}

/// Sweep and prune along one axis: for each moving kind, its entities by column (the cell their centre is in, across)
/// sorted by position along the road. Footprint queries look at a few columns and walk outward from a binary search,
/// instead of every entity of the kind.
/// One mover in a column of the index: (py, id, px).
type Slot = (i64, EntityId, i64);

#[derive(Clone, Debug, Default)]
struct MotionIndex {
    /// kind → column → (py, id, px), sorted: positions are kept here too, so a query never looks them up by name.
    cols: BTreeMap<String, BTreeMap<i64, Vec<Slot>>>,
}

impl World {
    pub fn new(seed: u64, width: i64, height: i64) -> Self {
        Self::new3(seed, width, height, 1)
    }

    pub fn new3(seed: u64, width: i64, height: i64, depth: i64) -> Self {
        let cells = (width.max(1) * height.max(1) * depth.max(1)) as usize;
        Self {
            seed,
            tick: 0,
            width,
            height,
            depth: depth.max(1),
            entities: BTreeMap::new(),
            grid: vec![Vec::new(); cells],
            by_kind: BTreeMap::new(),
            solid: BTreeSet::new(),
            next_id: 1,
            fields: BTreeMap::new(),
            terrain: None,
            cling: BTreeSet::new(),
            motion: BTreeMap::new(),
            index: None,
            vehicles: BTreeMap::new(),
            surface: None,
            tick_rate: 60,
            drive_cache: None,
        }
    }

    pub fn snapshot(&self) -> WorldSnapshot {
        WorldSnapshot {
            seed: self.seed,
            tick: self.tick,
            width: self.width,
            height: self.height,
            depth: self.depth,
            next_id: self.next_id,
            solid: self.solid.clone(),
            entities: self.entities.values().cloned().collect(),
            fields: self.fields.clone(),
            terrain: self.terrain.clone(),
            cling: self.cling.clone(),
            motion: self.motion.clone(),
            vehicles: self.vehicles.clone(),
            surface: self.surface.clone(),
            tick_rate: self.tick_rate,
        }
    }

    /// Builds the world from a snapshot; derived indexes are recomputed.
    /// An inconsistent snapshot (out of bounds, duplicate id, two solids in one cell) is rejected.
    pub fn from_snapshot(s: WorldSnapshot) -> Result<World, String> {
        if s.width < 1 || s.height < 1 || s.depth < 1 {
            return Err(format!("world size {}x{}x{}", s.width, s.height, s.depth));
        }
        let mut w = World::new3(s.seed, s.width, s.height, s.depth);
        w.tick = s.tick;
        w.solid = s.solid;
        let cells = w.grid.len();
        if let Some((name, _)) = s.fields.iter().find(|(_, v)| v.len() != cells) {
            return Err(format!("field '{name}' does not have one value per voxel"));
        }
        w.fields = s.fields;
        w.terrain = s.terrain;
        w.cling = s.cling;
        w.motion = s.motion;
        w.vehicles = s.vehicles;
        w.surface = s.surface;
        w.tick_rate = s.tick_rate.max(1);
        for e in s.entities {
            if !w.in_bounds3(e.x, e.y, e.z) {
                return Err(format!("entity {} at ({}, {}, {}) is outside the world", e.id, e.x, e.y, e.z));
            }
            if e.id >= s.next_id || w.entities.contains_key(&e.id) {
                return Err(format!("entity id {} is repeated or not below next_id {}", e.id, s.next_id));
            }
            if w.solid.contains(&e.kind) && w.blocked3(e.x, e.y, e.z) {
                return Err(format!("two solids at ({}, {}, {})", e.x, e.y, e.z));
            }
            let c = w.cell(e.x, e.y, e.z);
            let pos = w.grid[c].partition_point(|&i| i < e.id);
            w.grid[c].insert(pos, e.id);
            w.by_kind.entry(e.kind.clone()).or_default().insert(e.id);
            w.entities.insert(e.id, e);
        }
        w.next_id = s.next_id;
        Ok(w)
    }

    pub fn set_solid(&mut self, kinds: BTreeSet<String>) {
        self.solid = kinds;
    }

    pub fn is_solid(&self, kind: &str) -> bool {
        self.solid.contains(kind)
    }

    pub fn set_cling(&mut self, kinds: BTreeSet<String>) {
        self.cling = kinds;
    }

    pub fn clings(&self, kind: &str) -> bool {
        self.cling.contains(kind)
    }

    pub fn set_motion(&mut self, kinds: BTreeMap<String, Motion>) {
        self.motion = kinds;
    }

    /// How a kind moves continuously, if it does.
    pub fn motion(&self, kind: &str) -> Option<Motion> {
        self.motion.get(kind).copied()
    }

    /// Does a face of this voxel touch terrain (a floor below, a wall beside, a ceiling above)?
    pub fn touches_terrain(&self, x: i64, y: i64, z: i64) -> bool {
        [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)]
            .into_iter()
            .any(|(dx, dy, dz)| self.is_terrain(x + dx, y + dy, z + dz))
    }

    /// Could an entity of `kind` stand in this voxel? Inside the world, not terrain, not taken (solid kinds), and
    /// for a clinging kind, against terrain.
    pub fn can_enter(&self, kind: &str, x: i64, y: i64, z: i64) -> bool {
        self.in_bounds3(x, y, z)
            && !self.is_terrain(x, y, z)
            && !(self.solid.contains(kind) && self.blocked3(x, y, z))
            && (!self.cling.contains(kind) || self.touches_terrain(x, y, z))
    }

    /// Crawlers whose support is gone (dug away under them) fall straight down until they touch terrain (or reach
    /// the bottom). In id order. Returns who fell.
    pub fn settle_clingers(&mut self) -> Vec<EntityId> {
        let ids: Vec<EntityId> =
            self.cling.iter().filter_map(|k| self.by_kind.get(k)).flatten().copied().collect::<BTreeSet<_>>().into_iter().collect();
        let mut fell = Vec::new();
        for id in ids {
            let Some(e) = self.entities.get(&id) else { continue };
            let (x, y, mut z) = (e.x, e.y, e.z);
            let solid = self.solid.contains(&e.kind);
            let start = z;
            while !self.is_terrain(x, y, z)
                && !self.touches_terrain(x, y, z)
                && z + 1 < self.depth
                && !self.is_terrain(x, y, z + 1)
                && !(solid && self.blocked3(x, y, z + 1))
            {
                z += 1;
            }
            if z != start {
                let (from, to) = (self.cell(x, y, start), self.cell(x, y, z));
                self.grid[from].retain(|&i| i != id);
                let cell = &mut self.grid[to];
                let pos = cell.partition_point(|&i| i < id);
                cell.insert(pos, id);
                self.entities.get_mut(&id).expect("checked above").z = z;
                fell.push(id);
            }
        }
        fell
    }

    /// The neighbouring voxels an entity of `kind` could step into.
    pub fn enterable_neighbors(&self, kind: &str, x: i64, y: i64, z: i64) -> Vec<(i64, i64, i64)> {
        self.ring(x, y, z, 1).filter(|&(x, y, z)| self.can_enter(kind, x, y, z)).collect()
    }

    pub fn entities(&self) -> &BTreeMap<EntityId, Entity> {
        &self.entities
    }

    pub fn get(&self, id: EntityId) -> Option<&Entity> {
        self.entities.get(&id)
    }

    /// Voxel index: level by level, row by row.
    fn cell(&self, x: i64, y: i64, z: i64) -> usize {
        ((z * self.height + y) * self.width + x) as usize
    }

    pub fn in_bounds(&self, x: i64, y: i64) -> bool {
        self.in_bounds3(x, y, 0)
    }

    pub fn in_bounds3(&self, x: i64, y: i64, z: i64) -> bool {
        (0..self.width).contains(&x) && (0..self.height).contains(&y) && (0..self.depth).contains(&z)
    }

    /// Entity ids in the cell (ascending), level 0.
    pub fn at(&self, x: i64, y: i64) -> &[EntityId] {
        self.at3(x, y, 0)
    }

    /// Entity ids in the voxel (ascending).
    pub fn at3(&self, x: i64, y: i64, z: i64) -> &[EntityId] {
        if !self.in_bounds3(x, y, z) {
            return &[];
        }
        &self.grid[self.cell(x, y, z)]
    }

    /// Is there a solid entity in the cell (level 0), or terrain?
    pub fn blocked(&self, x: i64, y: i64) -> bool {
        self.blocked3(x, y, 0)
    }

    /// Solid entity or terrain in the voxel.
    pub fn blocked3(&self, x: i64, y: i64, z: i64) -> bool {
        self.is_terrain(x, y, z) || self.at3(x, y, z).iter().any(|id| self.solid.contains(&self.entities[id].kind))
    }

    /// Is the voxel solid ground (the terrain field is non-zero)? Blocks every mover.
    pub fn is_terrain(&self, x: i64, y: i64, z: i64) -> bool {
        self.terrain.as_ref().is_some_and(|t| self.field(t, x, y, z).is_some_and(|v| v != 0))
    }

    /// A solid kind cannot spawn into an occupied cell → None. Level 0.
    pub fn spawn(&mut self, kind: &str, state: &str, x: i64, y: i64, props: BTreeMap<String, i64>) -> Option<EntityId> {
        self.spawn3(kind, state, x, y, 0, props)
    }

    /// Nothing spawns inside terrain; a solid kind cannot spawn into an occupied voxel → None.
    pub fn spawn3(&mut self, kind: &str, state: &str, x: i64, y: i64, z: i64, props: BTreeMap<String, i64>) -> Option<EntityId> {
        self.index = None;
        let (x, y, z) = self.clamp3(x, y, z);
        if self.is_terrain(x, y, z) || (self.solid.contains(kind) && self.blocked3(x, y, z)) {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        let mut props = props;
        if self.motion.contains_key(kind) {
            // A moving thing starts at its cell's centre, still, on the ground, riding nothing.
            props.entry("px".into()).or_insert(x * FINE + FINE / 2);
            props.entry("py".into()).or_insert(y * FINE + FINE / 2);
            for p in &MOTION_PROPS[2..] {
                props.entry((*p).into()).or_insert(0);
            }
            if self.vehicles.contains_key(kind) {
                for p in crate::drive::VEHICLE_PROPS.iter().chain(&crate::drive::VEHICLE_INPUTS) {
                    props.entry((*p).into()).or_insert(0);
                }
            }
        }
        let e = Entity { id, kind: kind.into(), state: state.into(), x, y, z, props, genome: Vec::new() };
        self.entities.insert(id, e);
        let c = self.cell(x, y, z);
        self.grid[c].push(id); // ids are issued ascending, so order is preserved
        self.by_kind.entry(kind.into()).or_default().insert(id);
        Some(id)
    }

    pub fn despawn(&mut self, id: EntityId) -> Option<Entity> {
        self.index = None;
        let e = self.entities.remove(&id)?;
        let c = self.cell(e.x, e.y, e.z);
        self.grid[c].retain(|&i| i != id);
        if let Some(ids) = self.by_kind.get_mut(&e.kind) {
            ids.remove(&id);
        }
        Some(e)
    }

    /// One step on the same level.
    pub fn move_by(&mut self, id: EntityId, dx: i64, dy: i64) -> bool {
        self.move3(id, dx, dy, 0)
    }

    /// One step in 3D. Terrain stops everyone; a solid entity cannot enter an occupied voxel; a clinging one only
    /// voxels against terrain. True if it moved.
    pub fn move3(&mut self, id: EntityId, dx: i64, dy: i64, dz: i64) -> bool {
        let Some(e) = self.entities.get(&id) else { return false };
        let (dx, dy, dz) = (dx.signum(), dy.signum(), dz.signum());
        let open = |(x, y, z): (i64, i64, i64)| (x, y, z) != (e.x, e.y, e.z) && self.can_enter(&e.kind, x, y, z);
        let want = self.clamp3(e.x + dx, e.y + dy, e.z + dz);
        let target = if self.is_terrain(want.0, want.1, want.2) {
            // Slide along terrain: the first open axis-reduced step.
            [(dx, dy, 0), (0, 0, dz), (dx, 0, 0), (0, dy, 0)]
                .into_iter()
                .filter(|s| *s != (0, 0, 0))
                .map(|(sx, sy, sz)| self.clamp3(e.x + sx, e.y + sy, e.z + sz))
                .find(|p| open(*p))
        } else {
            open(want).then_some(want)
        };
        let Some((x, y, z)) = target else { return false };
        self.relocate(id, x, y, z, true);
        true
    }

    /// Moves an entity to a voxel (the grid follows). `shift_fine`: a moving kind's fine position moves by the
    /// same whole cells (a discrete move); off when the fine position is what moved.
    pub(crate) fn relocate(&mut self, id: EntityId, x: i64, y: i64, z: i64, shift_fine: bool) {
        self.index = None;
        let Some(e) = self.entities.get(&id) else { return };
        let (ox, oy) = (e.x, e.y);
        let from = self.cell(e.x, e.y, e.z);
        let to = self.cell(x, y, z);
        if from != to {
            self.grid[from].retain(|&i| i != id);
            let cell = &mut self.grid[to];
            let pos = cell.partition_point(|&i| i < id);
            cell.insert(pos, id);
        }
        let e = self.entities.get_mut(&id).expect("checked above");
        (e.x, e.y, e.z) = (x, y, z);
        if shift_fine && let (Some(px), Some(py)) = (e.props.get("px").copied(), e.props.get("py").copied()) {
            e.props.insert("px".into(), px + (x - ox) * FINE);
            e.props.insert("py".into(), py + (y - oy) * FINE);
        }
    }

    /// Exactly (dx, dy, dz) cells in one go (a dash, a leap): if the destination is inside the world and the kind
    /// can stand there (`can_enter`). Nothing between is checked. True if it moved.
    pub fn leap(&mut self, id: EntityId, dx: i64, dy: i64, dz: i64) -> bool {
        let Some(e) = self.entities.get(&id) else { return false };
        let (x, y, z) = self.clamp3(e.x + dx, e.y + dy, e.z + dz);
        if (x, y, z) == (e.x, e.y, e.z) || !self.can_enter(&e.kind, x, y, z) {
            return false;
        }
        self.relocate(id, x, y, z, true);
        true
    }

    // ---------------------------------------------------------------- continuous motion

    /// Moves every moving kind by its velocity (docs/architecture.md, Continuous motion): free movers first, in id
    /// order, then riders, carried by their mount's displacement this tick plus their own velocity. Height: gravity
    /// in the air, stopped by the ground (0) or, for a rider, its mount's `top`. Cells follow the fine positions.
    pub fn integrate_motion(&mut self) {
        if self.motion.is_empty() {
            return;
        }
        self.index = None;
        // Vehicles drive themselves (`drive.rs`): the physics layer moves them.
        self.drive_vehicles();
        let ids: Vec<EntityId> = self
            .motion
            .keys()
            .filter(|k| !self.vehicles.contains_key(*k))
            .filter_map(|k| self.by_kind.get(k))
            .flatten()
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut moved: BTreeMap<EntityId, (i64, i64)> = BTreeMap::new();
        let mut riders = Vec::new();
        for id in ids {
            match self.mount_of(id) {
                Some(m) => riders.push((id, m)),
                None => {
                    let d = self.glide(id, (0, 0), 0);
                    moved.insert(id, d);
                }
            }
        }
        for (id, m) in riders {
            let carry = moved.get(&m).copied().unwrap_or((0, 0));
            let top = self.entities.get(&m).and_then(|e| e.props.get("top").copied()).unwrap_or(0);
            let d = self.glide(id, carry, top);
            moved.insert(id, d);
        }
    }

    /// The live entity this one rides (its `mount` prop), if any; never itself.
    fn mount_of(&self, id: EntityId) -> Option<EntityId> {
        let m = self.entities.get(&id)?.props.get("mount").copied().unwrap_or(0);
        (m > 0 && m as EntityId != id && self.entities.contains_key(&(m as EntityId))).then_some(m as EntityId)
    }

    /// One tick of one mover: returns how far it moved (fine units).
    fn glide(&mut self, id: EntityId, carry: (i64, i64), floor: i64) -> (i64, i64) {
        let (w, h) = (self.width * FINE - 1, self.height * FINE - 1);
        let Some(e) = self.entities.get_mut(&id) else { return (0, 0) };
        let gravity = self.motion.get(&e.kind).map_or(0, |m| m.gravity);
        let p = &mut e.props;
        let get = |p: &BTreeMap<String, i64>, k: &str| p.get(k).copied().unwrap_or(0);
        let (px, py) = (get(p, "px"), get(p, "py"));
        let (nx, ny) = ((px + get(p, "vx") + carry.0).clamp(0, w), (py + get(p, "vy") + carry.1).clamp(0, h));
        let (mut ph, mut vh) = (get(p, "ph"), get(p, "vh"));
        let floor = floor.max(0);
        if ph > floor || vh > 0 {
            vh -= gravity;
            ph += vh;
        }
        if ph <= floor && vh <= 0 {
            (ph, vh) = (floor, 0);
        }
        p.insert("px".into(), nx);
        p.insert("py".into(), ny);
        p.insert("ph".into(), ph);
        p.insert("vh".into(), vh);
        let (cx, cy, z) = (nx.div_euclid(FINE), ny.div_euclid(FINE), e.z);
        if (cx, cy) != (e.x, e.y) {
            self.relocate(id, cx, cy, z, false);
        }
        (nx - px, ny - py)
    }

    /// Builds the broadphase for moving kinds (the engine does it after each tick's motion and physics). Queries give
    /// the same answers with or without it; with it they cost a few columns and a binary search instead of a scan.
    pub fn index_motion(&mut self) {
        if self.motion.is_empty() {
            return;
        }
        let mut idx = MotionIndex::default();
        for kind in self.motion.keys() {
            let cols = idx.cols.entry(kind.clone()).or_default();
            for id in self.by_kind.get(kind).into_iter().flatten() {
                let p = &self.entities[id].props;
                let (Some(px), Some(py)) = (p.get("px"), p.get("py")) else { continue };
                cols.entry(px.div_euclid(FINE)).or_default().push((*py, *id, *px));
            }
            for v in cols.values_mut() {
                v.sort_unstable();
            }
        }
        self.index = Some(idx);
    }

    /// Drops the broadphase: queries scan every entity (the reference the index must agree with).
    pub fn unindex_motion(&mut self) {
        self.index = None;
    }

    /// The indexed entities of `kind` in the columns that can reach across [lo, hi] (fine units), as sorted
    /// (py, id) lists; `None` if there is no index (then callers scan).
    fn columns(&self, kind: &str, lo: i64, hi: i64) -> Option<Vec<&[Slot]>> {
        let cols = self.index.as_ref()?.cols.get(kind)?;
        let hw = self.motion.get(kind).map_or(0, |m| m.size.0 / 2);
        let (c0, c1) = ((lo - hw).div_euclid(FINE), (hi + hw).div_euclid(FINE));
        Some(cols.range(c0..=c1).map(|(_, v)| v.as_slice()).collect())
    }

    /// A moving entity's footprint: (left, right, back, front) in fine units, and the way it faces (+1 or -1).
    fn footprint(&self, e: &Entity) -> Option<([i64; 4], i64)> {
        let m = self.motion.get(&e.kind)?;
        let (px, py) = (e.props.get("px").copied()?, e.props.get("py").copied()?);
        let facing = if e.props.get("vy").copied().unwrap_or(0) < 0 { -1 } else { 1 };
        Some(([px - m.size.0 / 2, px + m.size.0 / 2, py - m.size.1 / 2, py + m.size.1 / 2], facing))
    }

    /// Gap along the road from `me` to the nearest moving `kind` in front (`dir` +1) or behind (-1) of it, relative to
    /// the way it faces, whose extent across overlaps its own shifted by `dx`. FAR if none; negative if they overlap.
    pub fn gap(&self, me: EntityId, kind: &str, dx: i64, dir: i64) -> i64 {
        self.gap_to(me, kind, dx, dir).0
    }

    /// `gap` and who it is to (0 if nobody): the gap, then the lowest id among equals.
    pub fn gap_to(&self, me: EntityId, kind: &str, dx: i64, dir: i64) -> (i64, EntityId) {
        let Some(e) = self.entities.get(&me) else { return (FAR, 0) };
        let Some(([l, r, b, f], facing)) = self.footprint(e) else { return (FAR, 0) };
        let way = facing * dir;
        let (mut best, mut who) = (FAR, 0);
        if let Some(mut cols) = self.columns(kind, l + dx, r + dx) {
            // Every entity of a kind has the same footprint, so along a column the nearest centre ahead has the
            // nearest back edge. Walk each column outward from my centre: skip what does not overlap across, and
            // stop as soon as nothing further out can beat the best so far (a fixed-radius search that shrinks).
            // The column under the middle of the question goes first: it usually holds the answer.
            let Some(m) = self.motion.get(kind) else { return (FAR, 0) };
            let (hw, hl) = (m.size.0 / 2, m.size.1 / 2);
            let (lo, hi, my) = (l + dx, r + dx, (b + f) / 2);
            let mid = (lo + hi) / 2;
            cols.sort_by_key(|c| c.first().map_or(i64::MAX, |(_, _, px)| (px.div_euclid(FINE) * FINE + FINE / 2 - mid).abs()));
            for col in cols {
                if way > 0 {
                    let start = col.partition_point(|(py, _, _)| *py <= my);
                    for &(py, id, px) in &col[start..] {
                        let g = py - hl - f;
                        if g > best {
                            break;
                        }
                        if id != me && px + hw > lo && px - hw < hi && (g < best || id < who) {
                            (best, who) = (g, id);
                        }
                    }
                } else {
                    let end = col.partition_point(|(py, _, _)| *py < my);
                    for &(py, id, px) in col[..end].iter().rev() {
                        let g = b - (py + hl);
                        if g > best {
                            break;
                        }
                        if id != me && px + hw > lo && px - hw < hi && (g < best || id < who) {
                            (best, who) = (g, id);
                        }
                    }
                }
            }
            return (best, who);
        }
        for id in self.by_kind.get(kind).into_iter().flatten() {
            if *id == me {
                continue;
            }
            let Some(([ol, or, ob, of], _)) = self.footprint(&self.entities[id]) else { continue };
            if or <= l + dx || ol >= r + dx {
                continue;
            }
            let g = if way > 0 { ob - f } else { b - of };
            // Ahead means its middle is ahead of mine.
            let ahead = if way > 0 { ob + of > b + f } else { ob + of < b + f };
            if ahead && g < best {
                (best, who) = (g, *id);
            }
        }
        (best, who)
    }

    /// How many moving `kind` footprints overlap `me`'s.
    pub fn touching(&self, me: EntityId, kind: &str) -> i64 {
        let Some(e) = self.entities.get(&me) else { return 0 };
        let Some(([l, r, b, f], _)) = self.footprint(e) else { return 0 };
        let over = |id: &EntityId| {
            *id != me && self.footprint(&self.entities[id]).is_some_and(|([ol, or, ob, of], _)| or > l && ol < r && of > b && ob < f)
        };
        if let Some(cols) = self.columns(kind, l, r) {
            let hl = self.motion.get(kind).map_or(0, |m| m.size.1 / 2);
            let (lo, hi) = (b - hl, f + hl);
            return cols
                .iter()
                .map(|col| {
                    let start = col.partition_point(|(py, _, _)| *py <= lo);
                    col[start..].iter().take_while(|(py, _, _)| *py < hi).filter(|(_, id, _)| over(id)).count() as i64
                })
                .sum();
        }
        self.by_kind.get(kind).into_iter().flatten().filter(|id| over(id)).count() as i64
    }

    /// The moving `kind` whose footprint holds `me`'s centre (the lowest id if several); 0 if none.
    pub fn under(&self, me: EntityId, kind: &str) -> i64 {
        let Some(e) = self.entities.get(&me) else { return 0 };
        let (Some(px), Some(py)) = (e.props.get("px").copied(), e.props.get("py").copied()) else { return 0 };
        let holds = |id: &EntityId| {
            *id != me && self.footprint(&self.entities[id]).is_some_and(|([l, r, b, f], _)| px >= l && px < r && py >= b && py < f)
        };
        if let Some(cols) = self.columns(kind, px, px) {
            let hl = self.motion.get(kind).map_or(0, |m| m.size.1 / 2);
            return cols
                .iter()
                .flat_map(|col| {
                    let start = col.partition_point(|(y, _, _)| *y < py - hl);
                    col[start..].iter().take_while(move |(y, _, _)| *y <= py + hl).map(|(_, id, _)| *id)
                })
                .filter(holds)
                .min()
                .map_or(0, |id| id as i64);
        }
        self.by_kind.get(kind).into_iter().flatten().find(|id| holds(id)).map_or(0, |id| *id as i64)
    }

    pub fn props_mut(&mut self, id: EntityId) -> Option<&mut BTreeMap<String, i64>> {
        self.index = None;
        self.entities.get_mut(&id).map(|e| &mut e.props)
    }

    pub fn set_genome(&mut self, id: EntityId, genome: Vec<i8>) {
        if let Some(e) = self.entities.get_mut(&id) {
            e.genome = genome;
        }
    }

    pub fn set_state(&mut self, id: EntityId, state: String) {
        if let Some(e) = self.entities.get_mut(&id) {
            e.state = state;
        }
    }

    pub fn clamp(&self, x: i64, y: i64) -> (i64, i64) {
        (x.clamp(0, self.width - 1), y.clamp(0, self.height - 1))
    }

    pub fn clamp3(&self, x: i64, y: i64, z: i64) -> (i64, i64, i64) {
        (x.clamp(0, self.width - 1), y.clamp(0, self.height - 1), z.clamp(0, self.depth - 1))
    }

    // ---------------------------------------------------------------- fields

    /// Adds a field with every voxel at `init` (no-op if it exists).
    pub fn add_field(&mut self, name: &str, init: i64) {
        let n = self.grid.len();
        self.fields.entry(name.to_string()).or_insert_with(|| vec![init; n]);
    }

    pub fn set_terrain(&mut self, field: Option<String>) {
        self.terrain = field;
    }

    pub fn terrain(&self) -> Option<&str> {
        self.terrain.as_deref()
    }

    pub fn field(&self, name: &str, x: i64, y: i64, z: i64) -> Option<i64> {
        if !self.in_bounds3(x, y, z) {
            return None;
        }
        self.fields.get(name).map(|f| f[self.cell(x, y, z)])
    }

    pub fn set_field(&mut self, name: &str, x: i64, y: i64, z: i64, v: i64) {
        if self.in_bounds3(x, y, z) {
            let c = self.cell(x, y, z);
            if let Some(f) = self.fields.get_mut(name) {
                f[c] = v;
            }
        }
    }

    pub fn field_names(&self) -> impl Iterator<Item = &str> {
        self.fields.keys().map(String::as_str)
    }

    /// A whole field, in voxel order (physics reads and replaces it in one go).
    pub fn field_values(&self, name: &str) -> Option<&[i64]> {
        self.fields.get(name).map(Vec::as_slice)
    }

    pub fn replace_field(&mut self, name: &str, values: Vec<i64>) {
        if values.len() == self.grid.len()
            && let Some(f) = self.fields.get_mut(name)
        {
            *f = values;
        }
    }

    /// Entities of a kind, in id order.
    pub fn of_kind(&self, kind: &str) -> impl Iterator<Item = &Entity> {
        self.by_kind.get(kind).into_iter().flatten().map(|id| &self.entities[id])
    }

    pub fn count(&self, kind: &str) -> usize {
        self.by_kind.get(kind).map_or(0, BTreeSet::len)
    }

    /// Voxels on the shell at Chebyshev distance `d` (within bounds): level by level, row by row.
    /// At depth 1 this is the 2D ring in the same order as ever.
    fn ring(&self, cx: i64, cy: i64, cz: i64, d: i64) -> impl Iterator<Item = (i64, i64, i64)> + '_ {
        (cz - d..=cz + d)
            .filter(move |z| (0..self.depth).contains(z))
            .flat_map(move |z| (cy - d..=cy + d).flat_map(move |y| (cx - d..=cx + d).map(move |x| (x, y, z))))
            .filter(move |&(x, y, z)| (x - cx).abs().max((y - cy).abs()).max((z - cz).abs()) == d && self.in_bounds3(x, y, z))
    }

    /// Nearest `kind` (Chebyshev distance). On a tie the smaller id wins.
    pub fn nearest(&self, from: &Entity, kind: &str) -> Option<(&Entity, i64)> {
        self.nearest_where(from, kind, |_| true)
    }

    /// Nearest `kind` passing `keep`. Sparse kind → scan members one by one; dense kind →
    /// search outward ring by ring. Both give the same result (smallest distance, then smallest id).
    pub fn nearest_where(&self, from: &Entity, kind: &str, keep: impl Fn(&Entity) -> bool) -> Option<(&Entity, i64)> {
        self.nearest_within(from, kind, i64::MAX, keep)
    }

    /// Nearest `kind` passing `keep` at most `r` away (Chebyshev), or none: a bounded question searches only as far
    /// as it asks (radius 0 looks at one cell), not the whole world first.
    pub fn nearest_within(&self, from: &Entity, kind: &str, r: i64, keep: impl Fn(&Entity) -> bool) -> Option<(&Entity, i64)> {
        let members = self.by_kind.get(kind)?;
        if members.len() <= SPARSE {
            return members
                .iter()
                .filter(|&&id| id != from.id)
                .map(|id| &self.entities[id])
                .filter(|e| keep(e))
                .map(|e| (e, (e.x - from.x).abs().max((e.y - from.y).abs()).max((e.z - from.z).abs())))
                .filter(|(_, d)| *d <= r)
                .min_by_key(|(e, d)| (*d, e.id));
        }
        let max_d = self.width.max(self.height).max(self.depth).min(r);
        for d in 0..=max_d {
            let best = self
                .ring(from.x, from.y, from.z, d)
                .flat_map(|(x, y, z)| self.at3(x, y, z))
                .filter(|&&id| id != from.id && self.entities[&id].kind == kind && keep(&self.entities[&id]))
                .min();
            if let Some(id) = best {
                return Some((&self.entities[id], d));
            }
        }
        None
    }

    /// Count of `kind` (and optionally `state`) within radius `r` (Chebyshev, 3D), excluding self.
    pub fn around(&self, pos: (i64, i64, i64), exclude: EntityId, kind: &str, state: Option<&str>, r: i64) -> i64 {
        self.around_where(pos, exclude, kind, r, |e| state.is_none_or(|s| e.state == s))
    }

    /// Count of `kind` passing `keep` within radius `r` (3D), excluding self.
    pub fn around_where(
        &self,
        (cx, cy, cz): (i64, i64, i64),
        exclude: EntityId,
        kind: &str,
        r: i64,
        keep: impl Fn(&Entity) -> bool,
    ) -> i64 {
        let mut n = 0;
        for z in (cz - r).max(0)..=(cz + r).min(self.depth - 1) {
            for y in cy - r..=cy + r {
                for x in cx - r..=cx + r {
                    for id in self.at3(x, y, z) {
                        let e = &self.entities[id];
                        if e.id != exclude && e.kind == kind && keep(e) {
                            n += 1;
                        }
                    }
                }
            }
        }
        n
    }

    /// All in-bounds neighbouring voxels (8 on one level, 26 in 3D); in fixed order.
    pub fn neighbors(&self, x: i64, y: i64, z: i64) -> Vec<(i64, i64, i64)> {
        self.ring(x, y, z, 1).collect()
    }

    /// Neighbouring voxels with no solid and no terrain; in fixed order.
    pub fn free_neighbors(&self, x: i64, y: i64, z: i64) -> Vec<(i64, i64, i64)> {
        self.ring(x, y, z, 1).filter(|&(x, y, z)| !self.blocked3(x, y, z)).collect()
    }

    /// Stateless randomness: (seed, tick, entity, salt) → number.
    /// Rule evaluation order does not affect the result; no shared RNG state.
    pub fn rand(&self, entity: EntityId, salt: u64) -> u64 {
        splitmix64(
            self.seed
                ^ self.tick.wrapping_mul(0x9E37_79B9_7F4A_7C15)
                ^ entity.wrapping_mul(0xBF58_476D_1CE4_E5B9)
                ^ salt.wrapping_mul(0x94D0_49BB_1331_11EB),
        )
    }

    /// A die roll in 0..100.
    pub fn roll(&self, entity: EntityId, salt: u64) -> i64 {
        (self.rand(entity, salt) % 100) as i64
    }

    /// Fingerprint of the state (FNV-1a). Same seed + same input → same hash.
    pub fn hash(&self) -> u64 {
        // Depth and fields only enter the hash when a world has them: 2D games keep their hashes.
        let deep = self.depth > 1;
        let mut h = Fnv::new();
        h.u64(self.tick);
        h.u64(self.next_id);
        if deep {
            h.u64(self.depth as u64);
        }
        for e in self.entities.values() {
            h.u64(e.id);
            h.str(&e.kind);
            h.str(&e.state);
            h.u64(e.x as u64);
            h.u64(e.y as u64);
            if deep {
                h.u64(e.z as u64);
            }
            for (k, v) in &e.props {
                h.str(k);
                h.u64(*v as u64);
            }
            // Only learning agents have a genome: other games keep their hashes.
            if !e.genome.is_empty() {
                h.str("genome");
                e.genome.iter().for_each(|g| h.u64(*g as u8 as u64));
            }
        }
        // Fields are hashed a value at a time, not a byte at a time: every voxel is hashed every tick, and byte-wise
        // FNV over two 40x40x20 fields was most of a tick (0.5 of 0.73 ms in mound).
        for (name, values) in &self.fields {
            h.str(name);
            h.u64(values.len() as u64);
            values.iter().for_each(|v| h.word(*v as u64));
        }
        h.0
    }
}

/// For kinds below this count, a linear scan is cheaper than a ring search.
const SPARSE: usize = 64;

pub fn splitmix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
    fn bytes(&mut self, b: &[u8]) {
        for byte in b {
            self.0 ^= *byte as u64;
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
    fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }
    /// A whole word in one step (multiply, then rotate: every input bit reaches every output bit within a few words).
    fn word(&mut self, v: u64) {
        self.0 = (self.0 ^ v).wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(23);
    }
    fn str(&mut self, s: &str) {
        self.u64(s.len() as u64);
        self.bytes(s.as_bytes());
    }
}

#[cfg(test)]
mod motion_tests {
    use super::*;

    fn road() -> World {
        let mut w = World::new(1, 4, 100);
        let car = Motion { size: (600, 1600), gravity: 0 };
        let rider = Motion { size: (300, 400), gravity: 10 };
        w.set_motion([("car".to_string(), car), ("rider".to_string(), rider)].into_iter().collect());
        w
    }

    fn p(w: &World, id: EntityId, k: &str) -> i64 {
        w.get(id).unwrap().props[k]
    }

    fn set(w: &mut World, id: EntityId, k: &str, v: i64) {
        w.props_mut(id).unwrap().insert(k.into(), v);
    }

    #[test]
    fn a_moving_thing_glides_by_its_velocity_and_its_cell_follows() {
        let mut w = road();
        let car = w.spawn("car", "-", 1, 2, BTreeMap::new()).unwrap();
        assert_eq!((p(&w, car, "px"), p(&w, car, "py"), p(&w, car, "vy")), (1500, 2500, 0), "starts at its cell's centre");
        set(&mut w, car, "vy", 300);
        let mut ys = Vec::new();
        for _ in 0..5 {
            w.integrate_motion();
            ys.push(p(&w, car, "py"));
        }
        assert_eq!(ys, vec![2800, 3100, 3400, 3700, 4000], "every tick, not a cell every few ticks");
        assert_eq!((w.get(car).unwrap().x, w.get(car).unwrap().y), (1, 4), "the cell follows the fine position");
        assert!(w.at(1, 4).contains(&car) && !w.at(1, 2).contains(&car), "and so does the grid");
    }

    #[test]
    fn a_rider_is_carried_by_its_mount_and_lands_back_on_its_roof() {
        let mut w = road();
        let car = w.spawn("car", "-", 1, 2, BTreeMap::new()).unwrap();
        let me = w.spawn("rider", "-", 1, 2, BTreeMap::new()).unwrap();
        set(&mut w, car, "vy", 250);
        set(&mut w, car, "top", 500);
        set(&mut w, me, "mount", car as i64);
        set(&mut w, me, "ph", 500);
        w.integrate_motion();
        assert_eq!(p(&w, me, "py") - p(&w, car, "py"), 0, "moves with the car");
        // A jump straight up on a moving car comes down on the same car.
        set(&mut w, me, "vh", 60);
        let mut top = 0;
        for _ in 0..30 {
            w.integrate_motion();
            top = top.max(p(&w, me, "ph"));
        }
        assert!(top > 600, "it flew: {top}");
        assert_eq!((p(&w, me, "ph"), p(&w, me, "vh")), (500, 0), "stopped by the roof, not the road");
        assert_eq!(p(&w, me, "py"), p(&w, car, "py"), "still over the car it left");
        // Its mount gone, it falls to the road.
        w.despawn(car);
        for _ in 0..30 {
            w.integrate_motion();
        }
        assert_eq!(p(&w, me, "ph"), 0);
    }

    #[test]
    fn footprints_answer_what_is_ahead_behind_beside_and_under() {
        let mut w = road();
        let me = w.spawn("car", "-", 1, 10, BTreeMap::new()).unwrap();
        let front = w.spawn("car", "-", 1, 14, BTreeMap::new()).unwrap();
        let back = w.spawn("car", "-", 1, 7, BTreeMap::new()).unwrap();
        let beside = w.spawn("car", "-", 2, 12, BTreeMap::new()).unwrap();
        // Centres 4 cells apart, each 1.6 cells long: a gap of 2.4 cells.
        assert_eq!(w.gap(me, "car", 0, 1), 2400);
        assert_eq!(w.gap(me, "car", 0, -1), 1400);
        assert_eq!(w.gap(me, "car", 1000, 1), 400, "one lane over, the car beside is nearer");
        // Facing the other way (driving towards -y), ahead and behind swap.
        set(&mut w, me, "vy", -100);
        assert_eq!(w.gap(me, "car", 0, 1), 1400);
        assert_eq!(w.touching(me, "car"), 0);
        set(&mut w, front, "py", 11_000);
        assert_eq!(w.touching(me, "car"), 1, "footprints overlap");
        let rider = w.spawn("rider", "-", 2, 12, BTreeMap::new()).unwrap();
        assert_eq!(w.under(rider, "car"), beside as i64);
        let _ = back;
    }

    #[test]
    fn a_discrete_move_carries_the_fine_position() {
        let mut w = road();
        let car = w.spawn("car", "-", 1, 2, BTreeMap::new()).unwrap();
        assert!(w.leap(car, 1, 3, 0));
        assert_eq!((p(&w, car, "px"), p(&w, car, "py")), (2500, 5500));
    }
}

#[cfg(test)]
mod nearest_tests {
    use super::*;

    #[test]
    fn a_bounded_search_finds_what_the_unbounded_one_finds_within_its_radius() {
        let mut w = World::new(1, 7, 400);
        let me = w.spawn("probe", "-", 3, 200, BTreeMap::new()).unwrap();
        // Dense (ring search) and sparse (member scan) kinds.
        for y in (0..400).step_by(3) {
            w.spawn("dense", "-", (y % 7) as i64, y as i64, BTreeMap::new());
        }
        for y in [10, 150, 207, 390] {
            w.spawn("sparse", "-", 2, y, BTreeMap::new());
        }
        let from = w.get(me).unwrap().clone();
        for kind in ["dense", "sparse"] {
            let all = w.nearest(&from, kind).map(|(e, d)| (e.id, d));
            for r in [0, 1, 3, 7, 50, 500] {
                let within = w.nearest_within(&from, kind, r, |_| true).map(|(e, d)| (e.id, d));
                assert_eq!(within, all.filter(|(_, d)| *d <= r), "{kind} r={r}");
            }
        }
    }
}

#[cfg(test)]
mod index_tests {
    use super::*;

    /// Random roads: cars (and riders) in random lanes and places, some driving the other way. Every footprint
    /// question gets the same answer from the broadphase as from scanning every entity.
    #[test]
    fn the_broadphase_answers_exactly_what_a_scan_answers() {
        let mut rng = 0x1234_5678_u64;
        let mut next = |n: i64| {
            rng = splitmix64(rng);
            (rng % n as u64) as i64
        };
        for round in 0..40 {
            let mut w = World::new(round, 5, 300);
            let car = Motion { size: (600, 900), gravity: 0 };
            let rider = Motion { size: (300, 400), gravity: 7 };
            w.set_motion([("car".to_string(), car), ("rider".to_string(), rider)].into_iter().collect());
            let n = 20 + next(200);
            let mut ids = Vec::new();
            for _ in 0..n {
                let kind = if next(8) == 0 { "rider" } else { "car" };
                let id = w.spawn(kind, "-", next(5), next(300), BTreeMap::new()).unwrap();
                let p = w.props_mut(id).unwrap();
                p.insert("px".into(), next(5000));
                p.insert("py".into(), next(300_000));
                p.insert("vy".into(), next(3) - 1);
                ids.push(id);
            }
            // The cells follow the fine positions (as after a tick).
            w.integrate_motion();
            let mut asked = Vec::new();
            for &me in &ids {
                for kind in ["car", "rider"] {
                    for dx in [-1000, 0, 1000] {
                        for dir in [1, -1] {
                            asked.push(format!("{:?}", w.gap_to(me, kind, dx, dir)));
                        }
                    }
                    asked.push(format!("{} {}", w.touching(me, kind), w.under(me, kind)));
                }
            }
            w.index_motion();
            let mut indexed = Vec::new();
            for &me in &ids {
                for kind in ["car", "rider"] {
                    for dx in [-1000, 0, 1000] {
                        for dir in [1, -1] {
                            indexed.push(format!("{:?}", w.gap_to(me, kind, dx, dir)));
                        }
                    }
                    indexed.push(format!("{} {}", w.touching(me, kind), w.under(me, kind)));
                }
            }
            assert!(w.index.is_some(), "the index was used");
            assert_eq!(asked, indexed, "round {round}: the broadphase disagrees with the scan");
        }
    }
}
