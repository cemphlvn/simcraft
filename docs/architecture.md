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
| `sim-state` | State charts: nested states, layers, reusable machines, remember, interrupt/back, pick; memory encoding; selectors; step distances. Guards and actions are generic (`G`, `A`) | No (not even Rhai) |
| `sim-rules` | `GameDef` (RON), `EngineConfig` (TOML), Rhai compilation, dry-run validation, `impl Rules for Game` | Knows the schema, not the content |
| `sim-agent` | The `simcraft-agent` binary, JSON line protocol, ASCII map | No |
| `sim-ffi` | `cdylib` + `staticlib`, C header; the agent protocol behind `extern "C"` | No |
| `sim-gpu` | GPU renderer (`wgpu` + `winit`): textured quads, backdrops and atlas uploaded once; `simcraft-play`; web via wasm | No |
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
- Evaluation (read-only) runs on `[run] threads` cores (0 = all); results merge in entity-id order, so the core count never changes the outcome. `apply` is single-threaded.
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
| `Move(dx, dy)` | One step; `dx`, `dy` are expressions (e.g. `arg.dx`) |
| `On(Me \| It \| Nearest(kind), [...])` | Applies the nested actions to that entity instead of the owner |
| `Need(prop, min)` | Guard: `prop >= min` on the subject. Checked at request/eval time and again at apply time against the live state; if it fails the whole group is dropped (`short`). Prevents double spending under simultaneous moves |

**B. Rhai script (escape hatch):** the `script:` field returns an array of effect maps:
`#{op: "set"|"add", prop, value}`, `#{op: "emit", name}`, `#{op: "move", dx, dy}`, `#{op: "despawn"}`.

**What expressions can see:** `me.<prop>`, `me.x/y/state/kind/id`, `p.<param>`, `near.<kind>` (Chebyshev distance, 9999 if none), `count.<kind>`, `tick`, `roll` (0..99, deterministic per rule and entity).

**World queries (functions):** `around(kind, r)` / `around(kind, state, r)` count entities within Chebyshev radius `r` (self excluded); `nearest_prop(kind, prop, r, default)` reads a prop of the nearest such entity within `r` (else `default`: what you feel of the nest you sit in); `rand(n)` → 0..n-1, deterministic.

`near.<kind>` is computed only for kinds that some expression mentions as `near.<kind>`.

**Targets:** a rule or action with `target: Nearest(kind)` sees `it` (the target's props plus `it.dist`). With no such entity the rule does not fire.

**Actions** (`actions:` in `game.ron`) have the same shape as rules plus `args: [...]` (`arg.<name>` in expressions). They run only when an agent asks, and switches apply to them too.

**Layout:** `layout: (legend: {char: kind | (kind, {prop: value})}, rows: [...])` places entities and sets the world size; `.` and space are empty. The panel's `[world]` is then optional and must match if present.

**End:** `end: [(when, result)]`: world-level expressions (`count`, `p`, `tick`); the first true one ends the game, and the engine stops ticking.

**Score:** `score: "<expr>"` is evaluated for each controllable entity and summed per seat (or per entity without seats).

**Seats (panel):** `[agent] seats = {alice = 1, bob = 2}` makes the game multi-player. Requests carry `"as": "<seat>"`; an entity belongs to a seat when its `owner` prop equals the seat's number, and controllable kinds must declare `owner`.

**Kinds** in `game.ron`: `glyph`, `props`, optional `fsm`, `solid: bool`, `hidden: bool` (not drawn), `glyphs: {state: char}` for per-state rendering (the deepest matching state wins), and `senses: {name: expr}` (see Perception).

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

## 3D worlds and fields (physical environments)

A world has `width × height × depth` voxels; 2D games have depth 1 and behave (and hash) exactly as before.

- **Coordinates:** every entity has `x, y, z` (`z` = 0 is the top level; deeper levels have larger `z`).
  Distance is Chebyshev in 3D (`max(|dx|, |dy|, |dz|)`); neighbours are the 26 around a voxel (8 at depth 1).
- **Layout by levels:** `layout: (legend: {...}, cells: {',': {"soil": 1}}, levels: [ [rows of level 0], ... ])`
  (`rows:` alone is one level). `cells` sets field values at a glyph's voxels without placing an entity.
- **Fields** are numbers per voxel, owned by the world (hashed, snapshotted, replayed):
  `fields: { "temp": (init: 50, diffusion: 20, top: "env.seasons.warmth"), "soil": (init: 0) }`.
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
  `AddField(name, expr)` at the subject's voxel, `SetFieldAt(name, dx, dy, dz, expr)` at a voxel next to it (dig);
  `ClimbField(name)` steps to the open neighbour with the most `name`.
- `around` and `near` count and measure in 3D.

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
    frames: [[".kk.", ...], ...])}`; a kind's look names a sprite per state (`sprite: "ant_carry"`).
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

## Platforms and builds

One game, every platform: the rule file and the core never change; a thin shell per platform does
(research: `docs/research/distribution.md`).

**Why it matters to a game developer:** being cross-platform makes it easy to compile the game, put it live, and
playtest it with real players, on the web today and in the stores as the game grows, without rewriting it.

| Layer | What | Where it runs |
|---|---|---|
| Core | `sim-core`, `sim-state`, `sim-rules` (deterministic, integer, no I/O) | native and **wasm32** (feature `parallel` = rule evaluation on every core; off in the browser) |
| GPU renderer | `sim-gpu`: `wgpu` + `winit`. Everything is a textured quad drawn by one shader (nearest sampling, alpha blending): the sky, **backdrop layers uploaded once** and scrolled by UV offset in the shader (parallax on the GPU), the world cross-section as a small texture re-uploaded only when it changed, **sprites from one atlas** built at load. The CPU uploads what changed and a list of quads | Metal (macOS, iOS, visionOS), Vulkan (Linux, Android), DX12 (Windows), WebGPU / WebGL2 (browser, via wasm) |
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
- [ ] Platforms: core on wasm32, `sim-gpu` (wgpu), `simcraft-build` (web, macOS, Linux, Windows, Steam; iOS and Android next)
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
- [ ] Performance: `me` map is still rebuilt per entity per tick; world snapshot is cloned per tick
