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
