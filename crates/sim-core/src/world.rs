use std::collections::BTreeMap;

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
#[derive(Clone, Debug)]
pub struct World {
    pub seed: u64,
    pub tick: u64,
    pub width: i64,
    pub height: i64,
    pub entities: BTreeMap<EntityId, Entity>,
    next_id: EntityId,
}

impl World {
    pub fn new(seed: u64, width: i64, height: i64) -> Self {
        Self { seed, tick: 0, width, height, entities: BTreeMap::new(), next_id: 1 }
    }

    pub fn spawn(
        &mut self,
        kind: &str,
        state: &str,
        x: i64,
        y: i64,
        props: BTreeMap<String, i64>,
    ) -> EntityId {
        let id = self.next_id;
        self.next_id += 1;
        let (x, y) = self.clamp(x, y);
        let e = Entity { id, kind: kind.into(), state: state.into(), x, y, props };
        self.entities.insert(id, e);
        id
    }

    pub fn clamp(&self, x: i64, y: i64) -> (i64, i64) {
        (x.clamp(0, self.width - 1), y.clamp(0, self.height - 1))
    }

    pub fn count(&self, kind: &str) -> usize {
        self.entities.values().filter(|e| e.kind == kind).count()
    }

    /// En yakın `kind` (Chebyshev mesafesi). Eşitlikte küçük id kazanır.
    pub fn nearest(&self, from: &Entity, kind: &str) -> Option<(&Entity, i64)> {
        let mut best: Option<(&Entity, i64)> = None;
        for e in self.entities.values() {
            if e.id == from.id || e.kind != kind {
                continue;
            }
            let d = (e.x - from.x).abs().max((e.y - from.y).abs());
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((e, d));
            }
        }
        best
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
