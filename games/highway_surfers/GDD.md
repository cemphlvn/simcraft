# Highway Surfers: game design document

Status: **v2, the mobile game** (2026-09-29). Owner: Cem (designer). Evidence: `EVALS.md` (how it plays), `FEEL.md`
(how it moves). Engine walls met on the way: `docs/emergence.md` (H.*). History of this document: end of file.

## Pitch

The highway is a river of moving platforms and you are the only one on foot above it. Swipe in any direction to leap
from roof to roof, walk the length of a truck, drop behind onto the car that is catching up, and when a ramp on the
median comes by, take the **BIG JUMP** over the barrier into the oncoming lanes, where everything is twice as fast
and worth three times as much. The drivers do not like you up there.

## Why players come back (the thinking behind every system below)

A mobile game is played in the gaps of a day: on a bus, in a queue, before sleep. People come back to it when it
answers five questions well. Every system in this document serves one of them; a system that serves none is cut.

| Question | What answers it here | Measured by |
|---|---|---|
| **1. Is the next 10 seconds fun with no context?** (a cold open on a bus) | The leap: one swipe, a readable arc, a landing that thuds. Motion that never teleports. Traffic that is a puzzle you read at a glance | `FEEL.md`; first-swipe time; landings a minute |
| **2. Did I die because of me?** (fairness, or players quit) | Every threat is telegraphed: blinkers before a lane change, hazard lights before a driver throws you off, headlights and a horn from behind, ramps that glow. A reaction window of at least 0.6 s, always | `reaction_s` (eval); deaths without a warning = 0 |
| **3. Can I get better?** (mastery, or it is a slot machine) | Skill expression with a ceiling: diagonal leaps, walking a roof to set up a jump, chaining without touching the road, riding the oncoming side, near misses. The same seed is the same highway: a run can be learned | the spread between a good and a poor bot; score over attempts |
| **4. What do I get for coming back?** (progress between sessions) | Coins bank into boards and surfers (small, felt upgrades: airtime, magnet, a second chance); three missions at a time; a **daily highway** (one seed for everyone, one attempt ranked); a streak | missions completed per session; day-2 return (later, real players) |
| **5. Can I show it or beat someone?** (social) | Deterministic replays are tiny (a list of swipes): a friend's best run plays as a **ghost** beside you on the same seed; share a replay as a link. A daily leaderboard that is fair because the road is the same | ghost races started (later) |

Five things are cut on purpose: timers that stop you from playing, energy systems, ads that interrupt a run,
pay-to-win upgrades, and anything that needs a network to have fun.

## Pillars

1. **The leap is the game.** Any direction, one swipe. Anticipation, arc, landing, camera: great on its own.
2. **Read, then commit.** Everything dangerous announces itself first; then it is your read against the road.
3. **The other side is the prize.** The oncoming lanes are the high-risk, high-score half of the road, reached only
   by the BIG JUMP.
4. **Never idle.** Standing on a roof is a choice with a clock: the driver notices you.
5. **Fair and replayable.** Deterministic: one seed, one highway; every run can be watched and shared.

## The road

```
   oncoming ◀        median         ▶ with you
  ┌─────┬─────┬───────────────┬─────┬─────┐
  │  0  │  1  │ ▒▒ barrier ▒▒ │  3  │  4  │
  │  ▼  │  ▼  │   ◢ ramp ◣    │  ▲  │  ▲  │
  └─────┴─────┴───────────────┴─────┴─────┘
```

- Five columns: lanes 0–1 carry **oncoming** traffic (toward you, fast relative to you), the median (2) is a concrete
  barrier, lanes 3–4 flow **with you** (4 is the slow lane). You start on a car in lane 3.
- **Ramps on the median** come by every 15–25 seconds, glowing from far away. A swipe toward the median while a ramp
  is beside you is the **BIG JUMP**: over the barrier, two lanes, high and slow (slow motion at the apex), onto a
  roof on the other side. Without a ramp the barrier stops a sideways swipe (the swipe is refused, with a bump).
- Coming back is the same BIG JUMP the other way.
- The oncoming side scores ×3 per second and every oncoming vehicle you pass close is a near miss. Landing there is
  harder: roofs come at you, so you land on them as they pass under you.

## Traffic (dynamic: the puzzle)

| Behaviour | Telegraph | What it means for you |
|---|---|---|
| Each vehicle has its own cruising speed (lane speed ± a spread) | none needed: you see the gap closing | platforms drift apart and together |
| Car-following: a vehicle brakes to keep its distance | brake lights | your roof slows: the one behind closes in |
| Overtaking: a faster vehicle changes lane when blocked and the next lane is free | **blinker for 1 s**, then a smooth move | your roof may change lane under you (you ride it), a roof may cut into your target lane |
| **The driver notices you**: after 4–6 s on the same roof | **hazard lights** blink for 1 s | then it bucks: you are thrown into the air where you are. Leave before it does |
| Trucks and buses are long | their length | you can walk along them (a platform to set up a jump) |

Vehicles: car (low, short), van (medium), truck (tall, long), bus (tall, longest). Heights as before: a side hop
reaches a van from a car, not a truck; a jump reaches anything.

## Controls (one thumb, any direction)

