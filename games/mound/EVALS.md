# Mound: eval log

Every change is one step, measured on the same 5 seeds (`eval.toml`, 6000 ticks = 5 minutes), compared with the
step before. Raw results: `evals/NNN-*.json`. Re-run the last step: `tools/eval.py games/mound --check`.

The question: does work attract work? Termites dig earth and drop mud balls; a fresh ball smells, and carriers drop
more readily where it smells. The primary metric is **stacking**: for each built ball, how many built balls share its
column, averaged (about 1 when balls are scattered, high when balls land on balls). The control for any step is the
same step with the smell off (`--set drop_gain=0 --set follow=0`): what stigmergy adds is the difference.

| Step | Change | Stacking | Height | Pillars | Built | Learned |
|---|---|---|---|---|---|---|
| 000 | Baseline: first design as written | 1.13 | 2.6 | 0.2 | 145 | **No mound.** Scattered work: the control (smell off) gives 1.08. 265 balls dug, 146 placed: most termites spend the run carrying. The smell fades in about a second while a termite crawls 4 voxels a second, so a carrier rarely meets a fresh ball while it still smells |
| 001 | Stronger smell: a fresh ball `fresh` 10000 → 50000 (chosen from the probes) | 1.71 (1.36–2.05) | 4.4 | 4.6 (0–11) | 261 | **Work lands on work.** Control with the smell off stays at 1.08, so the rise is stigmergy. Balls over open air 7 → 55: the first overhangs. Seeds differ a lot (0 to 11 pillars). **Mud goes missing:** 311 drops but 261 balls. Not verified yet: two termites in one voxel dropping in the same tick (termites are not solid), or a termite left inside mud dropping onto mud. Candidate for a later step |
| 002 | **Engine, no game change:** crawlers fall when their floor is dug away (a test found a termite floating over the hole it dug under itself, on the first tick) | 1.60 (1.40–1.86) | 4.0 | 3.4 (0–7) | 264 | A little lower, within the spread of 5 seeds (001: 1.36–2.05). The control stays at 1.08. The missing mud is still there (309 drops, 264 balls) |

## Probes from 000 (one lever each, same seeds)

| Probe | Stacking | Height | Pillars | Built |
|---|---|---|---|---|
| `fresh=50000` (a fresh ball smells 5× stronger) | **1.71** | 4.4 | 4.6 | 261 |
| `drop_gain=400` (10× more ready to drop per unit of smell) | 1.65 | 4.6 | 2.8 | 311 |
| `drop_base=100` (5× more drops anywhere) | 1.47 | 4.2 | 3.0 | 690 |
| `crawl=8` | 1.16 | 2.4 | 0 | 147 |
| `follow=95` | 1.14 | 2.6 | 0 | 146 |
| `dig=5` | 1.10 | 2.0 | 0 | 134 |
| control: smell off | 1.08 | 2.0 | 0 | 135 |

Only the smell levers raise stacking; speed and following do not help while the smell is gone before anyone arrives.
More drops everywhere mostly add scattered balls.

A first probe run was thrown away: parallel probes shared their panel files (`target/eval/panel_<seed>.toml`) and
overwrote each other's settings, which showed as "less digging builds more". `tools/eval.py` now gives each
evaluation its own directory (and runs the seeds in parallel: 27 s → 5 s, identical numbers).
