# Mobile: the loop from a change to a phone

How long a developer waits between changing something and playing it on a phone. The goal is the live player
(`docs/architecture.md`, Mobile core): the app installed once, a saved game file on the phone in under a second.
Until then a change rebuilds the app natively, and this table is the baseline every later step is measured against.
Under a minute is good enough for now (2026-09-30); improving it is not the current priority.

**Setup:** an Apple computer on macOS 26 with Xcode 26, and an Apple phone paired with Xcode (the app installed over Wi-Fi, no cable).
**Change:** the rope scene (`crates/sim-mobile`) after an edit to its Rust code.

| Step | Path | Build (Rust + Xcode) | Install | Launch | Total | Learned |
|---|---|---|---|---|---|---|
| 000 | `tools/mobile/build.sh ios phone`: native rebuild, `devicectl` install and launch over Wi-Fi | 33 s (Rust 17 s, incremental; the rest `xcodebuild`) | 15 s | 7 s | **~55 s** | The install moves the whole 4 MB app over Wi-Fi and the phone verifies its signature: a floor any native loop pays. The live player skips build and install for data changes |

Other paths timed the same day (single runs, for reference):

| Path | Time | Note |
|---|---|---|
| `build.sh android apk`, including the emulator's cold boot | 61 s | Rust was already built (0.4 s); most of it is the boot |
| First Rust build of `sim-mobile` for `aarch64-linux-android` (`cargo ndk`) | 67 s | Once per machine and target |
| First Rust build of `sim-mobile` for `aarch64-apple-ios` (inside `build.sh ios archive`) | 53 s | Once per machine and target |
| First `gradle assembleDebug` (AGP 9.4.1 downloaded) | 47 s | Once per machine |
| `build.sh ios phone` after adding a dependency (Core Haptics) and changing the Xcode project | ~70 s | New crates compile from scratch and the project is regenerated: a one-off per such change |
| `build.sh ios phone`, only `main.m` changed, Rust already built | 9 s build | The phone was busy (a console session): install refused, see next row |
| `build.sh ios phone`, nothing to rebuild, the phone already connected | **6 s** | The install is fast when the Wi-Fi session to the phone is already up; the 15 s in step 000 includes setting it up |

Not timed yet: the iOS simulator loop, the Android loop with the emulator already running, a TestFlight round trip.

## On the phone (the rope scene, from the stats line)

| Date | Change | fps | Frame, median / worst (ms) | Work per frame (ms) | Tick (µs) |
|---|---|---|---|---|---|
| 2026-09-30 | Baseline | 60.0 | 16.68 / 17.0 | ~1.0 | ~15 |
| 2026-09-30 | ProMotion: Info.plist allows 120 Hz and a display link asks for it (`main.m`) | **119.9** | 8.34 / 8.9 | ~0.1 | ~11 |

## Next candidates

- The live player (`simcraft serve` plus the player's reload): a data change without build or install. Target:
  save to phone in under 1 s.
- For code changes: a Debug configuration without dead-code stripping, and `xcodebuild` with a warm derived-data
  cache (most of the 16 s outside Rust).
