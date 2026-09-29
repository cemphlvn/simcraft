# Emergence log

The engine is not designed up front. Games are written the way a designer *wants* to write them; when one hits a wall, the engine changes. Every change is recorded here as **symptom → need → refactor → evidence**.

Rule: a refactor that changes an existing game's behaviour must be deliberate. `golden_wolf_sheep_hash` (tick 300 = `ee9a66d10246d6f9`) guards this.

---

## Game 1: forest fire (`games/forest_fire`)

Drossel–Schwabl forest fire: every cell is a patch (Empty / Tree / Fire). A tree ignites if a neighbour burns or lightning strikes.

| # | Symptom | Need | Refactor | Evidence |
|---|---|---|---|---|
| 1.1 | `solid: true` and `glyphs:` in `game.ron` produced **no error**. They were silently ignored | Typos in the designer's file must fail the same way they do in the operator's panel | `deny_unknown_fields` on every `game.ron` struct | test `unknown_field_in_game_is_rejected` |
| 1.2 | `Function not found: around`, `rand` | Expressions must **query** the world ("burning neighbours?") and draw rare events (1 in 100 000); `roll` (0..99) is too coarse | Native Rhai functions `around(kind[, state], r)` and `rand(n)` bound to a per-tick world snapshot. `rand` stays deterministic: seed × tick × entity × salt × call index | fire runs; `forest_grows_and_burns` |
| 1.3 | `Unexpected variant Goto` | A rule must change state *and* do something (`Emit("lightning")`); FSM transitions cannot emit | `Goto(state)` action (→ `Effect::SetState`), validated against the kind's states | lightning events in the step report |
| 1.4 | Random placement stacks patches; a cell must hold exactly one | **Occupancy** as a concept, not a game hack | `solid` kinds: one per cell, cannot move into or spawn onto another solid; initial placement shuffles free cells | `solid_kinds_fill_one_per_cell` |
| 1.5 | Map showed only `.` because glyphs are per kind | Observation must reflect state | Optional per-state `glyphs` on a kind, validated against its states | fire front visible as `*` |
| 1.6 | Step report said `patch: 2048` every tick; tree density was invisible | Observation by state | `states: {kind: {state: n}}` in `observe` / `step` | fire-size measurement below |
| 1.7 | `near` was an O(n²) scan. A spatial grid alone made it **slower** (3.34 s → 5.41 s): 2000 sheep ring-searched for 8 wolves | Nearest search must suit both dense and sparse kinds | `World` owns a cell grid + per-kind index; mutation only via methods. Sparse kinds (≤ 64) scan members, dense kinds search rings. Same result either way (min distance, then min id) | golden hash unchanged; wolf/sheep 2000 entities × 100 ticks **3.34 s → 1.10 s** |
| 1.8 | Tuning the fire panel: 4000 ticks took 11 s. Profile: allocation dominated (Rhai maps rebuilt and cloned per entity per rule); 13 % in `near.patch`, which no rule reads | Operator sweeps must be cheap | `near.<kind>` computed only for kinds referenced in sources; `p` / `count` shared (`into_shared`) instead of copied; scope `rewind` instead of clone per rule | fire 2000 ticks **10.3 s → 3.98 s**; wolf/sheep **1.10 s → 0.53 s**; golden hash unchanged |

**Emergent result (not written in any rule):** with `grow / lightning = 10` fire sizes are heavy-tailed (64×32, 3000 ticks):

```
size     1-3     ##################### 21
size     4-15    ##############        14
size    16-63    ###########           11
size    64-255   #######                7
size   256-1023  ###                    3
```

At `grow / lightning = 100` almost every fire burned the whole map. The operator's panel is where that difference is chosen.

---

## Game 2: mercy dungeon (`games/mercy_dungeon`)

A hand-drawn dungeon. The hero can **fight** or **spare** each ghost. Killing raises LV (Level of Violence); mercy works better at LV 0. A hurt ghost turns Angry, chases and strikes back.

