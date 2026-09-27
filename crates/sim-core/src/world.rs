use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

pub type EntityId = u64;

#[derive(Clone, Debug, Serialize)]
pub struct Entity {
    pub id: EntityId,
    pub kind: String,
    pub state: String,
    pub x: i64,
    pub y: i64,
    pub props: BTreeMap<String, i64>,
}

/// Tüm oyun durumu. BTreeMap: iterasyon sırası id'ye göre sabit → determinizm.
/// Mutasyon yalnızca metotlarla: grid indeksi entity'lerle hep senkron kalır.
#[derive(Clone, Debug)]
pub struct World {
    pub seed: u64,
    pub tick: u64,
    pub width: i64,
    pub height: i64,
    entities: BTreeMap<EntityId, Entity>,
    /// Hücre başına entity id'leri (artan sırada). Türetilmiş veri; hash'e girmez.
    grid: Vec<Vec<EntityId>>,
    /// Kind başına entity id'leri. Seyrek kind'larda (8 kurt, 2000 koyun) aramayı ucuzlatır.
    by_kind: BTreeMap<String, BTreeSet<EntityId>>,
    /// Aynı hücrede ikinci bir solid bulunamaz (duvar, ağaç, kahraman).
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

    /// Hücredeki entity id'leri (artan).
    pub fn at(&self, x: i64, y: i64) -> &[EntityId] {
        if !self.in_bounds(x, y) {
            return &[];
        }
        &self.grid[self.cell(x, y)]
    }

    /// Hücrede solid bir entity var mı?
    pub fn blocked(&self, x: i64, y: i64) -> bool {
        self.at(x, y).iter().any(|id| self.solid.contains(&self.entities[id].kind))
    }

    /// Solid bir kind dolu hücreye doğamaz → None.
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
        self.grid[c].push(id); // id'ler artan verildiği için sıra korunur
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

    /// Bir adım. Solid bir entity dolu hücreye giremez. Hareket ettiyse true.
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

    /// Chebyshev mesafesi `d` olan halkadaki hücreler (sınır içinde).
    fn ring(&self, cx: i64, cy: i64, d: i64) -> impl Iterator<Item = (i64, i64)> + '_ {
        (cy - d..=cy + d)
            .flat_map(move |y| (cx - d..=cx + d).map(move |x| (x, y)))
            .filter(move |&(x, y)| (x - cx).abs().max((y - cy).abs()) == d && self.in_bounds(x, y))
    }

    /// En yakın `kind` (Chebyshev mesafesi). Eşitlikte küçük id kazanır.
    /// Seyrek kind → üyeleri tek tek tara; yoğun kind → halka halka dışarı ara.
    /// İki yol da aynı sonucu verir (en küçük mesafe, sonra en küçük id).
    pub fn nearest(&self, from: &Entity, kind: &str) -> Option<(&Entity, i64)> {
        let members = self.by_kind.get(kind)?;
        if members.len() <= SPARSE {
            return members
                .iter()
                .filter(|&&id| id != from.id)
                .map(|id| {
                    let e = &self.entities[id];
                    (e, (e.x - from.x).abs().max((e.y - from.y).abs()))
                })
                .min_by_key(|(e, d)| (*d, e.id));
        }
        let max_d = self.width.max(self.height);
        for d in 0..=max_d {
            let best = self
                .ring(from.x, from.y, d)
                .flat_map(|(x, y)| self.at(x, y))
                .filter(|&&id| id != from.id && self.entities[&id].kind == kind)
                .min();
            if let Some(id) = best {
                return Some((&self.entities[id], d));
            }
        }
        None
    }

    /// `r` yarıçapında (Chebyshev), kendisi hariç, `kind` (ve istenirse `state`) sayısı.
    pub fn around(&self, (cx, cy): (i64, i64), exclude: EntityId, kind: &str, state: Option<&str>, r: i64) -> i64 {
        let mut n = 0;
        for y in cy - r..=cy + r {
            for x in cx - r..=cx + r {
                for id in self.at(x, y) {
                    let e = &self.entities[id];
                    if e.id != exclude && e.kind == kind && state.is_none_or(|s| e.state == s) {
                        n += 1;
                    }
                }
            }
        }
        n
    }

    /// Komşu (8 yön) boş, solid içermeyen hücreler; sabit sırada.
    pub fn free_neighbors(&self, x: i64, y: i64) -> Vec<(i64, i64)> {
        self.ring(x, y, 1).filter(|&(x, y)| !self.blocked(x, y)).collect()
    }

    /// Durumsuz rastgelelik: (seed, tick, entity, salt) → sayı.
    /// Kural değerlendirme sırası sonucu etkilemez; paylaşılan RNG durumu yok.
    pub fn rand(&self, entity: EntityId, salt: u64) -> u64 {
        splitmix64(
            self.seed
                ^ self.tick.wrapping_mul(0x9E37_79B9_7F4A_7C15)
                ^ entity.wrapping_mul(0xBF58_476D_1CE4_E5B9)
                ^ salt.wrapping_mul(0x94D0_49BB_1331_11EB),
        )
    }

    /// 0..100 arası zar.
    pub fn roll(&self, entity: EntityId, salt: u64) -> i64 {
        (self.rand(entity, salt) % 100) as i64
    }

    /// Durumun parmak izi (FNV-1a). Aynı seed + aynı input → aynı hash.
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

/// Bu sayının altındaki kind'larda doğrusal tarama halka aramasından ucuz.
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
