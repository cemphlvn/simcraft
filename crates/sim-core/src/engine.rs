use std::marker::PhantomData;

use serde::{Deserialize, Serialize};

use crate::bus::{Bus, Msg};
use crate::effect::{Event, Group, apply};
use crate::world::{World, WorldSnapshot};

/// Snapshot format. Bumped on change; old formats are rejected.
pub const SNAPSHOT_FORMAT: u32 = 1;

/// The whole engine at a tick boundary: world, queued agent actions, outcome.
/// Restored with the same rules, future ticks are bit-for-bit identical.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub format: u32,
    pub world: WorldSnapshot,
    pub pending: Vec<Group>,
    pub outcome: Option<String>,
}

/// The game's rules. The engine does not know where they come from (RON, Rhai, WASM).
pub trait Rules {
    /// All consistency checks after loading. Errors are returned in one list.
    fn validate(&self, world: &World) -> Result<(), Vec<String>>;
    /// Read-only: looks at the world, produces intent (Group).
    fn eval(&self, world: &World) -> Vec<Group>;
    /// Is the game over? If so, the outcome (e.g. "win"). No ticks run after that.
    fn outcome(&self, _world: &World) -> Option<String> {
        None
    }
    /// Physics after the tick's effects are applied (fields: diffusion, pinned levels). Default: none.
    fn physics(&self, _world: &mut World) {}
    /// Does a restored world fit these rules (known kinds, valid states)?
    fn check_world(&self, _world: &World) -> Result<(), Vec<String>> {
        Ok(())
    }
}

// Typestate: the engine lifecycle is enforced at compile time.
// Engine<Loaded, _>::tick() does not exist → unvalidated rules never run.
pub struct Loaded;
pub struct Validated;
pub struct Running;

pub struct Engine<S, R: Rules> {
    world: World,
    rules: R,
    pending: Vec<Group>,
    outcome: Option<String>,
    bus: Bus,
    /// Hash the world after every tick (the default: replays and tests compare every tick). A host that only
    /// needs the hash now and then turns it off and asks `world().hash()` when it wants one; with bus subscribers
    /// it is always computed (every `Tick` message carries it).
    hash_ticks: bool,
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
            hash_ticks: self.hash_ticks,
            _state: PhantomData,
        }
    }
}

impl<R: Rules> Engine<Loaded, R> {
    pub fn new(world: World, rules: R) -> Self {
        Engine { world, rules, pending: Vec::new(), outcome: None, bus: Bus::default(), hash_ticks: true, _state: PhantomData }
    }

    pub fn validate(self) -> Result<Engine<Validated, R>, Vec<String>> {
        self.rules.validate(&self.world)?;
        Ok(self.into_state())
    }
}

impl<R: Rules> Engine<Validated, R> {
    pub fn start(self) -> Engine<Running, R> {
        let mut e: Engine<Running, R> = self.into_state();
        e.world.index_motion();
        e.outcome = e.rules.outcome(&e.world);
        e
    }
}

impl<R: Rules> Engine<Running, R> {
    /// Intent from outside (agent); applied before the rules on the next tick.
    pub fn queue(&mut self, group: Group) {
        self.pending.push(group);
    }

    pub fn outcome(&self) -> Option<&str> {
        self.outcome.as_deref()
    }

    /// Whether every tick hashes the world (see `hash_ticks`). Off: `TickReport::hash` is 0 unless the bus has
    /// subscribers; ask `world().hash()` instead.
    pub fn hash_every_tick(&mut self, on: bool) {
        self.hash_ticks = on;
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot { format: SNAPSHOT_FORMAT, world: self.world.snapshot(), pending: self.pending.clone(), outcome: self.outcome.clone() }
    }

    /// Restores the engine to a snapshot. If the rules reject the world, nothing changes.
    pub fn restore(&mut self, s: Snapshot) -> Result<(), Vec<String>> {
        if s.format != SNAPSHOT_FORMAT {
            return Err(vec![format!("snapshot format {} (this engine reads {SNAPSHOT_FORMAT})", s.format)]);
        }
        let world = World::from_snapshot(s.world).map_err(|e| vec![e])?;
        self.rules.check_world(&world)?;
        self.world = world;
        self.world.index_motion();
        self.pending = s.pending;
        self.outcome = s.outcome;
        if !self.bus.is_empty() {
            let (tick, hash) = (self.world.tick, self.world.hash());
            self.bus.publish(&Msg::Restore { tick, hash, snapshot: Box::new(self.snapshot()) });
        }
        Ok(())
    }

    /// Subscribers live here. The engine publishes each tick's events, hash and end;
    /// the host (agent layer) publishes the start and agent actions.
    pub fn bus(&mut self) -> &mut Bus {
        &mut self.bus
    }

    pub fn tick(&mut self) -> TickReport {
        if self.outcome.is_some() {
            let w = &self.world;
            return TickReport { tick: w.tick, hash: w.hash(), events: Vec::new(), outcome: self.outcome.clone() };
        }
        let mut groups = std::mem::take(&mut self.pending);
        groups.extend(self.rules.eval(&self.world)); // 1) read
        let events = apply(&mut self.world, groups); // 2) write
        self.world.integrate_motion(); // 3) continuous motion (kinds with `motion`)
        self.rules.physics(&mut self.world); // 4) the world's own physics
        self.world.index_motion(); // 5) the broadphase for the next tick's questions (derived, not hashed)
        self.world.tick += 1;
        self.outcome = self.rules.outcome(&self.world);
        let tick = self.world.tick;
        let hash = if self.hash_ticks || !self.bus.is_empty() { self.world.hash() } else { 0 };
        if !self.bus.is_empty() {
            for ev in &events {
                self.bus.publish(&Msg::Event(ev.clone()));
            }
            self.bus.publish(&Msg::Tick { tick, hash });
            if let Some(result) = &self.outcome {
                self.bus.publish(&Msg::End { tick, result: result.clone() });
            }
        }
        TickReport { tick, hash, events, outcome: self.outcome.clone() }
    }
}
