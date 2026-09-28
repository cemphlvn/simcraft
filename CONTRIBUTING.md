# Contributing to simcraft

[Türkçe](CONTRIBUTING.tr.md)

You design and develop; your AI can implement. That holds for contributions too: use any AI tool you like,
but **you** own the design and the review. If you cannot explain a line of your pull request, it is not ready.

## Four ways to contribute

| Contribution | Where | Needs Rust? |
|---|---|---|
| **A game** | `games/<name>/game.ron` + `engine.toml` | No |
| **An engine change** | `crates/` | Yes |
| **A host adapter or view** (Unity, Unreal, art) | `adapters/` | No (C#, C++, assets) |
| **A skill** (how an AI should help with something) | `skills/<name>/SKILL.md` | No |

## A game

Write it the way a designer wants to read it: named params, a comment per non-obvious rule, states named in
the designer's words. It must load (`echo '{"cmd":"quit"}' | cargo run -q -p sim-agent -- games/<name>`) and run
without `error:` events. Add a scenario in `test/scenarios/` that plays it and expects what matters
(`cargo run -p simtest -- test/scenarios/<name>.ron`; format in `docs/architecture.md`, "Testing"). If you found something interesting (a strategy that wins, a tipping point), add it to
`docs/emergence.md` with the numbers.

## An engine change

simcraft is not designed up front: **the engine changes when a game hits a wall.** Write the game first, the way you
wish you could; when it fails to load or cannot express something, that failure is the reason for the change.

1. Record it in [`docs/emergence.md`](docs/emergence.md): symptom → need → refactor → evidence.
2. If the protocol or the rule language changes, update [`docs/architecture.md`](docs/architecture.md) **first**.
   It is the single source of truth.
3. Keep the rules of the core ([`AGENTS.md`](AGENTS.md), which your AI tool reads too):
   - `sim-core` knows nothing about any game.
   - Determinism: no `HashMap` iteration, no floats, no shared RNG, no I/O from scripts.
   - Existing games must not change silently: golden hashes guard them. If a hash changes, say why in the PR.
4. Before you open the PR:
   ```bash
   cargo test && cargo clippy --all-targets
   ```

## A host adapter

The core decides; the host only displays. Adapter code talks to the C API (`crates/sim-ffi/include/simcraft.h`)
through the wrappers (`Simulation` in C#, `simcraft.hpp` in C++). Build the native library with
`adapters/build-native.sh`. The Unity wrapper has plain .NET tests: `dotnet run --project adapters/unity/tests`.
Say in the PR what you compiled and ran, and in which editor version.

## A skill

```bash
skills/new.sh simcraft-<topic>     # from skills/_template
skills/install.sh                  # try it in Claude Code
```

A skill tells an AI how to help a human with one job. Keep the roles clear (the human designs and decides, the AI
implements and reports) and **point to the repo instead of copying it**: link `docs/architecture.md`, example games
and scripts, so the skill never drifts from the engine.

## Pull requests

- One idea per PR, small enough to review in one sitting.
- Include the evidence: a test, a hash, a table, a screenshot.
- Code, comments and docs are in English; README and this guide also exist in Turkish, keep both in step.
- Be kind in review. Everyone here is learning, including the engine.
