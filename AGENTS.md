# simcraft

Read `docs/architecture.md` first. It is the single source of truth for the architecture.

- A new game = `games/<name>/game.ron` + `engine.toml`. The engine code is not touched.
- `sim-core` knows nothing about any game. Game-specific logic never goes there.
- Determinism must not break: no `HashMap` iteration, no floats, no shared RNG, no I/O from scripts.
- After a change: `cargo test && cargo clippy --all-targets`.
- If the protocol or the rule language changes, update `docs/architecture.md` first.
