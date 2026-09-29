# Eval-driven game development

You decide what the game should do; the evals tell you whether a change did it. The AI makes the change, runs the
evals and reports; you read the numbers and decide the next step.

## The loop

1. **Say what "better" means**, before changing anything: `games/<name>/eval.toml` lists the metrics, each with a
   goal (`max`, `min`, or `info` for context).
2. **Record a baseline:** `tools/eval.py games/<name> --save "baseline"`.
3. **One change per step.** Change the game, then `tools/eval.py games/<name> --save "<what changed>"`. The runner
   compares every metric with the step before and says `better`, `worse` or `=` against its goal.
4. **Write down what you learned** in `games/<name>/EVALS.md`: one row per step, including steps that made things
   worse or broke. A worse step is information, not failure.
5. **Probe before you guess.** `tools/eval.py games/<name> --set param=value` tries a lever without touching the
   game. Probe one lever at a time; rank them; then pick the change.
6. **Guard what you have:** `tools/eval.py games/<name> --check` re-runs the last saved step and fails if any value
   moved. simcraft is deterministic, so a saved eval is a regression test.

## `eval.toml`

```toml
seeds = [1, 2, 3, 4, 5]      # the same seeds every step
max_ticks = 2400             # overrides [run] max_ticks for the eval
sample_every = 20            # how often series are sampled

[metrics.survived]
expr = "end_tick"
goal = "max"
```

A metric is a Python expression over one run:

| Name | Meaning |
|---|---|
| `end_tick`, `result` | When and how the run ended (`result` from `end:`, or `None` at `max_ticks`) |
| `p` | The game's params, e.g. `p['summer']` |
| `events['name']` | How many times an event fired |
| `first('name', default)` | Tick of the first such event (`default`, else `end_tick`, if never) |
| `peak(s)`, `low(s)`, `mean(s)`, `last(s)`, `at(tick, s)` | Over a sampled series `s` |
| series `count.<kind>` | Entities of a kind |
| series `state.<kind>.<state>` | Entities of a kind in a state (active part, e.g. `state.ground.Trail`) |
| series `sum.<kind>.<prop>` | Sum of a prop over a kind, e.g. `sum.nest.food` |

Metrics are averaged over seeds (mean, min, max). Make metrics honest when the game breaks: a "first time" metric
should not reward a game that ends before the event could happen (give it the worst value instead).

## Example

`games/colony`: [`eval.toml`](../games/colony/eval.toml), [`EVALS.md`](../games/colony/EVALS.md). Step 003 there
broke the game; the eval showed it at once.

## Structures (fields)

A game that builds (a mound, a trail) is measured from a field, not from entities:

```toml
[structure]
field = "mud"   # the field to read ({"cmd":"field"} at every sample)
value = 2       # the voxels that count as built
pillar = 4      # a column with at least this many is a pillar
```

Series: `structure.built`, `structure.height` (tallest column), `structure.stacking` (for each built voxel, how many
built voxels share its column, averaged: about 1 for scattered work, high when work lands on work; independent of how
much was built), `structure.pillars`, `structure.roofs` (built voxels over open air: arches, overhangs).

These metrics read every voxel of every sample, so they live in `tools/evalmetrics.py`, written in Cython's
pure-Python mode: plain Python everywhere, compiled to C by `tools/build_evalmetrics.sh` where Cython is available
(`uv run --with cython`; about 6x faster). `python3 tools/evalmetrics.py` checks both give the same numbers. ruff
(`ruff.toml`) lints the eval code, perf rules included.
