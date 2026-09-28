#!/usr/bin/env bash
# Builds the Rust core once and hands it to every host adapter.
# The copies (libraries, header, sample games) are build output: git ignores them;
# the originals stay the single source of truth (crates/sim-ffi/include, games/).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

cargo build --release -p sim-ffi
out="target/release"

case "$(uname -s)" in
  Darwin) dylib="libsimcraft.dylib"; unity_dir="macOS";   ue_platform="Mac";   static="libsimcraft.a" ;;
  Linux)  dylib="libsimcraft.so";    unity_dir="Linux";   ue_platform="Linux"; static="libsimcraft.a" ;;
  MINGW*|MSYS*|CYGWIN*) dylib="simcraft.dll"; unity_dir="Windows"; ue_platform="Win64"; static="simcraft.lib" ;;
  *) echo "unsupported host $(uname -s)" >&2; exit 1 ;;
esac

unity="adapters/unity/com.simcraft.core"
mkdir -p "$unity/Plugins/$unity_dir" "$unity/Samples~/WolfSheep"
cp "$out/$dylib" "$unity/Plugins/$unity_dir/"
cp games/wolf_sheep/game.ron games/wolf_sheep/engine.toml "$unity/Samples~/WolfSheep/"

ue="adapters/unreal/Simcraft/Source/ThirdParty/SimcraftLib"
mkdir -p "$ue/lib/$ue_platform"
cp crates/sim-ffi/include/simcraft.h "$ue/include/"
cp "$out/$static" "$ue/lib/$ue_platform/"

echo "Unity : $unity/Plugins/$unity_dir/$dylib"
echo "Unreal: $ue/lib/$ue_platform/$static"
