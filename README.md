<div align="center">

# simcraft

**You design and develop. Your AI implements. simcraft keeps both of you honest.**

A deterministic simulation engine for game makers who build with AI.<br>
One readable game file, checked before it runs, built to run the same in the terminal, Unity and Unreal.

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust 2024](https://img.shields.io/badge/rust-2024-orange.svg)](https://www.rust-lang.org)
[![Agent Skills](https://img.shields.io/badge/AI-agent%20skills-8A2BE2.svg)](skills/)

[Quick start](#quick-start) · [Build with your AI](#build-a-game-with-your-ai) · [Games](games/) · [Architecture](docs/architecture.md) · [Contributing](CONTRIBUTING.md) · [Türkçe](README.tr.md)

</div>

```
##################
#f.f..........q.q#
#.V......$.....V.#
#f.f..........q.q#
##################
Day 14/150   score  you 124  ·  bob(builder) 131
  YOU (A)  wood  14  stone   0  gold   84  houses 1
  bob (B)  wood   0  stone   3  gold   51  houses 2
  MARKET   wood $18 (stock 3)   stone $10 (stock 14)
  · bob: sell_stone 4
  · house you
> sw 3
```
<sub>`games/market`, played in the terminal: the map, prices and rules come from one `game.ron`; bob is a scripted bot.</sub>

## Why simcraft

- **You stay the designer.** The game is one text file you can read, review and version: kinds, states, rules, what players may do. Your AI writes it; you decide what it says.
- **Mistakes stop at the door.** A typo in a rule, state or parameter stops the engine at load time, with every error listed at once in words an AI can fix.
- **Same seed, same game.** Integer maths, no shared randomness: every run can be replayed and verified tick by tick, so experiments are evidence.
- **Behaviour you can see.** State machines with nested states, layers, memory and interrupts, in the vocabulary of Unity's Animator and Unreal's StateTree.
- **Hosts display, the core decides.** The same `game.ron` runs headless for experiments and inside Unity or Unreal.

## Quick start

Needs [Rust](https://rustup.rs) and Python 3.

```bash
git clone https://github.com/cemphlvn/simcraft && cd simcraft
cargo build --release -p sim-agent
python3 agents/play_market.py        # trade wood for stone against a bot, 150 days
```

Then change one number in `games/market/engine.toml` (or a `params` value in `game.ron`), play again, and see a different game.

## Build a game with your AI

```bash
skills/install.sh          # Claude Code in this repo;  --user for every project
```

Then ask, in your own words: *"make a game where …"*.

| Skill | What your AI does |
|---|---|
| [`simcraft-game`](skills/simcraft-game/SKILL.md) | Treats you as the designer: asks what the player should feel and decide, writes `game.ron`, runs the engine's check, fixes every error, runs the game and tells you what happened |
| [`simcraft-experiment`](skills/simcraft-experiment/SKILL.md) | Answers *"what happens if …"*: runs the game many times across settings and seeds and shows you the evidence |
| [`simcraft-eval`](skills/simcraft-eval/SKILL.md) | Eval-driven development: you define what "better" means, every change is one measured step on fixed seeds, and a log keeps what you learned ([`docs/evals.md`](docs/evals.md)) |

Other AI tools: point them at `skills/<name>/SKILL.md` (Agent Skills format) and at [`docs/architecture.md`](docs/architecture.md). New skill: `skills/new.sh <name>`.

## Pick your path

| You study / love | Start here | You will touch |
|---|---|---|
| **Game design** | [`games/`](games/) (smallest first: `wolf_sheep`), the skills above | `game.ron`, `engine.toml`: rules, state machines, balancing. No Rust needed |
| **Computer engineering** | [`docs/architecture.md`](docs/architecture.md), then [`CONTRIBUTING.md`](CONTRIBUTING.md) | The Rust core: rules compiler, state charts, determinism, the C API |
| **Art and design** | [`adapters/unity`](adapters/unity/com.simcraft.core), [`adapters/unreal`](adapters/unreal/Simcraft) | Prefabs and actors per kind, a look per state (`glyphs`): what the player sees while the core decides |

**Six games so far:** `wolf_sheep` (predators and prey), `forest_fire` (fires of every size), `mercy_dungeon` (fight or spare),
`market` (two players trading), `gamedev` (a studio building games on its own engine), `colony` (ants, scent trails
and winter, built [eval-driven](games/colony/EVALS.md)). How the engine grew out of
them, change by change: [`docs/emergence.md`](docs/emergence.md).

## Tour

### The two files

```
games/wolf_sheep/
├── game.ron      # the world: kinds, states, rules, what players may do   (the designer's file)
└── engine.toml   # the panel: seed, population, switches, parameters      (the operator's file)
```

### Talk to the engine

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

### A rule

```ron
(name: "predation", for: "wolf", when: "near.sheep <= 1",
 then: [ Despawn(Nearest("sheep")), Set("hunger", "0"), Emit("kill") ]),
```

When a rule doesn't fit the built-in actions, write it in [Rhai](https://rhai.rs) instead.

### A state machine

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

### A switch

```toml
[switches]
predation = false
```

### Watch and replay

```toml
[bus]
log = "runs/market.jsonl"          # every act, event and tick hash
listen = "127.0.0.1:7878"          # the same, live: nc 127.0.0.1 7878
```

```bash
cargo run -q -p sim-agent -- games/market --replay runs/market.jsonl
# {"ok":true,"verified_ticks":150,"acts":234,...}
```

### Save and load

`{"cmd":"snapshot"}` returns the whole game; `{"cmd":"restore",...}` goes back to it, and the future is bit-identical.

### Unity and Unreal

The core is also a C library (`libsimcraft`, header `crates/sim-ffi/include/simcraft.h`).
`adapters/build-native.sh` builds it for the host adapters:

- Unity: `adapters/unity/com.simcraft.core` (UPM package, `SimcraftWorld` component)
- Unreal: `adapters/unreal/Simcraft` (plugin, `ASimcraftWorld` actor, Blueprint-callable)

Hosts display; the core decides. The same `game.ron` runs everywhere.

## Status

simcraft is young. What works today, and what does not yet:

- **Worlds are grids**, one cell per position, integer props. No physics, no continuous space.
- **No renderer of its own.** The terminal shows ASCII; pictures come from a host (Unity, Unreal).
- **Host adapters:** the C API, the C++ wrapper and the C# `Simulation` are tested; the Unity `SimcraftWorld` component
  and the Unreal module are written but not yet compiled in their editors.
- **Breaking changes will happen** before 1.0. Golden hashes make sure they never happen silently to existing games.

More detail: [`docs/architecture.md`](docs/architecture.md). Want to change the engine? [`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

MIT, see [LICENSE](LICENSE).
