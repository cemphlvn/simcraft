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
    entities: BTreeMap<EntityId, Entity>,
    /// Entity ids per cell (ascending). Derived data; not part of the hash.
    grid: Vec<Vec<EntityId>>,
    /// Entity ids per kind. Makes lookups cheap for sparse kinds (8 wolves, 2000 sheep).
    by_kind: BTreeMap<String, BTreeSet<EntityId>>,
    /// A cell cannot hold a second solid (wall, tree, hero).
    solid: BTreeSet<String>,
    next_id: EntityId,
    /// Numbers per voxel (temperature, soil...), in voxel order. Part of the world and its hash.
    fields: BTreeMap<String, Vec<i64>>,
    /// A voxel whose value in this field is non-zero is solid for every mover (soil, rock).
    terrain: Option<String>,
    /// Kinds that crawl: they only enter voxels touching terrain (walls, floors, ceilings), so they never float.
    cling: BTreeSet<String>,
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
        let (x, y, z) = self.clamp3(x, y, z);
        if self.is_terrain(x, y, z) || (self.solid.contains(kind) && self.blocked3(x, y, z)) {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        let e = Entity { id, kind: kind.into(), state: state.into(), x, y, z, props, genome: Vec::new() };
        self.entities.insert(id, e);
        let c = self.cell(x, y, z);
        self.grid[c].push(id); // ids are issued ascending, so order is preserved
        self.by_kind.entry(kind.into()).or_default().insert(id);
        Some(id)
    }

    pub fn despawn(&mut self, id: EntityId) -> Option<Entity> {
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
        let from = self.cell(e.x, e.y, e.z);
        let to = self.cell(x, y, z);
        self.grid[from].retain(|&i| i != id);
        let cell = &mut self.grid[to];
        let pos = cell.partition_point(|&i| i < id);
        cell.insert(pos, id);
        let e = self.entities.get_mut(&id).expect("checked above");
        (e.x, e.y, e.z) = (x, y, z);
        true
    }

    /// Exactly (dx, dy, dz) cells in one go (a dash, a leap): if the destination is inside the world and the kind
    /// can stand there (`can_enter`). Nothing between is checked. True if it moved.
    pub fn leap(&mut self, id: EntityId, dx: i64, dy: i64, dz: i64) -> bool {
        let Some(e) = self.entities.get(&id) else { return false };
        let (x, y, z) = self.clamp3(e.x + dx, e.y + dy, e.z + dz);
        if (x, y, z) == (e.x, e.y, e.z) || !self.can_enter(&e.kind, x, y, z) {
            return false;
        }
        let from = self.cell(e.x, e.y, e.z);
        let to = self.cell(x, y, z);
        self.grid[from].retain(|&i| i != id);
        let cell = &mut self.grid[to];
        let pos = cell.partition_point(|&i| i < id);
        cell.insert(pos, id);
        let e = self.entities.get_mut(&id).expect("checked above");
        (e.x, e.y, e.z) = (x, y, z);
        true
    }

    pub fn props_mut(&mut self, id: EntityId) -> Option<&mut BTreeMap<String, i64>> {
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
        let members = self.by_kind.get(kind)?;
        if members.len() <= SPARSE {
            return members
                .iter()
                .filter(|&&id| id != from.id)
                .map(|id| &self.entities[id])
                .filter(|e| keep(e))
                .map(|e| (e, (e.x - from.x).abs().max((e.y - from.y).abs()).max((e.z - from.z).abs())))
                .min_by_key(|(e, d)| (*d, e.id));
        }
        let max_d = self.width.max(self.height).max(self.depth);
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
