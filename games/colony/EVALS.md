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
