use std::marker::PhantomData;

use serde::Serialize;

use crate::effect::{Event, Group, apply};
use crate::world::World;

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
        Engine { world: self.world, rules: self.rules, pending: self.pending, outcome: self.outcome, _state: PhantomData }
    }
}

impl<R: Rules> Engine<Loaded, R> {
    pub fn new(world: World, rules: R) -> Self {
        Engine { world, rules, pending: Vec::new(), outcome: None, _state: PhantomData }
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
        TickReport { tick: self.world.tick, hash: self.world.hash(), events, outcome: self.outcome.clone() }
    }
}