| # | Symptom | Need | Refactor | Evidence |
|---|---|---|---|---|
| 2.1 | `Unexpected field named layout`. Before 1.1 it would have been **silently ignored**, leaving an empty dungeon | A hand-authored map, with world size taken from it | `layout: (legend: {char: kind}, rows: [...])`; panel `[world]` becomes optional (must match if given) | `layout_sets_world_and_places_entities`, `panel_world_must_match_layout`, `layout_char_not_in_legend_is_rejected` |
| 2.2 | The only agent verb was a built-in `move` | What an agent may do is **game design**, not engine design | `actions:` in `game.ron`: same shape as rules, with `args` (`arg.<name>` in expressions); run only when asked. `move` itself became a declared action (`Move(dx, dy)`); wolf/sheep migrated. Operator switches cover actions too | `action_errors_are_specific`, `operator_can_switch_off_an_action` |
| 2.3 | A fight must lower **the ghost's** hp, and needs to read it (`it.hp`) | Effects on another entity, with its props visible | Rule/action `target: Nearest(kind)` → `it` in scope (with `it.dist`); `On(Me \| It \| Nearest(k), [...])` applies nested actions to that entity; validated recursively (`It` without `target` fails) | `it_without_target_is_rejected` |
| 2.4 | The game can be won or lost; the step report couldn't say so | Win/lose as data | `end: [(when, result)]` over `count`, `p`, `tick`; `Rules::outcome`; a finished engine stops ticking; step returns `done` + `result` | `end_condition_finishes_the_game` |
| 2.5 | An `act` could only be judged at the next tick | Immediate feedback for agents | The world does not change between `act` and `step`, so the action is evaluated at `act` time and its group is queued; refusals say what was needed (`refused: needs Nearest("ghost") and it.dist <= 1 && it.hp > 0`) | `action_refused_when_condition_fails` |
| 2.6 | `observe` on entity 1 returned a wall; the agent had no way to find its hero | An agent needs to know its own entities | `you: [ids]` in `info` and `observe` | scripted agent uses `info.you` |
| 2.7 | Violent run ended at LV 4 after 3 kills | **Not an engine change.** A ghost at 0 hp is removed next tick (every rule sees the previous tick); the hero hit the corpse. Changing that would break order-independence | Game fix: `fight` requires `it.hp > 0`. Logged as a design pitfall of simultaneous update | LV = kills in every run below |

**Emergent result:** a scripted agent (`agents/mercy_dungeon.py`) with three policies, 4 seeds each, all wins:

| Policy | Ticks | HP lost | LV | Spares per ghost |
|---|---|---|---|---|
| pacifist | 25–32 | 0 | 0 | ~2 |
| violent | 25–32 | 1–3 | 3 | — |
| regret (kill one, then spare) | 29–38 | 0–1 | 1 | ~4–5 |

One early kill makes every later mercy more expensive; no rule says "regret is slow".

---

## Game 3: market (`games/market`)

Two seats, simultaneous turns. Alice's village grows wood, Bob's grows stone; a house needs both, so they trade through one market whose prices follow its stock.

| # | Symptom | Need | Refactor | Evidence |
|---|---|---|---|---|
| 3.1 | Legend `'A': ("village", {"owner": 1})` → `Expected string` | Per-instance props in a hand-drawn map (who owns which village) | Legend entry is `kind` or `(kind, {prop: value})`; unknown props are rejected | `legend_props_set_owners`, `legend_prop_typo_is_rejected` |
| 3.2 | Panel `seats` unknown; one stdin, two players | Several agents in one game, each limited to its own entities | Panel `[agent] seats = {name = owner}`; requests carry `"as"`; an entity is yours if its `owner` prop is your seat's number; `you` is per seat | `seats_enforce_ownership`, `seats_need_an_owner_prop` |
| 3.3 | `score` unknown | A benchmark needs a number per player | `score:` expression per controllable entity, summed per seat; in every `step` / `observe` | `scores_are_per_seat` |
| 3.4 | Two same-tick buys of 8 from a stock of 10 were both accepted; **stock went to -7** and one village held 16 stone. Each order was checked against the same start-of-tick world | **Resources must not be spent twice** under simultaneous moves. Atomic groups (1.x) only covered dead targets | `Need(prop, min)`: checked at request time (feedback) and again at apply time against the live state; if it fails the whole group is dropped with a `short` event | `need_prevents_double_spend`: one `short`, stock ≥ 0 |
| 3.5 | *(expected, did not happen)* World-level state such as prices | Globals | **Not added.** A singleton `market` entity with props, reached via `target: Nearest("market")`, carried it without friction | the market game itself |

**Emergent result:** alice's score, 150 days, per strategy pair (symmetric game, `agents/market.py`):

| alice ↓ \ bob → | builder | dumper | speculator |
|---|---|---|---|
| builder | 1815 | 1363 | 1130 |
| dumper | 798 | 450 | – |
| speculator | 2161 | – | 2157 |

- Dumping hurts everyone, the dumper included: prices crash to $1.
- Speculation (sell only when dear, buy only when cheap) is the best response to both builder and speculator.
- Unlike a prisoner's dilemma, the speculator equilibrium (2157 each) beats cooperative building (1815 each): withholding keeps prices up.

