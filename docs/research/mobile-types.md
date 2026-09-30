# Design note: the fundamental types of casual mobile games, and what simcraft needs

2026-09-30. Input: the five reference games (screenshots in the session: Smash Fest, Idle Cat Gunner, Pixel Flow,
Arrows, Block Out), Türkiye's top five (Royal Match, Royal Kingdom, Match Factory, Color Block Jam, Pixel Flow),
Twisted Tangle (ropes), and the layering model of AutoGameUI (arXiv 2411.03709). Output: the types a mobile core
must have, checked against what `sim-core` and `sim-physics` already provide. Not built on any existing simcraft
game: the mobile infrastructure is designed from these types.

## 1. The games, taken apart

| Game | Board | Pieces | The verb | Space and physics | Containers | Numbers |
|---|---|---|---|---|---|---|
| **Arrows** | Grid | **Polylines** on the grid with a head direction | Tap: the piece slides out along its head | **Ray along a direction**: blocked if any cell ahead is occupied | – | Hearts (3) |
| **Block Out** / Color Block Jam | Grid with walls | **Polyominoes** with a colour | Drag: **slide until blocked** along an axis | Grid kinematics; **gates** on the border (colour and width) | Generators, elevators (moving cells) | Timer, lock and move counters |
| **Pixel Flow** | A picture as a grid of coloured cubes | Shooters (colour, ammo) | Tap: send a shooter onto the belt | **A closed path** (the belt) that carries riders; each fires along a row or column at the first cube of its colour | **Bench of 5 slots**, belt capacity | Ammo per shooter |
| **Royal Match** (match-3) | Grid, cells with **layers** (floor, piece, blocker with HP) | Tiles with a colour | Swap two neighbours | **Gravity cascade** (fall and refill) | – | Moves, goals, blocker HP |
| **Match Factory** | A 3D bowl | A pile of objects | Tap to pick an object | **Rigid-body pile** | **Tray of 7**: three alike clear | Timer |
| **Smash Fest** | A pedestal (table) | Cans, jars, stone, wood, glass | Aim (drag), fire | **Rigid-body stacks** that stand, then topple; a projectile; **materials** (break, heavy, bouncy) | – | Shots |
| **Twisted Tangle** | Pegs | **Ropes** between pegs | Drag a rope's end to another peg | **Rope constraints**; crossings | – | – |
| **Idle Cat Gunner** | An open 2D field | Cats, enemies, bullets (hundreds) | Merge guns (drag), else automatic | **Continuous movers**, circle hits | **Merge grid** | **Huge numbers** ("6.89T", "542.8AL") |

## 2. The types, factored out

| # | Type | Used by | In simcraft now | Needed |
|---|---|---|---|---|
| T1 | **Grid board** with walls | Arrows, Block Out, match-3, Pixel Flow | `World` cell grid, `solid` | – |
| T2 | **Cell layers** (floor, piece, blocker; each with its own occupant) | Match-3 blockers, Block Out ice and locks | One solid per cell | Layers per cell (a piece and a blocker in one cell) |
| T3 | **Shaped pieces**: polyomino, polyline; one entity covering many cells | Arrows, Block Out, Color Block Jam | One entity, one cell | A `shape:` on a kind: the cells it covers, relative to its anchor, occupied as one solid |
| T4 | **Grid rays and slides**: first occupant along a direction; slide until blocked | Arrows (exit test), Block Out (drag), Pixel Flow (line of fire) | `ahead`/`behind` for movers only | `first_along(dir)`, `slide(dir)` for shaped pieces |
| T5 | **Paths**: a curve with riders at a distance along it | Pixel Flow belt, conveyors, lanes | `mount` (riding an entity) | A path with a length, riders at `s`, capacity |
| T6 | **Slots / trays**: a bounded, ordered container that clears on N alike | Pixel Flow bench (5), Match Factory tray (7), Screwdom holders | Props and rules can fake it | A declared container, so "stuck" (full) is an engine event every eval can count |
| T7 | **Gravity cascade** on a grid | Match-3 | Rules can do it slowly | Later (not on the path of the next game) |
| T8 | **Continuous 2D bodies** with collision (circles, boxes) | Cat Gunner, projectiles | `motion` (position, velocity) with no collision response | Tier-2 physics (below) |
| T9 | **Rigid-body stacks** (rest, then topple) | Smash Fest, Match Factory | 2D box contact for cars (`sim_physics::contact`) | Tier 2: warm starting, resting contact, sleeping; 3D later |
| T10 | **Constraints** (ropes, chains, struts) | Twisted Tangle, the next game | – | **Tier 1: integer Verlet** (below) |
| T11 | **Materials** (mass, bounce, friction, break threshold) | Smash Fest, ropes | Car-specific constants | A material table any body or point refers to |
| T12 | **Levels and progression** | All of them (hundreds to thousands of levels) | One layout per game | `levels/*.ron` plus a sequence; stars, hearts, boosters |
| T13 | **Big numbers** | Idle games | `i64` props (up to 9.2 × 10¹⁸) | A mantissa-exponent number, for idle games only; later |
| T14 | **Pictures as levels** | Pixel Flow | – | An importer: image → grid of colours → level |

