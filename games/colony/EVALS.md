# Colony: eval log

Every change is one step, measured on the same 5 seeds (`eval.toml`), compared with the step before.
Raw results: `evals/NNN-*.json`. Re-run the last step: `tools/eval.py games/colony --check`.

| Step | Change | Survived (mean ticks) | Deliveries | First delivery | Learned |
|---|---|---|---|---|---|
| 000 | Baseline: design as written | 137.6 | 2.0 | 69 | Nurses bred into famine; nobody found food; all starved on one tick |
| 001 | Brood needs a reserve (`brood_reserve = 20`) | 133.4 | 1.8 | 79 | Slightly *worse*: fewer births means fewer foragers. Breeding was not the bottleneck; finding food is. Kept (sound biology, matters once food flows) |
| 002 | Scouts search outward (`MoveAway("nest")` when no scent) | 146.4 | 2.8 | 79 | Better on every goal; trails start to form (8 cells) |
| 003 | Staggered starvation (own tolerance per ant) | 1 | 0 | – | **Broken, caught by the eval.** Rules see the start of the tick: `starve` read the default tolerance 0 before `temperament` set it. The metric `first_delivery` also scored dying early as "fast" |
| 004 | Fix: tolerance starts at −1; `first_delivery` scores 2400 if never | 186.2 | 3.2 | 1001* | +27 % over step 002. *2 of 5 seeds never deliver (scored 2400) |
| 005 | More scouts (`scout_chance` 2 → 6) | 275.6 | 9.6 | 75 | Every seed now finds food; trails 21 cells. Same numbers as the probe (determinism) |
| 006 | Easier recruitment (`contacts` 2 → 1, panel) | 322.6 | 19.8 | 75 | Deliveries double; one seed reaches winter. `winter_store` still 0 |
| 007 | Richer world: a berry is worth 6 food (`berry_value` 1 → 6; step 006 re-checked identical with the new param at 1) | 1734.2 | 165.2 | 75 | 3 of 5 colonies live all five years; winter store 89. Probes: 3 → 439, 4 → 568, 5 → 1134, 6 → 1734, 10 → 1804 but boom and bust (67 ants, one crash at tick 484) |
| 008 | Measure income and food per trip (no game change; `--check` identical on the old metrics) | 1734.2 | 165.2 | 75 | Baseline for value: 991 food brought home, exactly 6 per trip |
| 009 | Berry value fluctuates: per-bush quality (50–150 %) × seasonal ripeness (0 → peak mid-season → 0) | 865 | 78.2 | 72 | **Worse (−50 %).** Food per trip barely moved (6.3), deliveries halved: ripeness starts at 0 each summer, so the colony meets its early famine with its poorest food (a *spring gap*) |
| 010 | Trails weighted by value (scent = value × `trail_per_value`) | 861.6 | 41.2 | 72 | **Worse again.** Food per trip fell (6.15): no concentration on the best bushes. Low early values mean weak early trails, exactly when the colony needs them. The mechanism is real in ants; here the constraint is timing, not choice |
| 011 | Refactor: seasons moved to a shared environment (`envs/seasons.ron`, read as `env.seasons.*`) | 861.6 | 41.2 | 72 | **Identical** to step 010 on every metric and seed (`--check`): the engine change and the move changed nothing the game does |
| 012 | Measure the loop: `loop_closed` (alive after 5 years), `spring_store` (no game change) | 861.6 | 41.2 | 72 | The loop closes in 0 of 5 worlds. Colonies die in late winter or the first 20 days of summer; autumn is the drain (full appetite, falling supply) |
| 013 | Perception: agents sense the world (`senses`), perfect senses | 861.6 | 41.2 | 72 | **Identical.** Ants, bushes and the nest no longer read `env`/`tick`; the engine refuses direct reads |
| 014 | Metabolism follows *felt* warmth (nest climate at home, open air outside) | 2400 | 203.6 | 66 | **Loop closed 5 of 5.** The nest has a climate: insulation, solar gain, workers' heat (all from wood-ant research) |
| 015 | Brood grows with warmth (lay chance × felt warmth) | 2135.2 | 197.8 | 66 | Mixed: stores up (spring 85 → 102), one world lost (4 of 5). Births barely move (5.4 → 5.6): brood is limited by nurses, not warmth |
| 016 | Ants move with warmth (walking speed ∝ temperature, min 10 %) | 1149.8 | 61.8 | 78 | **Loop 1 of 5.** Slow ants in the cool start of summer break recruitment and trails: scent evaporates per tick while ants crawl |
| 017 | Fix: no walls. Ants are not solid, so "search outward" walked them into the wall ring (seen in the viewer) | 948.2 | 49 | 78 | Within the noise of 5 seeds (loop still 1 of 5). Correctness, not tuning |
| 018 | **Engine, no game change:** state-machine and environment rules get their random salt from their identity (name, machine, state), not their position, so adding a rule or an action elsewhere never changes their dice (see colony3d EVALS, Next) | 836.4 | 43.4 | 540.6 | A reseed, not a change: every roll is new. It shows how luck-bound the colony is on 5 seeds: one seed never finds food (first delivery 2400), survival 948 → 836 |

