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

## What the engine became

| | Before the games | After three games |
|---|---|---|
| World | entity map, O(n²) `near` | grid + per-kind index, solid occupancy |
| Rule language | `when`/`then`, 8 actions, `near`/`count`/`roll` | + `around`, `rand`, `Goto`, `Move`, `On`, `Need`, `target`/`it` |
| Agents | built-in `move` | declared actions with args, act-time feedback, seats, scores |
| Game file | kinds, fsms, rules, params (unknown fields ignored) | + layout (with props), actions, end, score; strict |
| Speed (wolf/sheep, ~2000 entities × 100 ticks) | 3.34 s | 0.53 s |

Deliberately not changed: the one-tick death lag (2.7) and globals (3.5).
