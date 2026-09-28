# simcraft

**You design and develop. Your AI implements. simcraft keeps both of you honest.**

simcraft is a simulation engine for game makers who build with AI. You describe the game: who lives
in the world, what they want, what they can do, how it ends. Your AI writes it as one readable file,
`game.ron`. The engine checks every line before it runs, reports every mistake at once in words your
AI can fix, and then runs the game the same way every time.

The game file is yours: readable, versioned, reviewable. The same file runs headless for experiments,
in Unity, and in Unreal.

[Türkçe](README.tr.md)

```
games/wolf_sheep/
├── game.ron      # the world: kinds, states, rules, what players may do   (the designer's file)
└── engine.toml   # the panel: seed, population, switches, parameters      (the operator's file)
```

## One minute

Needs [Rust](https://rustup.rs) and Python 3.

```bash
git clone https://github.com/cemphlvn/simcraft && cd simcraft
cargo build --release -p sim-agent
python3 agents/play_market.py        # trade wood for stone against a bot, 150 days
```

Then change one number in `games/market/engine.toml` (or `game.ron` params), play again, and see a different game.

## Build a game with your AI

```bash
skills/install.sh          # Claude Code in this repo;  --user for every project
```

Then ask, in your own words: *"make a game where …"*. The `simcraft-game` skill makes the AI treat you as
the designer: it asks what the player should feel and decide, writes `game.ron`, runs the engine's check,
fixes every error, runs the game and tells you what happened. `simcraft-experiment` answers *"what happens if …"*
by running the game many times and showing you the evidence.

Other AI tools: point them at `skills/<name>/SKILL.md` (Agent Skills format) and at `docs/architecture.md`.

## Pick your path

| You study / love | Start here | You will touch |
|---|---|---|
| **Game design** | `games/` (five games, smallest first: `wolf_sheep`), the skills above | `game.ron`, `engine.toml`: rules, state machines, balancing. No Rust needed |
| **Computer engineering** | [`docs/architecture.md`](docs/architecture.md), then [`CONTRIBUTING.md`](CONTRIBUTING.md) | The Rust core: rules compiler, state charts, determinism, the C API |
| **Art and design** | `adapters/unity`, `adapters/unreal` | Prefabs and actors per kind, a look per state (`glyphs`), what the player sees while the core decides |

Five games so far, each in its own folder under `games/`: `wolf_sheep`, `forest_fire`, `mercy_dungeon`, `market`
(two players), `gamedev` (a studio building games on its own engine). How the engine grew out of them, change by
change: [`docs/emergence.md`](docs/emergence.md).

## Talk to the engine

```bash
cargo run -q -p sim-agent            # wolf/sheep; add a path for another game: -- games/market
```

Talk to it over stdin, one JSON line at a time:

```json
{"cmd":"info"}
{"cmd":"observe"}
{"cmd":"act","actions":[{"entity":41,"do":"move","args":{"dx":1,"dy":1}}]}
{"cmd":"step","n":10}
```

It answers one JSON line at a time. Humans, scripts and AI agents all play it the same way.
Scripted players live in `agents/` (e.g. `python3 agents/market.py speculator builder`).
Play the market yourself against a bot: `python3 agents/play_market.py`.

## A rule

```ron
(name: "predation", for: "wolf", when: "near.sheep <= 1",
 then: [ Despawn(Nearest("sheep")), Set("hunger", "0"), Emit("kill") ]),
```

When a rule doesn't fit the built-in actions, write it in [Rhai](https://rhai.rs) instead.

## A state machine

States inside states, layers side by side, reusable machines, remember, interrupt and back, pick:

```ron
"Work": (
    remember: true, recheck: true,
    pick: First([ ("Commute", "me.x != me.hx"), ("Build", r#"near_in("project", "Production") == 0"#) ]),
    states: { "Commute": (...), "Build": (use: "focus", rules: [ ... ]) },
),
```

Rules bind to states by inheritance (`state: "Work"`), composition (rules written inside a state)
and distance (`depth`, `steps_to("Shipped")`, `around("dev", "Burnout", 6)`).

## A switch

```toml
[switches]
predation = false
```

## Watch and replay

```toml
[bus]
log = "runs/market.jsonl"          # every act, event and tick hash
listen = "127.0.0.1:7878"          # the same, live: nc 127.0.0.1 7878
```

```bash
cargo run -q -p sim-agent -- games/market --replay runs/market.jsonl
# {"ok":true,"verified_ticks":150,"acts":234,...}
```

## Save and load

`{"cmd":"snapshot"}` returns the whole game; `{"cmd":"restore",...}` goes back to it, and the future is bit-identical.

## Unity and Unreal

The core is also a C library (`libsimcraft`, header `crates/sim-ffi/include/simcraft.h`).
`adapters/build-native.sh` builds it for the host adapters:

- Unity: `adapters/unity/com.simcraft.core` (UPM package, `SimcraftWorld` component)
- Unreal: `adapters/unreal/Simcraft` (plugin, `ASimcraftWorld` actor, Blueprint-callable)

Hosts display; the core decides. The same `game.ron` runs everywhere.

## Guarantees

- **Deterministic.** Same seed, same inputs, same world, every tick.
- **Checked before it runs.** A typo in a rule, switch or parameter stops the engine at load time.

More detail: [`docs/architecture.md`](docs/architecture.md). Want to change the engine? [`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

MIT — see [LICENSE](LICENSE).
