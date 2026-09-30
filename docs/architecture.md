# simcraft: architecture

A headless, deterministic simulation engine whose rules are defined outside the engine.
This file is the project's **single source of truth**. If the code contradicts it, either the code gets fixed or this file is updated first.
How the engine got here, change by change, is in [`emergence.md`](emergence.md).

## Product direction (decided 2026-09-28)

A simulation core that game developers adopt from **Unity and Unreal** today, and that a standalone simcraft editor can reuse later.

| Layer | Choice | Job |
|---|---|---|
| Simulation core | Rust (`sim-core`, `sim-state`, `sim-rules`) | Agents, rules, state machines, time, world state |
| Shared boundary | Versioned C API (`sim-ffi`) | Load a game, commands in, results out, snapshots |
| Unity adapter | C# package over the C API | Feels native to Unity developers |
| Unreal adapter | C++ plugin + Blueprints over the C API | Feels native to Unreal developers |
| Standalone (later) | Own editor/renderer | Same core |

- **The rule file is the product, not the language.** One `game.ron` runs unchanged in every host. The boundary loads a whole game; it does not expose engine internals one function at a time.
- Hosts **display**; the core **decides**. Physics, animation and rendering stay in the host.
- Headless, fixed ticks, integer-only, seeded: the same game gives the same hash on every platform (lockstep and replays for free).
- Risk to check before promising: console toolchains (NDA SDKs) and static linking for IL2CPP/iOS.

## Three roles, three files

| Role | File | Decides | Does not touch |
|---|---|---|---|
| **Designer** | `games/<game>/game.ron` | The world: kinds, FSMs, rules, tunable knobs (`params`) | Seed, population, which rules are on |
| **Operator** (the person running the steam engine) | `games/<game>/engine.toml` | How the engine runs: seed, duration, world size, starting population, **switches**, **hyperparameters**, the Rhai safety valve, agent access | The rules themselves |
| **Agent** | stdin/stdout JSON | Which declared action a controllable entity takes | Rules and panel |

The operator can only turn the knobs the designer exposed. If `engine.toml` names a rule, parameter or kind that `game.ron` does not define, the engine **does not start**. Typos are never silently ignored.

## Layers

```
game.ron ──┐                    ┌── sim-agent  (JSON stdio; first client)
engine.toml┴─► sim-rules ──────►├── sim-ffi    (C API) ──► Unity (C#) / Unreal (C++)
              (parse, compile    └── (later) sim-tui
               Rhai, validate)
                │    │ impl Rules
       sim-state◄┘    ▼
   (state charts) sim-core  (World, Effect, apply, Engine<typestate>)
```

| Crate | Contents | Knows the game? |
|---|---|---|
| `sim-core` | `World` (entities + cell grid + per-kind index, mutation only via methods), `Effect`, `Group`, `apply`, `Engine<Loaded→Validated→Running>`, `trait Rules`, hash, snapshot/restore | No |
| `sim-physics` | The physics layer, blind to games and the world: plain data in, a fixed step, plain data out. So far `fixed`: `Fx` (Q48.16 fixed point, `i128` intermediates, exact integer `sqrt`), `Angle` (binary angle, 2^32 a turn, sin/cos from a compile-time integer table), `curve` (piecewise-linear data, e.g. a torque curve). Plan: `docs/plans/physics-and-vehicles.md` | No |
| `sim-state` | State charts: nested states, layers, reusable machines, remember, interrupt/back, pick; memory encoding; selectors; step distances. Guards and actions are generic (`G`, `A`) | No (not even Rhai) |
| `sim-rules` | `GameDef` (RON), `EngineConfig` (TOML), Rhai compilation, dry-run validation, `impl Rules for Game` | Knows the schema, not the content |
| `sim-agent` | The `simcraft-agent` binary, JSON line protocol, ASCII map | No |
| `sim-ffi` | `cdylib` + `staticlib`, C header; the agent protocol behind `extern "C"` | No |
| `sim-gpu` | HD renderer (`wgpu` 29 + `winit` 0.30): a game's side-view `stage.ron` (2.5D quads) or first-person `track.ron` (3D: perspective camera, depth, fog), composed without a GPU (`stage`, `track`, tested) and drawn by `gpu`; camera effects driven by game events (`fx`); `simcraft-play` (window, `--shot`, `--record`, `--bench`), `simcraft-import` | No |
| `sim-kernel` (`kernel/`) | 32-bit integer tensor machine that runs ONNX graphs (brains, rules as graphs); `Graph` builds models in code; `genome`/`mutate` for learning | No |
| `sim-render` | Terminal renderer (cell buffer, diff, frame loop), primitives, 3D voxel view, `view.ron`; `simcraft-view` | No |

## Tick loop

```
tick:
  groups = agent queue              (first: agent movement overrides rule movement)
         + state machine step       (one group per entity: transitions, picks, enter/exit)
         + rules                    (entity id order × rule order in game.ron)
  apply(groups)                     (the single write point)
  tick += 1; hash
```

- **Rules never mutate the world.** They produce a `Group`: the effects of one rule firing.
- **Groups are atomic.** If an entity the group touches already died earlier this tick, the whole group is dropped. If that entity is someone else, a `conflict` event is emitted (two wolves cannot eat the same sheep); if it is the group's own actor, the group is dropped silently. Likewise, if any `Need` in the group no longer holds against the live state, the group is dropped with a `short` event.
- An entity **moves at most once per tick**. The first `Move` wins.
- Evaluation (read-only) runs on `[run] threads` cores (0 = all) once 48 entities have something to evaluate (idle kinds do not count), in jobs of at least 16; results merge in entity-id order, so the core count never changes the outcome. `apply` is single-threaded. Field physics (diffusion and decay, one fused pass) runs level by level on the same cores in large worlds: each voxel reads only the old field, so the result is the same at any core count.
- The world's hash covers every entity and every field voxel. By default it is computed every tick (replays and tests
  compare every tick); a host that does not read it turns it off (`Engine::hash_every_tick(false)`: the agent
  protocol hashes once per `step`, a game window never) and asks `world().hash()` when it needs one. With bus
  subscribers every tick is hashed anyway; fields are hashed a value at a time (not byte-wise FNV), since they are most of a 3D world's state.
- FSM transitions are applied in `apply`. Rules see the old state for the rest of that tick.
- **Solid kinds** occupy their cell: at most one solid per cell. A solid cannot move into, or spawn onto, a cell holding another solid (a blocked spawn emits `blocked`).

## Event bus

Everything that happens is published once, in order, on `Engine::bus()`: `start` (game, seed, source fingerprint, start hash), `act` (every agent request, accepted or refused), `event` (game and engine events), `tick` (tick + hash), `end`, `restore` (with the snapshot, so a log with a "load game" still replays).

- `sim-core` owns only the message type, `Sink` and `Bus` (with an optional name `Filter`); it does no I/O. Sinks live in the host.
- The operator wires outputs in the panel: `[bus] log = "runs/x.jsonl"` (JSONL, flushed every tick) and/or `listen = "127.0.0.1:7878"` (TCP, every client gets every line from when it connects; slow clients are dropped, the simulation never waits).
- **Replay:** `simcraft-agent GAME --replay runs/x.jsonl` re-applies the accepted acts on a fresh engine and checks every tick's hash. A log holds only acts and hashes; determinism regenerates everything else. `same_source` reports whether `game.ron` + `engine.toml` match the recording.
- With no subscribers publishing costs nothing.

## Determinism (non-negotiable)

| Source | Safeguard |
|---|---|
| Iteration order | `BTreeMap`, id order |
| Randomness | No shared RNG: `rand(seed, tick, entity, salt)` via splitmix64. Evaluation order does not affect results. Rhai `rand(n)` adds a per-evaluation call index |
| Salts | A top-level rule's salt is its index + 1 (append new rules at the end). Actions, rules written in states and environment rules are salted by **identity** (name, machine, state path): adding a rule or an action never changes the dice of the others |
| Arithmetic | Rhai `no_float` + `only_i64`; props are `i64` |
| Script side effects | `me`, `p`, `near` etc. are constants. Scripts only return effect maps; `print` is disabled |
| Verification | Test: same panel → identical hash on every tick for 300 ticks |

## Rule language

**A. Declarative (default):** `when` (Rhai expression → bool), then `then: [...]`.

| Action | Meaning |
|---|---|
| `Set(prop, expr)` / `Add(prop, expr)` | Writes the entity's own prop |
| `Emit(name)` | Emits an event (agents see it) |
| `Despawn(Me \| Nearest(kind))` | If there is no target, the rule does not fire |
| `Spawn(kind)` | Spawns at the entity's position from the kind's template |
| `MoveToward(kind)` / `MoveAway(kind)` / `Wander` | One step (8 directions) |
| `Climb(kind, prop)` | One step up a gradient: to the neighbouring cell whose `kind` entity has the highest `prop`, if higher than here. Ties go to the first in a per-entity, per-tick shuffled order. Nothing higher → no move (a later rule may move instead) |
| `Goto(state)` | Changes the entity's FSM state (validated against the kind's states) |
| `Move(dx, dy)` | One step; `dx`, `dy` are expressions (e.g. `arg.dx`). Asked for more than one cell it still takes one, and says so: a `clamped: …` event names the rule (`simcraft-check` reports it) |
| `MoveBy(dx, dy)` | Exactly `dx`, `dy` cells in one tick (a dash, a leap) if the destination is open; nothing between is checked. The entity's one move this tick |
| `On(Me \| It \| Nearest(kind), [...])` | Applies the nested actions to that entity instead of the owner |
| `Need(prop, min)` | Guard: `prop >= min` on the subject. Checked at request/eval time and again at apply time against the live state; if it fails the whole group is dropped (`short`). Prevents double spending under simultaneous moves |

**Compiled rules** (`sim_rules::native`): at load, every expression and script is also parsed into a native form
when it is in the common subset (integer arithmetic, comparisons, `&& || !`, `if`, blocks with `let`, `me.* it.*
sense.* p.*`, `roll`, `tick`, the engine functions, and script lists of `set`/`add` effect maps). Those run without
the interpreter; anything else, and anything unexpected at run time (overflow, a missing prop), is answered by Rhai.
Same order of evaluation and calls (so `rand` sees the same dice); `fast_paths_change_nothing` compares every game
both ways, `SIMCRAFT_NATIVE_CHECK=1` checks every compiled answer against the interpreter.

**B. Rhai script (escape hatch):** the `script:` field returns an array of effect maps:
`#{op: "set"|"add", prop, value}`, `#{op: "emit", name}`, `#{op: "move", dx, dy}`, `#{op: "despawn"}`.

