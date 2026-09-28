use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::world::{EntityId, World};

/// Kuralların dünyaya dair "niyet"i. Dünyayı yalnızca `apply` değiştirir.
#[derive(Clone, Debug)]
pub enum Effect {
    Set { e: EntityId, prop: String, v: i64 },
    Add { e: EntityId, prop: String, d: i64 },
    SetState { e: EntityId, state: String },
    Move { e: EntityId, dx: i64, dy: i64 },
    Spawn { kind: String, state: String, x: i64, y: i64, props: BTreeMap<String, i64> },
    Despawn { e: EntityId },
    Emit { e: EntityId, name: String },
    /// Grup uygulanmadan önce: prop >= min olmalı (canlı durumda). Tutmazsa grup düşer.
    Need { e: EntityId, prop: String, min: i64 },
}

impl Effect {
    fn target(&self) -> Option<EntityId> {
        match self {
            Effect::Set { e, .. }
            | Effect::Add { e, .. }
            | Effect::SetState { e, .. }
            | Effect::Move { e, .. }
            | Effect::Despawn { e }
            | Effect::Emit { e, .. }
            | Effect::Need { e, .. } => Some(*e),
            Effect::Spawn { .. } => None,
        }
    }
}

/// Bir kuralın tek bir ateşlemesi. Atomiktir: dokunduğu entity'lerden biri
/// bu tick'te daha önce yok olduysa grubun tamamı düşer (iki kurt aynı koyunu yiyemez).
#[derive(Clone, Debug)]
pub struct Group {
    pub source: String,
    /// Grubun sahibi. Sahip bu tick'te öldüyse grup sessizce düşer (conflict değil).
    pub actor: Option<EntityId>,
    pub effects: Vec<Effect>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub tick: u64,
    pub source: String,
    pub entity: EntityId,
    pub name: String,
}

/// Tek yazma noktası. Sıra: gruplar geldiği sırayla; ilk `Move` kazanır
/// (agent grupları kurallardan önce geldiği için agent hareketi önceliklidir).
pub fn apply(world: &mut World, groups: Vec<Group>) -> Vec<Event> {
    let mut events = Vec::new();
    let mut moved = BTreeSet::new();

    for g in groups {
        let dead = g.effects.iter().filter_map(Effect::target).find(|&id| world.get(id).is_none());
        if let Some(id) = dead {
            if g.actor == Some(id) {
                continue;
            }
            events.push(Event { tick: world.tick, source: g.source, entity: id, name: "conflict".into() });
            continue;
        }
        let short = g.effects.iter().find_map(|ef| match ef {
            Effect::Need { e, prop, min } => {
                let have = world.get(*e).and_then(|x| x.props.get(prop)).copied().unwrap_or(0);
                (have < *min).then_some(*e)
            }
            _ => None,
        });
        if let Some(id) = short {
            // aynı tick'te daha önce uygulanan bir grup kaynağı tüketti
            events.push(Event { tick: world.tick, source: g.source, entity: id, name: "short".into() });
            continue;
        }

        for ef in g.effects {
            match ef {
                Effect::Set { e, prop, v } => {
                    if let Some(props) = world.props_mut(e) {
                        props.insert(prop, v);
                    }
                }
                Effect::Add { e, prop, d } => {
                    if let Some(props) = world.props_mut(e) {
                        *props.entry(prop).or_insert(0) += d;
                    }
                }
                Effect::SetState { e, state } => world.set_state(e, state),
                Effect::Move { e, dx, dy } => {
                    if moved.insert(e) {
                        world.move_by(e, dx, dy);
                    }
                }
                Effect::Spawn { kind, state, x, y, props } => {
                    let parent = g.actor.unwrap_or(0);
                    if world.spawn(&kind, &state, x, y, props).is_none() {
                        // solid bir kind dolu hücreye doğamadı
                        events.push(Event { tick: world.tick, source: g.source.clone(), entity: parent, name: "blocked".into() });
                    }
                }
                Effect::Despawn { e } => {
                    world.despawn(e);
                }
                Effect::Emit { e, name } => {
                    events.push(Event { tick: world.tick, source: g.source.clone(), entity: e, name });
                }
                Effect::Need { .. } => {}
            }
        }
    }
    events
}
