# Mound: eval log

Every change is one step, measured on the same 5 seeds (`eval.toml`, 6000 ticks = 5 minutes), compared with the
step before. Raw results: `evals/NNN-*.json`. Re-run the last step: `tools/eval.py games/mound --check`.

The question: does work attract work? Termites dig earth and drop mud balls; a fresh ball smells, and carriers drop
more readily where it smells. The primary metric is **stacking**: for each built ball, how many built balls share its
column, averaged (about 1 when balls are scattered, high when balls land on balls; it also grows with density, so it
is read against its control). The control for any step is the
same step with the smell off (`--set drop_gain=0 --set follow=0`): what stigmergy adds is the difference.

| Step | Change | Stacking | Height | Pillars | Built | Learned |
|---|---|---|---|---|---|---|
| 000 | Baseline: first design as written | 1.13 | 2.6 | 0.2 | 145 | **No mound.** Scattered work: the control (smell off) gives 1.08. 265 balls dug, 146 placed: most termites spend the run carrying. The smell fades in about a second while a termite crawls 4 voxels a second, so a carrier rarely meets a fresh ball while it still smells |
| 001 | Stronger smell: a fresh ball `fresh` 10000 → 50000 (chosen from the probes) | 1.71 (1.36–2.05) | 4.4 | 4.6 (0–11) | 261 | **Work lands on work.** Control with the smell off stays at 1.08, so the rise is stigmergy. Balls over open air 7 → 55: the first overhangs. Seeds differ a lot (0 to 11 pillars). **Mud goes missing:** 311 drops but 261 balls. Not verified yet: two termites in one voxel dropping in the same tick (termites are not solid), or a termite left inside mud dropping onto mud. Candidate for a later step |
| 002 | **Engine, no game change:** crawlers fall when their floor is dug away (a test found a termite floating over the hole it dug under itself, on the first tick) | 1.60 (1.40–1.86) | 4.0 | 3.4 (0–7) | 264 | A little lower, within the spread of 5 seeds (001: 1.36–2.05). The control stays at 1.08. The missing mud is still there (309 drops, 264 balls) |
| 003 | Metric: share of termites carrying at the end; lever: tired arms (`tire`, off), no game change | 1.60 | 4.0 | 3.4 | 264 | Every value as 002. **The colony locks up: 96.6% of termites hold a ball at the end**: digging is easy, dropping waits for a strong smell |
| 004 | **Tired arms:** the chance to drop grows by `tire` (10) per 100 000 for each tick carried (chosen from the probes) | 2.55 (2.51–2.59) | 6.2 | 108.6 | 2298 | **Carrying 96.6% → 31.6%, nine times as much built**, roofs 49 → 914. Smell-off control at the same settings: stacking 2.06, pillars 36: the smell still adds +0.5 stacking (as in 001: +0.5) and three times the pillars. Missing mud: 2497 drops, 2298 balls (8%) |

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

## Probes from 003 (tired arms, one lever each)

| Probe | Stacking | Height | Pillars | Built | Carrying at the end |
|---|---|---|---|---|---|
| `tire=10` | **2.55** | 6.2 | 108.6 | 2298 | 31.6% |
| `tire=30` | 2.54 | 6.6 | 116.8 | 2679 | 19.2% |
| `tire=3` | 2.47 | **7.0** | 80.8 | 1844 | 49.6% |
| `tire=1` | 2.38 | 6.6 | 58.0 | 1412 | 61.2% |
| `drop_base=100` | 2.17 | 6.2 | 28.4 | 916 | 73.4% |
| control at `tire=10` (smell off) | 2.06 | 4.8 | 36.4 | 2340 | 26.6% |

**The metric, corrected:** stacking was described as independent of how much is built; it is not. When balls are
dense even random drops land on each other (the control's stacking rose 1.08 → 2.06 with nine times the balls). From
004 on, `density` (balls per column) is logged, and stacking is read against its control at the same settings.

A first probe run was thrown away: parallel probes shared their panel files (`target/eval/panel_<seed>.toml`) and
overwrote each other's settings, which showed as "less digging builds more". `tools/eval.py` now gives each
evaluation its own directory (and runs the seeds in parallel: 27 s → 5 s, identical numbers).
