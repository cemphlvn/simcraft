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
    pub props: BTreeMap<String, i64>,
}

/// The whole world, in portable form. Grid and kind index are derived; not carried.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldSnapshot {
    pub seed: u64,
    pub tick: u64,
    pub width: i64,
    pub height: i64,
    pub next_id: EntityId,
    pub solid: BTreeSet<String>,
    pub entities: Vec<Entity>,
}

/// All game state. BTreeMap: iteration order is fixed by id → determinism.
/// Mutation only through methods: the grid index always stays in sync with entities.
#[derive(Clone, Debug)]
pub struct World {
    pub seed: u64,
    pub tick: u64,
    pub width: i64,
    pub height: i64,
    entities: BTreeMap<EntityId, Entity>,
    /// Entity ids per cell (ascending). Derived data; not part of the hash.
    grid: Vec<Vec<EntityId>>,
    /// Entity ids per kind. Makes lookups cheap for sparse kinds (8 wolves, 2000 sheep).
    by_kind: BTreeMap<String, BTreeSet<EntityId>>,
    /// A cell cannot hold a second solid (wall, tree, hero).
    solid: BTreeSet<String>,
    next_id: EntityId,
}

impl World {
    pub fn new(seed: u64, width: i64, height: i64) -> Self {
        let cells = (width.max(1) * height.max(1)) as usize;
        Self {
            seed,
            tick: 0,
            width,
            height,
            entities: BTreeMap::new(),
            grid: vec![Vec::new(); cells],
            by_kind: BTreeMap::new(),
            solid: BTreeSet::new(),
            next_id: 1,
        }
    }

    pub fn snapshot(&self) -> WorldSnapshot {
        WorldSnapshot {
            seed: self.seed,
            tick: self.tick,
            width: self.width,
            height: self.height,
            next_id: self.next_id,
            solid: self.solid.clone(),
            entities: self.entities.values().cloned().collect(),
        }
    }

