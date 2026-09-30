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
| T.7 | "Enhanced 3D optimization, multicore." Measured first: a mound frame took ~8.6 ms (2.2 building it, 6.1 in the GPU column, the same at 320x180: vertex work, not pixels) and a tick 0.77 ms on 1 core or 10 (0.56 ms with no termites at all) | Frames and ticks that scale with what matters | Terrain kept on the GPU, fog in the shader, bodies culled and coarser far away, a table for sRGB→linear, bodies, mesher and colour conversion on all cores; ticks: parallel from 48 active entities, fused multicore diffusion, fields hashed a value at a time (byte-wise FNV over 64k voxels was 0.5 ms a tick) | Frame 2.18 → 0.24 ms, GPU 6.09 → 1.72 ms, worst frame 5 ms; mound tick 0.77 → 0.35 ms, colony3d 0.28 → 0.20, wolf_sheep 0.20 → 0.17. Every game's hash is the same at 1 core and at all; colony3d and mound get new fingerprints (fields hashed differently) with `eval --check` identical, every other golden hash unchanged |
| T.8 | "Some frames drop. Is there a way to sweep for them, and debug the source and the state of that moment by its hash? And stress what could get heavy (many termites, smoke)." | A drop that can be caught, rebuilt and blamed on a phase | `sim_gpu::perf`: per-frame phases and counts, spike reports (hash, camera, action log), `--repro` (replay to the hash, render cold/warm/rebuilt), `--sweep` (twice: same frame and hash = the game's), `--stress` (every drawn kind, the terrain, the shown field, four levels) | `a_spike_rebuilds_to_the_same_state_by_its_hash` (700 frames, the same hash and the same vertices); a sweep at a 6 ms budget: a 34 ms frame in one run, none in the other, no spike in both (from outside); a rebuilt spike: a dig or a drop costs +0.4 ms to build and +0.24 ms to draw, the GPU's first draw 10.8 ms; stress: nothing breaks 16.7 ms (960 termites p95 10.1 ms, worst 11.9 ms), 1920 do not fit a 40x40 ground, a world full of smoke stays at ~85k vertices (drawn within 8 voxels of you only). A report from another build is refused by its hash. (A worktree built into the shared `target/` had left stale crates behind: tests and first measurements used old code until `cargo clean -p` of every member) |
| T.9 | "Focus on the modelling engine: build models in the best tools, bring them here easily, wrap them with Higgsfield assets, and define how the engine expects them, with the state machine and core props. As realistic as you can." | Models from other tools, bound to the game's states | glTF import (`sim_gpu::model`), instanced GPU skinning with PBR (`sim_gpu::skin`), the `model` contract in views, `simcraft-model`, `simcraft-check` for contracts; Blender headless tools (`rig_insect.py`, `inspect.py`), `tools/tileable.py`. On the way: Blender's heat weighting failed on generated meshes (all levels, even from a remeshed proxy), replaced by weights by distance to the bones; the GPU device asked for no storage buffers (WebGL2 limits), now it asks for what the GPU has | `a_rigged_model_loads_with_its_clips_sockets_and_textures`, `walking_moves_the_legs_and_the_weights_hold_together`; `simcraft-check` names a wrong state, clip or socket at every level of detail; close-up shots of the Tripo termite (glossy head, segmented translucent abdomen, the ball in its mandibles); 960 model termites p95 9.6 ms at 1280x720 |
| T.10 | "The termites look real, the mound is cubes." | An organic surface through the same voxels | `surface: Smooth` (surface nets, relaxed corners, smooth normals, occlusion); outside the world is untouched ground up to its first height (a hole at the border, where pits met the edge, had been there with cubes too: found with a placed camera, `--eye`) | `a_smooth_surface_keeps_flat_ground_on_the_voxel_faces_and_rounds_a_pillar`; a sweep found the new mesher rebuilding in 7 ms every few frames (the spikes); a prefix sum for occlusion and per-cell normals: no frame over 10 ms in two runs |
| T.11 | 115 of 120 termites held a ball at tick 3000 | A colony that keeps building | Eval-driven (`games/mound/EVALS.md` 003–004): a metric for the share carrying, then tired arms (`tire`: the drop chance grows with the time carried), chosen from five probes | carrying 96.6% → 31.6%, nine times as much built, stacking 2.55 against a smell-off control of 2.06; the stacking metric turned out to grow with density (documented; read against a control) |
| T.12 | The look of Empire of the Ants (`docs/research/macro-visuals.md`) | A macro camera | `lens`: depth of field (`sim_gpu::post`), sun shadow maps for terrain and skinned models, haze towards the sun | shadows darken 25% of a test frame; +3 ms GPU per frame at 1280x720; close-ups of a termite sharp against a blurred mound |
| T.13 | Crawlers stood on voxel faces while the drawn surface was rounded | Standing on what is drawn | `SurfaceMap`: each crawler on the drawn surface's nearest vertex plane, oriented by its normal | `crawlers_stand_on_the_drawn_surface_and_tilt_with_its_curve`; per-foot placement (leg IK) is not done: the legs still play their clip |


## Request: a mobile lane game, surfing on traffic (game 10, `highway_surfers`)

"A mobile game with a great game feel, camera movements, a lane game like Subway Surfers called Highway Surfers;
we won't make assets until the feel is distinguishing for mobile; arrow keys on desktop." You ride the roofs of moving
traffic and hop between lanes ([`GDD.md`](../games/highway_surfers/GDD.md), [`PLAN.md`](../games/highway_surfers/PLAN.md)).

| # | Symptom | Need | Refactor | Evidence |
|---|---|---|---|---|
| H.1 | A rider and its roof drifted apart: `pace` gives every entity its own phase | Speeds on one shared clock | None in the engine: every lane moves on `tick % period == 0` (`tick` through a sense), and the rider moves on its roof's beat, *including the tick it lands* (a first version used the old lane's beat that tick and slid off the roof it had just landed on) | `highway_surfers_ride.ron`: a hop and a jump land, a minute of riding costs nothing |
| H.2 | Traffic as pictures has no roof to stand on, and there are no assets yet (on purpose) | Greybox volumes whose tops are surfaces | Track `kinds.X.block` (size, colour, lift, `surface`): shaded boxes; `lift_by` (a coin at the height in its prop) | `a_rider_stands_on_its_roof_jumps_on_an_arc_and_lands_back_on_it` |
| H.3 | The third-person body sat at a fixed height; hops and jumps were camera tricks | A rider that stands on what is drawn under it and flies on arcs that land on the game's tick | Track `rider`: parts (a greybox stack), `air` (state → apex; the arc lasts the `timer` prop's ticks, sampled between ticks), gravity when the ground drops away, `poses`, and `ground`: in those states it stands on the road, since the picture follows the rules (a test caught it standing on a truck the game had knocked it off) | `a_rider_the_game_puts_on_the_road_falls_to_it_and_stays_there_as_traffic_runs_it_over` |
| H.4 | The chase camera was glued to the rider; from an outside lane half the screen was desert, and traffic behind filled the bottom of a portrait screen | A camera with its own feel | `camera.follow_height`/`rise`, `lag`, `follow_x`, `speed_fov`, `speed_streaks`; blocks between the rider and the camera fade to ghosts; fx channels `squash` and `tilt` for the body; `hitstop` (the world stops, the camera keeps shaking); `screen` (a portrait window and shots); arrow keys up/down | `traffic_behind_the_rider_fades_and_the_camera_frames_the_road_from_an_outside_lane`, `a_hitstop_stops_the_world_then_eases_back`; contact sheets of a bot's replay |
| H.5 | Being run over cost two lives: the `Wobbly` layer only turns on a tick after the hit | Invulnerability that starts with the hit | In the game: the hit rules check the `guard` timer, not the state | `highway_surfers_fall.ron`: one life per hit |

