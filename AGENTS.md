# simcraft

Read `docs/architecture.md` first. It is the single source of truth for the architecture.

- A new game = `games/<name>/game.ron` + `engine.toml`. Write it the way a designer wants to; change the engine only when the game hits a wall, and record it in `docs/emergence.md` (symptom → need → refactor → evidence).
- Refactors must not silently change existing games: `golden_wolf_sheep_hash` guards this.
- `sim-core` knows nothing about any game. Game-specific logic never goes there.
- Determinism must not break: no `HashMap` iteration, no floats, no shared RNG, no I/O from scripts.
- After a change: `cargo test && cargo clippy --all-targets`.
- If the protocol or the rule language changes, update `docs/architecture.md` first.
