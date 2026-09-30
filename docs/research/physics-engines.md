# Research: are game engines and physics engines separate things? (2026-09-30)

Question (Cem): Unity and Unreal keep "the engine" and "the physics engine" as separate pieces — how, exactly?
What are the best-known physics engines, how compatible would they be with simcraft, and how long would it take
to write our own? The point isn't physics for its own sake: it's what the survey exposes about inefficiencies in
`sim-core`'s current hand-written motion code, on the way to AAA-scale games.

Context (ground truth, not repeated here): `docs/architecture.md` ("Product direction", "Layers", "Tick loop",
"Determinism", "Continuous motion"); `docs/research/continuous-time.md` (already settled: fixed step + render
interpolation is the right shape, the bug was gating movement behind `pace` instead of integrating every tick).
Today's "physics" is `crates/sim-core/src/world.rs`: `integrate_motion` walks every moving entity once
(`glide`), height uses one gravity constant, footprint queries (`gap`, `touching`, `under`) each do an O(k) scan
over `by_kind.get(kind)` recomputing a footprint (a `BTreeMap<String, i64>` lookup on `"px"`, `"py"`, `"vx"`,
`"vy"` per call) for every entity of that kind, with no spatial index. `highway_surfers` is the first game to
lean on this (~100 moving vehicles, a rider that jumps and lands on roofs), and the product direction is a core
Unity and Unreal developers adopt through a C API, where hosts display and the core decides — which raises the
question directly: does "the core decides" include physics, or does physics stay with the host that already has
one?

## 1. Separate or the same thing? How Unity, Unreal and Godot actually draw the line

**They are separate, on purpose, everywhere this was checked.** In every engine surveyed, "the game engine" is a
host that owns the frame loop, rendering, asset pipeline and scripting API, and "the physics engine" is a
swappable library behind a narrow interface: give it bodies and shapes, step it by a fixed `dt`, read back
transforms. The pattern repeats at three different points in engine history:

- **Unity**: 3D physics is NVIDIA PhysX behind Unity's `Rigidbody`/`Collider` API; 2D physics is Box2D behind
  `Rigidbody2D`. Neither is visible as "PhysX" or "Box2D" to most users — Unity's manual documents its own
  component API, not the vendored engine (Unity Technologies, *Physics*, Unity 6 Manual). Separately, Unity's
  Data-Oriented Technology Stack (DOTS) ships **Unity Physics**, a from-scratch, ECS-native, *stateless*
  rigid-body engine (no persistent physics-world object; simulation state lives entirely in ECS components), with
  **Havok Physics for Unity** as a drop-in, API-compatible, higher-fidelity replacement for the same ECS data —
  i.e. Unity itself treats "which physics engine" as a pluggable choice behind one interface, twice over (Unity
  Technologies, *Physics integrations in Unity*, Unity 6.7 Manual).
- **Unreal**: PhysX was the default rigid-body/collision backend through UE4. UE5 replaced it with **Chaos**,
  Epic's own engine, built with Intel over several years and first shown at GDC 2019, shipping as UE5's default
  in 2022 (Epic Games / Intel, *Unreal Engine's New Chaos Physics System*, Intel whitepaper; ACM SIGGRAPH history
  archive, *Causing Chaos: Physics and Destruction in Unreal Engine*). The architecturally interesting change in
  UE5 is not the solver, it's the **scheduling**: Chaos runs physics on its own thread at its own fixed rate,
  decoupled from the game thread's tick, specifically so networked physics can stay in sync regardless of frame
  hitches (Epic Games, *UE5 Migration Guide*). That is Unreal independently arriving at exactly Fiedler's fixed-step
  argument (`docs/research/continuous-time.md`, §4c) — for physics, not for gameplay logic — and separating it
  from "the engine" by giving it its own thread, not just its own library.
