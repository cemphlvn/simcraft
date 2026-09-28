---
name: simcraft-game
description: Build, fix and extend a simcraft game (games/<name>/game.ron + engine.toml) with the human as the designer. Use when the user wants to make a new game, add a mechanic, a state machine, an action for players, or fix a game.ron / engine.toml error.
---

# simcraft: build a game with its designer

**Roles.** The human is the designer and developer: they decide what the game is, what matters, what is fun. You are the implementor: you turn their intent into `game.ron`, run it, and show them what happened. Never decide the design silently. When a choice changes the game (a number, a rule, who can do what), say it in one line and let them pick.

## Where the truth is

- Rule language, state machines, validation, protocol: `docs/architecture.md` in the simcraft repo (https://github.com/cemphlvn/simcraft/blob/main/docs/architecture.md). Read the sections you need before writing; do not guess syntax.
- Worked examples, smallest first: `games/wolf_sheep`, `games/forest_fire`, `games/mercy_dungeon` (actions, layout, win/lose), `games/market` (two players, scores), `games/gamedev` (state charts: nesting, layers, `use`, remember, interrupt/back, pick).
- Why each feature exists: `docs/emergence.md`.

## The loop

1. **Intent.** Ask what the player should feel or decide, what the world contains, how it ends. Restate it as kinds, states and rules in plain words; confirm.
2. **Write** `games/<name>/game.ron` the way a designer would read it: named params instead of magic numbers, a comment per non-obvious rule, states named in the designer's words. Put everything the operator may tune in `params` and set them in `engine.toml`.
3. **Check.** The engine refuses anything it cannot run and lists every error at once:
   ```bash
   echo '{"cmd":"quit"}' | cargo run -q -p sim-agent -- games/<name>
   ```
   `{"ok":true,"ready":...}` means it loaded and every expression was dry-run. Otherwise fix each error in `errors` (they name the rule, state or field) and check again. Do not stop at the first one.
4. **Run and look.**
   ```bash
   printf '{"cmd":"step","n":100}\n{"cmd":"observe"}\n' | cargo run -q -p sim-agent -- games/<name>
   ```
   Report to the designer in their words: what happened, which events fired, what surprised you. An `error: …` event is a runtime bug; fix it.
5. **Iterate** on what the designer says. Small steps, one mechanic at a time.

## Rules of the craft

- Deterministic by construction: integers only, randomness via `roll` / `rand(n)`. Same seed, same game.
- Rules never write the world; they propose effects, applied together. A rule sees the state from the start of the tick.
- Prefer a state machine when behaviour has phases; bind rules to states (`state: "Work"` covers everything inside `Work`) instead of repeating `when` guards.
- `enter` runs on every entry, including a resume through `remember` or `back`; put one-time side effects (spawning) in guarded rules.
- If the game needs something the language cannot express, stop and tell the designer: that is an engine change (see `AGENTS.md` and `CONTRIBUTING.md`), not a workaround inside the game.
