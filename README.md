# simcraft

A Rust-based game engine.

You write the game in a file. The engine runs it.

```
games/wolf_sheep/
├── game.ron      # the world: kinds, states, rules
└── engine.toml   # the switches: seed, population, which rules are on, parameters
```

## Run

```bash
cargo run -q -p sim-agent
```

Talk to it over stdin, one JSON line at a time:

```json
{"cmd":"info"}
{"cmd":"observe"}
{"cmd":"act","actions":[{"entity":41,"move":[1,1]}]}
{"cmd":"step","n":10}
```

It answers one JSON line at a time. Humans, scripts and AI agents all play it the same way.

## A rule

```ron
(name: "predation", for: "wolf", when: "near.sheep <= 1",
 then: [ Despawn(Nearest("sheep")), Set("hunger", "0"), Emit("kill") ]),
```

When a rule doesn't fit the built-in actions, write it in [Rhai](https://rhai.rs) instead.

## A switch

```toml
[switches]
predation = false
```

## Guarantees

- **Deterministic.** Same seed, same inputs, same world, every tick.
- **Checked before it runs.** A typo in a rule, switch or parameter stops the engine at load time.

More detail: [`docs/architecture.md`](docs/architecture.md).

## License

MIT — see [LICENSE](LICENSE).