None of these numbers are written in any rule; they come from `price = price_k / (stock + 5)` plus two agents.

---

## Request: multicore

Not a game this time. The operator asked for more ticks per second.

| # | Symptom | Need | Refactor | Evidence |
|---|---|---|---|---|
| M.1 | One core: forest fire 2,048 cells ≈ 480 ticks/s; wolf/sheep ~2,100 entities ≈ 170 | Use the other 9 cores | Rule evaluation only reads, so entities are split across cores (rayon, 64 per batch) and results merged in id order; `apply` stays single-threaded. Rhai `sync`; query context became thread-local. Panel: `[run] threads` (0 = all, 1 = one) | `thread_count_does_not_change_the_world`: 1, 4 and all cores give the same hash every tick; golden hash unchanged |
| M.2 | First multicore run was **up to 10× slower** (forest fire 479 → 25 ticks/s) | The single-core optimisation 1.8 (`p`, `count` as shared Rhai values) became `Arc<RwLock>` under `sync`: every `p.grow` read took a lock, and 10 cores fought over it | No sharing: each batch builds one scope with `p`/`tick`/`count`; each entity pushes `me`/`near` on top and rewinds | 10 cores: forest fire **2.4×**, wolf/sheep big **2.2×**, 32,768 cells **2.8×**; single-core also faster (479 → 542) |
| M.3 | Small games (≤ 100 entities) were 10–30 % slower on the pool | Scheduling costs more than the work | Worlds under 256 entities take the sequential path | mercy / wolf_sheep back to 1.0× |

Why not ~10×: the world snapshot copy and `apply` are still sequential (Amdahl), and 6 of the 10 cores are efficiency cores.

---

## Request: event bus

Also not a game. Until now events only came back to whoever sent `step`; nothing else could watch, and a match could not be kept.

| # | Need | Refactor | Evidence |
|---|---|---|---|
| B.1 | Anyone (a viewer, a log, LOBI) can follow a run without being the player | `sim-core`: `Msg` (`start`, `act`, `event`, `tick`, `end`), `Sink`, `Bus` with name filters; the engine publishes events, hashes and the end; the host publishes start and every act. No I/O in the core | `bus_filter_delivers_only_named_messages` |
| B.2 | Keep a match; show that nothing was lost | JSONL file sink (`[bus] log`) + `--replay`: re-applies accepted acts and checks every tick hash | a 150-day market match (234 acts) replays with 150/150 ticks verified; `bus_log_replays_to_the_same_hashes` |
| B.3 | Tampering or a changed game must not replay silently | Hash check per tick; `start` carries a fingerprint of `game.ron` + `engine.toml` | one `sell` changed from n to n+1 → `diverged at tick 3`; `tampered_log_is_detected` |
| B.4 | Watch live from another process | TCP sink (`[bus] listen`), non-blocking for the simulation (200 ms write timeout, slow clients dropped) | a second socket saw a refused `build` and six `tick` lines as they happened |

A log is small because determinism does the rest: 437 lines for 150 days of two players.

---

## Game 4: gamedev (`games/gamedev`)