**Common to all:** portrait, one thumb, a board in the middle third, a HUD at the top (level, coins, hearts, timer), boosters at the bottom, popups (win, fail, offers).

## 3. Physics as tiers

| Tier | What | Where | For |
|---|---|---|---|
| 0 | Grid kinematics: rays, slides, gates, cascades | `sim-core` queries (T4) | Arrows, Block Out, Pixel Flow, match-3 |
| 1 | **Constraints**: Verlet points, `DISTANCE` / `MAX` (a rope: slack allowed) / `MIN` (a strut), weights (a pinned point weighs infinitely), a fixed number of relaxation passes, integer `Fx` throughout | **`sim_physics::verlet`** (new) | Ropes, chains, bridges, cloth strips |
| 2 | Rigid bodies: circles and boxes, sequential impulses with warm starting, slop, sleeping | `sim_physics` (grows from `contact`) | Stacks, piles, projectiles |

Tier 1 comes first: the next game uses ropes, it's small, and it's the tier whose determinism is easiest to prove (fixed pass count, fixed order, exact integer square root). apelann's Unity ropes and Cut the Rope's source (`legendary-mobile-games.md`) are the two reference points. Their difference (a rod versus a rope, equal split versus weights) is built in from the start.

## 4. Layers of a mobile screen

AutoGameUI models a game screen as a **tree of nodes** with position, rotation, scale, **anchor**, opacity, texture and font, a **hierarchy**, an explicit **rendering order**, and a **meaning** (text, image, button, list, slider, toggle, progress bar). It keeps the *UX tree* (functional controls) separate from the *UI tree* (art) and matches them. For a mobile core that becomes:

| Layer (back to front) | Holds | Anchored to |
|---|---|---|
| `backdrop` | Sky, scenery (drawn once, rarely changes) | The whole screen, cropped to fill |
| `board` | The world: the grid or the physics space, drawn by a 2.5D camera | The **safe area**, fitted (never cropped) |
| `pieces` | Entities, ropes, bodies | The board's coordinates |
| `fx` | Particles, flashes, trails (game feel) | The board, or the screen for shake |
| `hud` | The UX tree: level, coins, hearts, timer, booster bar; each node bound to a prop or an action | Safe-area edges (top, bottom) |
| `overlay` | Popups: win, fail, pause | The screen centre |

Rules:
- The **UX tree is data** (what a node shows, which action it fires). The **skin is separate** (which picture draws it), matched by node name, so a designer changes the art without touching the controls.
- Every node has an **anchor** and **render order**, so one layout fits every aspect ratio from 16:9 to 21:9 and every notch.
- The simulation never sees a layer. Layers read the world and bus events.

## 5. The mobile core (`sim-mobile`), smallest useful version

1. **Shell:** winit + wgpu on iOS (a static library in an Xcode project generated by `xcodegen`) and Android (`cargo-ndk` + a Gradle project with `NativeActivity`, NDK r28+ for Play's 16 KB pages). Lifecycle: drop the surface on suspend and recreate it on resume. Portrait. Safe areas.
2. **Loop:** a fixed simulation tick with interpolation between ticks. Frame time never changes a result.
3. **Gestures** (pure, testable without a phone): tap, drag, swipe, pull-and-release, and **swipe-across** (the finger's segment against a shape's segments: catching a rope, cutting one).
4. **Layers** (§4), drawn as simple shapes first (rectangles, circles, lines, rope strips); textures later.
5. **Haptics** as an interface with a no-op backend on desktop, Core Haptics on iOS, `VibrationEffect` on Android. Called from bus events, never from the simulation.
6. **Build:** `tools/mobile/build.sh ios|android`, producing a signed-for-development app for the simulator or a device, and an archive or AAB ready for store signing.

The first scene on it is new: a rope you catch by swiping across it (Tier 1 plus swipe-across). It proves physics, gestures, layers, haptics and the build in one screen.

## Sources

The screenshots and research of `mobile-games.md`, `legendary-mobile-games.md`, `mobile-template.md` ·
[AutoGameUI, arXiv 2411.03709](https://arxiv.org/abs/2411.03709) ·
[winit on Android](https://docs.rs/winit/latest/winit/platform/android/index.html) ·
[android-activity](https://lib.rs/crates/android-activity) ·
[16 KB page sizes](https://16kbchecker.com/guides/16kb) ·
[Xcode 26 requirement](https://www.developer.apple.com/news/upcoming-requirements/) ·
[Privacy manifests](https://developer.apple.com/news/?id=pvszzano)
