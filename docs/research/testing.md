# Research: how game and engine developers test, and what simcraft adopts

2026-09-28. Question: build a test suite library under `test/` for game developers and engine developers;
learn from Godot, Unity and Unreal; adopt an unopinionated library if one exists.

| Ecosystem | Tool | What it gives | Opinionated? |
|---|---|---|---|
| Godot | **GdUnit4** (GUT is the older, lighter one) | Assertions, mocking, and a **scene runner** that runs a scene and simulates mouse, keyboard, touch and input actions, then waits for signals or values | Yes: lives inside the Godot editor, GDScript/C# |
| Unity | **Unity Test Framework** | NUnit tests in Edit mode and Play mode (in the editor or a standalone player) | Yes: Unity's lifecycle |
| Unreal | **Automation Framework** + **Gauntlet** | Functional (level) tests; Gauntlet runs sessions on many platforms and collects results, and "does not require any specific game-side automation code or test framework" | Gauntlet is deliberately not |
| Rust | **insta** | Snapshot ("golden master") tests; `cargo insta review` shows diffs and accepts changes | No: any serialisable output |
| Rust | **proptest** | Property-based tests with shrinking to the smallest failing case | No |

## What simcraft adopts

- **insta** for snapshots and **proptest** for properties: general, unopinionated, widely used.
- Our own layer on top, in simcraft's style (data, like `game.ron`): **scenario files** = the scene-runner idea without an
  editor. A scenario loads a game, overrides the panel, steps, acts as a player, and expects things about the world;
  it can snapshot what it saw. Game developers write scenarios; engine developers write Rust tests with the same helpers.
- Determinism makes this unusually strong: a scenario is exactly reproducible, a failing run can be replayed, and a
  whole run can be a golden hash.

Sources: [GdUnit4](https://github.com/godot-gdunit-labs/gdUnit4) · [GdUnit4Net](https://github.com/godot-gdunit-labs/gdUnit4Net) ·
[Unity Test Framework guide](https://unity.com/how-to/automated-tests-unity-test-framework) ·
[Unreal Automation Test Framework](https://dev.epicgames.com/documentation/en-us/unreal-engine/automation-test-framework-in-unreal-engine) ·
[Gauntlet](https://docs.unrealengine.com/4.27/en-US/TestingAndOptimization/Automation/Gauntlet) ·
[The topography of Unreal test automation in 2025](https://andrewfray.wordpress.com/2025/04/09/the-topography-of-unreal-test-automation-in-2025/) ·
[insta](https://github.com/mitsuhiko/insta) · [Snapshot testing (Rust Project Primer)](https://www.rustprojectprimer.com/testing/snapshot.html) ·
[Rust testing libraries](https://www.rustfinity.com/blog/rust-testing-libraries)
