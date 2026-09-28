# Forage: eval log

Every change is one step, measured on the same 5 seeds (`eval.toml`), compared with the step before.
Raw results: `evals/NNN-*.json`. Re-run the last step: `tools/eval.py games/forage --check`.

The question: can ants that are told nothing learn to forage? Each ant's brain is its own int8 network
(`brain:` in `game.ron`); young inherit it with mutation; carrying food home keeps an ant alive and lets it breed.

| Step | Change | Late rate (deliveries / 1000 ticks) | Per ant | First breeding | Learned |
|---|---|---|---|---|---|
| – | First design (not saved): life only from deliveries, no noise input | ≈ 0 (3 seeds, 30000 ticks) | – | never | **Nothing to select.** A random brain almost never finds food *and* comes back, so nobody breeds; the rare successes came from newcomers (random search), and the control (`inherit: false`) did as well |
| 000 | Baseline: a berry eaten on the spot gives life (`snack`), a noise input, easier breeding | 192.8 (97.6–286.8) | 6.04 | 1328 | **The colony learns.** First 1000 ticks: 9 deliveries; after 15000: 193 per 1000. Finding food now pays on its own, so selection has a first rung; noise lets a brain explore instead of looping |
| 001 | **Engine, no game change:** state-machine rules salted by identity (colony step 018). A reseed | 205 (89.2–371.2) | 6.19 | 1328 | Still learns, within the noise of 5 seeds (192.8 → 205). The control is unaffected in kind (no heredity, no learning) |

## Control (not saved: the same step with `inherit: false`)

| | Late rate | Per ant | Births | Ants |
|---|---|---|---|---|
| Heredity (step 000) | 192.8 | 6.04 | 1653 | 31.8 |
| No heredity | 1.44 | 0.09 | 40 | 16.7 |

Same world, same brains at birth, same rewards; only inheritance differs: ×134. What the colony gains is learned,
and it is learned by selection.
