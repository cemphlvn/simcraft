---
name: simcraft-eval
description: Eval-driven game development for simcraft games: define what "better" means as metrics (eval.toml), change the game one step at a time, measure every step on fixed seeds, probe levers, and keep a log of what was learned. Use when the user wants to improve, balance or fix a game with evidence, or asks to "measure", "eval", "compare" or "track" a game.
---

# simcraft: eval-driven game development

**Roles.** The human decides what "better" means and chooses every change; you propose, implement, measure and
report. Never apply a change the evals did not motivate without saying so, and never hide a step that made things worse.

## Where the truth is

- The method and the metric language: `docs/evals.md` (https://github.com/cemphlvn/simcraft/blob/main/docs/evals.md).
- The runner: `tools/eval.py` (`--save`, `--set`, `--check`).
- A full example with a broken step and probes: `games/colony/eval.toml`, `games/colony/EVALS.md`.

## The loop

1. **Metrics first.** If `games/<name>/eval.toml` does not exist, propose metrics from the designer's intent
   (one primary score, a few that explain it, some `info`), confirm them, write the file.
2. **Baseline:** `tools/eval.py games/<name> --save "baseline"`.
3. **One change, one step:** implement exactly one change, `--save "<change>"`, read the verdicts.
4. **Report** in the designer's words: what moved, what did not, what surprised you. Add a row to `games/<name>/EVALS.md`
   (change, key metrics, what was learned). Keep broken and worse steps in the log.
5. **Stuck? Probe:** `--set param=value`, one lever at a time, same seeds; present a ranked table; let the designer pick.
6. **Before handing back:** `tools/eval.py games/<name> --check` must say identical.

## Rules of the craft

- Same seeds every step, or the comparison means nothing.
- A metric must stay honest when the game breaks (a "first time" metric gets the worst value if the event never happens).
- If a metric definition changes, say so: numbers before and after are not comparable.
- Rules see the start of the tick: a prop set by one rule is not visible to another rule in the same tick. Evals catch
  these bugs fast (e.g. colony step 003).
