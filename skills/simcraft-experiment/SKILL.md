---
name: simcraft-experiment
description: Tune and test a simcraft game by running it many times: sweep engine.toml parameters and seeds, compare outcomes, and report emergent results to the designer. Use when the user asks "what happens if…", wants balancing, or wants to know which strategy or setting wins.
---

# simcraft: experiments with the designer

**Roles.** The designer asks the question and judges the answer; you design the runs, execute them, and report evidence. Say what you will vary and how many runs before running.

## Where the truth is

- Panel (`engine.toml`): `[run]` seed and length, `[params]` overrides, `[switches]` rules on/off, `[agent]` seats. See `docs/architecture.md` (https://github.com/cemphlvn/simcraft/blob/main/docs/architecture.md).
- Scripted players: `agents/market.py`, `agents/mercy_dungeon.py`. Example of a sweep and how results are written up: `docs/emergence.md` (game 3 strategy table, game 4 culture sweep).

## Method

1. **Question → variables.** One question per experiment ("does crunch help?"). Pick the params/switches to vary and what to measure (scores, event counts from `step`, props from `observe`).
2. **Panels.** Write one panel per setting (e.g. under a scratch folder) and run with `--config`:
   ```bash
   cargo build -q --release -p sim-agent
   ./target/release/simcraft-agent games/<name> --config /tmp/panel_a.toml
   ```
   Drive it over stdin/stdout JSON (one request per line, one reply per line). Step in large batches (`{"cmd":"step","n":240}`) and count `events` by `name`.
3. **Seeds.** At least 3 seeds per setting; report the mean and say when seeds disagree.
4. **Report** a small table (setting → outcome) and 2–3 plain sentences: what changed, what did not, what no rule states directly (the emergent part). Distinguish evidence from your interpretation.
5. **Keep it** if the designer wants: add the table to `docs/emergence.md` under the game.

Determinism means a surprising result can always be replayed: note the seed and panel.