- **Godot**: ships **Godot Physics**, its own engine, behind a `PhysicsServer2D`/`PhysicsServer3D` abstraction
  that any backend can implement. Godot 4.4 added **Jolt Physics** (see §2) as a second, selectable 3D backend —
  first as a community extension (`godot-jolt`), then natively in the engine as of 4.4, chosen per-project via
  `physics/3d/physics_engine` (Godot Engine docs, *Using Jolt Physics*; GameFromScratch, *Godot 4.4 Gets Native
  Jolt Physics Support*). As of this research the native integration is explicitly marked experimental and not
  yet at feature parity with the mature extension — a live example of how much work "swap the physics engine
  behind the interface" costs even with a clean abstraction already in place.

**The shared shape, restated as an interface:** a physics engine owns bodies, shapes, constraints and a fixed-`dt`
step function; the host engine owns everything else (scripting, rendering, asset streaming, gameplay logic) and
talks to physics through a narrow boundary — create/destroy body, set/read transform and velocity, step, query
(raycast, overlap). Nobody who ships a AAA engine has physics compute gameplay decisions or own "the truth" of
where an entity conceptually *is* outside of position/orientation; the host still decides what the position
*means*. This maps onto simcraft's own layering claim almost exactly (`docs/architecture.md`: "Hosts display, the
core decides") — except simcraft's own C API product direction puts *decides* on the simcraft side, which is the
opposite assignment from what Unity/Unreal do with physics (there, the host's own physics engine decides collision
outcomes, and simcraft's core would only be told about them, or would have to re-decide them for determinism —
see §5).

## 2. The physics engines themselves

