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
| `Goto(state)` | Changes the entity's FSM state (validated against the kind's states) |
| `Move(dx, dy)` | One step; `dx`, `dy` are expressions (e.g. `arg.dx`) |
| `On(Me \| It \| Nearest(kind), [...])` | Applies the nested actions to that entity instead of the owner |
| `Need(prop, min)` | Guard: `prop >= min` on the subject. Checked at request/eval time and again at apply time against the live state; if it fails the whole group is dropped (`short`). Prevents double spending under simultaneous moves |

**B. Rhai script (escape hatch):** the `script:` field returns an array of effect maps:
`#{op: "set"|"add", prop, value}`, `#{op: "emit", name}`, `#{op: "move", dx, dy}`, `#{op: "despawn"}`.

**What expressions can see:** `me.<prop>`, `me.x/y/state/kind/id`, `p.<param>`, `near.<kind>` (Chebyshev distance, 9999 if none), `count.<kind>`, `tick`, `roll` (0..99, deterministic per rule and entity).

**World queries (functions):** `around(kind, r)` / `around(kind, state, r)` count entities within Chebyshev radius `r` (self excluded); `rand(n)` → 0..n-1, deterministic.

`near.<kind>` is computed only for kinds that some expression mentions as `near.<kind>`.

**Targets:** a rule or action with `target: Nearest(kind)` sees `it` (the target's props plus `it.dist`). With no such entity the rule does not fire.

**Actions** (`actions:` in `game.ron`) have the same shape as rules plus `args: [...]` (`arg.<name>` in expressions). They run only when an agent asks, and switches apply to them too.

**Layout:** `layout: (legend: {char: kind | (kind, {prop: value})}, rows: [...])` places entities and sets the world size; `.` and space are empty. The panel's `[world]` is then optional and must match if present.

**End:** `end: [(when, result)]`: world-level expressions (`count`, `p`, `tick`); the first true one ends the game, and the engine stops ticking.

**Score:** `score: "<expr>"` is evaluated for each controllable entity and summed per seat (or per entity without seats).

**Seats (panel):** `[agent] seats = {alice = 1, bob = 2}` makes the game multi-player. Requests carry `"as": "<seat>"`; an entity belongs to a seat when its `owner` prop equals the seat's number, and controllable kinds must declare `owner`.

**Kinds** in `game.ron`: `glyph`, `props`, optional `fsm`, `solid: bool`, and `glyphs: {state: char}` for per-state rendering (the deepest matching state wins).

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
- [x] World snapshot / restore
- [x] `sim-ffi`: versioned C API
- [ ] Unity adapter (C# package) + sample: `Native`/`Simulation` tested with .NET (`adapters/unity/tests`); `SimcraftWorld` and the importer not yet compiled in Unity
- [ ] Unreal adapter (C++ plugin + Blueprints): written; `simcraft.hpp` compiled and tested, the UE module not yet
- [ ] MCP wrapper (so external agents can connect directly)
- [ ] `sim-tui` (ratatui) viewer
- [ ] Parameter sweep: the operator's panel tunes itself (survival / oscillation score)
- [x] Spatial grid + per-kind index for `near` (was O(n²))
- [ ] Performance: `me` map is still rebuilt per entity per tick; world snapshot is cloned per tick
