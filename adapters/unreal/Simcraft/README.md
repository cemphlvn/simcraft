# simcraft for Unreal

The core decides, Unreal displays. The game lives in `game.ron`; this plugin runs it.

## Install

1. From the repo root: `adapters/build-native.sh` (builds the Rust core, copies `libsimcraft.a` and `simcraft.h`
   into `Source/ThirdParty/SimcraftLib/`).
2. Copy `adapters/unreal/Simcraft` into your project's `Plugins/` and regenerate project files.
3. Put `game.ron` + `engine.toml` in `Content/Games/<name>/`; add that folder to
   *Project Settings → Packaging → Additional Non-Asset Directories to Package*.

## Use

- Place **Simcraft World**; set `GameFolder`, `KindActors` (actor class per kind), `CellSize`.
- Actors can implement **Simcraft View** (`OnSimcraftSpawn`, `OnSimcraftState(Glyph)`).
- `OnBusMessages` fires with every event (JSON array) once per frame.
- `SaveGame()` / `LoadGame(Save)`: the save string is the whole game; the future after loading is bit-identical.
- `Simulation → Request(Json)` speaks the full agent protocol (`act`, `observe`, …): see `docs/architecture.md`.

`Source/ThirdParty/SimcraftLib/include/simcraft.hpp` is plain C++17 (no Unreal types) and is tested outside Unreal.

**Status:** the C++ wrapper is compiled and tested; the Unreal module is written against UE 5 APIs
but not yet compiled in the Unreal editor.