Three developers share a studio and one in-house engine; 1 tick = 1 hour. Each dev ideates, prototypes until fun, builds content, polishes, ships. Content brings feature creep: the game needs engine features the engine lacks and **hits a wall**. The dev is interrupted: refactor the shared engine (slow now, every later project benefits) or hack around it (fast now, the debt breeds bugs in everyone's games). The game is our own workflow, simulated.

Unlike games 1–3 this change was **requested** (a state engine with composable primitives), not discovered. The game was still written first, the way a designer wants it, and loaded against the old engine to find the walls.

| # | Symptom | Need | Refactor | Evidence |
|---|---|---|---|---|
| 4.1 | `Unexpected field named enter in FsmDef` (first load) | A dev's life is not flat: `Awake` holds `Work` holds `Build`; `Mood` runs beside `Life`; entering a state does something | New crate `sim-state` (game- and Rhai-agnostic; guards/actions are handles): nested `states`, `layers`, `enter`/`exit`/`then`, outer-first transitions (a parent can always interrupt), one group per entity per tick | 16 unit tests; tick-300 hashes of **all four** earlier games unchanged (flat machines keep plain state names). Cost: 1000 ticks of forest fire 0.77 → 0.81 s, wolf/sheep 0.104 → 0.112 s (+6–8 %: memory decoded per entity per tick, `roll` in transition guards) |
| 4.2 | Flow is needed in Design, Build, Playtest and Refactor | Write a behaviour once, use it everywhere | `use: "focus"` mounts a machine; rules written inside a state travel with it (composition). One rule, many mounts, one switch | `switching_off_a_machine_rule_turns_it_off_at_every_mount` |
| 4.3 | After coffee, back to the task; after a wall, back to *exactly* where you were, even from inside a refactor that hit its own wall (debt cleanup) | Memory of where you were | `remember` (history); `interrupt`/`back` on transitions, `Interrupt`/`Back` actions; nested interrupts stack (recursion, max 16). Leaving a level drops what was saved inside it | ~12 cleanups nested inside refactors per year; memory string `…#Life.Awake.Work=Build^Life.Awake=Work.Build.Flow` |
| 4.4 | "What to work on" follows the project's phase; "refactor or hack" is a trade-off | Decisions, not just transitions | `pick: First` (selector) and `pick: Best` (utility; a tie keeps the current), `recheck` every tick | tipping point below |
| 4.5 | Rules need the state machine at three kinds of distance | Distance as a first-class binding | Hierarchy: `state:` matches a state and everything inside it, `depth: N`, `depth_in`. Transition graph: `steps_to` (precomputed table). Space: `around(kind, state, r)` with paths, `near_in`, target `NearestIn(kind, state)` | crunch starts one step from `Shipped`; pep talks go to a colleague one step from `Burnout`; burnout spreads within `office_r` |
| 4.6 | Projects spawned beside the desk, never worked on | **Not an engine change.** `Work` remembers `Ideate`; a coffee break resumed into it and `enter: [Spawn]` fired away from the desk. `enter` runs on every entry, including a resume | Game fix: spawn in a guarded rule. Logged as a pitfall of `remember` + side-effecting `enter` | every project sits on its dev's desk |
| 4.7 | *(expected, did not happen)* Transitions that read another entity (`it`) | — | **Not added.** Rules with a `target` call `Interrupt`/`Back` (`hit_wall`, `unblocked`) | the project machine |

**Emergent result:** total fame of the studio, one year, 3 seeds each (`care`: 0 = always hack, 10 = always refactor):

| care | crunch | fame | shipped | hacks | engine features | engine debt | burnouts |
|---|---|---|---|---|---|---|---|
| 0–5 | off | 288 | 51 | 792 | 0 | 1584 | 0 |
| 0–5 | on | 392 | 42 | 585 | 0 | 1170 | 11.0 |
| 6 | off | 5875 | 93 | 26 | 45 | 22 | 0 |
| 6 | on | 5459 | 89 | 34 | 43 | 24 | 2.7 |
| 10 | off | 6228 | 96 | 0 | 49 | 0 | 0 |
| 10 | on | 6491 | 99 | 0 | 52 | 0 | 0 |

- **A tipping point, not a slope.** Care 0 to 5 give *identical* studios: once hacking wins the utility pick, nobody ever refactors, the engine never gains a feature, every later game needs more hacks. Between 5 and 6 fame jumps 15×. No rule says "debt compounds".
- Crunch helps only a healthy studio (+4 % at care 10) and hurts a borderline one (−7 % at care 6); in a hacking studio it buys a little fame with 11 burnouts.
- Pep talks at care 6 with crunch: without them 6.3 burnouts instead of 2.7, but fame is 7 % *higher* (5831): helping costs the helper energy.

---

## Game 5: colony (`games/colony`)

An ant colony through the seasons, built from the literature (`docs/research/ant-colony.md`): response thresholds and
age polyethism for jobs, Gordon's interaction rate for leaving the nest, stigmergy (scent trails that evaporate) for
finding food, winter dormancy on a store. Nobody plays; the score is how long the colony survives. The first game
built **eval-driven** (`docs/evals.md`): every design change measured on fixed seeds (`games/colony/EVALS.md`).

| # | Symptom | Need | Refactor | Evidence |
|---|---|---|---|---|
| 5.1 | `Unexpected variant named Climb` (first load) | Searching ants must step to the neighbouring cell with the most scent: a *gradient*, not the nearest entity | `Climb(kind, prop)`: one step to the neighbour whose `kind` has the highest `prop`, if higher than here; ties from a per-entity shuffled start; nothing higher → no move, so a later rule may move instead | `climb_steps_up_the_gradient_and_stops_at_the_top`; `climb_on_a_missing_prop_is_rejected`; trails form in the colony (up to 17 cells) |
| 5.2 | The ground is solid (one tile per cell), so `free_neighbors` offered an ant no cell to climb to | Non-solid movers see every neighbouring cell | `World::neighbors` (in-bounds ring, game-agnostic) | same tests; golden hashes unchanged |
| 5.3 | *(observed, not changed)* Everyone needs the season | Globals | **Not added** (as in 3.5). A `sun` entity with a season machine; others ask `near_in("sun", "Winter") < 9999`. It works, but reads awkwardly: the second game to want a global | `games/colony/game.ron` |
| 5.4 | Colony died on tick 1 after a change (eval step 003) | **Not an engine change.** `starve` read a default prop before `temperament` set it (rules see the start of the tick, as in 2.7) | Game fix: `tolerance` starts at −1. The eval caught it at once | `games/colony/EVALS.md` step 003 → 004 |

| 5.5 | Berry value should follow the season; the colony asked `near_in("sun", "Winter") < 9999` and kept the season maths inside the game | The world's own state machine, written once, shared by games, readable by every expression; the third game to want globals (3.5, 5.3) | **Environments:** `envs/<name>.ron` merged in as a hidden singleton entity (machine, props, rules, params); read as `env.<name>.<prop>` / `.state`; salts after every other rule so adding one shifts nothing; maths helpers `clamp`, `pct`, `ramp`, `triangle` | colony rewritten on `envs/seasons.ron`: **identical** metrics on all seeds (eval step 011); `environments_are_checked_like_everything_else` |
| 5.6 | Environments should later run as optimized native code (C++) and stay compatible | A contract that is proven, not promised | `NativeEnv` (pure step: tick, params, props, state → props, state) and `conformance`: runs `.ron` and native side by side, compares the environment and the world hash every tick | hand-written Rust seasons bit-identical for three years of the colony; an off-by-one-tick version is caught |

**Emergent result:** one-lever probes (`EVALS.md`) show foraging is a positive feedback loop that needs a spark.
More food or slower eating only buys time; scouting makes every seed find food; recruitment sustains it. No rule
states "discovery before recruitment".

---

## Game 6: colony 3D (`games/colony3d`)

The colony rebuilt for a physical world and the simcraft renderer: the nest is a place underground, temperature and
scent are fields. Also the first game built on perception (`senses`) from the start.

| # | Symptom | Need | Refactor | Evidence |
|---|---|---|---|---|
| 6.1 | The nest was one cell with a scripted climate | A physical environment: soil, tunnels, heat that moves | **3D worlds** (depth, `z`, 26-neighbourhood, level-0 wrappers keep 2D identical) and **fields** (per-voxel integers, native diffusion, `top` pinned by an expression, `terrain`) | `a_3d_world_has_levels_terrain_and_digging`, `heat_diffuses_down_through_the_levels`; all 2D golden hashes unchanged |
| 6.2 | Soil temperature never moved (all levels 50 while the air went 0 → 99) | Slow flows must not round to zero | **Not an engine change.** Integer diffusion truncates; the game stores centidegrees (0..10000). Documented as a rule for fields | deep chamber lags the seasons by ~¼ year |
| 6.3 | 559 ground entities carried the scent; they had to evaporate by rule | Pheromone as a field that evaporates | Field `decay` (% per tick, after diffusion) | scent trails in the surface view |
| 6.4 | Ants heading to the nest stopped dead against the soil ceiling | Movers follow tunnels | A step into terrain **slides** (dx,dy,0) → (0,0,dz) → (dx,0,0) → (0,dy,0); only worlds with terrain | ants go down the shaft and up again |
| 6.7 | The interface was readable but not designed | Pixel art with a parallax feel | `Diorama` component: RGBA pixel buffer, sprites and palettes in asset packs (`assets/ants_pixel.ron`), procedural backdrop layers (hills, trees, clouds) scrolling at their own speed, seasons in the sky and foliage; shown as true pixels (kitty graphics protocol, zlib, re-sent only when changed) or half-blocks | `the_diorama_paints_pixels_in_both_modes`; frame 0.6 ms (pixels) / 0.3 ms (half-blocks) |
| 6.8 | Hand-drawn art does not scale to whole landscapes | Generated art that is still true pixel art | Higgsfield CLI generates layers on flat magenta; `simcraft-pixelate` keys, crops, scales and quantizes (deterministic); asset packs load PNG images; `image` backdrop layers and a soil texture | `generated_art_loads_from_the_landscape_pack`; `views/generated.ron`; 8 credits |
| 6.6 | The nest's levels need to be seen together, from changing angles, without re-rendering the world every frame | Layered 2.5D with perspective states | `Layers` projection: one cached frame per level, perspective states (order, focus, step, fade, `click`), click / `p` to switch, lazy redraw per level | `layers_redraw_lazily` (first frame 3 of 6 layers, unchanged frame reused, a tick redraws only changed layers); `games/colony3d/views/layers.ron` |
| 6.5 | Nothing could show the nest's inside | Interfaces as reusable components | `sim-render`: diffing terminal renderer, projections 2D / 2.5D / 3D / custom, components with props, themes, asset packs, `view.ron` | `simcraft-view games/colony3d`; frame in ~1.4 ms |

---

## Request: adoptable from Unity and Unreal

Not a game. Product direction (architecture.md): hosts display, the core decides, one `game.ron` everywhere.

| # | Need | Refactor | Evidence |
|---|---|---|---|
| U.1 | A host must save and load a game (and a player must be able to "load game") | `World`/`Engine` snapshot and restore (derived indexes rebuilt; queued acts and outcome kept; `format` version); `Rules::check_world` refuses a world from another game; `restore` goes on the bus so logs still replay | `snapshot_restores_the_exact_future` (wolf/sheep, gamedev, market, through JSON); `foreign_snapshots_are_rejected`; `a_log_with_a_restore_still_replays`; market at tick 20 = 4.3 KB |
| U.2 | One protocol, not two (stdio and C) | `sim_agent::Session` moved into a library; the binary and the C API both use it | four games' hashes unchanged through the binary |
| U.3 | A C boundary hosts can call | `sim-ffi` (`libsimcraft`, ABI 1): load from text, JSON requests, JSON-free `entities` per frame, bus `drain`; NULL- and panic-safe | golden wolf/sheep hash reproduced through the C API; a C program (`examples/smoke.c`) links the static library and runs game 4 |
| U.4 | Save without a JSON library in the host | A save file is the `snapshot` reply as-is: `restore` ignores extra fields | `a_save_file_is_the_snapshot_reply_as_is` |
| U.5 | Unity and Unreal faces | `adapters/`: C# package (`Simulation` has no UnityEngine; `SimcraftWorld` component) and UE plugin (`simcraft.hpp` has no Unreal types; `USimcraftSimulation`, `ASimcraftWorld`) | `simcraft.hpp` compiled with `-Wall -Wextra` and tested; the C# `Simulation` passes 10 checks on .NET 10 against the real library (32-byte struct layout, golden hash, save/load, errors); the Unity component and the UE module are **not compiled yet** |

---

## Request: raise the tick rate, keep the speeds; a kernel for brains

Not a game yet. Research: `docs/research/kernel.md`.

| # | Symptom | Need | Refactor | Evidence |
|---|---|---|---|---|
| K.1 | The tick rate was a viewer flag (10 ticks/s hard-coded in `simcraft-view` and `simcraft-feel`); speeds were per-tick chances (`rand(100) < warmth`), which grow with the rate and step unevenly | The operator sets ticks per second; designers write speeds per second | `[run] tick_rate` (default 10, must be ≥ 1); `tick_rate` in expressions; `pace(n)` / `pace(n, secs)` (Bresenham, per-entity phase); viewers play at `tick_rate` at 1x; `info` reports it | `pace_keeps_speed_per_second_at_any_tick_rate` (12 steps in 4 s at 10 and 60 ticks/s); `pace_spaces_steps_evenly` (every 20 ticks at 60); golden hashes unchanged |
| K.2 | Agents should learn, and rules may be graphs, at under 1 MB per agent on a phone | A deterministic runtime for standard ONNX files, small enough for wasm | `kernel/` (`sim-kernel`): prost-decoded ONNX subset, 32-bit integer ops with WGSL semantics, budget, `genome`/`mutate`, `Graph` builder | int8 policy 16→32→4 = 1360-byte file, 56 bytes per agent's own genome; bit-identical to a hand-written reference and to onnxruntime 1.30 (1000 agents); 236 ns per agent at 100k agents, one thread (onnxruntime: 40 ns) |
| K.3 | Game 7 (`forage`): ants must learn to forage, with nothing in the rules saying where food or home is | A kind that decides with a network; per-entity weights; heredity | `brain:` on a kind (inputs, hidden, outputs → `sense.<choice>`); `Entity.genome` (hashed only when present, saved, restored); `Spawn` passes the parent's genome mutated; `toward_x/y` direction queries; brain inputs in the dry run | `a_colony_of_brains_learns_to_forage`; `brains_survive_save_and_load`; forage eval 000: 193 deliveries / 1000 ticks late vs 1.4 with `inherit: false`; every other game's golden hash unchanged |

---

## Request: a game that looks like a quality game

| # | Symptom | Need | Refactor | Evidence |
|---|---|---|---|---|
| R.1 | Generated sprites pixelated to 12 px turned to mud; ants larger than bushes; the art was bound to the terminal's pixel budget | Sprites from pictures; one pixel density | Sprites take image frames (baked to RGBA at load); `simcraft-pixelate --mode --outline --frames` | `an_image_sprite_blits_its_frames_and_reports_missing_ones`; majority downscale keeps edges |
| R.2 | "Why pixelate at all?" Nothing drew above terminal resolution; `sim-gpu` was in this document but not in the repo | An HD renderer; art in real space | `sim-gpu`: `stage.ron`, perspective parallax, depth rows, soil and galleries, shots on springs, season grade and cards, `simcraft-import` | 5 stage tests; colony3d shots in three seasons; `--bench`: GPU 2–2.6 ms, compose 0.02 ms per 1600x900 frame |
| R.3 | "Something should accumulate and be visible", and "I should define what the player can do and connect it to the engine" | Quantities as visuals; player actions as UI; animation as data | `sim_render::anim` (keyframes, 10 easings, stacking, a player); stage `piles` with remembered items (enter, leave, spoil) and `buttons` bound to declared actions through `Game::act`; per-quad rotation; `play.toml`; `--record` | 4 anim tests, 3 pile and button tests; a 6 s recording of presses (lit, squash, overshoot, refused shake) and deliveries |
| E.1 | Adding colony3d's player actions (never used by evals) changed the ants' behaviour: spring store 50 → 0. Colony3d's EVALS had already met it ("new rules shift the salts of later rules") | Adding a rule or action must not change anyone else's dice | Actions, state-machine rules and environment rules are salted by identity (name, machine, state), top-level rules keep index + 1 | Old and new colony3d `game.ron` give identical evals under the new salts; `golden_wolf_sheep_hash` and every game without state-machine rules unchanged; colony and colony3d re-baselined as engine steps (018, 001): a reseed, which showed how luck-bound their balance is on 5 seeds |
| R.4 | "An FPS where I walk forward and do things by position" (game 8, `lanes`): nothing drew from inside the world | A 3D camera that rides with an entity; effects that answer game events | `track.ron`; a 3D pass (perspective, depth, fog) between 2D passes; `math`, `fx` (event → animation over camera channels, built-in dash/jump/landing/crash), views on springs, `simcraft-import` for the road art | 4 track tests, 2 fx tests, 2 math tests; recordings in first person and chase view |
| R.5 | "1 2 3 4 are positions and the move between them is the core mechanic; the designer tunes it and it must be fun on its own" | The switch as data: where the positions are and how moving between them feels | The game holds the heading (`goto` + `lane_to`, one position a tick); the track's `switch` (positions, timing per lane, curve, hop, start/arrive effects scaled by distance, bank clamped on a spring) | `a_switch_glides_overshoots_and_lands_on_the_position_with_effects_scaled_by_distance`; `lanes_goto_moves_one_position_per_tick_and_refuses_where_you_already_are`; a scripted driver finishes with 119 switches in 40 s |
| E.2 | A test that placed the car with `move3` found it moved one cell: `Move` clamps each axis to one step, so lanes' dash (`Move("0", "2")`) had never moved two cells, silently | Multi-cell moves; and mistakes like this must report themselves | `MoveBy(dx, dy)` (exact, destination checked); `Move` asked for more emits `clamped: …`; `simcraft-check` presses the game's buttons and reports it; the edit hook runs it on every change | the check finds the clamp in a copy of lanes with the old `Move` (rule and first tick named); with `MoveBy` the scripted driver finishes at tick 366 instead of 457 |
| R.6 | "Add a replay; show how many lives I have as I hit; hitting should be smoother, funnier, and never leave me stuck" | Runs as data; damage you can see; hits that do not stop play | presses logged and saved (`runs/*.jsonl`), `R`/`N`/`--replay`; hits smash through (`Despawn(Nearest…)`, `Wobbly` instead of `Stunned`: steer on, no second hit); lives meter with bursting icons, `gone` tumble animations, `hurt` and a bouncy crash effect | `a_replay_of_a_run_is_the_same_run`; `a_hit_leaves_a_ghost_and_a_lost_life_bursts_off_the_meter` |

| E.3 | "Keep improving the test surface and linting, so efficient code reveals itself." Nothing measured work; speed was a feeling | Exact, reviewable cost per game | Work counters (evals, queries, maps; per rule checks and fires), work profile snapshots per game, `simcraft-check` cost notes, clippy perf lints, `tools/check.sh` | The profiles showed lanes copying the whole world for every button check (~360 copies/s) and forest_fire building 2048 `me` maps a tick for expressions that never read `me` |
| E.4 | The profiles, fixed | The same results for less work | World lent to queries instead of copied; per-kind plans (idle kinds skipped, `me`/`near` only where mentioned); native guards for rules and transitions (`true`, `me.p op n`) and a scope-free skip when every guard is false | Per tick: colony evals 1420 → 262, queries 686 → 118, maps 661 → 102; forest_fire maps 2048 → 0; lanes maps 231 → 8. 200 ticks, one core: colony 184 → 73 ms, forest_fire 417 → 193 ms, lanes 19 → 7 ms. Every golden hash, `fast_paths_change_nothing`, and eval `--check` (colony, colony3d, forage) identical |

---

## Request: a termite mound, played as a termite (game 9, `mound`)

"Stigmergy: the work done so far tells the next worker what to do. A new game in 3D; I play the termite with WASD
and the mouse, a smooth, termite-y feel, shift runs, space jumps." Built eval-driven: [`games/mound/EVALS.md`](../games/mound/EVALS.md)
(does work attract work?) and [`games/mound/FEEL.md`](../games/mound/FEEL.md) (how moving feels).

| # | Symptom | Need | Refactor | Evidence |
|---|---|---|---|---|
| T.1 | A termite on a mound must walk up walls; in 3D, `Wander` floats off into the air | Crawlers | `cling: true`: a clinging kind only enters voxels touching terrain (`World::can_enter`, one test for moves, leaps, `Climb`, `ClimbField`); `Wander` picks among those; `[spawn]` places crawlers on surfaces (random 3D spawning had also dropped entities inside terrain without a word) | `ground_starts_under_air_and_crawlers_start_on_it`; every other game's golden hash unchanged (the test is exactly the old one for non-clinging kinds) |
| T.2 | A test found a termite floating on the first tick: it had dug the earth under its own feet | Support that can vanish | Crawlers whose support is gone fall until they touch terrain (`World::settle_clingers`, after `apply`) | `crawlers_never_float_while_they_dig_climb_and_build` (1500 ticks); mound eval step 002 records it as an engine step |
| T.3 | You aim a mud ball two voxels away; `SetFieldAt` clamped every offset to one voxel, silently (like `Move` once did, E.2) | A reach | `SetFieldAt` is exact; the game guards the reach in `when` (no game used it, so nothing changed) | `you_dig_and_drop_exactly_where_you_reach_and_no_farther` |
| T.4 | A 40x40 ground six levels deep would be 240 layout rows | Ground under air, declared | Fields: `from_level` (`init` from that level down) | the same test |
| T.5 | The mound lives in a field; evals saw only entities | Fields for agents and evals | Protocol: `{"cmd":"field","name":F}`; eval.toml `[structure]` (built, height, stacking, pillars, roofs) in `tools/evalmetrics.py`, Cython pure-Python mode (plain Python anywhere, compiled where Cython is: 0.34 vs 2.2 ms a sample, same numbers) | mound evals 000–002; parallel probes once overwrote each other's panels ("less digging builds more"): each evaluation now has its own directory, and seeds run in parallel (27 s → 5 s, identical) |
| T.6 | First person in a world that is built while you walk in it | A voxel view, the body in the view, the truth in the game | `roam.ron`, `sim_gpu::roam` (faces with ambient occlusion, clinging bodies, mandibles, smell view), `sim_gpu::walker` (the feel as data and pure logic), `simcraft-play` roam session (raw mouse, captured cursor), `--feel`, `simcraft-check` for roam views | walker tests (glide, walls, climbing, jump, look ease, the look ray); feel step 001: the eye glides after the body, eye jolts 0.162 → 0.042 |

---

## What the engine became

| | Before the games | After five games |
|---|---|---|
| World | entity map, O(n²) `near` | grid + per-kind index, solid occupancy |
| Rule language | `when`/`then`, 8 actions, `near`/`count`/`roll` | + `around`, `rand`, `Goto`, `Move`, `On`, `Need`, `target`/`it` |
| Agents | built-in `move` | declared actions with args, act-time feedback, seats, scores |
| Game file | kinds, fsms, rules, params (unknown fields ignored) | + layout (with props), actions, end, score; strict |
| State machines (game 4) | flat FSM, first matching transition | state charts: nesting, layers, `use`, `remember`, interrupt/back, `pick`, rules bound by inheritance, composition and distance |
| Speed (wolf/sheep, ~2000 entities × 100 ticks) | 3.34 s | 0.53 s |

Deliberately not changed: the one-tick death lag (2.7) and globals (3.5).