## Probes after step 004 (one lever each, `--set`, not saved)

| Lever | Survived | Deliveries | Reading |
|---|---|---|---|
| `patience = 160` | 186 | 3.2 | No effect: searchers don't give up early, they never get there |
| `meal_every = 32` | 219 | 4.6 | Buys time, fixes nothing |
| `start_food = 80` | 271 | 8.0 | Buys time, fixes nothing |
| `contacts = 1` | 228 | 8.6 | Recruitment: one seed reaches winter (30 deliveries), high variance |
| `scout_chance = 6` | 276 | 9.6 | Discovery: every seed finds food (first delivery 53–114) |

Foraging is a positive feedback loop (deliveries → recruits at the entrance → trails → deliveries). It needs a spark:
enough scouts to find food before the store runs out. Discovery lights it, recruitment sustains it.

## After step 006: what limits the colony (a wrong guess, corrected by probes)

First guess: *supply*. 7 bushes × 1 berry per 12 ticks ≈ 0.58 food/tick against 12 ants eating ≈ 0.75 food/tick.
**Wrong.** Probing faster regrowth (`regrow_every` 8, 6, 4, 3) changed nothing at all, and at tick 200 every bush was
full. The real limit was **haul rate**: one berry per 20–30-tick round trip ≈ 0.04 food/tick per forager, so feeding
12 ants would take ~19 foragers. A trip has to be worth more than the forager eats (like a seed that feeds many
harvester ants): hence `berry_value` in step 007.

## After step 007: next question

`births` stays at ~4 even with plenty of food. Observed in seed 1: nurses 8 → 3 → 0 by tick 120 (age polyethism turns
every nurse into a forager, and brood was held back early), so the colony can live but cannot grow. Seed 1 also shows
an early famine: 13 ants at tick 60, 1 ant at tick 120, before foraging pays off; the lone survivor lasts to tick 538.

## Probes around steps 014–016

- **Thatching** (ants at home who feel cold rebuild insulation), probed after 014: *worse* (loop 5 → 4 of 5). In the model
  warmth only costs (more burn), nothing rewards it. Hence 015/016: warmth must also pay (brood, speed). Left off.
- After 016: `min_speed` 30 → loop 1/5, 50 → 0/5; `warmth_lag` 0 → 0/5, 120 → 2/5. Non-monotonic: at 5 seeds the
  differences are within noise now. Next decisions need more seeds.
- Missing physics: pheromone evaporation should slow in the cold (volatility falls with temperature), so cold ants are
  slow but their trails last longer. The scent currently ignores temperature.