**What expressions can see:** `me.<prop>`, `me.x/y/state/kind/id`, `p.<param>`, `near.<kind>` (Chebyshev distance, 9999 if none), `count.<kind>`, `tick`, `tick_rate` (ticks per second of game time, from `engine.toml`), `roll` (0..99, deterministic per rule and entity).

**World queries (functions):** `around(kind, r)` / `around(kind, state, r)` count entities within Chebyshev radius `r` (self excluded); `nearest_prop(kind, prop, r, default)` reads a prop of the nearest such entity within `r` (else `default`: what you feel of the nest you sit in); `rand(n)` → 0..n-1, deterministic; `pace(n)` / `pace(n, secs)` → true on `n` evenly spaced ticks out of every second (or `secs` seconds) of game time, each entity with its own phase. Write speeds with `pace`, not `rand(100) < speed`: the cadence stays even and the speed per second stays the same at any `tick_rate`. `tick_rate` and `pace` are an agent's own clock, so kinds with `senses` may use them without a sense.

`near.<kind>` is computed only for kinds that some expression mentions as `near.<kind>`.

**Targets:** a rule or action with `target: Nearest(kind)` sees `it` (the target's props plus `it.dist`). With no such entity the rule does not fire.

**Actions** (`actions:` in `game.ron`) have the same shape as rules plus `args: [...]` (`arg.<name>` in expressions). They run only when an agent asks, and switches apply to them too.

**Layout:** `layout: (legend: {char: kind | (kind, {prop: value})}, rows: [...])` places entities and sets the world size; `.` and space are empty. The panel's `[world]` is then optional and must match if present.

**End:** `end: [(when, result)]`: world-level expressions (`count`, `p`, `tick`); the first true one ends the game, and the engine stops ticking.

**Score:** `score: "<expr>"` is evaluated for each controllable entity and summed per seat (or per entity without seats).

**Seats (panel):** `[agent] seats = {alice = 1, bob = 2}` makes the game multi-player. Requests carry `"as": "<seat>"`; an entity belongs to a seat when its `owner` prop equals the seat's number, and controllable kinds must declare `owner`.

**Kinds** in `game.ron`: `glyph`, `props`, optional `fsm`, `solid: bool`, `hidden: bool` (not drawn), `glyphs: {state: char}` for per-state rendering (the deepest matching state wins), and `senses: {name: expr}` (see Perception), `cling: bool` (a crawler: it only enters voxels touching `terrain`, so it walks floors, walls and ceilings and never floats; `Wander` and `Climb` choose among those voxels, `[spawn]` places it on surfaces, and if its support is dug away it falls until it touches terrain again).

## State machines (`fsms:`)

Words come from tools designers already know (Unity Animator, Unreal StateTree / behavior trees), not from statechart theory. A flat machine is still just `(initial, transitions)`; everything below is opt-in.

| You write | What it means | Known as |
|---|---|---|
| `states: {"Awake": (...)}` | States inside states. Being in `Awake.Work.Build` means being in `Build`, `Work` and `Awake` | sub-state machine |
| `layers: {"Life": (...), "Mood": (...)}` | Machines that run side by side in one entity (alphabetical order) | Animator layers |
| `use: "focus"` | Mount another machine here, with its states, transitions, `enter`/`exit` and rules | nested / linked machine |
| `remember: true` | Coming back resumes the child you left | history |
| `pick: First([(state, when), ...])` | On entry, the first option whose `when` holds (else `initial`, else the first option) | selector |
| `pick: Best([(state, score), ...])` | On entry, the highest score; ties go to the earlier option | utility AI |
| `recheck: true` | Re-run `pick` every tick while inside; switch only if a different child wins (`Best`: strictly higher) | reactive selector |
| `enter: [...]`, `exit: [...]` | Actions when the state is entered / left (any `Do`) | OnEnter / OnExit |
| `rules: [...]` inside a state | Rules that live in the state: active while in it or below it; no `for` | StateMachineBehaviour |
| `(from, to, when, then)` | Declared on the state whose children they connect. `from: "*"` = any child (Any State). Paths go deeper: `"Dev.Polish"` | transition |
| `interrupt: true` on a transition, or `Interrupt(state)` | Save where you are inside that level, then go | push |
| `back: true` on a transition (instead of `to`), or `Back` | Return to where the interrupt was saved. Nested interrupts stack (recursion); nothing saved → the level's default entry | pop |

**Order in a tick.** Levels are checked from the outside in (a parent can always interrupt its children, like Any State). At each level the first matching transition wins, then that subtree is done for the tick; if none, `recheck` runs. Each layer takes at most one transition per tick. All changes of one entity form **one group**: `exit` (innermost first), transition `then`, `enter` (outermost first), then the new state. An action whose target is missing is skipped, not the transition. `enter` does not run at birth; a newborn starts in `initial` (or the first `pick` option).

**Changing states** only changes what differs: going from `Work.Build` to `Work.Design` leaves `Work` untouched. A transition to the state you are in re-enters it. Leaving a level drops the interrupts saved inside it.

**Binding rules to states: three ways**

| Binding | How | Example |
|---|---|---|
| Inheritance | `state: "Work"` matches `Work` and anything inside it. `"Awake.Work"` is a path (matched from the end) | a rule on `Work` runs in `Design`, `Build`, `Playtest` |
| Composition | rules written inside a state, or carried by a `use`d machine | `focus` brings `concentrate` to every state that uses it |
| Distance | hierarchy: `depth: N` (active state at most N levels below); transition graph: `steps_to("Shipped")`; space: `around(kind, state, r)`, `near_in(kind, state)`, target `NearestIn(kind, state)` | "one step from shipping → crunch", "a burnt-out colleague within 6 cells" |

**Expressions** see `me.state` as the active states (`Life.Awake.Work|Mood.Tired`) and can call `in_state("Work")`, `in_state(it, "Shipped")`, `depth_in("Work")` (−1 if not inside), `steps_to("Shipped")` / `steps_to(it, "Shipped")` (fewest machine transitions, `9999` if unreachable, 0 if already there), `near_in(kind, state)` (distance to the nearest such entity, `9999` if none).

**Validation.** Names may not contain `. | # ^ = ,` or be `*`. Every compound state needs `initial` or `pick`; transitions name existing children; `use` must exist and must not loop (`a → b → a`: use `interrupt` for recursion); a state with `use` cannot also declare `states`/`layers`/`initial`/`pick`/`transitions`; every `state:` selector, glyph key and `Goto`/`Interrupt` target must match a state of its kind (`Goto` needs exactly one). Machine rule names share the switch namespace.

**In the world** the state stays one string (`sim-core` is untouched): the active states, then `#` remembered children, then `^` saved interrupts, e.g. `Life.Awake.Stuck.Refactor.Warmup|Mood.Tired#Life.Awake.Work=Build^Life.Awake=Work.Build.Flow`. A flat machine's state is still just `Roam`, so existing games keep their hashes. Observers (`states` in `observe`/`step`) see only the active part.

## Perception (`senses`)

By default an agent does **not** see external reality. It senses it, through functions its kind defines:

```ron
"ant": (glyph: 'a', fsm: "ant",
        senses: {
            "warmth": r#"if near.nest <= 1 { env.seasons.warmth / 2 + p.nest_warmth } else { env.seasons.warmth }"#,
        }),
```

- A kind's `senses` are expressions evaluated once per entity per tick, at the start of the tick. They see
  everything: `me`, `p`, `tick`, `count`, `env`, `near` and the spatial queries. Their results are `sense.<name>`.
- Everything else a kind writes (its rules, actions, state machine guards, picks, `enter`/`exit`/`then`) sees `me`,
  `p`, `sense`, `roll`, `rand`, `arg`, `it`, and the spatial queries (`near`, `around`, `near_in`, targets,
  `in_state`, `steps_to`), but **not** `env`, `tick` or `count`. Using them there fails at load, with a hint to add
  a sense. What a kind knows about the world is exactly what its senses say; senses can be local, lagged or noisy.
- Environments' own rules, `end` and `score` see everything: they are the world and the judge.
- `perception: Direct` in `game.ron` turns this off (every expression sees everything). The first five games use it;
  new games get senses.

## Brains (`brain:`): agents that learn

A kind can think instead of (or besides) following rules: a small integer network, run by `sim-kernel`, picks one
of its `outputs` each tick. Every entity has its own weights (its **genome**); young inherit it with mutation. What
is selected is whatever the game rewards (life, young): natural selection, deterministic and replayable.

```ron
"ant": (fsm: "ant",
        senses: { "food_x": r#"toward_x("bush", "Ripe")"#, "home_x": r#"toward_x("nest")"#, ... },
        brain: (inputs: { "food_x": "sense.food_x * 100", "load": "me.load * 100", "noise": "rand(201) - 100" },
                hidden: [8],                                 // hidden layer sizes (default [8])
                outputs: ["north", "east", "south", "west"],
                sense: "choice",                             // default "choice"
                mutation: 40, step: 12,                      // per mille of genes a newborn changes, by ±1..step
                inherit: true)),                             // false = young get fresh random weights (the control)
...
(name: "north", when: r#"sense.choice == "north""#, then: [ Move("0", "-1") ]),
```

- **Inputs** are expressions in the kind's own view (`me`, `p`, `sense`, spatial queries, `rand`), in name order,
  clamped to -127..127; a bias input (64) is added. The network is int8 weights, int32 sums, `Relu`, ÷128, clipped
  back to int8, then `ArgMax` over the outputs (ties → the first).
- **When:** after the kind's senses, at the start of the tick. The chosen output's name is `sense.<sense>` for the
  kind's rules and state machine this tick. A brain needs `perception: Senses`.
- **Genomes:** random (-32..32) at the start, per entity, from the seed. `Spawn(kind)` by an entity of the same kind
  → the parent's genome mutated (`world.rand` of the parent and rule); by anything else → a fresh one. The genome is
  part of the entity (`genome`, one byte per weight): hashed, saved in snapshots, restored. Worlds without brains
  hash exactly as before.
- **Direction queries** (any kind): `toward_x(kind)`, `toward_y(kind)`, `toward_x(kind, state)`, `toward_y(kind, state)`
  → -1, 0 or 1: which way the nearest such entity lies (0 if none).
- Cost: forage's brain (6 inputs, 8 hidden, 4 outputs) is 88 bytes of genome per ant.
- Evidence: `games/forage` (EVALS.md): the same world with `inherit: false` forages at <1 % of the rate.

## 3D worlds and fields (physical environments)

A world has `width × height × depth` voxels; 2D games have depth 1 and behave (and hash) exactly as before.

- **Coordinates:** every entity has `x, y, z` (`z` = 0 is the top level; deeper levels have larger `z`).
  Distance is Chebyshev in 3D (`max(|dx|, |dy|, |dz|)`); neighbours are the 26 around a voxel (8 at depth 1).
- **Layout by levels:** `layout: (legend: {...}, cells: {',': {"soil": 1}}, levels: [ [rows of level 0], ... ])`
  (`rows:` alone is one level). `cells` sets field values at a glyph's voxels without placing an entity.
- **Fields** are numbers per voxel, owned by the world (hashed, snapshotted, replayed):
  `fields: { "temp": (init: 50, diffusion: 20, top: "env.seasons.warmth"), "soil": (init: 0) }`.
  - `from_level: n`: `init` only from level `n` down (deeper); above it the field starts at 0. Ground under air
    without a layout: `"mud": (init: 1, from_level: 14)`.
  - `diffusion`: % of the difference to the average of the 6 face neighbours moved per tick (integers; heat spreads
    through soil). `top`: a world-level expression pinned onto level 0 after diffusion, every tick (the air above
    the ground holds its value at the end of each tick).
  - Physics runs natively after `apply`, every tick, in voxel order: deterministic, no Rhai per voxel.
  - Fields are integers: use fine units (e.g. centidegrees, 0..10000) so that slow flows do not round to zero.
  - `decay`: % of a field's value lost per tick (pheromone evaporation), applied after diffusion.
  - `terrain: "soil"`: a voxel whose `soil` field is non-zero is solid for every mover (dig by setting it to 0).
    A step into terrain **slides** along it: it tries (dx, dy, 0), then (0, 0, dz), then (dx, 0, 0), then (0, dy, 0),
    and takes the first open one. (Only worlds with terrain slide; 2D games have none.)
- **Rule language:** `me.z`, `it.z`; `Move3(dx, dy, dz)`; `MoveToward` / `MoveAway` / `Wander` / `Climb` work in 3D;
  `field("temp")` (at me) and `field_at("temp", dx, dy, dz)` in expressions; actions `SetField(name, expr)`,
  `AddField(name, expr)` at the subject's voxel, `SetFieldAt(name, dx, dy, dz, expr)` exactly at that offset (dig, drop: a
  reach; the game guards how far, e.g. `when: "abs(arg.dx) <= p.reach"`);
  `ClimbField(name)` steps to the open neighbour with the most `name`.
