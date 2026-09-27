# simcraft: architecture

A headless, deterministic simulation engine whose rules are defined outside the engine.
This file is the project's **single source of truth**. If the code contradicts it, either the code gets fixed or this file is updated first.
How the engine got here, change by change, is in [`emergence.md`](emergence.md).

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
engine.toml┴─► sim-rules ──────►│
              (parse, compile    └── (later) sim-tui, replay
               Rhai, validate)
                     │ impl Rules
                     ▼
               sim-core  (World, Effect, apply, Engine<typestate>)
```

| Crate | Contents | Knows the game? |
|---|---|---|
| `sim-core` | `World` (entities + cell grid + per-kind index, mutation only via methods), `Effect`, `Group`, `apply`, `Engine<Loaded→Validated→Running>`, `trait Rules`, hash | No |
| `sim-rules` | `GameDef` (RON), `EngineConfig` (TOML), Rhai compilation, dry-run validation, `impl Rules for Game` | Knows the schema, not the content |
| `sim-agent` | The `simcraft-agent` binary, JSON line protocol, ASCII map | No |

## Tick loop

```
tick:
  groups = agent queue              (first: agent movement overrides rule movement)
         + FSM transitions          (first match per entity)
         + rules                    (entity id order × rule order in game.ron)
  apply(groups)                     (the single write point)
  tick += 1; hash
```

- **Rules never mutate the world.** They produce a `Group`: the effects of one rule firing.
- **Groups are atomic.** If an entity the group touches already died earlier this tick, the whole group is dropped. If that entity is someone else, a `conflict` event is emitted (two wolves cannot eat the same sheep); if it is the group's own actor, the group is dropped silently.
- An entity **moves at most once per tick**. The first `Move` wins.
- FSM transitions are applied in `apply`. Rules see the old state for the rest of that tick.
- **Solid kinds** occupy their cell: at most one solid per cell. A solid cannot move into, or spawn onto, a cell holding another solid (a blocked spawn emits `blocked`).

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

**B. Rhai script (escape hatch):** the `script:` field returns an array of effect maps:
`#{op: "set"|"add", prop, value}`, `#{op: "emit", name}`, `#{op: "move", dx, dy}`, `#{op: "despawn"}`.

**What expressions can see:** `me.<prop>`, `me.x/y/state/kind/id`, `p.<param>`, `near.<kind>` (Chebyshev distance, 9999 if none), `count.<kind>`, `tick`, `roll` (0..99, deterministic per rule and entity).

**World queries (functions):** `around(kind, r)` / `around(kind, state, r)` count entities within Chebyshev radius `r` (self excluded); `rand(n)` → 0..n-1, deterministic.

`near.<kind>` is computed only for kinds that some expression mentions as `near.<kind>`.

**Targets:** a rule or action with `target: Nearest(kind)` sees `it` (the target's props plus `it.dist`). With no such entity the rule does not fire.

**Actions** (`actions:` in `game.ron`) have the same shape as rules plus `args: [...]` (`arg.<name>` in expressions). They run only when an agent asks, and switches apply to them too.

**Layout:** `layout: (legend: {char: kind}, rows: [...])` places entities and sets the world size; `.` and space are empty. The panel's `[world]` is then optional and must match if present.

**End:** `end: [(when, result)]`: world-level expressions (`count`, `p`, `tick`); the first true one ends the game, and the engine stops ticking.

**Kinds** in `game.ron`: `glyph`, `props`, optional `fsm`, `solid: bool`, and `glyphs: {state: char}` for per-state rendering.

## Validation (typestate `Loaded → Validated`)

`Engine<Loaded>` has no `tick` method, so unvalidated rules cannot run: this is enforced at compile time. `validate` returns every error in a single list:

1. RON/TOML schema (`deny_unknown_fields` in both files)
2. Rhai syntax (compiled at load time)
3. Cross-references: switch ↔ rule name, param ↔ `game.ron params`, kinds, FSMs, states
4. **Dry run:** every expression is evaluated once against the template of each kind it applies to. Typos like `me.hungr` are caught here (`fail_on_invalid_map_property`)

Game (runtime) states are **data** (FSM, strings). Engine states are **types** (typestate).

## Agent protocol (`simcraft-agent [GAME_DIR] [--config PANEL.toml]`)

One JSON request per line, one JSON response per line. On startup it prints `{"ok":true,"ready":...}`. On failure it prints `{"ok":false,"stage":"load|validate","errors":[...]}` and exits with code 2.

| Request | Response |
|---|---|
| `{"cmd":"info"}` | Game, kinds (glyph, props, states), controllable kinds, `you` (your entity ids), declared `actions`, effective switches/params, command schema |
| `{"cmd":"observe"}` | Full map (ASCII), counts, entities |
| `{"cmd":"observe","entity":ID}` | `observe_radius` window, `@` = you, visible entities |
| `{"cmd":"act","actions":[{"entity":ID,"do":"<action>","args":{...}}]}` | Evaluated now, applied on the next `step` before rules. Per-action `results` with `ok` or the reason (`refused: needs …`, `unknown action`, `takes args`, `switched off`, `not controllable`) |
| `{"cmd":"step","n":N}` | tick, done, result (from `end`), hash, counts, states (`{kind: {state: n}}`), events (game events plus `conflict`, `blocked`, `error: …`) |
| `{"cmd":"hash"}` | State fingerprint (replay/verification) |

## Roadmap

- [x] core + typestate + atomic groups + determinism tests
- [x] RON (A) + Rhai (B) + dry-run validation
- [x] engine.toml panel (switches, hyperparameters, safety valve)
- [x] JSON stdio agent interface, wolf/sheep
- [x] Layout, declared actions, targets (`it`, `On`), end conditions (game 2)
- [ ] Event log → `sim-replay` (same log → same hash)
- [ ] MCP wrapper (so external agents can connect directly)
- [ ] `sim-tui` (ratatui) viewer
- [ ] Parameter sweep: the operator's panel tunes itself (survival / oscillation score)
- [x] Spatial grid + per-kind index for `near` (was O(n²))
- [ ] Performance: `me` map is still rebuilt per entity per tick; world snapshot is cloned per tick