    /// Builds the world from a snapshot; derived indexes are recomputed.
    /// An inconsistent snapshot (out of bounds, duplicate id, two solids in one cell) is rejected.
    pub fn from_snapshot(s: WorldSnapshot) -> Result<World, String> {
        if s.width < 1 || s.height < 1 {
            return Err(format!("world size {}x{}", s.width, s.height));
        }
        let mut w = World::new(s.seed, s.width, s.height);
        w.tick = s.tick;
        w.solid = s.solid;
        for e in s.entities {
            if !w.in_bounds(e.x, e.y) {
                return Err(format!("entity {} at ({}, {}) is outside the world", e.id, e.x, e.y));
            }
            if e.id >= s.next_id || w.entities.contains_key(&e.id) {
                return Err(format!("entity id {} is repeated or not below next_id {}", e.id, s.next_id));
            }
            if w.solid.contains(&e.kind) && w.blocked(e.x, e.y) {
                return Err(format!("two solids at ({}, {})", e.x, e.y));
            }
            let c = w.cell(e.x, e.y);
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

    pub fn entities(&self) -> &BTreeMap<EntityId, Entity> {
        &self.entities
    }

    pub fn get(&self, id: EntityId) -> Option<&Entity> {
        self.entities.get(&id)
    }

    fn cell(&self, x: i64, y: i64) -> usize {
        (y * self.width + x) as usize
    }

    pub fn in_bounds(&self, x: i64, y: i64) -> bool {
        (0..self.width).contains(&x) && (0..self.height).contains(&y)
    }

    /// Entity ids in the cell (ascending).
    pub fn at(&self, x: i64, y: i64) -> &[EntityId] {
        if !self.in_bounds(x, y) {
            return &[];
        }
        &self.grid[self.cell(x, y)]
    }

    /// Is there a solid entity in the cell?
    pub fn blocked(&self, x: i64, y: i64) -> bool {
        self.at(x, y).iter().any(|id| self.solid.contains(&self.entities[id].kind))
    }

    /// A solid kind cannot spawn into an occupied cell → None.
    pub fn spawn(
        &mut self,
        kind: &str,
        state: &str,
        x: i64,
        y: i64,
        props: BTreeMap<String, i64>,
    ) -> Option<EntityId> {
        let (x, y) = self.clamp(x, y);
        if self.solid.contains(kind) && self.blocked(x, y) {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        let e = Entity { id, kind: kind.into(), state: state.into(), x, y, props };
        self.entities.insert(id, e);
        let c = self.cell(x, y);
        self.grid[c].push(id); // ids are issued ascending, so order is preserved
        self.by_kind.entry(kind.into()).or_default().insert(id);
        Some(id)
    }

    pub fn despawn(&mut self, id: EntityId) -> Option<Entity> {
        let e = self.entities.remove(&id)?;
        let c = self.cell(e.x, e.y);
        self.grid[c].retain(|&i| i != id);
        if let Some(ids) = self.by_kind.get_mut(&e.kind) {
            ids.remove(&id);
        }
        Some(e)
    }

    /// One step. A solid entity cannot enter an occupied cell. True if it moved.
    pub fn move_by(&mut self, id: EntityId, dx: i64, dy: i64) -> bool {
        let Some(e) = self.entities.get(&id) else { return false };
        let (x, y) = self.clamp(e.x + dx.signum(), e.y + dy.signum());
        if (x, y) == (e.x, e.y) {
            return false;
        }
        if self.solid.contains(&e.kind) && self.blocked(x, y) {
            return false;
        }
        let from = self.cell(e.x, e.y);
        let to = self.cell(x, y);
        self.grid[from].retain(|&i| i != id);
        let cell = &mut self.grid[to];
        let pos = cell.partition_point(|&i| i < id);
        cell.insert(pos, id);
        let e = self.entities.get_mut(&id).expect("checked above");
        (e.x, e.y) = (x, y);
        true
    }

    pub fn props_mut(&mut self, id: EntityId) -> Option<&mut BTreeMap<String, i64>> {
        self.entities.get_mut(&id).map(|e| &mut e.props)
    }

    pub fn set_state(&mut self, id: EntityId, state: String) {
        if let Some(e) = self.entities.get_mut(&id) {
            e.state = state;
        }
    }

    pub fn clamp(&self, x: i64, y: i64) -> (i64, i64) {
        (x.clamp(0, self.width - 1), y.clamp(0, self.height - 1))
    }

    pub fn count(&self, kind: &str) -> usize {
        self.by_kind.get(kind).map_or(0, BTreeSet::len)
    }

    /// Cells on the ring at Chebyshev distance `d` (within bounds).
    fn ring(&self, cx: i64, cy: i64, d: i64) -> impl Iterator<Item = (i64, i64)> + '_ {
        (cy - d..=cy + d)
            .flat_map(move |y| (cx - d..=cx + d).map(move |x| (x, y)))
            .filter(move |&(x, y)| (x - cx).abs().max((y - cy).abs()) == d && self.in_bounds(x, y))
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
                .map(|e| (e, (e.x - from.x).abs().max((e.y - from.y).abs())))
                .min_by_key(|(e, d)| (*d, e.id));
        }
        let max_d = self.width.max(self.height);
        for d in 0..=max_d {
            let best = self
                .ring(from.x, from.y, d)
                .flat_map(|(x, y)| self.at(x, y))
                .filter(|&&id| id != from.id && self.entities[&id].kind == kind && keep(&self.entities[&id]))
                .min();
            if let Some(id) = best {
                return Some((&self.entities[id], d));
            }
        }
        None
    }

    /// Count of `kind` (and optionally `state`) within radius `r` (Chebyshev), excluding self.
    pub fn around(&self, pos: (i64, i64), exclude: EntityId, kind: &str, state: Option<&str>, r: i64) -> i64 {
        self.around_where(pos, exclude, kind, r, |e| state.is_none_or(|s| e.state == s))
    }

    /// Count of `kind` passing `keep` within radius `r`, excluding self.
    pub fn around_where(
        &self,
        (cx, cy): (i64, i64),
        exclude: EntityId,
        kind: &str,
        r: i64,
        keep: impl Fn(&Entity) -> bool,
    ) -> i64 {
        let mut n = 0;
        for y in cy - r..=cy + r {
            for x in cx - r..=cx + r {
                for id in self.at(x, y) {
                    let e = &self.entities[id];
                    if e.id != exclude && e.kind == kind && keep(e) {
                        n += 1;
                    }
                }
            }
        }
        n
    }

    /// Neighboring (8 directions) empty cells with no solid; in fixed order.
    pub fn free_neighbors(&self, x: i64, y: i64) -> Vec<(i64, i64)> {
        self.ring(x, y, 1).filter(|&(x, y)| !self.blocked(x, y)).collect()
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
        let mut h = Fnv::new();
        h.u64(self.tick);
        h.u64(self.next_id);
        for e in self.entities.values() {
            h.u64(e.id);
            h.str(&e.kind);
            h.str(&e.state);
            h.u64(e.x as u64);
            h.u64(e.y as u64);
            for (k, v) in &e.props {
                h.str(k);
                h.u64(*v as u64);
            }
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
    fn str(&mut self, s: &str) {
        self.u64(s.len() as u64);
        self.bytes(s.as_bytes());
    }
}