| Engine | Language | License | 2D/3D | Determinism | Performance notes (found) | Maturity | Integration model |
|---|---|---|---|---|---|---|---|
| **Box2D v3** | C (v3 rewrite; v2.x was C++) | zlib | 2D | Same-machine deterministic by construction; multithreading via constraint-graph coloring (assign each constraint to a color class with no shared bodies, so colors run in parallel with no data races) keeps results independent of thread count (Catto, *Releasing Box2D 3.0*) | SIMD (body state packed to 32 bytes, fits AVX/256-bit lanes); v3 overhauled CCD to a bounded speculative + time-of-impact hybrid, replacing classic conservative advancement's unbounded iteration counts | 18 years old (GDC 2006 concept → Sept 2007 open source → v3.0 Aug 2024); huge install base (Angry Birds, Limbo, thousands of games) | Standalone C library; language bindings; not tied to any host engine |
| **Box3D** | C | zlib (per Box2D's pattern) | 3D | Not yet independently verified here; too new | "Soft step" solver for stacking/joints, successor to Box2D's sequential-impulse line | Announced June 2026, by the same author after Box2D v3 — i.e. a mature 2D author's *first* 3D engine, built over "a few years" while at a game studio (Kintsugiyama) alongside a shipping title | Standalone, early days |
| **Jolt** | C++17 | MIT | 3D | Same-machine deterministic ("replicate the inputs, replicate the simulation" — good enough for the rollback/lockstep networking Guerrilla wanted); explicit "read the Deterministic Simulation docs for the limits" caveat in its own README, i.e. cross-platform determinism is not unconditionally claimed | Multithreaded by design (streaming load/unload without locking the sim; parallel collision queries); Guerrilla reports **doubling their simulation frequency while using less CPU** after switching from a commercial engine (Rouwe, GDC 2022, *Architecting Jolt Physics for Horizon Forbidden West*) | Started as Jorrit Rouwe's personal/hobby project; shipped in Horizon Forbidden West (2022) and Death Stranding 2 (2024); 11.6k GitHub stars | Standalone; adopted wholesale by a AAA studio, replacing a commercial engine, after evaluation |
| **PhysX 5** | C++ | BSD-3 (source); free GPU binaries | 3D (+cloth/soft body/particles from the former FleX) | Not primarily marketed on determinism; float-based | GPU acceleration (CUDA) proportional to scene arithmetic complexity; added signed-distance-field collision, position-based-dynamics cloth/inflatables in v5 | NVIDIA-owned, long-lived (Unreal's default through UE4; still Unity's default 3D backend); actively released through 5.4/5.5 | Deeply embedded in Unity and (historically) Unreal; also standalone |
| **Havok** | C++ | Proprietary (licensed) | 3D | Not determinism-first; float-based | Long AAA pedigree (Skyrim, Halo); "Havok Physics for Unity" ships as an ECS-compatible drop-in for Unity Physics | Oldest commercial AAA physics brand still active (Indiana Jones and the Great Circle, Street Fighter 6) | Licensed middleware; plugs into Unity DOTS and Unreal as a swap-in for the default |
| **Bullet** | C++ | zlib | 2D/3D | Float-based, no cross-platform determinism claims found | Used by Blender, Maya, Houdini, 3ds Max, Cinema 4D — a DCC-tool physics standard more than a shipping-game one | Stable release 3.2.4 (April 2022); development described by its own community as slowed/under-resourced relative to newer engines | Standalone; widely embedded in content-creation tools, less so in recent AAA games |
| **Rapier** | Rust | Apache-2.0 | 2D and 3D | Same-machine deterministic by default; **cross-platform** determinism requires the `enhanced-determinism` feature *and* strict IEEE 754-2008 compliance on every target *and* routing all transcendental math through `nalgebra`'s `ComplexField`/`RealField` instead of native float methods (Rapier docs, *Determinism*). `enhanced-determinism` cannot combine with `simd8`; it does combine with `parallel` (identical results at any thread count) | f32/f64 only — **no fixed-point/integer mode**; the docs make no mention of one | Actively developed by Dimforge (also builds nalgebra, `parry` collision); JS/C/Python bindings | Rust-native crate; a Bevy plugin exists |
| **Avian** | Rust | MIT/Apache-2.0 | 2D and 3D | Built specifically ECS-native for Bevy — no separate "physics world" object, all state lives as ECS components; fixed-schedule stepping, no variable `dt`, no external randomness; `enhanced-determinism` feature routes all float ops through `libm` to reduce cross-platform drift, with its own automated cross-platform determinism tests | Built-in transform interpolation between fixed steps (i.e. it independently reinvented Fiedler's pattern as a first-class feature, same as Chaos and simcraft) | Newer (successor to `bevy_xpbd`), active | Deep ECS integration, Bevy-only in practice |
| **Chaos** | C++ | Bundled with Unreal (not separately licensed) | 3D | Runs on its own fixed-rate thread specifically to keep networked physics deterministic-enough to stay in sync regardless of game-thread frame time (Epic, UE5 Migration Guide) | Built with Intel; "highly parallelized, asynchronous" by design; adds native fracture/destruction | Public since GDC 2019; UE5 default since 2022; multi-year, cross-company effort | Deeply embedded in Unreal; not usable outside it |

**Reading across the row:** every engine that markets determinism as a feature (Box2D v3, Jolt, Rapier,
Avian) does so because a *game feature* needed it — Box2D's multithreading needed to not depend on thread count,
Jolt needed rollback-safe networking for a AAA studio, Rapier/Avian both frame it explicitly as "for networked
games or replay systems." None of them treat determinism as free; each names a specific cost (Rapier: cannot
combine `enhanced-determinism` with `simd8`, must avoid native float math; Box2D: the color-based scheduler
itself, plus a fixed overflow bucket solved single-threaded when colors run out; Jolt: an explicit caveat that
there are limits to read before relying on it). **None of the surveyed engines has a fixed-point/integer mode.**
That single fact matters more than any other for simcraft's fit question (§5): the entire modern physics-engine
ecosystem has converged on float + engineered determinism (careful math, strict IEEE 754 compliance, no SIMD
paths that reorder float ops), not on avoiding floats altogether the way simcraft's core does.

## 3. The techniques that make them fast, and which simcraft's `world.rs` doesn't have

A rigid-body engine's per-step pipeline, as described across the sources above and general physics-engine
literature: integrate velocities → **broadphase** (cheaply cull the O(n²) pair check down to plausible
overlaps, via a dynamic AABB tree or sweep-and-prune along a sorted axis) → **narrowphase** (exact overlap and
contact-point generation between the surviving pairs, e.g. GJK for distance/closest-points on convex shapes, EPA
to resolve actual penetration) → group bodies into **islands** via union-find over contact pairs, so a whole
resting cluster sleeps or wakes as one unit instead of per-body → a **constraint solver** (sequential impulses
with warm-started, persistent contact caches; Box2D v3 and Box3D's "soft step" and TGS variants trade a little
accuracy for much better convergence on stacks and joints) → **continuous collision detection** for fast/thin
objects that would otherwise tunnel through thin geometry in one step → write results back, ideally from data
already laid out **SoA** (structure-of-arrays) so the same loop vectorizes with **SIMD** and splits across
**threads** by color or island with no false sharing.

Against that pipeline, `crates/sim-core/src/world.rs` today has:

- **No broadphase at all.** `gap`, `touching` and `under` each call `self.by_kind.get(kind)` and linear-scan
  *every* entity of that kind, recomputing `footprint()` (four `BTreeMap` lookups: `"px"`, `"py"`, plus size) per
  candidate, per call, per tick. This is fine at `highway_surfers`' ~100 moving vehicles — it's the textbook
  case broadphase exists to avoid at higher counts (O(n·k) becomes the dominant cost exactly where a AABB tree or
  a lane-indexed grid would turn it into O(1)-ish per query).
- **No islands, no sleeping.** Every moving entity is integrated every tick (`integrate_motion` walks
  `self.motion.keys()` and every entity of those kinds) regardless of whether it's sitting still against the
  ground with zero velocity. At AAA-scale entity counts, a stationary lane of parked cars or a resting pile still
  costs a full glide() call each.
- **No real solver — one entity moves, and one axis of "collision" (footprint overlap) is a read-only query, not
  a resolved constraint.** `touching`/`under`/`gap` tell a rule *that* something overlaps; nothing pushes bodies
  apart, computes an impulse, or handles two entities that both want the same footprint next tick except through
  the existing solid-kind/one-move-per-tick apply-time conflict machinery (`docs/architecture.md`, Tick loop). That
  is a deliberate, much simpler model than rigid-body contact resolution, and it is the right model for what
  simcraft's games need today (arcade lane-runner, footprint checks) — but it does not generalize to stacking,
  joints or resting contact forces without becoming a different kind of system.
- **`BTreeMap<String, i64>` props, not SoA.** Every prop read (`px`, `vx`, `mount`, `top`...) is a string-keyed
  tree lookup, and every entity carries its own independently-allocated map. This is the right choice for "rules
  are data, props are whatever the designer names in `game.ron`" — a physics engine's fixed, compiler-known field
  layout is not a like-for-like comparison — but it is the concrete reason a hot loop like `glide()` cannot
  vectorize or even predict its own memory access pattern: each `get(p, "px")` is a tree walk, not an array index.
- **Rhai evaluated per entity per tick for rules, but not for motion itself** (integration is native Rust, per
  the architecture doc's "Physics runs natively after `apply`"), which is already the right split — the
  inefficiency to watch is in the *query* functions (`gap`/`touching`/`under`), which are native but still O(k)
  with no index, called from Rhai expressions that may run every tick for every entity of a kind.
- **No SIMD, no multithreaded physics step.** `integrate_motion` is a single pass over a `Vec<EntityId>` built
  fresh from a `BTreeSet` each call. At AAA entity counts this is a straightforward place to parallelize (each
  free mover's `glide()` touches only its own entity and its mount, which is exactly the kind of
  no-shared-mutation-between-workers structure Box2D's graph coloring and simcraft's own rule-evaluation
  parallelism already exploit elsewhere in the engine) — it simply hasn't needed to yet.

None of this is a defect relative to what `highway_surfers` needs; `docs/architecture.md`'s own efficiency
counters (`evals`, `queries`, `checks`, `fires`, a budget of "25 evals or 10 queries per entity-tick") are
designed to catch exactly the failure mode where an O(n·k) footprint scan becomes the bottleneck before it does —
the counters exist because the current model's ceiling is known and watched, not unknown.

## 4. Determinism: floats vs fixed-point, and what it costs everyone else

Every mainstream physics engine surveyed is float-based; simcraft's core is integer-only by rule
(`docs/architecture.md`, Determinism: `no_float`, `only_i64`). Three distinct determinism *strategies* were
found in the wild, and simcraft's approach is a fourth, more restrictive than any of them:

1. **"Locally deterministic" (the default for Box2D, Rapier, Jolt, most engines):** the same machine, same
   compiler, same binary reproduces the same result. Good enough for single-player replays and same-binary
   networking, useless for cross-platform lockstep.
2. **"Cross-platform deterministic" float engineering (Rapier's `enhanced-determinism`, Box2D v3's graph
   coloring, Avian's `libm`-routed math):** achieved by *constraining* float usage — strict IEEE 754-2008
   compliance on every target, no platform-native transcendental functions, careful thread-count-independent
   scheduling. This is real engineering effort with a named cost every time (Rapier: incompatible with one SIMD
   feature; general pattern: gives up some raw throughput for guaranteed bit-identical results across CPUs/OSes).
3. **Fixed-point-by-design for cross-platform lockstep (Photon Quantum):** Quantum ships its own `FP` type
   (Q48.16 fixed-point) plus `FPVector2/3`, `FPMatrix`, `FPQuaternion`, and its **own** 2D/3D physics engines
   built on that type, specifically because Quantum's whole reason to exist is deterministic lockstep
   multiplayer for Unity games where the host engine's float physics cannot be trusted across clients (Photon
   Engine docs, *Quantum 3 ECS Fixed Point*). This is the closest existing analogue to simcraft's own choice, and
   it is telling that the team that needed hard cross-platform determinism for physics **did not** adopt any of
   the float engines above and instead wrote a fixed-point one from scratch, as a for-profit, dedicated product.
4. **Integer-only, fine-unit fixed-point (simcraft):** stricter than all three — no floats anywhere in the core,
   not even behind a feature flag, verified by a same-panel, same-hash test over 300 ticks. This is the same
   family as Quantum's `FP` type, one step further (Quantum's Q48.16 is still floating-point-shaped fixed math;
   simcraft's `FINE = 1000` fine units are plain `i64` arithmetic, no fractional bits at all).

**What it costs, honestly:** nobody in this survey gets cross-platform determinism for free. Quantum pays for it
by maintaining an entire parallel math/physics stack instead of using an off-the-shelf engine. Rapier/Box2D/Avian
pay for it by giving up some SIMD paths and constraining float usage. simcraft pays for it today by having no
access to any existing physics library at all — every one of them would reintroduce floats (or, for Rapier,
merely *reduce* nondeterminism risk, not eliminate floats) the moment it touched the core.

## 5. Fit for simcraft: three paths, evaluated honestly

**(a) Embed Rapier (or another Rust engine) behind `motion`.** Rapier is Rust-native, well-documented, and its
`enhanced-determinism` story is the best-engineered cross-platform determinism found among general-purpose
engines. But it is f32/f64 throughout — there is no fixed-point mode, and the docs never mention one. Embedding
it inside `sim-core` would mean either (i) breaking the `no_float`/`only_i64` rule for the motion subsystem only
(a real architecture change, and a determinism *demotion* from "verified bit-identical" to "verified
bit-identical if every target platform is strictly IEEE 754-2008 compliant and every math call goes through
nalgebra" — weaker, and now dependent on someone else's dependency graph), or (ii) converting simcraft's fixed
units to floats at the boundary and back, which reintroduces exactly the platform-float risk the core exists to
avoid, just at a smaller radius. Rapier is also a full 2D/3D rigid-body engine (joints, stacking, soft bodies);
simcraft's actual game needs today are footprint queries and simple kinematics, not general rigid-body dynamics —
adopting it would import a large surface area (and its own broadphase/narrowphase/solver, none of which is
data-driven the way `game.ron` rules are) to solve a much smaller problem.

**(b) Keep our own integer kinematics, add a proper broadphase.** This is additive, not a rewrite: keep
`integrate_motion`/`glide` exactly as they are (they already match Fiedler's fixed-step model, confirmed correct
in `docs/research/continuous-time.md`), and replace the O(n·k) scans inside `gap`/`touching`/`under` with a
spatial index — a uniform grid keyed by cell (simcraft already has cells) or a lane-indexed structure specific to
`highway_surfers`' road shape. This preserves every determinism guarantee already proven (same integer arithmetic,
same iteration order, same hash test), costs nothing in the "rules are data" story (footprint queries stay native
functions callable from Rhai, just faster), and is a natural extension of what the engine already does elsewhere
(the world's own per-kind index, `by_kind`, *is* a crude spatial index already — this is sharpening an existing
tool, not adding a new category of one).

**(c) Delegate to the host engine's physics in the Unity/Unreal adapters.** Attractive on paper — Unity and
Unreal developers already trust PhysX/Chaos/Havok, and the product direction explicitly wants simcraft to feel
native to them. But it directly contradicts the one architectural invariant the whole product is built on:
"hosts display, the core decides," headless determinism, replays and lockstep "for free" because the same seed
and inputs produce the same hash on every platform (`docs/architecture.md`, Product direction). Every physics
engine surveyed in §2 is float-based and, at best, only *locally* deterministic by default; handing collision
outcomes to the host's physics would mean the core's hash no longer determines the game state, breaking replay
verification and any future lockstep multiplayer the moment two hosts (or two versions of the same host) disagree
by an ULP. This path is viable only for effects that are genuinely host-owned and cosmetic — ragdolls, particle
debris, camera shake — exactly the boundary `docs/architecture.md` already draws ("Physics, animation and
rendering stay in the host"), and *not* for anything a rule, a hash, a replay or an agent's observation depends
on. That boundary is already correctly drawn; the risk is scope creep past it, not the absence of a rule.

**Recommendation: (b).** Path (a) trades a small, already-correct subsystem for a large, float-based dependency
that weakens the exact guarantee (bit-identical hashes) the product is being pitched on. Path (c) is already
the design for cosmetic host-side effects and should stay bounded there. Path (b) is strictly additive, keeps
every existing test and hash green, and directly targets the one place the current design will actually break
first — an O(n·k) scan at AAA entity counts — without importing anything.

**Effort estimate**, cross-checked against what the sources actually took:

| Level | What it buys | Estimate | Evidence |
|---|---|---|---|
| Arcade kinematics + a real broadphase (path b) | Fixes the actual bottleneck (§3) for footprint queries at 10x–100x today's entity counts | **Days to low weeks** | This is strictly less work than Box2D itself: Box2D *started* as a 200-line, single-file teaching sample for a GDC talk (Catto, GDC 2006) before it grew into a general engine; a grid/AABB-tree broadphase over existing integer positions is a well-understood, self-contained data structure, not a new numerical method |
| Full rigid-body dynamics: stacking, joints, resting contacts | General-purpose physics, the thing Box2D/Jolt/PhysX/Chaos actually are | **Months to years, and longer to reach AAA robustness** | Box2D: GDC 2006 concept → Sept 2007 first public release (~1+ year to "usable"), then **17 more years** of continuous refinement to the v3.0 rewrite (Aug 2024) that finally added multithreading and a modern solver. Jolt: built by one senior AAA physics engineer (20+ years' prior experience) as an unpaid personal project before Guerrilla adopted it — no public timeline found, but it was not a weekend or a month, and Guerrilla still spent a dedicated GDC-talk's worth of integration work adapting their whole engine around it. Chaos: multi-year, multi-company effort (Epic + Intel), publicly shown 2019, still not fully at parity with the engine it replaced until UE5's 2022 release — three-plus years even with an incumbent, dedicated team and outside expert help. Even Erin Catto's *second* engine (Box3D, 2026), built by the person who already solved this once, took "a few years" |

**Honest uncertainty:** exact person-month figures for Jolt's pre-hobby-to-production timeline and for Chaos's
internal team size were not found in the sources checked here (GitHub, Guerrilla's own write-up, and Epic/Intel's
public materials do not state them); the multi-year figures above are bounded from public release dates, not
from internal schedules, and should be read as lower bounds on effort, not upper ones.

## 6. What this reveals about `sim-core`'s architecture, in the order to fix it

Ranked by which one breaks first as entity counts and game variety grow toward AAA scale, and how directly each
maps to a technique physics engines already treat as table stakes (§3):

1. **Spatial acceleration structure for footprint queries** (a grid keyed by cell, or an AABB tree over moving
   entities). This is the direct analogue of broadphase, and it is the one thing in this whole survey that every
   single physics engine has and `world.rs` does not. It is also the cheapest fix (§5, path b) and the one the
   architecture's own cost budget (25 evals / 10 queries per entity-tick) is already positioned to catch getting
   expensive.
2. **Islands / sleeping for stationary movers.** `integrate_motion` currently walks every entity with `motion`
   every tick regardless of velocity. A cheap "zero velocity and not falling → skip" fast path (in the spirit of
   the engine's existing native-guard fast paths, `docs/architecture.md` "Fast paths") would cut real work for
   parked/idle movers at scale, verified the same way existing fast paths are (`fast_paths_change_nothing`,
   hash-compared).
3. **Compiled/native guards ahead of Rhai, extended to motion-adjacent queries.** The architecture already does
   this for simple `when` clauses; the same principle — recognize a common query pattern and answer it without a
   full Rhai round-trip — applies to `gap`/`touching`/`under` call sites once a spatial index exists, since the
   index itself, not the Rhai call, becomes the cost center.
4. **SoA-friendly hot paths for motion props specifically**, without abandoning `BTreeMap<String, i64>` for
   general props. `px`/`py`/`vx`/`vy`/`ph`/`vh`/`mount` are a fixed, engine-owned set (already special-cased as
   "engine-owned props" per `docs/architecture.md`); they are the one place a parallel array (indexed by entity
   slot, alongside the existing prop map for everything else) would pay for itself the way SoA pays for it in
   every engine surveyed, without touching the designer-facing "props are whatever `game.ron` names" contract.
5. **Parallel integration of independent movers.** `integrate_motion`'s free-movers-then-riders structure is
   already close to Box2D's color-class idea in miniature (movers don't touch each other's state; riders read
   their mount's already-computed displacement) — it is a natural, low-risk target for the same core-count-
   independent parallelism the rule evaluator already has, once (1) makes per-entity motion work heavy enough to
   be worth splitting.

Notably absent from this list: a constraint solver, joints, or general rigid-body stacking. Nothing in
`highway_surfers` or any game named in `docs/architecture.md` needs them, and §5's recommendation is explicit
that adopting a general rigid-body engine (path a) would import exactly that unneeded surface area. The fixes
above make the *existing* kinematic model (positions, velocities, footprints) hold up at AAA entity counts; they
do not turn simcraft into a rigid-body physics engine, because nothing in the product direction asks it to be
one.

## Sources

1. Unity Technologies. *Physics* (Manual). https://docs.unity3d.com/2022.3/Documentation/Manual/PhysicsSection.html
2. Unity Technologies. *Physics integrations in Unity* (Unity 6.7 Manual).
   https://docs.unity.com/en-us/engine/6000.7/manual/physics-section/physics-integrations
3. Epic Games. *Unreal Engine 5 Migration Guide*.
   https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-engine-5-migration-guide
4. Epic Games / Intel. *Unreal Engine's New Chaos Physics System* (Intel whitepaper).
   https://www.intel.com/content/dam/develop/external/us/en/documents/unreal-engines-new-chaos-physics-system-screams-with-in-depth-intel-cpu-optimizations.pdf
5. ACM SIGGRAPH History Archives. *Causing Chaos: Physics and Destruction in Unreal Engine* (Lentine and Allen).
   https://history.siggraph.org/experience/causing-chaos-physics-and-destruction-in-unreal-engine-by-lentine-and-allen/
6. Godot Engine docs. *Using Jolt Physics* (4.4). https://docs.godotengine.org/en/4.4/tutorials/physics/using_jolt_physics.html
7. GameFromScratch. *Godot 4.4 Gets Native Jolt Physics Support*. https://gamefromscratch.com/godot-4-4-gets-native-jolt-physics-support/
8. Erin Catto. *Releasing Box2D 3.0*. https://box2d.org/posts/2024/08/releasing-box2d-3.0/
9. Erin Catto. *Announcing Box3D*. https://box2d.org/posts/2026/06/announcing-box3d/
10. Erin Catto. Box2D source and history. https://github.com/erincatto/box2d ; Box2D on Wikipedia:
    https://en.wikipedia.org/wiki/Box2D
11. dev.to (Wren Calloway). *The interesting part of Box2D v3 isn't the cache. It's the graph coloring.*
    https://dev.to/wrencalloway/the-interesting-part-of-box2d-v3-isnt-the-cache-its-the-graph-coloring-10o2
12. Jorrit Rouwe / Guerrilla Games. *Architecting Jolt Physics for Horizon Forbidden West*, GDC 2022.
    https://www.guerrilla-games.com/read/architecting-jolt-physics-for-horizon-forbidden-west ·
    slides: https://media.gdcvault.com/GDC+2022/Speaker+Slides/ArchitectingJoltPhysics_Rouwe_Jorrit.pdf
13. Jorrit Rouwe. Jolt Physics repository and README. https://github.com/jrouwe/JoltPhysics
14. Dimforge. *Determinism* (Rapier docs). https://rapier.rs/docs/user_guides/rust/determinism/
15. Dimforge. *Announcing the Rapier physics engine*. https://dimforge.com/blog/2020/08/25/announcing-the-rapier-physics-engine/
16. NVIDIA. PhysX 5 documentation. https://nvidia-omniverse.github.io/PhysX/physx/5.4.0/index.html
17. Havok / Wikipedia. *Havok (software)*. https://en.wikipedia.org/wiki/Havok_(software) ; https://www.havok.com/
18. Bullet Physics. Repository and license. https://github.com/bulletphysics/bullet3 ;
    https://en.wikipedia.org/wiki/Bullet_(software)
19. Avian Physics (Bevy). Repository, docs, and determinism notes. https://github.com/avianphysics/avian ;
    DeepWiki determinism page: https://deepwiki.com/avianphysics/avian/10.3-determinism
20. Photon Engine. *Quantum 3 — ECS — Fixed Point*. https://doc.photonengine.com/quantum/current/manual/quantum-ecs/fixed-point ;
    *Quantum 3 Intro*: https://doc.photonengine.com/quantum/v3/quantum-intro
21. Software Engineering Radio. *SE Radio 739: Erin Catto on Video Game Physics Engines* (episode description
    only; full transcript not accessed). https://se-radio.net/2026/09/se-radio-739-erin-catto-on-video-game-physics-engines/
22. Glenn Fiedler. *Fix Your Timestep!* — cited via `docs/research/continuous-time.md`, source 10.
    https://gafferongames.com/post/fix_your_timestep/

---

**Epistemological note.** This is a snapshot as of 2026-09-30. Performance-benchmark numbers with specific
multipliers were found for Jolt (Guerrilla's "doubled simulation frequency, less CPU") and for SoA/SIMD in
general (5–20x cited for 10,000+ entity workloads in unrelated data-oriented-design sources, not physics-specific
benchmarks) but not for head-to-head PhysX-vs-Havok-vs-Chaos-vs-Jolt numbers on comparable scenes; no source
surveyed publishes those, likely because licensing and hardware differences make fair comparison hard, and this
absence is itself worth naming rather than papering over with an unsourced number. The effort estimates in §5 are
bounded from public release dates and named authors' own retrospective framing ("a few years," "a personal
project"), not from disclosed internal schedules; treat them as plausible lower bounds, not verified durations.
Re-evaluate this document if simcraft ever needs joints, general stacking, or soft bodies — none of which any
game named in `docs/architecture.md` needs today, and none of which this research recommends building.