| Mobile | Desktop (development) | Does |
|---|---|---|
| Swipe ← → | ← → | side leap one lane (BIG JUMP toward the median beside a ramp) |
| Swipe ↑ | ↑ | leap forward (about two car lengths ahead of your roof) |
| Swipe ↓ | ↓ | leap backward (onto the car that is catching up) |
| Swipe diagonally | mouse drag diagonally | leap forward or backward and across at once |
| Tap | space | jump straight up (the roof keeps moving under you) |
| Hold | shift (hold) | crouch: pass under a bridge on a tall roof |
| Hold and slide | w / s (hold) | walk forward or back along the roof |

A swipe in the air is buffered and taken on landing (the game feels like it listened). The mouse drag on desktop is
the same code as the touch swipe, so everything is testable before a phone exists.

## Hazards

| Hazard | Telegraph | Answer |
|---|---|---|
| Asphalt | you are on it | leap onto a roof that passes; traffic from behind (your side) or ahead (oncoming side) hits you |
| Bridge / gantry | seen from far, a shadow on the road | crouch on a tall roof, or be on a low one |
| A bucking driver | hazard lights | leave the roof in time |
| A side too tall | its height | jump instead of hop |
| The barrier | always there | only with a ramp |

Three hits end the run (a short invulnerability after each). The run is 1–3 minutes: traffic grows denser and the
drivers grow grumpier (shorter patience) over time.

## Score, combo, reward

- **Score** = distance × side multiplier (×1 with you, ×3 oncoming) × combo multiplier (+1 per 5 landings without
  touching the road, up to ×5) + near misses + coins.
- **Near miss**: an oncoming vehicle passing within half a lane while you are in the air or on a roof beside it.
- **Coins** float in lines that teach a route (a line across three roofs is a leap chain; a line over the median
  is a ramp coming).
- The combo multiplier is on screen, big, and resets with a crack when you touch the road.

## Game feel (the camera is a character)

| Moment | Camera | Surfer | Target |
|---|---|---|---|
| Ride | spring follow, trails leaps, frames the road from outside lanes | knees bend | calm, fast |
| Leap start | FOV punch, lean into the direction (forward: tilt down; back: pull out) | crouch 60 ms, stretch | response ≤ 1 frame after the swipe |
| Arc | follows on springs, never a cut | tilts into the move | no overshoot |
| Landing | dip scaled by the fall | squash, stretch | a tall-to-low drop hits harder |
| BIG JUMP | pulls back and up, slow motion 0.35 s at the apex, the road below | spins | the moment people share |
| Near miss | FOV kick, whoosh | none | reward the risk |
| Bucked | hazard flash, then a jolt | flung up | "I should have left" |
| Hit | hitstop, shake, red edges | tumbles | clear what hit you |

## Look (greybox until the feel is right, then art)

- Vehicles are **shaped** greybox: body, cabin, wheels, headlights, brake lights and blinkers that light up with
  their state. Oncoming vehicles face you (headlights).
- The median barrier and its ramps are geometry, the ramps lit.
- HUD: score and combo as big numbers, lives, coins. Nothing else on screen.

## Worlds (themes: the same game, three looks; `t` switches)

| | CYBERRUN | BLUE OCEAN | SAVANNA STAMPEDE (bonus) |
|---|---|---|---|
| Where | inside a computer: a circuit-board data highway | a clean Caribbean sea | the savanna at golden hour |
| You | a program avatar on a hover disc | a surfer on a board | an adventurer on a wooden board |
| Low / mid / tall | data packet / transport / server rack | jet ski / speedboat / ferry | zebra / rhino / elephant |
| Median, ramps | firewall, cyan boost pads | coral line, gold wave ramps | dry riverbank, termite mounds |
| Pickup | data crystal | conch shell | sun medal |
| Music | synthwave, 128 bpm | marimba and steel drums, 115 bpm | djembe and kalimba, 124 bpm |

A world is a reward too (meta): the first is free, the others unlock by distance ridden.

## Meta (between runs; after the core loop is fun)

1. Wallet of coins; boards (airtime, magnet, one save) and surfers (look only). Small, felt, never pay-to-win.
2. Missions, three at a time ("land on 5 buses", "ride the oncoming side 10 s", "3 near misses in one leap").
3. Daily highway: one seed for everyone, a ranked first attempt, then free practice.
4. Ghosts: your best run and a friend's play beside you (a replay is a list of swipes, so this is cheap).

## Build order (each step measured, `EVALS.md` / `FEEL.md`)

1. The two-way road: five columns, oncoming traffic, the median barrier, ramps, the BIG JUMP.
2. Leap in any direction: swipes (mouse drag and touch), forward and backward leaps, tap, hold to crouch, walking
   the roof.
3. Dynamic traffic: own speeds, car-following, blinkers and lane changes, the driver who notices you.
4. Shaped vehicles with lights; HUD numbers (score, combo).
5. Scoring: sides, combo, near misses; risk metrics (`reaction_s`, `passive_survival`).
6. Meta and the phone build.

## History

- **v0** (2026-09-29): four lanes one way, cells and ticks, hop / jump / duck. Failed its playtest: bad cars,
  teleporting motion, a static pattern, a replay trap on R.
- **v1**: continuous motion at 60 Hz, physics leaps, traffic only around the player, R restarts. Playtest: riding
  a roof is safe forever (nothing makes you move); the designer asked for two directions, jumps between them, any
  direction, walking on roofs, and a game people come back to. This document.