| H.6 | Playtest: "cars are bad, everything is like teleporting, is there something our system is fundamentally missing?" Measured (`FEEL.md` 000): a nearby vehicle stood still in 68% of frames, then covered three frames of road in one | Continuous motion (research: `docs/research/continuous-time.md`: the field's answer is a fixed step with sub-cell positions and velocities, and drawing between steps; event-driven exact simulation does not pay at this scale) | Engine `motion` on a kind: fine positions (1000 to a cell), velocities, height and gravity, mounts (a rider is carried), integrated by the engine every tick; footprint queries `ahead`, `behind`, `touching`, `under`, `prop_of`; `SpawnAt`; `Tween::place` draws fine positions. Kinds without `motion` are untouched | `motion_tests` (4); golden hashes unchanged; `FEEL.md` 001–002: stall 0.683 → 0.000, step ratio 3.04 → 1.001 at 60 ticks a second |
| H.7 | The renderer drew its own curve and arcs for the rider; drawing the simulated position instead showed a snap (rider jerk 0.267) | The picture is the simulation | A moving followed entity is drawn from its fine position; the game steers across with eased velocity instead of snapping to the lane | `FEEL.md` 003: rider jerk 0.008, half the renderer's old curve |
| H.8 | 1,527 entities simulated to show a few dozen ("cars are not optimized") | Traffic only around the player | In the game, with the footprint queries: directors ride along with the surfer and send vehicles in where there is room; far vehicles leave | `FEEL.md` 004: 478 entities (84 vehicles), tick 1.17 → 0.38 ms, frame 1.7 → 0.24 ms |
| H.10 | Playtest: "after the first game it starts playing by itself, I lift my hands". `R` meant *replay* (inherited from `lanes`); every player presses R to restart, and the replay (their own presses, played back) looked like the game playing itself, again on every R. The only sign was the window title | Keys that mean what players expect, and states visible on screen | `R`/`N` = new run, `V` = watch again (after a run only); the run's end dims the screen, a replay reddens its edges; the key decisions moved from the window into `TrackPlay::intent` | `nothing_plays_by_itself_and_r_restarts_instead_of_replaying` |
| H.11 | "Two directions, jumps between them, stay on top and move, jump in any direction with a swipe" | A two-way road, a barrier with ramps, swipes, holding | Track `road.median`, `wedge`/`pulse` blocks; `swipes` (eight directions and a tap, moves tried in order; mouse drag = touch); buttons `hidden` and `release`; in the game: `leap(dx, dy)`, `vault`, `big_jump` beside a ramp, `walk`, `crouch`/`stand`, a forgiving landing band. The oncoming side carries you back: the surfer starts 700 rows in (a bot fell 18 times at the start of the world before that was seen) | `highway_surfers_big_jump.ron`, `a_swipe_has_one_of_eight_directions_or_is_a_tap`; the bot makes 3 BIG JUMPs a minute, no falls |
| H.12 | "Three totally different themes (CYBERRUN, BLUE OCEAN with marimba vibes, a bonus), cars to environment to the player" | One game, many worlds | Track `themes` (overrides by name), pictures over invisible blocks facing their direction, a pictured rider, music with `rodio` (the first sound in simcraft); 42 images and 3 loops from Higgsfield (`assets/src/README.md`) | a shot per theme; `simcraft-check` loads every theme's pictures |
| H.13 | A bot's minute took minutes: every `observe` drew the whole 3,200-row road as ASCII and listed every coin | Observing only the surroundings | Agent protocol: `observe` takes `near: R`; eval `[player] near` | the bot's observations shrink to the 91 rows around it |
| H.14 | Playtest: "it still stutters sometimes: is our engine not optimized, or are we missing a physics engine?" Measured (`--feel`, now with spikes): a frame cost 31 ms on average and up to 90 ms, every frame over budget; a tick 3.2 ms. A profile (macOS `sample`) put 89% of the time in one path | Questions that cost what they ask | Two engine fixes, no physics engine needed: a window asked the game whether each of its 12 keys could be pressed on *every frame* (each question evaluates all the surfer's senses), now once a tick and only for drawn buttons; `nearest_prop(kind, prop, r, ..)` searched the whole world for the nearest one before checking `r` (radius 0 = one cell, but it ring-searched up to 3,200 rows), now `World::nearest_within` stops at `r` | frame 31 → 0.08 ms, worst 90 → 1.0 ms, frames over 8 ms 600 → 0, tick 3.2 → 0.47 ms; `a_bounded_search_finds_what_the_unbounded_one_finds_within_its_radius`; golden hashes unchanged. Research on physics engines: `docs/research/physics-engines.md` |
| H.15 | "Start, and go eval-driven; make new games if the engine needs them" (physics research §6). A new game to stress the need: `traffic`, a four-lane highway driven by the Intelligent Driver Model and a light MOBIL (cars keep a speed-dependent distance and change lanes when blocked). Measured with a new tool: 1,600 cars cost 257 ms a tick (quadratic: 4× the cars, 20× the time) | A broadphase, then rules that do not pay the interpreter every tick | `tools/perf.py` (tick cost at growing counts, the world's hash compared between steps); `World::index_motion`: sweep and prune per column, rebuilt after each tick's motion, walks stop when nothing further can be nearer, positions kept in the index; `ahead_id`/`behind_id`; moving kinds start one per cell. `sim_rules::native`: rule expressions compiled once and run natively (closure compilation), the rest in Rhai; `SIMCRAFT_NATIVE_CHECK` | `games/traffic/PERF.md` 000–003: 257 → 4.97 ms a tick at 1,600 cars (51×), CPU 1,851 → 18 ms (100×), same world at every step; `the_broadphase_answers_exactly_what_a_scan_answers`; `fast_paths_change_nothing` covers compiled rules; the native check found its own slot bug on the first run |
| H.16 | "Build it like an engine simulator, so any car can be built with it", and a first-person race on a NASCAR oval as dogfooding | A physics layer, cars and tracks as data | `sim-physics` (fixed point, binary angles, CORDIC, tracks of banked arcs, a dynamic bicycle, driver aids, a lap planner and an autopilot); `vehicle:` and `track:` in the rule language; the core steps vehicles after the rules, with state in hidden props | oracle tests (circle, understeer gradient, μ·g skidpad, a banked turn held by the slope, braking distance, top speed); `games/race/LAPS.md`: a stock car laps the Charlotte-sized oval in 30.37 s against the 29.355 s pole; four of the eight steps spun, each for a named reason |
| H.9 | Evals measured games nobody plays; "teleporting" was an impression | Games measured as played; feel measured as drawn | `tools/eval.py` `[player]` (a Python policy per game), `max_seconds`; `--feel` for tracks | `EVALS.md` / `FEEL.md` 000–004; the probe's own flaw (counting a stopped world) found and fixed at step 002 |

A scripted player (`games/highway_surfers/bot.py`) predicts landings from the rules alone and never misses one: the
rules are simple enough to be read by a program, which is what evals of difficulty will need.
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