- `around` and `near` count and measure in 3D.

## Continuous motion (`motion:` on a kind)

Cells are fine for herds and colonies; an action game needs things that glide, brake and drift. A kind with
`motion` moves continuously: a position finer than a cell and a velocity, integrated by the engine every tick
(Fiedler's fixed step; the renderer draws between ticks, so nothing teleports). Research and the alternatives
weighed: `docs/research/continuous-time.md`. Kinds without `motion` are untouched (same hashes).

```ron
"car": (glyph: 'c', motion: (size: (620, 1700))),              // footprint: across, along (fine units)
"surfer": (glyph: 'S', motion: (size: (300, 500), gravity: 9)), // falls: height loses 9 fine units/tick² of speed
```

- **Units.** `FINE = 1000` fine units to a cell. Integer throughout (deterministic, no floats).
- **Engine-owned props** of a moving kind (created at spawn, readable and writable like any prop):
  `px`, `py` (its centre, absolute, fine units; starts at its cell's centre), `vx`, `vy` (fine units per tick),
  `ph`, `vh` (height above the ground and its speed: a jump), `mount` (the id of the entity it rides, 0 = none).
  The cell (`x`, `y`) follows: `x = px / FINE` (floored), so every cell-based query still works.
- **Integration**, after `apply`, before fields: every free mover `px += vx`, `py += vy`; in the air
  (`ph > 0` or `vh > 0`) `vh -= gravity`, `ph += vh`, and the ground (0) stops it (`ph = 0`, `vh = 0`). A
  **mounted** entity is carried: it moves by its mount's displacement this tick plus its own velocity, and cannot
  sink below its mount's `top` prop (a roof); if its mount is gone it moves freely. Riders are integrated after
  the free movers, so a rider sees where its mount got to.
- **Flows** (hybrid automata, research §8): a state's motion is its velocity; write it with state rules
  (`Set("vx", ...)` while `Changing`). Discrete moves (`Move`, `MoveBy`) shift `px`/`py` by whole cells.
- **Footprint queries** (fine units; boxes of `size` about `px`, `py`; "ahead" is the way the entity faces, the
  sign of its `vy`, +y when standing):

| Function | Returns |
|---|---|
| `ahead(kind)` / `ahead(kind, dx)` | Gap from my front to the back of the nearest `kind` ahead whose extent across overlaps mine (shifted `dx` across: the next lane), `FAR` (999999) if none |
| `behind(kind)` / `behind(kind, dx)` | The same, behind me |
| `touching(kind)` | How many `kind` footprints overlap mine |
| `under(kind)` | The id of the `kind` whose footprint holds my centre (0 if none) |
| `prop_of(id, prop, default)` | A prop of that entity |

- **`SpawnAt(kind, dx, dy)`** spawns `kind` at my cell offset by (`dx`, `dy`) cells (expressions): traffic
  made ahead of and behind a player instead of a whole road laid out at the start.
- **Broadphase:** footprint queries use a sweep-and-prune index (per kind and column, sorted along the road), built
  after each tick's motion; any change drops it and queries scan until it is rebuilt. Same answers either way
  (`the_broadphase_answers_exactly_what_a_scan_answers`). `ahead_id(kind [, dx])` / `behind_id` give who is there.
  At the start, moving kinds from `[spawn]` take one cell each.
- **Checked:** a moving kind cannot be `solid` or `cling` (it moves by velocity, not by steps), and its game
  cannot declare the engine-owned props itself.

## Vehicles (`vehicle:` on a kind, `track:` in the game)

A moving kind with `vehicle: "stock_car"` is a car, and the physics layer (`sim-physics`) drives it every tick,
straight after the rules (tick step 3, before other motion). Cars are data: `vehicles/<name>.ron` next to or above
the game, else `assets/vehicles/<name>.ron` (`VehicleDef`: mass, geometry, torque curve as dyno points, gears,
brakes, tyres, aero, sub-steps; a missing number with a rule of thumb derives itself). Vehicle worlds use 1 m cells,
so fine units are millimetres.

```ron
kinds: { "car": (motion: (size: (1996, 4912)), vehicle: "stock_car", props: { ... }) },
track: (file: "tracks/charlotte.ron", origin: (680000, 60000), grid: (spacing: 9000, columns: 2, gap: 5000)),
```

- **Intent (props the car reads; rules, actions or the autopilot write them):**
  - `throttle` and `brake`, 0..1000;
  - `steer`, -1000 (right) to 1000 (left);
  - `pilot` 1 hands the car to the engine's autopilot, which drives `line` mm left of the centreline at `pace` ‰
    of its planned limit;
  - `grid` N puts the car in grid slot N on its first tick.
- **State (props the engine writes; a game may read them, never declare them):**
  - `px`, `py`, and `vx`, `vy` in mm per tick;
  - `yaw`, 65536 a turn, counterclockwise from +x;
  - `speed` (mm/s), `g_long` and `g_lat` (mm/s², what the driver feels);
  - `track_s` and `track_off` (mm along the track and left of its centreline);
  - the full-precision physics state in hidden props (`_x`, `_vx`, `_yaw`, ...), so hashes, snapshots and replays
    cover cars without anything new.
- **The model (`sim-physics`, `docs/plans/physics-and-vehicles.md`):**
  - a dynamic bicycle: slip-angle tyres, linear to the limit then sliding, a friction circle shared with drive
    and brakes;
  - load transfer, downforce and banking;
  - the kinematic bicycle at walking pace;
  - 8 sub-steps a tick;
  - aids: traction control and ABS.
- **Tracks (`TrackDef`):**
  - straights and left/right arcs with banking, eased across joins;
  - `Track::pose(s, offset)` and `locate(x, y)` answer where things are;
  - the track must close on itself.
- **Autopilot:**
  - a quasi-steady-state lap plan per (kind, line);
  - pure pursuit steering and feed-forward pedals, with the aids on.
- **Measured:**
  - oracle tests against textbook vehicle dynamics (`sim-physics`);
  - `simcraft-physics-bench` scenes, including `lap`, a stock car on the Charlotte-sized oval against the real
    pole (`games/race/LAPS.md`).
- **Not yet:** contact between cars (they pass through each other), a gearbox and RPM (layer 3), and suspension
  (layer 4).

## Renderer (`sim-render`)

A framework for game interfaces, in this workspace, with its own terminal renderer.

- **Native renderer:** a cell buffer (character, foreground, background in 24-bit colour); each frame is diffed
  against the last and only changed cells are written, inside a synchronized update (no tearing). Terminal I/O via
  `crossterm` only. The frame loop targets a refresh rate independent of the simulation's tick rate.
- **Dimensionality is a view, not the world:** the core is as many dimensions as the game needs (depth 1 = 2D); a
  world view picks a **projection**:
  - `(dim: "2D", level: 0)`: one level, top-down.
  - `(dim: "2.5D", perspectives: [...])`: **layered frames**. Each world level is one layer frame; a perspective
    state stacks them: `(name: "nest", order: [5, 4, 3, 0], focus: 3, step: (2, -1), fade: 45, click: "surface")`.
    `order` is back to front (unlisted levels are not drawn), each layer is shifted by `step` × its place in the stack,
    empty voxels are transparent, layers other than `focus` are faded to `fade` %. Clicking the component (or `p`)
    goes to the `click` state (default: the next one). Without `perspectives` all levels stack top-down.
    **Lazy:** a layer frame is re-rendered only when its level changed (entities, terrain or the tint field on that
    level), invisible layers are never rendered, and an unchanged frame reuses the last composite. The panel title
    reports how many layers were redrawn.
  - `(dim: "3D", yaw, pitch, zoom, cut)`: voxels and entities ray-cast from an orbit camera, with a cutaway to see inside.
  - `(dim: "custom", n: 3, x, y, fixed: [(axis, value)])`: world axes to screen x / y, the rest fixed ("slice" any
    axis: a vertical cross-section is x → screen x, z → screen y, y fixed). Any view may `tint` by a field.
- **Components** (like a frontend component library): every widget is a component, registered by name, configured
  by **props**, styled by the **theme**. Built in: `Title`, `World` (a projection), `Env`, `Inspector` (one entity:
  props, senses, active states), `Series` (sparklines), `Counts`, `Legend`, `Events`, `Help`, `Text`. Your own
  components: implement `Component` in Rust and register it, or compose existing ones in data (`components:` in a
  view). Layout: `Rows` / `Cols` with `Fixed`, `Percent` or `Fill` sizes.
- **Themes** (like CSS tokens and classes): `Theme(colors: {"text": (230, 230, 230), ...}, border: Rounded,
  classes: {"warning": {"text": (240, 190, 90)}})`. Components ask for tokens, never raw colours; a node's `class`
  overrides tokens for that subtree. Built in: `dark`, `light`; a game may bring `theme.ron`.
- **Asset libraries** (like game-dev asset packs): `assets/<pack>.ron` gives each kind its look, separate from its
  rules: glyph and colour per state, and a voxel colour for 3D. Packs are reusable across games; without one, the
  game's `glyphs` and a stable colour per kind are used.
- **Pixel art** (the `Diorama` component): a side-view "ant farm" drawn into an RGBA pixel buffer, shown as true
  pixels through the kitty graphics protocol (Ghostty, kitty, WezTerm; zlib-compressed, re-sent only when the
  picture changed, integer-upscaled so pixels stay crisp) or as half-block characters anywhere else (`--blocks`).
  - Sprites and tiles are data in asset packs: `palette: {'k': (r, g, b)}`, `sprites: {"ant_walk": (fps: 6,
    frames: [[".kk.", ...], ...])}`, or frames that are the pack's images (`"ant_walk": (fps: 8, images:
    ["ant_walk_0", "ant_walk_1"])`, e.g. generated and pixelated; alpha 0 is transparent); a kind's look names a
    sprite per state (`sprite: "ant_carry"`). Every sprite is baked to RGBA frames once when packs are merged
    (later packs replace sprites of the same name), so drawing never looks up a palette; a sprite naming a missing
    image stops the view at load.
  - Parallax backdrop layers are data in the view: `(kind: "hills" | "trees" | "clouds", color, height, detail,
    seed, speed, haze)` or `(kind: "image", image: "hills", height, speed, haze)`; each scrolls at `speed` % of the
    camera (far layers move slower), fades into the horizon and follows the season. `soil: "soil"` textures the ground.
  - Asset packs name images: `images: {"hills": "pixel/hills.png"}` (paths relative to the pack). Any picture becomes
    pixel art with `simcraft-pixelate IN OUT --height N --colors K` (key out magenta, crop, scale, median-cut
    palette); generated sources and prompts are recorded next to them (`assets/src/README.md`).
  - Props: `plane` (the world row cut open), `tile` (pixels per voxel), `sky` (pixels above the ground). The sky and
    backdrop colours follow an environment's props (`season_env`), e.g. warmth and the season state.
  - The camera follows the selected entity; `h` / `l` pan.
- **Views as data:** `games/<name>/view.ron` names a theme and asset packs, defines composite components, and lays
  out components with props, the way `game.ron` defines the game:
  `C(name: "Series", props: {"names": ["nest.food"]}, class: "warning")`. The Rust API is the same in code.
- **In-process:** the renderer links the engine; no JSON per frame. `simcraft-view games/<name>` runs a game with its view.

## Environments (`envs/<name>.ron`)

An environment is the world's own state machine: seasons, weather, a market, a day cycle. It is written like a game
and shared between games.

```ron
Environment(
    name: "seasons",
    params: { "summer": 240, "autumn": 120, "winter": 120 },
    props:  { "ripeness": 0 },
    fsm: "season",
    fsms: { "season": (initial: "Summer", transitions: [ ... ]) },
    rules: [ (name: "ripen", then: [ Set("ripeness", "triangle(tick % 480, 360, 100)") ]) ],
)
```

- A game lists what it uses: `environments: ["seasons"]`. `Game::load` finds `envs/seasons.ron` in the game's folder
  or the nearest ancestor with an `envs/` folder; `Game::from_parts` takes them as text (the C API does not yet).
- It becomes a **hidden singleton entity** of kind `seasons` (its state machine, props and rules are ordinary ones;
  its rules have no `for`). It is placed where the game's layout puts its glyph, else at (0, 0) after the layout.
  Hidden kinds are not drawn. Being an entity, it is in the hash, snapshots and replays.
- **Reading it:** every expression sees `env.<name>.<prop>` and `env.<name>.state` (the active states), a snapshot
  from the start of the tick. Agents can combine it with their own state and the world (`pick: Best` scores,
  `Climb`) into dynamic utilities.
- **Merging:** its params join `p` (the operator tunes them in `engine.toml` as usual), its machines join `fsms`.
  A name clash with the game (kind, param, machine, rule) is an error.
- **Compatibility** is the existing dry run: a game reading `env.seasons.ripness` fails at load with the typo named.
- **Native implementations:** `Game::set_native_env(name, impl NativeEnv)` replaces the environment's own machine
  and rules with a pure function `(tick, params, props, state) → (props, state)`, integer-only. It is correct
  only if `sim_rules::conformance` shows it bit-identical to the `.ron` reference, tick by tick, on the game's
  panel. Rust today; C++ through the C API with the same contract.

**Maths helpers** (integers, deterministic): `clamp(x, lo, hi)`; `pct(x, percent)` = `x * percent / 100`;
`ramp(x, len, peak)` rises from 0 at 0 to `peak` at `len` (clamped); `triangle(x, len, peak)` is 0 at 0, `peak`
at `len / 2`, 0 at `len` (and 0 outside).

## Checking: bugs that show themselves

- **Format** (`rustfmt.toml`: the house style, width 140). `cargo fmt --all` formats everything; the edit hook
  formats each Rust file Claude writes, `ruff` each Python file.
- **`simcraft-check <game>`** (a fraction of a second): loads the game with every panel it has (`engine.toml`,
  `play.toml`), checks its views (every image; every animation channel; every button names a declared action with
  its args, for a kind the game has; piles, meters, bars and the switch read props that exist), then plays 300 ticks
  pressing the views' buttons in turn (or the declared actions), and reports `error` (broken) and `warn` (runs, but
  probably not as meant): error events, `clamped` moves, on-screen buttons the game never takes, a game that ends at
  once. Exit code 1 on errors.
- **Claude hook** (`.claude/settings.json`, `.claude/hooks/check.sh`): after every edit, the file is formatted and,
  for a game's files (or a shared asset pack), `simcraft-check` runs; its findings go straight back to Claude, so a
  broken edit is fixed in the same loop instead of being searched for later.
- Principle: a mistake the engine can notice, it reports where it happened (the rule, the tick, the fix), instead of
  quietly doing something else. New engine features add their check to `simcraft-check`.
- **`tools/check.sh`** (`--quick` skips evals): the whole gate, cheapest first, stops at the first failure: fmt check,
  clippy with warnings as errors, ruff (`ruff.toml`: width 140, bugbear), `cargo test`, `simcraft-check` on every
  game, `eval --check` on every game with `evals/`.

## Efficiency that shows itself

Time is noise; work is not. The rules count what they do, exactly (`Game::work`, `rule_work`): the same numbers on
every machine and at any core count, so they are tested like any other output.

- **Counters:** `evals` (Rhai evaluations: conditions, values, senses, scripts), `queries` (spatial and field
  queries), `maps` (entity maps built for `me` / `it`); per rule, `checks` (entities it was tried on) and `fires`
  (groups it produced).
- **Work profiles** (`test/tests/all/work.rs`, `test/snapshots/work__<game>.snap`): every game's counts per tick and
  per entity-tick over 60 ticks, and its hottest rules. A change that makes a game do more work is a snapshot diff to
  review; an optimization is one too, with its size. `work_counts_do_not_depend_on_the_core_count` keeps them exact.
- **`simcraft-check` cost notes:** the same counts for 300 played ticks, the three hottest rules, rules checked but
  never fired; over budget (25 evals or 10 queries per entity-tick) it warns.
- **Clippy perf lints** (`[workspace.lints.clippy]`, every crate opts in): needless clones and passes by value,
  eager `ok_or`/`unwrap_or` calls, allocations in `to_string`/`format!` loops, boxed collections.
- **Fast paths, each exact** (`fast_paths_change_nothing` runs every game with them off, `Game::set_fast_paths`, and
  compares every tick's hash):
  - a kind's *plan*, computed once: kinds with nothing to evaluate are skipped; `me` and `near` are built only for
    kinds whose expressions mention them (read from the source text, so it can only err towards building);
  - native guards: a `when` (rule or transition) that is `true` or `me.<prop> <op> <number | p.param>` joined by
    `&&` is checked in Rust; a kind whose machine stays put and whose every rule guard is false is skipped before any
    scope is built (colony's ground: 559 cells a tick);
  - the world is lent to queries (`bind_world` returns a guard), never copied: binding costs the same at any size.

## Validation (typestate `Loaded → Validated`)

`Engine<Loaded>` has no `tick` method, so unvalidated rules cannot run: this is enforced at compile time. `validate` returns every error in a single list:

1. RON/TOML schema (`deny_unknown_fields` in both files)
2. Rhai syntax (compiled at load time)
3. Cross-references: switch ↔ rule name, param ↔ `game.ron params`, kinds, FSMs, states
4. **Dry run:** every expression is evaluated once against the template of each kind it applies to. Typos like `me.hungr` are caught here (`fail_on_invalid_map_property`)

Game (runtime) states are **data** (FSM, strings). Engine states are **types** (typestate).

## Agent protocol (`simcraft-agent [GAME_DIR] [--config PANEL.toml]`)

One JSON request per line, one JSON response per line. On startup it prints `{"ok":true,"ready":...,"bus":{...}}`. On failure it prints `{"ok":false,"stage":"load|validate","errors":[...]}` and exits with code 2.

| Request | Response |
|---|---|
| `{"cmd":"info"[,"as":SEAT]}` | Game, kinds (glyph, props, states), controllable kinds, `seats`, `you` (your entity ids), declared `actions`, `score`, effective switches/params, command schema |
| `{"cmd":"observe"[,"as":SEAT]}` | Full map (ASCII), `you`, `scores`, counts, states, entities |
| `{"cmd":"observe","entity":ID}` | `observe_radius` window, `@` = you, visible entities |
| `{"cmd":"field","name":F}` | `tick`, `width`, `height`, `depth`, `values`: the field at every voxel, in voxel order (x fastest, then y, then z). What evals measure structures with (a mound, a trail) |
| `{"cmd":"act"[,"as":SEAT],"actions":[{"entity":ID,"do":"<action>","args":{...}}]}` | Evaluated now, applied on the next `step` before rules. Per-action `results` with `ok` or the reason (`refused: needs …`, `unknown action`, `takes args`, `switched off`, `not controllable`, `not yours`) |
| `{"cmd":"step","n":N}` | tick, done, result (from `end`), `scores`, hash, counts, states (`{kind: {state: n}}`), events (game events plus `conflict`, `short`, `blocked`, `error: …`) |
| `{"cmd":"hash"}` | State fingerprint (replay/verification) |
| `{"cmd":"snapshot"}` | `tick`, `hash`, `game`, `source` (fingerprint of `game.ron` + `engine.toml`), `snapshot`: the whole engine between ticks (world, queued acts, outcome; `format` = 1) |
| `{"cmd":"restore","snapshot":{...}[,"source":S]}` | Back to that moment; the future is bit-identical. Refused (nothing changes) if the format differs, the world is inconsistent, a kind or state is unknown to this game, or `source` does not match. Extra fields are ignored, so a **save file is the `snapshot` reply as-is**: restore = `{"cmd":"restore", …save}` |

## C API (`sim-ffi`, `include/simcraft.h`, ABI 1)

`libsimcraft.{dylib,so,dll,a}`. The host loads a whole game as text; everything else is the agent protocol above, so there is one protocol, not two (`simcraft-agent` and the C API share `sim_agent::Session`).

| Function | Job |
|---|---|
| `simcraft_abi_version()` | Check at startup against `SIMCRAFT_ABI_VERSION` |
| `simcraft_new(game_ron, engine_toml, &err)` / `simcraft_free` | Load (NULL + JSON error on failure) / release |
| `simcraft_request(sim, json)` | Any protocol request → JSON reply |
| `simcraft_step(sim, n)` | Advance without building JSON (stops at `end` / `max_ticks`) |
| `simcraft_entities(sim, out, cap)` | Hot path: `{id, x, y, kind, glyph}` per entity into a flat array, no JSON. `glyph` is the designer's state → look map |
| `simcraft_kind_name(sim, i)` | Kind index → name (owned by the handle) |
| `simcraft_drain(sim)` | Every bus message since the last drain (JSON array) |
| `simcraft_string_free(s)` | Every returned `char*` |

NULL-safe, panic-safe, UTF-8, one handle per thread at a time. `[bus]` sinks in `engine.toml` are not attached through the C API; the host drains instead.

## Testing (`test/`, crate `simtest`)

One place for every test, for game developers and engine developers (research: `docs/research/testing.md`).
Adopted, unopinionated: **insta** (snapshots, `cargo insta review`) and **proptest** (properties with shrinking).
Ours on top: **scenarios**, the scene-runner idea as data.

```
test/
├── scenarios/*.ron     game developers: play a game, expect things (no Rust)
├── snapshots/          accepted snapshots (insta)
├── tests/              engine developers: Rust integration tests (engine, render, scenarios, properties)
└── src/                the library: scenario format, runner, world expressions; `simtest` binary
```

```ron
Scenario(
    name: "a hungry wolf hunts",
    game: "games/wolf_sheep",            // relative to the repository
    seed: 7, params: { "wolf_hunt_at": 3 }, switches: { "predation": true },
    steps: [
        Step(20),
        Expect("count.wolf >= 1 && events.kill >= 1"),
        Until("states.wolf.Hunt > 0", 200),
        Act((kind: "wolf", do: "move", args: { "dx": 1, "dy": 0 })),
        Refused((kind: "wolf", do: "fly"), "unknown action"),
        Hash("ee9a66d10246d6f9"),
        Snapshot("after the hunt"),
        SaveLoad(50),
    ],
)
```

| Step | Meaning |
|---|---|
| `Step(n)` | Advance `n` ticks (stops at the end) |
| `Until(expr, max)` | Advance until `expr` holds; fails after `max` ticks |
| `Expect(expr)` | `expr` must hold now |
| `Act(act)` / `Refused(act, text)` | A player action on the first entity matching `id` or `kind` (and `as` a seat); `Refused` must fail with `text` in the reason |
| `Hash(hex)` | The world hash now (a golden run) |
| `Snapshot(name)` | A readable summary of the world (counts, states, events, singletons' props) as an insta snapshot |
| `SaveLoad(n)` | Snapshot, run `n` ticks, restore, run `n` again: both futures must be identical |
| `Probe(expr)` | Print the value (while writing a scenario) |

`expect_error: "text"` instead of steps: the game must fail to load with `text` in an error (validation tests).

**Expressions** are Rhai over the world: `tick`, `p.<param>`, `count.<kind>`, `states.<kind>.<state>` (leaf state
name, e.g. `states.ant.Carry`), `sum.<kind>.<prop>`, `events.<name>` (since the start), `env.<name>.<prop>`,
`done`, `result`.

Run: `cargo test -p simtest` (everything), `cargo run -p simtest -- test/scenarios/wolf_sheep.ron` (one file),
`cargo insta review` (accept snapshot changes).

## Presentation: capabilities and game feel

Between the simulation and the pixels sit two layers. Neither changes the simulation (it stays deterministic);
both only decide how it is shown.

**Capabilities** (`sim_render::caps`): what this machine can do, detected once at start.

| Probe | Values | Used for |
|---|---|---|
| Window + GPU | Metal / Vulkan / DX12 / WebGPU / WebGL2 / none | the GPU renderer (`sim-gpu`) when there is one |
| Display refresh | Hz of the monitor | frame pacing (vsync) |
| Terminal graphics | kitty protocol, true colour, half-blocks | the terminal renderer's path |
| Cell size | pixels per terminal cell | square pixels in the terminal |

`simcraft-play` opens a GPU window when it can and falls back to the terminal; the title says which path was
chosen and why. `--terminal` / `--window` force one.

**Tick rate** (`[run] tick_rate` in `engine.toml`, default 10): the operator's hyperparameter for ticks per second
of game time. Viewers play at `tick_rate` ticks/s at 1x (`--speed` overrides it; the title shows `N ticks/s = Kx`);
rules see it as `tick_rate` and `pace`. A game written with `pace` and per-second quantities keeps its speeds at any
tick rate; one that counts raw ticks runs faster when the rate rises. Pick per game type (`docs/research/kernel.md`):
ecosystems 10–20, RTS 10–30, action 60, management 1–5.

**Game feel** (`sim_render::feel`): the simulation ticks at a fixed rate; frames are drawn at the display's rate and
**interpolate** between the last two ticks, so an ant glides from cell to cell instead of jumping ("fix your
timestep"). A view declares its feel as data:

```ron
feel: (
    interpolate: true,                          // positions between ticks
    camera: (stiffness: 60, damping: 1.0),      // critically damped spring follow (damping 1 = no overshoot)
    walk_bob: 1,                                // pixels a walking sprite bobs
),
```

**Animation** (`sim_render::anim`): animation is data, sampled as a pure function of the time since it started,
so any frame can be computed (and tested) alone, at any frame rate, and the simulation never sees it. An `Anim` is a
set of tracks over the channels `scale`, `x`, `y` (in the thing's own size), `rot` (degrees), `alpha`, `bright`;
each track is keys `(time, value)` or `(time, value, ease)` with easing `linear step quad_in quad_out quad_in_out
cubic_out sine_in_out back_out elastic_out bounce_out`; `loop: true` repeats. Poses stack (`then`: scales and
alphas multiply, offsets and rotations add), which is how a hover sits on an idle loop and a press on both; a
`Player` runs one-shots over a looping base; `unknown_channels` catches typos at load.

```ron
"press": (tracks: { "scale": [(0.0, 1.0), (0.07, 0.84, quad_out), (0.3, 1.06, back_out), (0.42, 1.0)] }),
"idle":  (tracks: { "rot": [(0.0, -3.0), (1.6, 3.0, sine_in_out), (3.2, -3.0, sine_in_out)] }, loop: true),
```

Feel is a component library like the rest of the renderer: new feel components (easing, tweens, screen shake,
hit-stop, particles) are added there, used by every backend, and switched on per view.

Feel is developed **eval-driven**, like rules: `simcraft-feel <game> --view <view>` plays the game headless at a
fixed frame rate with the viewer's own code and measures camera jumps, jerk and lag, sprite steps and stutter;
`--set` tries a setting, `--save` records a step (`games/<name>/feel-evals/`, log in `games/<name>/FEEL.md`).

## Kernel: ONNX graphs (`kernel/`, crate `sim-kernel`)

A **32-bit integer tensor machine** that runs standard ONNX files. Its job: brains that learn, and (next) rules
compiled to graphs, at a cost a phone can carry for thousands of agents.

- **One model per kind, one row per entity.** Every entity of a kind is a row of one batch; the weights are shared.
  A row never depends on the rest of its batch.
- **Integer only, bit-exact.** Values are `i32`, wrapped to their ONNX type (int8, uint8, int16, uint16, int32,
  int64 within 32 bits, bool) after every op, like WGSL, so a CPU, wasm and a future GPU backend agree bit for bit.
  Division truncates; `x / 0 = x` (WGSL's rule). Float tensors, `QuantizeLinear`/`DequantizeLinear`/`QLinear*`
  (float scales), external weights and unknown ops are refused at load with the reason. A quantised model rescales
  with `Div` by a constant.
- **Operators:** Add Sub Mul Div Min Max Sum And Or Xor Not Equal Less Greater LessOrEqual GreaterOrEqual BitShift
  Identity Neg Abs Sign Relu Clip Cast Where MatMul MatMulInteger Gemm (alpha = beta = 1) ArgMax ArgMin ReduceSum
  ReduceMax ReduceMin Reshape Flatten Squeeze Unsqueeze Concat Gather Transpose Constant.
- **Budget:** a model over `Budget::model_bytes` (1 MB) does not load. `footprint` reports shared weight bytes,
  bytes per row (one agent) and genome bytes.
- **Learning:** `genome()` = the model's int8 weights in file order (one byte per weight); `run_genome` runs an
  agent's own genome; `mutate(genome, seed, per_mille, step)` makes a deterministic child. Offline training
  (PyTorch → integer-only ONNX) and in-game evolution use the same files.
- **Ecosystem:** `Graph` writes models in code; the files pass `onnx.checker` (full check) and onnxruntime returns
  the same bits (verified on a 16→32→4 int8 policy, 1000 agents).
- Games use it through a kind's `brain` (see Brains).

## HD stage (`games/<name>/stage.ron`, `sim-gpu`)

The GPU face of a game: high-resolution art (natural-history illustration in `games/colony3d`), no pixelation. The
terminal renderer stays the developer's view; the stage is what players see. Same world, same determinism: the
stage only reads.

**Space.** x along the world (1 = a cell), y down (0 = the ground line, 1 = one level underground), and **depth**
into the screen (1 = the cut plane). A point at depth d moves 1/d as fast as the camera and is drawn 1/d as big, so
**parallax is perspective**, not a per-layer speed. World rows behind the cut stand `row_depth` deeper each, so the
surface reads as a strip of land: ants on far rows are smaller and hazier.

```ron
Stage(
    assets: ["nature_hd"], cells_across: 8.0, horizon: 0.6, plane: 9, row_depth: 0.05,
    season_env: "seasons", follow: "ant", sky: "sky",
    layers: [ (image: "mountains", depth: 30.0, height: 50.0, haze: 35, blur: 1.0),
              (image: "grass_front", depth: 0.55, height: 0.5, lift: -0.12, front: true, blur: 1.6) ],
    soil: (image: "soil", scale: 5.0, hollow: "hollow", darken: 55),
    kinds: { "ant": (frames: ["ant", "ant_b"], height: 0.42, bob: 0.015, fps: 7.0,
                     states: { "Carry": ["ant_carry"], "Dormant": ["ant_dormant"] }) },
    season_cards: { "Autumn": "card_autumn" }, vignette: 35,
)
```

- **Layers** stand on the ground line (vertical position rides with the ground, so looking down into the nest never
  sinks the mountains behind the soil); `height` and `lift` are world units at the layer's depth; `haze` blends
  towards the horizon colour, `blur` is a mip bias (depth of field for free), `front` layers are drawn over the
  sprites and fade out as the camera goes underground. Bands repeat along x only (`--tile` at import makes them
  seamless; `mirror: true` for art that is not).
- **Soil** is one repeating texture below the ground line, darker with depth; **hollows** (voxels of the cut plane
  without terrain) are soft dark blobs, stretched over the joint to hollow neighbours, so they read as galleries.
- **Kinds**: frames per state (the deepest matching selector, as in asset packs), height in world units, a walk
  cycle that runs only while the entity moves, a bob, a contact shadow; facing follows the last step.
- **Seasons**: a colour grade per season (multiply + desaturation), the season's sky gradient behind the painted sky,
  snow in winter; `season_cards` fade in over the scene when the season changes (a transition screen).
- **Camera shots** (`c` cycles; springs, so a cut is a move): *close* on the followed entity (surface, or down the
  cut), *wide* (the whole world), *nest* (down at the chambers). The followed entity is one the stage shows.
- **Draw**: one pipeline, instanced quads (a 4-vertex strip each), premultiplied alpha, sRGB textures with mip
  chains, linear light; consecutive quads with one texture and wrap mode are one draw call. Quad colours are sRGB.
- **Import** (`simcraft-import IN OUT [--max N] [--crop | --bottom] [--white] [--tile PCT]`): the texture's import
  settings, like an engine's: trim, cap the size (area average, alpha-weighted), key a white ground softly (scenery a
  background remover would erase), make a band seamless. Sources and prompts: `assets/src/README.md`.
- **Piles** (quantities made visible): `piles: [(kind: "nest", prop: "food", item: "berry", per: 1, max: 60,
  size: 0.14, offset: (0.75, 0.0), spoil: (prop: "spoiled", item: "berry_spoiled"))]`. A prop becomes a heap of
  items standing at the entity (centre first, rows rising into the gaps). The renderer remembers items between
  frames (`PileMemory`): new units drop in on top (`enter`: a bounce and a pop, staggered so a delivery pours in),
  missing ones leave from the top (`leave`: lift and shrink), and the bottom `spoil` units show the spoiled item.
  What was there at the first frame does not pop. `anims` overrides `enter`, `leave`, `idle` (defaults in code).
- **Buttons** (what the player can do): `buttons: [(action: "lay_egg", on: "nest", icon: "btn_egg", key: "1",
  at: (0.07, 0.87), size: 0.11, args: {}, anims: {...})]`. A button is a declared game action (`game.ron`
  `actions`) for the first entity of kind `on`; every frame it asks `Game::act` whether the game would take it now
  (the same checks as any agent) and looks lit or greyed accordingly, easing between the two. Mouse (hit by circle,
  hover) or its key presses it: the action is queued for the next tick and the button plays `press`, or the game
  refuses and it plays `denied` (the reason shows in the window title). A press always shows at full colour.
  Animations `idle` (loops), `hover` (held), `press`, `denied` have defaults and are overridden per button.
- **Player panel**: `simcraft-play` runs `games/<name>/play.toml` when it exists (the player's settings: which kinds
  are controllable, gameplay levers such as spoilage), else `engine.toml`; `--panel` picks another. Evals and tests
  keep `engine.toml`.
- **Record** (`--shot out.png --record N [--press ACTION@FRAME]...`): N frames at 30 fps from the shot state, with
  presses on given frames: a clip for review (`ffmpeg -i out_%03d.png`), and a way to judge animation, not frames.
- **Measured** (`simcraft-play games/colony3d --bench 600`, 1600x900, M-series Mac): sim 0.18–0.48 ms, compose
  0.02 ms, GPU 2–2.6 ms per frame (with a full CPU wait per frame, so an upper bound); up to 244 quads; textures
  43 MB with mips (11 MB if block-compressed).

## First person in a voxel world (`games/<name>/roam.ron`, `sim-gpu`)

You are one entity of the game (game 9, `games/mound`): WASD walks, the mouse looks (click captures it, esc lets it
go), shift runs, space jumps, walking into a wall climbs it, left click drops, right click digs, a held key shows an
invisible field (the pheromone).

- **Who owns what.** The body moves continuously in the view (`sim_gpu::walker`: velocity eases toward what the keys
  ask, the view eases after the mouse, the eye glides after the body, gravity, collision with terrain voxels,
  climbing). The game follows it one voxel a tick through a declared action (`crawl(dx, dy, dz)`); digging and
  dropping are declared actions on the voxel the crosshair ray meets (`dig`/`drop(dx, dy, dz)`, relative to your
  entity), checked by the game like any agent's, so the game stays the one truth: deterministic, replayable.
- **`roam.ron`:** `you` (kind), `actions` (crawl, dig, drop, `reach`, the `carrying` prop), `materials` (terrain value
  → image, texture scale, tint), `sky`, `fog`, `far`, `kinds` (crawler bodies: colours, size, the state or prop that
  shows a ball in the mandibles), `smell` (field, colour, full strength, radius), `feel` (`WalkFeel`: eye height,
  body size, walk/run speeds, `accel`/`air` ease rates, climb, jump, gravity, mouse sensitivity, `look_smooth`,
  bob, fov and run fov, lean, landing dip, `eye_glide`), `keys`.
- **Drawing:** terrain as `surface: Blocks` (faces open to air, per-corner ambient occlusion) or `surface: Smooth`
  (surface nets through the same voxels: flat ground exactly on the voxel faces, edges and corners rounded by
  relaxation, smooth normals, ambient occlusion from a prefix sum of open samples; beside the world, the untouched
  ground continues up to its first height, so a pit at the edge has an outer wall); crawlers stand on the drawn
  surface (its vertex and normal nearest their voxel's face: they tilt over rounded edges); `lens: (aperture, haze,
  shadow)`: a macro lens (depth of field from a golden-angle disk blur by each pixel's distance, autofocus on what
  the crosshair looks at, a placed camera on its target), sun shadows (a 2048² map over 22 voxels around where you
  look; terrain and models cast and receive), and haze brighter towards the sun; crawlers from segments and six stepping legs, oriented on the face they cling to; your mandibles
  and the ball you hold in view; the targeted face glows (red after a refusal); distance fog; the painted sky turns
  with the view; the ground goes on beyond the world's edge.
- **Cost:** the terrain is kept on the GPU (`Gpu::keep`) and uploaded only when it changes; fog is computed on the
  GPU from the distance to the eye; bodies behind you or lost in the fog are not built, far ones are coarse and
  legless; bodies, the terrain mesher and the colour conversion run on all cores. `simcraft-play <game> --bench N`
  reports where a frame goes and the worst frame after warm-up.
- **Frame drops, found and rebuilt** (`sim_gpu::perf`). Every frame records its phases (ticks, body, terrain
  rebuild, frame build, draw and present, the window's own work) and exact counts (ticks run, vertices, bodies,
  whether the terrain was rebuilt). In the window, a frame over `--budget` (ms, default 20) writes a spike report to
  `runs/spikes/`: the tick, the world's hash, the camera, the phases, and the log of every action since the start;
  the percentiles and the spikes print when the window closes.
  - `--repro FILE`: the same game, panel and seed, the log replayed tick by tick, the hash checked (a different
    state is refused, not profiled); the frame rendered again cold, warm, and with the terrain rebuilt on a warm
    GPU; the ticks before it timed again.
  - `--sweep N`: the scripted player for N frames, twice, headless. A spike on the same frame with the same hash in
    both runs is the game's or the engine's (reports written); one seen once came from outside (OS, driver).
  - `--stress N`: each load the view has, pushed through four levels, N frames each: every drawn kind (×1, ×4, ×8,
    ×16 its spawn count), the terrain (10–100% of columns built), the shown field (5–100% of the air). The table
    gives tick and frame percentiles, vertices and the worst phase, and where p95 first breaks the budget.
- **Measured:** `simcraft-play <game> --feel` runs the real controller through a scripted walk, turn, hop and climb
  and prints the feel metrics (`games/<name>/FEEL.md`); `--shot` after `--ticks` stands you facing the tallest
  thing built (`--camera wide`: from above). `simcraft-check` checks the view against the game (your kind is
  controllable, the three actions take `dx`, `dy`, `dz`, drawn kinds, the smell field, the carried prop).

## Models: from modelling tools to the game (`sim_gpu::model`, `sim_gpu::skin`)

A kind can be drawn with a model made in the best tools and bound to the game's own state machine. The format
between tools is glTF 2.0 (`.glb`): Blender, Unity, Unreal and Higgsfield's image-to-3D generators all write it.
Research and choices: `docs/research/animation.md`.

- **Pipeline (the termite):** a reference image (Higgsfield) → a textured PBR mesh (Higgsfield image-to-3D: Tripo,
  Meshy) → rigged and animated headless in Blender (`tools/blender/rig_insect.py`: anatomy read from the geometry,
  bones with the contract's names, a `carry` socket, clips `walk`/`carry`/`dig`/`idle`, weights by distance to the
  bones, three levels of detail) → `assets/models/*.glb`. Higgsfield rigs humanoids only; insects are rigged by us.
  `tools/blender/inspect.py` renders any `.glb` from three sides and its clips frame by frame.
- **Import:** `Model::load` reads meshes, PBR materials (base colour, normal, metal-roughness, occlusion), skins,
  node hierarchy and clips (translation, rotation, scale; linear, step, cubic), and samples every clip once at
  30 fps into palettes (a matrix per node and per skin joint). Conventions are glTF's: +Y up, +Z forward, feet at
  y = 0.
- **Drawing crowds:** each model is uploaded once (mesh, mipmapped textures, all palettes in one storage buffer);
  each character is one 96-byte instance (transform, two frames and their blend, tint). The vertex shader skins;
  the fragment shader lights with the PBR maps (normal maps from screen-space derivatives, GGX specular, sky and
  ground ambient, wrap lighting and a transmitted back-light for thin bodies). One instanced draw per model part.
  Needs storage buffers (Metal, Vulkan, DX12); without them (WebGL2) models are skipped.
- **The contract** (`roam.ron`, per kind):
  `model: (file, lods: [(from_distance, file)], length, clips: [(state_selector, clip)], still, rate, carry, textures)`.
  The game's state picks the clip (first match; `*` = any), `still` plays when it has not moved for half a second,
  every entity has its own clock, the `carry` socket holds a carried ball (`carries_in`: a state or a prop),
  `textures` re-skins a material with an image from the asset packs (e.g. a Higgsfield texture).
- **Checked:** `simcraft-check` refuses a clip state the kind's machine does not have, a clip or socket a file (at any
  level of detail) does not have, a material to re-skin that does not exist. `simcraft-model FILE --game G --kind K`
  prints what the engine sees, warns about convention mistakes (forward axis, feet, units) and writes the contract.
- **Measured** (`--stress`, 1280x720): 960 model termites (12k/3k/800 triangles at 0–4/4–10/10+ voxels), p95
  9.6 ms, worst 11.7 ms, 2.9M vertices a frame; the distances were chosen by probing 7/18, 4/10 and 3/7.

## First person: tracks (`games/<name>/track.ron`, `sim-gpu`)

A game whose world is lanes (x) and a road ahead (y) can be played from inside: the camera rides with the followed
entity. Same simulation, same rules; the track only reads. `simcraft-play` picks it when the game has a `track.ron`.

**Space.** X across the road (0 = its middle), Y up (0 = the road), Z forward (row + 0.5). A real 3D pass: perspective
camera (`math::Eye`: position, target, roll, field of view), depth buffer, perspective-correct textures, distance fog
towards the horizon colour. Around it, 2D passes: sky and skyline behind, hood, effects and HUD in front.

```ron
Track(
    assets: ["road"], follow: "car", sky: "sky", skyline: (image: "mesas", height: 0.2, drift: 0.4),
    road: (image: "asphalt", lanes: 4, repeat: 2.5, shoulder: "sand", shoulder_width: 45.0),
    camera: (height: 0.62, back: 0.9, fov: 68.0, pitch: 4.0, rumble: 0.006, bank: 0.9, max_bank: 9.0),
    views: [ (name: "chase", height: 1.35, back: 3.2, fov: 62.0, pitch: 10.0, body: ("car_rear", 0.75)) ],
    view: 70.0, fog: ((236, 150, 118), 18.0, 68.0),
    kinds: { "cone": (frames: ["cone"], height: 0.62), "oil": (frames: ["oil"], height: 0.95, flat: true),
             "coin": (frames: ["coin"], height: 0.42, lift: 0.25, spin: 0.8, bob: 0.06) },
    scenery: [ (images: ["cactus", "rock"], height: (1.4, 3.2), every: 3.0, side: (2.0, 14.0), seed: 2) ],
    hood: (image: "hood", height: 0.3, follow: 0.6),
    switch: (...), fx: {}, buttons: [...], meters: [...], bars: [...],
)
```

- **Road**: a textured strip (`repeat` world units per tile), dashed lane lines and solid edges (`paint`), shoulders
  and ground to the horizon. **Kinds**: standing props face the camera (height; width from the picture), `flat` ones
  lie on the road (oil, pads), `lift`/`bob`/`spin` for pickups, soft contact shadows; frames per state as elsewhere.
  **Scenery**: roadside decoration placed deterministically along the road (not in the rules): speed you can see.
- **Views** (`c` cycles; the main `camera` first): height, distance behind, field of view, pitch, hood on/off, and a
  `body` image for the followed entity seen from outside. The camera travels between views on springs.
- **Switch** (moving between positions: the core mechanic of games like `lanes`). The game says *where*: a prop of
  the followed entity (`prop`, e.g. `lane_to`) set by the player's action; the simulation moves there cell by cell,
  so what is on the way still counts. The track says *how it feels*: `positions` (world X of each, may be uneven),
  duration `base + per_lane × lanes crossed`, the `ease` curve (`back_out` overshoots and settles), a `hop` of the
  camera per lane, and `start` / `arrive` effects scaled by the lanes crossed (a 1 → 4 swing punches three times a
  2 → 3 step). A new switch starts from wherever the camera is (fast inputs stay smooth); between switches the camera
  holds the heading. The bank follows the curve's sideways speed, clamped to `max_bank`, on a spring.
- **Camera effects** (`fx`, built in): a game event of the followed entity starts an animation over camera channels
  `fov shake lift roll pitch streaks flash vignette`; running effects add up. Defaults: `dashed` (field-of-view
  punch, speed lines, jolt, flash), `jumped` (an arc), `landed` (a dip and a jolt), `crashed` (hard shake, flash,
  pinch), `braked`, `skidded`, soft flashes for pickups. `fx: { "dashed": (tracks: {...}) }` replaces one; unknown
  channels are refused at load.
- **HUD**: `buttons` as on the stage (declared actions, `Game::act` decides lit or grey; several buttons may share a
  key, the live one is pressed: keys `1`–`4`, `space`, `enter`...; buttons in one spot show as one), `meters` (a prop
  as a row of icons), `bars` (a prop as a bar, pulsing below `warn`).
- **Record** as for stages (`--shot out.png --record N --press KEY@FRAME`, `--view N`); effects of warm-up ticks are
  dropped before the first frame.
- **Replay**: every accepted press is kept as (tick, action, args); the simulation is deterministic, so the presses
  from the same start are the run. A finished run is saved to `runs/<game>-<unix time>.jsonl`. In the window `R`
  or `N` (or any game key once the run is over) starts a new run; `V` watches the run just ended again (red edges
  and a pulsing dot on screen; any game key leaves it). Nothing replays unless asked (`TrackPlay::intent`, tested); `--replay FILE` plays a saved run in a window
  or into `--shot` / `--record` (a video of a real run). A replay ignores the player's keys.
- **Gone things** (`kinds.X.gone`): when an entity of that kind vanishes near the camera (smashed, picked up), it
  keeps playing an animation where it was: `x` (sideways, away from the followed entity), `y`, `rot`, `scale`,
  `alpha`, carried along at `push` × the followed entity's speed (a smashed cone flies ahead and tumbles off).
- **Meters animate**: a new icon pops in, a lost one bursts off (grows, spins, fades) while the rest shake.
  `blink: "Wobbly"` blinks the followed entity's body while it is in that state (invulnerable after a hit). The
  `hurt` effect channel reds the edges of the screen.
- **Moving kinds** (`motion`) are drawn from their fine positions between ticks (`Tween::place`): they glide at
  their velocity. A moving followed entity is drawn where the simulation has it, across and in height (its arcs
  are physics); `switch` then only plays its effects.
- **Feel, measured** (`--feel --replay RUN`): the run replayed headless at 60 frames a second through this code;
  `stall` (frames a moving thing near the camera did not move), `step_ratio` (largest step over average step),
  `eye_jerk`, `rider_jerk`, tick and frame cost (`sim_gpu::track::feel_probe`, `games/highway_surfers/FEEL.md`).
- **Themes** (`themes: [(name, assets, sky, skyline, road, fog, kinds, rider, scenery, music)]`): the same game in
  another world; a theme names only what it changes (`Track::themed`). `t` switches in the window, `--theme NAME`
  picks one for shots and feel runs; every theme's packs load at the start and every picture is checked then.
  **Music** (`music`: an MP3 next to the game or under `assets/`) loops with `rodio`; `m` mutes; no sound device, no
  error. A kind can be a **picture over an invisible block** (`block: (.., invisible: true)`: the rider stands on the
  block, the player sees the picture), facing the way it drives (`oncoming` frames when its `vy` is below 0), drawn
  `face` in front of its middle. The rider can be a picture too (`rider.image`).
- **Road**: `median: (lane, width, height, color)` makes one lane a barrier between two directions (solid lines both
  sides); blocks can be `wedge` (a ramp) and `pulse` (a glow).
- **Swipes** (`swipes: { "left": [(action, args), ...], ..., "tap": [...] }`): eight directions and a tap, each a
  list of moves tried in order (the first the game takes is made, as a key tries its buttons). A mouse drag is the
  same code as a finger (`sim_gpu::track::swipe_dir`). Buttons can be `hidden` (keys only) and have a `release`
  action (hold to crouch, hold to walk).
- **From outside** (`rider`, third person; `highway_surfers`): the followed entity drawn as a stack of greybox
  `parts`, standing on the top of whatever `block` is under it (drawn positions, between ticks). In an `air` state
  (selector → apex) it flies an arc from where it took off to what is under it, lasting as long as its `timer` prop
  counts down, so it lands on the tick the game does; when the ground drops away it falls with `gravity`. `poses`
  scale it by state (a crouch), fx channels `squash` and `tilt` act on it, and in its `ground` states it stands on
  the road whatever is in its cell: the picture follows the rules. Blocks between it and the camera fade to ghosts.
- **Blocks** (`kinds.X.block: (size, color, lift, surface)`): a kind drawn as a shaded box instead of a picture
  (greybox before assets); its top is a surface unless `surface: false` (a bridge deck). `lift_by: (prop, k)` raises
  a standing picture by a prop (a coin at height `h`).
- **Chase camera**: `follow_height` (how much of the rider's height the eye rises with, on the `rise` spring), `lag`
  (the eye trails the rider across on a spring), `follow_x` (how much of the move across it follows: the road stays
  framed from an outside lane), `speed_fov` (degrees per unit a second), `speed_streaks` (speed lines from this speed).
- **Hitstop** (`hitstop: { event: (secs, scale, recover) }`): the world runs at `scale` for `secs`, then eases back;
  the camera's effects keep playing. **Screen**: `screen: (w, h)` shapes the window and shots (portrait for a phone);
  `--size` overrides it. Arrow keys are `left right up down`.

## Input: actions, schemes and contexts (`input.ron`)

What the player presses is an abstraction, like everything else: devices produce **actions**, and actions go to
the view (camera, time, selection) or to the game (a declared action, through the same checks as any agent).
Modelled on Unreal's Enhanced Input and Unity's Input System, but as data, and with contexts bound to the game's
state.

```ron
Input(
    // Named control schemes; the player picks one (`--scheme left_hand`), or a view sets its default.
    scheme: "right_hand",
    schemes: {
        "right_hand": {
            "pan":     Dpad(up: "w", down: "s", left: "a", right: "d"),   // four keys -> one 2D direction
            "pause":   Key("space"),
            "select":  Key("tab"),
            "act":     Mouse(left),
        },
        "left_hand": { "pan": Dpad(up: "up", down: "down", left: "left", right: "right"), ... },
    },
    // Contexts, highest priority first; the first active one that binds an action wins.
    contexts: [
        // When the selected entity can be controlled, the pad walks it (a game action).
        (name: "drive", when: "selected.controllable", actions: {
            "pan": Game(do: "move", args: { "dx": "x", "dy": "y" }),
        }),
        (name: "watch", actions: {
            "pan": View(Pan), "pause": View(Pause), "select": View(SelectNext),
        }),
    ],
)
```

- **Bindings:** `Key(name)` (`"a"`..`"z"`, `"0"`..`"9"`, `"space"`, `"tab"`, `"enter"`, `"esc"`, arrows `"up"`...,
  `"plus"`, `"minus"`, `"["`, `"]"`), `Dpad(up, down, left, right)` (a 2D action), `Mouse(left | right | middle)`,
  `Scroll`. Every backend (terminal, GPU window, browser) translates its events into these names.
- **Targets:** `View(Pause | Faster | Slower | Step | SelectNext | Pan | Orbit | Perspective | CutIn | CutOut | Quit)`
  or `Game(do, args)`: a declared action of the selected entity; `args` map to the input (`"x"`, `"y"` for a 2D
  action, or a number). Refusals are shown, never hidden.
- **Contexts** are active when `when` holds: an expression over the view (`paused`, `selected.kind`,
  `selected.controllable`, `selected.state`) and the world (as scenarios see it: `count`, `env`, `tick`...). The first
  active context (in order) that binds an action handles it; unbound actions fall through to the next.
- A game ships `games/<name>/input.ron`; without one the viewer's defaults apply (the keys it always had).
  Schemes and bindings are data, so rebinding is editing (or later a settings screen writing) that file.

## Platforms and builds

One game, every platform: the rule file and the core never change; a thin shell per platform does
(research: `docs/research/distribution.md`).

**Why it matters to a game developer:** being cross-platform makes it easy to compile the game, put it live, and
playtest it with real players, on the web today and in the stores as the game grows, without rewriting it.

| Layer | What | Where it runs |
|---|---|---|
| Core | `sim-core`, `sim-state`, `sim-rules` (deterministic, integer, no I/O) | native and **wasm32** (feature `parallel` = rule evaluation on every core; off in the browser) |
| GPU renderer | `sim-gpu` (see HD stage): every image one mipmapped sRGB texture uploaded once, every quad one instance; a frame uploads only the instance list | Metal (macOS, iOS, visionOS), Vulkan (Linux, Android), DX12 (Windows), WebGPU / WebGL2 (browser, via wasm; not built yet) |
| Terminal renderer | `sim-render` (cells, half-blocks, kitty graphics) | developer and debugging view |
| Platform shell | `simcraft-build` | one command per target (below) |

`simcraft-build <game> --target <t> [--view <file>]` bundles the game, its views and asset packs (the files are
embedded, so the build is one self-contained artifact) and produces:

| Target | Output | Needs |
|---|---|---|
| `web` | `dist/<game>-web/`: `index.html`, `.wasm`, JS glue (WebGPU, falls back to WebGL2); a static site for itch.io and web portals, the base for WebXR and Ray-Ban web apps | `wasm32-unknown-unknown`, `wasm-bindgen` CLI |
| `macos` | `dist/<game>-macos/<Game>.app` (the layout a Steam or Epic depot takes; sign and notarize before upload) | Xcode command line tools |
| `linux` / `windows` | native executable in `dist/<game>-<target>/` (Steam depots, Epic BuildPatchTool) | that target's toolchain |
| `steam` | the native build plus SteamPipe scripts (`app_build.vdf`, one depot per OS) in `dist/<game>-steam/` | Steamworks app and depot ids (`--app`, `--depot`) |
| `ios` / `android` | not yet: the same `sim-gpu` code, wrapped by Xcode / Gradle projects (IPA, AAB) | |

`simcraft-build --list` shows every target and whether this machine can build it.

## Host adapters (`adapters/`)

| Host | Layer that needs the host | Layer that does not (tested without the host) |
|---|---|---|
| Unity (`adapters/unity/com.simcraft.core`, UPM package; tests in `adapters/unity/tests`) | `SimcraftWorld` (MonoBehaviour: fixed tick rate, one GameObject per entity, prefab per kind or a coloured cube, `ISimcraftView` gets state glyph changes, bus events, save/load), `.ron`/`.toml` importer | `Simcraft.Native` (P/Invoke; `__Internal` on iOS/WebGL), `Simcraft.Simulation` (IDisposable) |
| Unreal (`adapters/unreal/Simcraft`, plugin) | `USimcraftSimulation` (Blueprint-callable), `ASimcraftWorld` (actor per entity, class per kind, `ISimcraftView`), `SimcraftLib` third-party module | `simcraft.hpp` (header-only C++17 RAII wrapper) |

Grid → world: `x → X`, `y → −Z` (Unity) / `−Y` (Unreal), times `CellSize`. Native binaries are built and copied by `adapters/build-native.sh`, never committed.

## Roadmap

- [x] core + typestate + atomic groups + determinism tests
- [x] RON (A) + Rhai (B) + dry-run validation
- [x] engine.toml panel (switches, hyperparameters, safety valve)
- [x] JSON stdio agent interface, wolf/sheep
- [x] Layout, declared actions, targets (`it`, `On`), end conditions (game 2)
- [x] Seats, scores, `Need` guards, legend props (game 3)
- [x] Event bus: JSONL log, live TCP stream, verified replay
- [x] State charts: `sim-state` + game 4 (gamedev)
- [x] Gradients (`Climb`) + game 5 (colony); eval-driven development (`tools/eval.py`, `docs/evals.md`)
- [x] Environments (`envs/`, `env.<name>`, native implementations with conformance), maths helpers
- [x] Test suite (`test/`, `simtest`): scenarios, insta snapshots, proptest properties
- [x] Game feel: tick interpolation, spring camera, walk bob; `simcraft-feel` evals
- [x] Input: actions, schemes (right/left hand, mouse), contexts bound to game state (`input.ron`)
- [ ] Capabilities: pick renderer and quality per machine
- [x] HD stage: `sim-gpu` (wgpu), `stage.ron`, `simcraft-play`, `simcraft-import`; colony3d in natural-history art
- [x] First person: `track.ron`, 3D camera and fog, views, built-in camera effects, tunable position switch (game 8, `games/lanes`)
- [x] First person in a voxel world: `roam.ron`, WASD and mouse, crawlers (`cling`), exact reach (`SetFieldAt`), the
  `field` request and structure evals (game 9, `games/mound`)
- [x] Models: glTF import, instanced GPU skinning with PBR, the state-machine contract, `simcraft-model`, a rigged
  photoreal termite (Higgsfield → Blender), macro laterite and mud textures
- [x] Mound: an organic surface (surface nets), sun shadows, a macro lens and haze, crawlers on the drawn surface;
  the colony no longer locks up carrying (tired arms, eval step 004)
- [ ] Mound next: per-foot placement on walls (leg IK on the same bones, near termites only), grass stalks, replays
  of your runs, a queen, the missing mud (8% of drops)
- [x] Checking: `rustfmt.toml`, `simcraft-check`, the edit hook; `MoveBy` and the `clamped` warning
- [x] Tracks: replays (`R`, `N`, `--replay`), lives meter, gone animations, hurt effect
- [ ] Track next: a feel eval for switches (arrival time, overshoot, bank), text for scores, sound
- [ ] Stage next: texture handles instead of names, cached static quads (soil, hollows), sprite atlas, compressed
  textures (KTX2: BC7 / ASTC), simulation on its own thread with a double-buffered snapshot, the web build
- [ ] Platforms: core on wasm32, `simcraft-build` (web, macOS, Linux, Windows, Steam; iOS and Android next)
- [ ] C API: pass environment files with the game (hosts cannot load games with `environments` through `simcraft_new` yet)
- [x] 3D worlds (depth, z, 26-neighbourhood) and fields (per-voxel numbers, native diffusion, decay, terrain)
- [x] `sim-render`: native terminal renderer, components, themes, asset packs, projections, `view.ron`
- [x] Colony underground: the nest as a physical environment (game 6, `games/colony3d`)
- [x] Perception: per-kind `senses`
- [x] World snapshot / restore
- [x] `sim-ffi`: versioned C API
- [ ] Unity adapter (C# package) + sample: `Native`/`Simulation` tested with .NET (`adapters/unity/tests`); `SimcraftWorld` and the importer not yet compiled in Unity
- [ ] Unreal adapter (C++ plugin + Blueprints): written; `simcraft.hpp` compiled and tested, the UE module not yet
- [ ] MCP wrapper (so external agents can connect directly)
- [ ] `sim-tui` (ratatui) viewer
- [ ] Parameter sweep: the operator's panel tunes itself (survival / oscillation score)
- [x] Spatial grid + per-kind index for `near` (was O(n²))
- [x] Performance: work counters and profiles, perf lints, per-kind plans, native guards, no world copies (`tools/check.sh`)
- [ ] Performance next: build `me` lazily inside a kind that mixes guarded and unguarded rules; guards for `rand(n) < p.x`
- [x] Tick rate as an operator hyperparameter (`tick_rate`, `pace`)
- [x] Kernel: integer ONNX runtime (`sim-kernel`)
- [x] Brains: a kind's network, per-entity genomes, heredity with mutation (game 7, `games/forage`)
- [ ] Kernel next: brains from `.onnx` files (offline training), batching identical genomes, rules as graphs, WGSL backend
- [ ] Adapters read `tick_rate` (Unity/Unreal still have their own `ticksPerSecond`)
