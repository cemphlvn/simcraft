use std::marker::PhantomData;

use serde::{Deserialize, Serialize};

use crate::bus::{Bus, Msg};
use crate::effect::{Event, Group, apply};
use crate::world::{World, WorldSnapshot};

/// Anlık görüntü biçimi. Değişirse artar; eski biçim reddedilir.
pub const SNAPSHOT_FORMAT: u32 = 1;

/// Bir tick sınırında motorun tamamı: dünya, kuyruktaki agent eylemleri, sonuç.
/// Aynı kurallarla geri yüklenince gelecek tick'ler bit bit aynıdır.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub format: u32,
    pub world: WorldSnapshot,
    pub pending: Vec<Group>,
    pub outcome: Option<String>,
}

/// Oyunun kuralları. Motor bunların nereden geldiğini (RON, Rhai, WASM) bilmez.
pub trait Rules {
    /// Yükleme sonrası tüm tutarlılık kontrolleri. Hatalar tek listede döner.
    fn validate(&self, world: &World) -> Result<(), Vec<String>>;
    /// Salt okunur: dünyaya bakar, niyet (Group) üretir.
    fn eval(&self, world: &World) -> Vec<Group>;
    /// Oyun bitti mi? Bittiyse sonuç (ör. "win"). Sonrasında tick işlemez.
    fn outcome(&self, _world: &World) -> Option<String> {
        None
    }
    /// Geri yüklenen bir dünya bu kurallara uyuyor mu (bilinen kind'lar, geçerli durumlar)?
    fn check_world(&self, _world: &World) -> Result<(), Vec<String>> {
        Ok(())
    }
}

// Typestate: motorun yaşam döngüsü compile time'da korunur.
// Engine<Loaded, _>::tick() yoktur → doğrulanmamış kural asla koşmaz.
pub struct Loaded;
pub struct Validated;
pub struct Running;

pub struct Engine<S, R: Rules> {
    world: World,
    rules: R,
    pending: Vec<Group>,
    outcome: Option<String>,
    bus: Bus,
    _state: PhantomData<S>,
}

#[derive(Debug, Serialize)]
pub struct TickReport {
    pub tick: u64,
    pub hash: u64,
    pub events: Vec<Event>,
    pub outcome: Option<String>,
}

impl<S, R: Rules> Engine<S, R> {
    pub fn world(&self) -> &World {
        &self.world
    }
    pub fn rules(&self) -> &R {
        &self.rules
    }
    fn into_state<T>(self) -> Engine<T, R> {
        Engine {
            world: self.world,
            rules: self.rules,
            pending: self.pending,
            outcome: self.outcome,
            bus: self.bus,
            _state: PhantomData,
        }
    }
}

impl<R: Rules> Engine<Loaded, R> {
    pub fn new(world: World, rules: R) -> Self {
        Engine { world, rules, pending: Vec::new(), outcome: None, bus: Bus::default(), _state: PhantomData }
    }

    pub fn validate(self) -> Result<Engine<Validated, R>, Vec<String>> {
        self.rules.validate(&self.world)?;
        Ok(self.into_state())
    }
}

impl<R: Rules> Engine<Validated, R> {
    pub fn start(self) -> Engine<Running, R> {
        let mut e: Engine<Running, R> = self.into_state();
        e.outcome = e.rules.outcome(&e.world);
        e
    }
}

impl<R: Rules> Engine<Running, R> {
    /// Dış dünyadan (agent) gelen niyet; bir sonraki tick'te kurallardan önce uygulanır.
    pub fn queue(&mut self, group: Group) {
        self.pending.push(group);
    }

    pub fn outcome(&self) -> Option<&str> {
        self.outcome.as_deref()
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            format: SNAPSHOT_FORMAT,
            world: self.world.snapshot(),
            pending: self.pending.clone(),
            outcome: self.outcome.clone(),
        }
    }

    /// Motoru bir anlık görüntüye döndürür. Kurallar dünyayı kabul etmezse hiçbir şey değişmez.
    pub fn restore(&mut self, s: Snapshot) -> Result<(), Vec<String>> {
        if s.format != SNAPSHOT_FORMAT {
            return Err(vec![format!("snapshot format {} (this engine reads {SNAPSHOT_FORMAT})", s.format)]);
        }
        let world = World::from_snapshot(s.world).map_err(|e| vec![e])?;
        self.rules.check_world(&world)?;
        self.world = world;
        self.pending = s.pending;
        self.outcome = s.outcome;
        if !self.bus.is_empty() {
            let (tick, hash) = (self.world.tick, self.world.hash());
            self.bus.publish(Msg::Restore { tick, hash, snapshot: Box::new(self.snapshot()) });
        }
        Ok(())
    }

    /// Aboneler burada. Motor her tick'in olaylarını, hash'ini ve sonunu yayınlar;
    /// host (agent katmanı) başlangıcı ve agent eylemlerini.
    pub fn bus(&mut self) -> &mut Bus {
        &mut self.bus
    }

    pub fn tick(&mut self) -> TickReport {
        if self.outcome.is_some() {
            let w = &self.world;
            return TickReport { tick: w.tick, hash: w.hash(), events: Vec::new(), outcome: self.outcome.clone() };
        }
        let mut groups = std::mem::take(&mut self.pending);
        groups.extend(self.rules.eval(&self.world)); // 1) oku
        let events = apply(&mut self.world, groups); // 2) yaz
        self.world.tick += 1;
        self.outcome = self.rules.outcome(&self.world);
        let (tick, hash) = (self.world.tick, self.world.hash());
        if !self.bus.is_empty() {
            for ev in &events {
                self.bus.publish(Msg::Event(ev.clone()));
            }
            self.bus.publish(Msg::Tick { tick, hash });
            if let Some(result) = &self.outcome {
                self.bus.publish(Msg::End { tick, result: result.clone() });
            }
        }
        TickReport { tick, hash, events, outcome: self.outcome.clone() }
    }
}
