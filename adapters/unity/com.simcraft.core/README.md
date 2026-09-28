# simcraft for Unity

The core decides, Unity displays. The game lives in `game.ron`; this package runs it.

## Install

1. Build the native library and copy it in: `adapters/build-native.sh` (from the repo root).
2. Package Manager → *Add package from disk* → `adapters/unity/com.simcraft.core/package.json`.

## Use

1. Put `game.ron` and `engine.toml` anywhere under `Assets/` (they import as TextAssets).
2. Empty GameObject → *Add Component* → **Simcraft World**; assign both files.
3. Play. Each entity gets a GameObject: the prefab you set for its kind, or a coloured cube.

- Prefabs can implement `ISimcraftView` to hear about state changes (`OnSimcraftState(glyph)`).
- `BusMessages` gives every event (JSON array) once per frame.
- `Save()` / `Load(save)`: the save string is the whole game; the future after `Load` is bit-identical.
- `Sim.Request(json)` speaks the full agent protocol (`act`, `observe`, …): see `docs/architecture.md`.

`Simcraft.Simulation` and `Simcraft.Native` do not use UnityEngine and can be used from any .NET code.

**Status:** `Simulation`/`Native` are tested with plain .NET (`dotnet run --project adapters/unity/tests`).
`SimcraftWorld` and the importer are written against Unity 2021.3+ APIs but not yet compiled in the Unity editor.
