# simcraft

Read `docs/architecture.md` first. It is the single source of truth for the architecture.

- A new game = `games/<name>/game.ron` + `engine.toml`. Write it the way a designer wants to; change the engine only when the game hits a wall, and record it in `docs/emergence.md` (symptom → need → refactor → evidence).
- Refactors must not silently change existing games: `golden_wolf_sheep_hash` guards this.
- `sim-core` knows nothing about any game. Game-specific logic never goes there.
- Determinism must not break: no `HashMap` iteration, no floats, no shared RNG, no I/O from scripts.
- Tests live in `test/` (crate `simtest`): scenarios in `test/scenarios/*.ron`, Rust tests in `test/tests/`, snapshots in `test/snapshots/` (review with `cargo insta review`). Unit tests may stay next to their code.
- After a change: `tools/check.sh --quick` (fmt, clippy, ruff, tests, `simcraft-check` on every game; without `--quick` also evals). A diff in `test/snapshots/work__*.snap` means a game now does more (or less) work: say which and why before accepting it. After every edit a hook (`.claude/hooks/check.sh`) formats the file and runs `simcraft-check` on the game it belongs to: fix what it reports before moving on. Build it once with `cargo build --release -p sim-gpu`.
- Make mistakes visible: if the engine can notice one, it reports it where it happened (rule, tick, fix) instead of silently doing something else, and `simcraft-check` learns to catch it.
- If the protocol or the rule language changes, update `docs/architecture.md` first.
