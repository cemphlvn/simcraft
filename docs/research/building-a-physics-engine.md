# Research: how to start building a physics engine, and which ontologies make the optimization problems visible (2026-09-30)

Question (Cem): before `crates/sim-physics` exists, survey the canonical writing on building a game physics engine
from scratch, in the order a builder should read it; the recommended build order (first milestone, what to test at
each stage, what to postpone); and the conceptual vocabularies ("ontologies") that make performance and
correctness problems *visible*, so improvement stays iterative — measured, like the rest of simcraft.

Ground truth, not repeated here: `docs/architecture.md` ("Efficiency that shows itself", "Continuous motion", the
tick loop's determinism guarantees); `docs/research/physics-engines.md` (already surveys Box2D v3, Jolt, PhysX,
Havok, Bullet, Rapier, Avian, Chaos, and their broadphase → narrowphase → islands → solver → CCD pipeline at the
*what exists* level, and already recommends simcraft's own integer kinematics + a broadphase over embedding any of
them); `docs/research/continuous-time.md` (fixed step + render interpolation, already the architecture);
`docs/research/driving-physics.md` (the vehicle model layers, fixed-point format, BAM angles, oriented-box SAT
already scoped for `sim-physics`); `docs/plans/physics-and-vehicles.md` (the crate layout and order of work already
drafted); `games/traffic/PERF.md` and `tools/perf.py` (the measuring culture already in place — sweep-and-prune
broadphase, compiled rules, hash-on-demand, each step named, timed, and hash-checked against the world it changes);
`docs/evals.md` (the eval-driven loop: baseline, one change, measure, write it down).

This document's job is narrower than those: not *what physics engines exist* (already answered) but *how a builder
reads their teaching material in order*, and *what vocabulary turns "it's slow" or "it's wrong" into something a
profiler or a test can point at* — the two things `docs/plans/physics-and-vehicles.md`'s open questions still need
before `sim-physics`'s first commit.

## 1. The canonical reading order

Ordered the way a builder should actually encounter them — cheapest concept first, each one unlocking the
vocabulary the next uses. "Serves" names the build step (§2) each source is read for.

| Order | Source | What it teaches | Serves | Link |
|---|---|---|---|---|
| 1 | Glenn Fiedler, *Fix Your Timestep!* (2004) | Decouple simulation from render rate; the accumulator pattern | Already adopted (`continuous-time.md`); read again as the frame this whole reading list sits inside | https://gafferongames.com/post/fix_your_timestep/ |
| 2 | Glenn Fiedler, *Game Physics* series — *Integration Basics*, *Simulating a Simple Spring*, *3D Rigid Body Simulation* (2004–2011) | Euler vs. semi-implicit (symplectic) Euler vs. RK4; why semi-implicit Euler is the practical default (cheap, conditionally stable, "good enough" for game bodies); springs as the simplest constraint | Step 1 (integration) | https://gafferongames.com/categories/game-physics/ |
| 3 | Randy Gaul, *How to Create a Custom Physics Engine*, parts 1–4 (2013) | The smallest complete loop end to end: AABBs, a manifold, impulse resolution, friction, in a few hundred lines — the "toy that runs" milestone, in code you can read in one sitting | Step 2 (first milestone: boxes falling and resting) | https://code.tutsplus.com/series/how-to-create-a-custom-physics-engine--gamedev-12715 |
| 4 | Allen Chou, *Game Physics* blog series — *Resolution: Constraints & Sequential Impulse*, *Contact Constraints*, *Stability: Warm Starting*, *Stability: Slops* (2013–2014) | Why "set velocity to zero" is wrong and deriving a constraint's Jacobian is right; sequential impulses as small, repeated, convergent corrections (not one big solve); warm starting (reuse last tick's impulse as this tick's starting guess) and slop (a small allowed penetration) as the two tricks that make iterative solving both fast and stable | Step 3 (solver) | https://allenchou.net/game-physics-series/ |
| 5 | Erin Catto, Box2D GDC talks — *2006 concept talk*, *Soft Constraints* (2011), *Continuous Collision* (2013), *Solver2D* posts (2020s), *Releasing Box2D 3.0* (2024) | Sequential impulses at production quality: warm starting, soft constraints (bias as a spring, not a hard correction), bounded-iteration continuous collision (trading exactness for a guaranteed-fast answer), and — in the v3 posts — constraint-graph coloring for thread-count-independent parallelism | Step 3→4 (solver hardening, parallelism) | https://box2d.org/posts/ ; https://box2d.org/files/ErinCatto_ContinuousCollision_GDC2013.pdf |
| 6 | Dirk Gregorius, *The Separating Axis Test Between Convex Polyhedra*, GDC 2013 | SAT done right: face and edge cases via Gauss-map overlap, the actual algorithm behind "does shape A overlap shape B" for convex hulls (simcraft's 2D oriented-box case, per `driving-physics.md` §5, needs only the 4 face-normal axes, a strict subset) | Step 3 (narrowphase) | https://media.gdcvault.com/gdc2013/slides/822403Gregorius_Dirk_TheSeparatingAxisTest.pdf |
| 7 | Christer Ericson, *Real-Time Collision Detection* (2004), chapters on closest-point/distance tests and spatial partitioning | The reference once a specific shape pair or a specific spatial structure (grid, BVH, sweep-and-prune) needs to be exactly right — a lookup book, not a front-to-back read | Step 2/4 (narrowphase primitives, broadphase structures) as needed | (book; no free canonical link found) |
| 8 | Ian Millington, *Game Physics Engine Development*, 2nd ed. (2010) | A second, slower walk over the same ground as Gaul/Chou/Catto (particles → rigid bodies → collision → contact resolution) with more exposition and exercises; useful as the textbook cross-check if a step from 3–6 is unclear, not as the first thing read | Cross-check for steps 1–4 | https://shop.elsevier.com/books/game-physics-engine-development/millington/978-0-12-381976-5 |
| 9 | Mike Acton, *Data-Oriented Design and C++*, CppCon 2014 | "Where there is one, there are many": design for the batch, not the instance; know the data before writing the transform; SoA over AoS because the transform, not the object, is what runs | Step 4 (making the solver fast, not just correct) | https://www.youtube.com/watch?v=rX0ItVEVjHc |
| 10 | Jorrit Rouwe, *Architecting Jolt Physics for Horizon Forbidden West*, GDC 2022 | What breaks at AAA scale once correctness is solved: lock contention between physics and game threads, a lock-free broadphase, a lock-free island-building algorithm, streaming without stalling the sim — the chapter simcraft will need only if `sim-physics` ever runs on its own thread | Step 5 (scale, if ever needed) | https://media.gdcvault.com/GDC+2022/Speaker+Slides/ArchitectingJoltPhysics_Rouwe_Jorrit.pdf |
| 11 | Erin Catto, *Simulation Islands* (Box2D blog, 2023) and *Determinism* (Box2D blog, 2024) | Concrete, load-bearing engineering for two of simcraft's own open questions: persistent islands via union-find (§3.5) and a named, reproducible determinism test (§3.6) — both read as "how do I test this," not "what is this" | Step 5 (islands, sleeping) and the determinism test itself | https://box2d.org/posts/2023/10/simulation-islands/ ; https://box2d.org/posts/2024/08/determinism/ |

**Not on the critical path, read only if the need shows up:** Bullet's source (the DCC-tool-standard engine,
already characterized in `physics-engines.md` §2 as less actively developed and less relevant to games); Photon
Quantum's fixed-point docs (already read for `driving-physics.md` §3 — the closest analogue to simcraft's own
integer choice, not new teaching here); Rapier/Avian design notes (Rust-native, already surveyed in
`physics-engines.md` §2 for their determinism engineering, not for build-order teaching, since neither publishes a
"how we built this" narrative the way Catto/Rouwe/Gaul/Chou do).

## 2. The recommended build order

Cross-referencing Gaul's four-part structure, Chou's series order, Catto's own talks (which describe Box2D's
actual history: a 2006 single-file teaching demo before 17 years of hardening), and `docs/plans/physics-and-vehicles.md`'s
already-drafted order — the sources converge tightly on this sequence, not several competing ones:

| Stage | Build | Test at this stage | Postpone |
|---|---|---|---|
| 0 | The number type and vector/box primitives (`sim-physics::fixed`, `geom`) | Unit tests: known values, round-trip, overflow behaviour at extremes | Angles/trig (BAM16, sin/cos tables) until something needs a heading — Gaul and Chou's own toy engines start axis-aligned |
| 1 | Integration only (semi-implicit Euler), no collision at all | A single falling body's height matches the closed-form parabola within one ULP-equivalent (integer rounding) tolerance, every tick, by hand computation | Any constraint solver — an integrator with nothing to react to cannot hide a bug, which is why every source (Fiedler, Gaul, Chou) starts here |
| 2 | Broadphase + narrowphase, read-only (report overlaps, resolve nothing) | `the_broadphase_answers_exactly_what_a_scan_answers`-style property: the index agrees with an O(n²) scan on the same random scenes (simcraft already has this pattern for the traffic broadphase, `games/traffic/PERF.md`) | Continuous collision detection — Catto's own framing is that CCD is a refinement once discrete narrowphase already works, not a co-requisite |
| 3 | A minimal solver: sequential impulses, one iteration, no warm starting, no friction | Two boxes settle to a stable, non-interpenetrating rest within a bounded number of ticks; a stacked pair does not explode | Friction, warm starting, soft constraints — Chou's series explicitly orders "get an impulse that works" before "make it stable and cheap" |
| 4 | Warm starting + a penetration-slop constant + (if boxes stack) multiple solver iterations | A stack of N boxes (Jolt's own "pyramid" scene, §3.5) stays standing for M ticks without visible sinking; solver iteration count vs. residual penetration is a number you can log, not a guess | A full Baumgarte/soft-constraint tuning pass — get one constant that visibly works before tuning it |
| 5 | Islands + sleeping (only once there is something worth sleeping — i.e., resting stacks or resting traffic) | A resting body's per-tick cost drops to near zero; waking on contact propagates correctly through a chain (Box2D's own regression: a body put to sleep while its island stays half-awake causes visible overlap) | Multithreading the solver — Catto's graph coloring and Rouwe's lock-free islands are both *after* a correct single-threaded island system exists in their own histories |
| 6 | Parallel/scaled solving, if entity counts ever demand it | `tools/perf.py`-style scaling curve, same shape as `games/traffic/PERF.md`'s table: cars/bodies × ms/tick, hash unchanged at every core count | Everything in Rouwe's talk (lock-free broadphase, streaming) unless a profile names it as the bottleneck, per simcraft's own rule: "a step that does not show in the numbers is not kept" |

**What every source agrees to postpone, unconditionally, until a game asks for it:** joints (beyond what a
vehicle's own bicycle-model hinge needs), soft bodies, cloth, general polyhedral contact manifolds beyond boxes.
None of Gaul, Chou, or Catto's early material builds these first; they arrive, if at all, after stages 0–5 are
solid — the same conclusion `physics-engines.md` §6 and `driving-physics.md` §7 already reached independently for
simcraft specifically (no rigid-body stacking/joints needed for any named game).

## 3. Ontologies: what makes a performance or correctness problem visible

Each candidate, verified against a real source (not assumed), with the problem it exposes, the evidence it works,
and how it maps onto what simcraft already has (`tools/perf.py`, `docs/evals.md`, `test/`, `docs/emergence.md`).

### 3.1 Pipeline-stage decomposition, timed per stage

**What it is.** Every source in §1 that describes a working engine (Catto, Rouwe, Gaul, Chou) decomposes the tick
into the same named stages: integrate → broadphase → narrowphase → solve → (CCD) → sleep/wake. This is not a
diagram, it is a *unit of measurement*: each stage gets its own timer, and a regression in one stage is legible
without touching the others.

**What it makes visible.** Which stage is the actual bottleneck, as opposed to "the tick is slow." `PERF.md`'s own
step 003 found exactly this kind of thing by profiling: "parallel rule evaluation 56%, the world's hash every tick
13%, `integrate_motion` 11%, `apply` 7%" — a stage breakdown of a tick that is not yet physics but is already the
same idea.

**Evidence it works.** Every optimization in `games/traffic/PERF.md` (broadphase, compiled rules, bare kinds, hash
on demand) was found by first knowing which stage of the tick was expensive, then fixing that stage specifically —
the log's own "learned" column names a profile or a specific bottleneck at every step, never a blind guess.

**Maps onto simcraft.** Directly: extend `tools/perf.py`'s per-tick timing to per-stage timing inside
`sim-physics::step` (integrate / broadphase / narrowphase / solve / sleep), the same way `PERF.md` already reports
a percentage breakdown from a profile. This is additive to the existing tool, not a new one.

### 3.2 Data-oriented design (Acton): "where there is one, there are many"

**What it is.** Design the data layout and the loop for the batch case (N bodies), not the single case; separate
fields by *when they are needed* (hot per-tick state vs. cold, rarely-read configuration); prefer structure-of-arrays
so a loop over one field is a linear scan, not a chase through a tree of unrelated fields.

**What it makes visible.** Cache-miss-bound loops that look fine in isolation but scale badly — exactly the failure
`physics-engines.md` §3 already diagnosed in `world.rs` today: "`BTreeMap<String, i64>` props... every `get(p,
"px")` is a tree walk, not an array index," the concrete reason `glide()` "cannot vectorize or even predict its own
memory access pattern."

**Evidence it works.** Acton's own claim (10x from "relatively simple transformations with data usage in mind") is
qualitative in the talk; the load-bearing, already-cited evidence *for simcraft specifically* is `PERF.md` step 002
itself: replacing string-keyed prop lookups with positions "kept in the index (no string lookups per candidate)"
was the single biggest jump in the whole log (0.75 ms → 0.52 ms at 100 cars, and the shrinking-search change in the
same step took 1600 cars from 47.0 ms to 6.44 ms) — data-oriented design, already practiced, already measured, just
not yet named as such in that file.

**Maps onto simcraft.** `sim-physics` gets to start data-oriented rather than retrofit it: `VehicleState`,
contact manifolds and island membership as parallel arrays indexed by a body slot, not `BTreeMap` lookups, from the
first commit — `physics-engines.md` §6's item 4 ("SoA-friendly hot paths for motion props") already recommends this
for `sim-core`'s existing motion props; `sim-physics` is the place to do it without a migration.

### 3.3 Benchmark scenes as a fixed, named suite

**What it is.** Jolt's `PerformanceTest` and Box2D's own benchmark scenes are not ad hoc "try some numbers" — they
are a small number of *named, fixed* scenes, each chosen to stress one specific thing: Jolt's Pyramid (1,240 boxes,
stresses island splitting), Ragdoll/RagdollSinglePile (3,680 bodies with active motors, stresses the solver under
motorized constraints), ConvexVsMesh and LargeMesh (stress narrowphase against complex static geometry). Box2D's
own island benchmarks (182 pyramids / 10,010 bodies; a 2,000-box tumbler) are the same idea, one scene per
concern.

**What it makes visible.** Regressions and gains that are specific to a mechanism, not "the game got slower" —
Box2D's own persistent-islands number (182-pyramid scene: 0.69 ms → 0.01 ms, "an order of magnitude") is legible
*because* the scene isolates island building from everything else the engine does.

**Evidence it works.** The Box2D number just cited is a measured, published before/after on a fixed scene, not a
qualitative claim; Jolt's scene suite is the actual tool Guerrilla used to justify "doubled simulation frequency,
less CPU" when deciding to switch engines (`physics-engines.md` §2).

**Maps onto simcraft.** `tools/perf.py` already does exactly this for `games/traffic` (a fixed seed, growing car
counts, a saved history compared step to step) — the missing piece is a small set of *named* scenes for
`sim-physics` specifically, each isolating one mechanism the way Jolt's do: a "stack" scene (N boxes settling,
stresses the solver + islands), a "traffic" scene (what already exists), a "braking" scene (one vehicle, hard
brake, stresses the vehicle model's load transfer). `games/traffic/perf/` already stores exactly this kind of
history as JSON; the pattern generalizes without a new tool.

### 3.4 Solver convergence: iteration count vs. residual, penetration depth, warm-start reuse

**What it is.** A sequential-impulse solver's correctness is not binary — it is bounded by iteration count against
a measurable residual (how far from satisfied a constraint still is after N passes), and its *stability* is a
named, tunable tradeoff: Baumgarte/soft-constraint bias trades penetration correction speed against injected
energy ("if you adjust the velocities too little, the constraints will continue to drift; too much and the system
will explode" — Chou's and the general physics-engine literature's own framing), and a fixed "slop" (an accepted
small penetration) is what stops the solver fighting floating-point noise forever.

**What it makes visible.** Whether a solver bug is "not enough iterations" (a stack sinks visibly) or "too much
correction" (a stack pops apart) — two failure modes that look similar ("things are wrong") until iteration count
and penetration depth are both logged per tick, at which point they are opposite directions on the same number.

**Evidence it works.** Every source in §1 that discusses stability (Catto's soft-constraints talk, Chou's "Slops"
post, the Havok and Bullet forum material on Baumgarte) names the same two knobs (iteration count, bias/slop) and
the same two failure directions; this is convergent testimony across independently-built engines, not one source's
opinion.

**Maps onto simcraft.** A natural extension of the existing "efficiency that shows itself" counters
(`docs/architecture.md`): a `sim-physics` solver reports, per tick, its iteration count and worst residual
penetration the same way rules report `evals`/`queries`/`checks`/`fires` — a number that is tested like any other
output, and a `test/scenarios/*.ron`-style scene (`Expect("max_penetration <= p.slop")`) is a direct, cheap
correctness test once that number exists.

### 3.5 Islands and sleeping (union-find over the contact graph)

**What it is. Already detailed at the algorithm level in §1's Catto reading; restated here as the *ontology*, not
the implementation:** a simulation is not one graph of N bodies, it is a set of connected components (islands),
and "is this thing worth simulating right now" is an island-level question, not a per-body one — sleeping one body
in a half-awake island causes visible artifacts (overlap, dislodged joints), a failure Box2D's own history names
explicitly.

**What it makes visible.** Wasted work on things that are not moving — exactly the gap `physics-engines.md` §3
already named in `world.rs` today: "`integrate_motion` walks `self.motion.keys()` and every entity of those kinds"
regardless of velocity, so "a stationary lane of parked cars... still costs a full `glide()` call each."

**Evidence it works.** Box2D's own persistent-islands measurement (§3.3: 0.69 ms → 0.01 ms on a 10,010-body scene)
is a direct, order-of-magnitude, published number for exactly this technique.

**Maps onto simcraft.** This is `physics-engines.md` §6's item 2 ("islands / sleeping for stationary movers"),
already recommended, now given the concrete mechanism (union-find over touching pairs, merge on contact, defer
split, wake the whole island on any contact) instead of a placeholder "a cheap fast path." Verified the same way
existing fast paths are — `fast_paths_change_nothing`, hash-compared — since a sleeping body must produce bit-
identical future ticks to a body that was never allowed to sleep.

### 3.6 Determinism as a named, numeric CI test (not a property asserted, a property measured)

**What it is.** Box2D's own answer to "is this still deterministic" is not a code review — it is a fixed scenario
(*Falling Hinges*: multiple bodies, rapid movement, collision, sleeping, joint limits, run across x64/ARM and
MSVC/Clang/GCC) that emits exactly two numbers every run: how many ticks until every body sleeps, and a hash of
every body's transform at that point. On the version checked here, that was 310 ticks and `0x5e70e5fe`. A CI job
runs it on every pull request; a changed number is investigated, not silently accepted, and can be updated only
deliberately.

**What it makes visible.** The exact class of bug simcraft already guards against by different means (integer-only
arithmetic instead of float engineering) but the *testing methodology* generalizes regardless of the underlying
number format: a named scenario, a small number of emitted invariant values, run on every change, compared against
a recorded baseline.

**Evidence it works.** Box2D names three concrete causes it found and fixed this way: multithreaded write order
into shared arrays, FMA/fast-math reordering, and `atan2f` disagreeing across platforms (motivating a custom
implementation) — three real, previously-invisible bugs, each caught by this one test.

**Maps onto simcraft.** simcraft already has the stronger version of this (integer-only, `no_float`, same-hash
tests over 300 ticks, `SaveLoad` scenario steps) — the contribution here is naming Falling-Hinges-style scenario
design as the pattern to reuse for `sim-physics` specifically: a scene chosen to exercise every mechanism at once
(a stack settling, a vehicle braking hard, bodies going to sleep and being woken), one `Hash(...)` scenario step,
same shape as `test/scenarios/wolf_sheep.ron`'s existing `Hash("ee9a66d10246d6f9")` step — not a new mechanism, a
new scene.

### 3.7 Property-based testing / invariants as conservation-law tests

**What it is.** Instead of (or in addition to) example-based tests, assert something that must hold for *any*
generated scene: momentum is conserved across an elastic collision (within a tolerance), energy does not increase
without an external force, a resolved contact leaves no interpenetration beyond the accepted slop. `proptest`
already generates and shrinks such cases in other domains; the physics-specific instance is "generate random valid
scenes, assert the conservation law, not a specific number."

**What it makes visible.** Bugs an example-based test would need to be lucky to hit — an edge case in the solver
that only shows up at a specific relative velocity or shape configuration, which a hand-written scenario is
unlikely to name in advance.

**Evidence it works.** This is the weakest-evidenced ontology in this survey: the search conducted here found
property-based testing well-evidenced as a *general* technique (QuickCheck's original conception, its wide adoption
across languages) and found physics engines' own conservation checks described in isolated implementation writeups
(kinetic-energy retention thresholds for elastic collisions, momentum checks), but did not find a published,
quantified case of *combining* the two — a proptest-driven physics conservation-law suite with measured bug counts
found — in any of the major engines surveyed. Treat the combination as a reasonable synthesis, not a proven
technique with its own track record.

**Maps onto simcraft.** simcraft already has `proptest` adopted (`docs/architecture.md`, Testing) and a working
example of the exact shape needed: `the_broadphase_answers_exactly_what_a_scan_answers` (40 random roads, both
directions, sideways shifts) is already a property test whose invariant is "the fast index agrees with the naive
scan" — the direct analogue for `sim-physics` is "the solver's post-step state agrees with [conservation law]
within [slop]," generated over random valid pre-step states, the same testing shape simcraft already trusts.

### 3.8 The roofline model (memory-bound vs. compute-bound)

**What it is.** Plot achievable throughput against arithmetic intensity (work done per byte moved); a kernel below
the diagonal "roof" is memory-bandwidth-bound (more compute per byte would help), a kernel below the flat roof is
compute-bound (faster memory would not help). The "ridge point" is the arithmetic intensity above which a kernel
stops being memory-bound.

**What it makes visible.** *Which kind* of optimization is worth trying — vectorizing a memory-bound loop wastes
effort (the bottleneck is bandwidth, not arithmetic), and adding prefetching to a compute-bound loop wastes effort
the other direction.

**Evidence it works.** Well-evidenced as a general HPC/GPU performance-analysis tool (its origin and continued
use in CPU/GPU vendor documentation); no source found in this survey applies it specifically to a game physics
engine's solver loop with published numbers — the closest concrete analogue found here is data-oriented design's
qualitative cache-miss framing (§3.2), which is roofline's intuition without the formal plot.

**Maps onto simcraft.** Lower priority than §3.1–3.6: simcraft's own measuring culture (`tools/perf.py`, wall vs.
CPU milliseconds, `work.rs`'s exact per-tick counters) already distinguishes "more work" from "the same work,
slower" without needing a formal roofline plot, and at simcraft's current entity counts (hundreds to low
thousands) no profile cited anywhere in this research or in `PERF.md` has yet pointed at memory bandwidth,
specifically, as the ceiling — every bottleneck found so far (§3.1's stage breakdown) was an algorithmic one
(quadratic scans, redundant scope-building), which the existing tools already caught. Worth adopting only if a
future profile shows compute time flat while memory traffic scales — not before.

### 3.9 Amdahl's law and per-core-count scaling curves

**What it is.** The speedup from parallelizing a fraction *p* of the work is bounded by `1 / ((1-p) + p/n)` as core
count `n` grows — a hard ceiling set by whatever fraction of the work stays serial (in simcraft's case, `apply` is
explicitly single-threaded by design, `docs/architecture.md`'s tick loop).

**What it makes visible.** Whether throwing more cores at a problem is worth doing at all, and where the serial
remainder (not the parallel part) is the actual ceiling.

**Evidence it works.** A 60-year-old, universally cited result (Amdahl, 1967); not physics-specific, but directly
applicable the moment `sim-physics` considers parallelizing its solve step, since `apply`'s single-threaded write
point is exactly the kind of fixed serial fraction the law describes.

**Maps onto simcraft.** `tools/perf.py` already reports both wall-clock and summed CPU milliseconds per tick,
which is precisely the pair of numbers Amdahl's law needs (their ratio is the effective core utilization); no new
tool required, just reading the existing two columns together once `sim-physics` has anything running in parallel
worth measuring this way. Not urgent: `PERF.md`'s own numbers (2.72 ms at 1,600 cars, far under a 16.7 ms budget)
show simcraft is not yet in the regime where this ceiling matters.

### Summary table

| Ontology | Problem it makes visible | Strength of evidence | Priority for `sim-physics` now |
|---|---|---|---|
| Pipeline-stage timing | Which stage is the bottleneck | Strong (already practiced in `PERF.md`) | Adopt immediately |
| Data-oriented design | Cache-miss-bound loops from pointer-chasing layouts | Strong (measured in `PERF.md` step 002) | Adopt from the first commit |
| Named benchmark scenes | Regressions specific to one mechanism | Strong (Box2D/Jolt published numbers) | Adopt immediately, small suite |
| Solver convergence metrics | Sinking vs. exploding, opposite failure directions | Strong (convergent across sources) | Adopt once a solver exists (stage 3) |
| Islands / sleeping | Wasted work on things not moving | Strong (Box2D: order-of-magnitude, published) | Adopt at stage 5 |
| Determinism as a named CI scenario | Platform/thread/compiler nondeterminism | Strong (Box2D names 3 real bugs found this way) | Adopt immediately (simcraft's stronger version already exists; extend it) |
| Property-based conservation tests | Edge cases an example test wouldn't think to write | Moderate (technique proven generally; the physics-specific combination not found published) | Adopt once a solver exists; treat as a synthesis, not an established practice |
| Roofline model | Memory- vs. compute-bound | Well-evidenced generally, physics-specific application not found | Defer until a profile asks for it |
| Amdahl's law | Ceiling from a fixed serial fraction | Well-evidenced generally | Defer until parallel solving is attempted |

## 4. Concrete recommendation for simcraft: the minimal set, in order

1. **Per-stage timers inside `sim-physics::step`** (§3.1), reported through the same `tools/perf.py` shape
   already used for `games/traffic` — extend, don't replace.
2. **Data-oriented state from the first commit** (§3.2): `VehicleState`/contact/island data as parallel arrays
   indexed by body slot, never a `BTreeMap` lookup, inside `sim-physics` specifically (a clean-room decision,
   unlike `sim-core`'s existing props, which stay as they are per `physics-engines.md` §6).
3. **A determinism scenario in the Falling-Hinges shape** (§3.6), as one more `test/scenarios/*.ron` file: a
   scene exercising integration, broadphase, narrowphase, solve and sleep together, with a `Hash(...)` step —
   simcraft's existing determinism machinery is already stronger than Box2D's; this just gives `sim-physics` its
   own version of the same named test, from day one.
4. **A small, named benchmark-scene suite** (§3.3): "stack" (N boxes settling — the direct analogue of Jolt's
   Pyramid), "brake" (one vehicle, hard stop — load transfer and solver convergence under a single strong
   constraint), and the existing "traffic" scene once vehicles route through `sim-physics`. Each gets its own
   `perf.py`-style saved history.
5. **Solver iteration count and worst residual penetration as reported counters** (§3.4), following the exact
   pattern of `evals`/`queries`/`checks`/`fires` in `docs/architecture.md`'s "Efficiency that shows itself" —
   tested like any other output, not eyeballed.
6. **Islands and sleeping** (§3.5), only once stage 4/5 of §2's build order is reached and something is actually
   resting long enough to be worth not simulating — verified the same way existing fast paths are, hash-compared.
7. **A property-based conservation test** (§3.7), once a solver exists to test: the direct sibling of
   `the_broadphase_answers_exactly_what_a_scan_answers`, generating random valid pre-step states and asserting a
   conservation law (or the accepted slop) holds after `step`.
8. **Defer roofline and Amdahl's-law-driven scaling analysis** (§3.8, §3.9) until a profile — not a prediction —
   names memory bandwidth or a fixed serial fraction as the actual ceiling; nothing measured anywhere in this
   research or in `PERF.md` is there yet.

This order follows §2's build order directly: timers and data layout exist before there is anything to time;
determinism and a benchmark scene exist before the solver does, so the solver is built against a fixed, checkable
target from its first commit; solver metrics arrive with the solver; islands arrive only once sleeping has
something to buy; the two deferred ontologies wait for evidence, per simcraft's own standing rule that "a step
that does not show in the numbers is not kept."

## Sources

1. Fiedler, G. *Fix Your Timestep!*. gafferongames.com, 2004. https://gafferongames.com/post/fix_your_timestep/
2. Fiedler, G. *Game Physics* series (Integration Basics, Simulating a Simple Spring, 3D Rigid Body Simulation, etc).
   https://gafferongames.com/categories/game-physics/
3. Gaul, R. *How to Create a Custom Physics Engine*, parts 1–4. Envato Tuts+, 2013.
   https://code.tutsplus.com/series/how-to-create-a-custom-physics-engine--gamedev-12715
4. Chou, A. *Game Physics* series: *Introduction*, *Resolution — Constraints & Sequential Impulse*, *Resolution —
   Contact Constraints*, *Stability — Warm Starting*, *Stability — Slops*. allenchou.net, 2013–2014.
   https://allenchou.net/game-physics-series/
5. Catto, E. *Continuous Collision*, GDC 2013. https://box2d.org/files/ErinCatto_ContinuousCollision_GDC2013.pdf
6. Catto, E. *Releasing Box2D 3.0*. https://box2d.org/posts/2024/08/releasing-box2d-3.0/ (already cited in
   `docs/research/physics-engines.md`)
7. Catto, E. *Simulation Islands*. Box2D blog, 2023. https://box2d.org/posts/2023/10/simulation-islands/
8. Catto, E. *Determinism*. Box2D blog, 2024. https://box2d.org/posts/2024/08/determinism/
9. Gregorius, D. *Physics for Game Programmers: The Separating Axis Test Between Convex Polyhedra*, GDC 2013.
   https://media.gdcvault.com/gdc2013/slides/822403Gregorius_Dirk_TheSeparatingAxisTest.pdf
10. Ericson, C. *Real-Time Collision Detection*. Morgan Kaufmann/CRC Press, 2004 (book; reference chapters on
    closest-point tests and spatial partitioning).
11. Millington, I. *Game Physics Engine Development*, 2nd ed. CRC Press, 2010.
    https://shop.elsevier.com/books/game-physics-engine-development/millington/978-0-12-381976-5
12. Acton, M. *Data-Oriented Design and C++*, CppCon 2014. https://www.youtube.com/watch?v=rX0ItVEVjHc ;
    slides: https://github.com/CppCon/CppCon2014/blob/master/Presentations/Data-Oriented%20Design%20and%20C++/
13. Rouwe, J. *Architecting Jolt Physics for Horizon Forbidden West*, GDC 2022.
    https://media.gdcvault.com/GDC+2022/Speaker+Slides/ArchitectingJoltPhysics_Rouwe_Jorrit.pdf ; notes:
    https://jrouwe.nl/architectingjolt/ArchitectingJoltPhysics_Rouwe_Jorrit_Notes.pdf
14. Jolt Physics. *PerformanceTest* documentation (Pyramid, Ragdoll, RagdollSinglePile, ConvexVsMesh, LargeMesh
    scenes). https://github.com/jrouwe/JoltPhysics/blob/master/Docs/PerformanceTest.md
15. Havok. *Physics Constraint Solver Deep Dive: PGS & Baumgarte*. https://www.havok.com/blog/how-havoks-constraint-solver-works-pgs-baumgarte/
16. Bullet Physics forum. *Contact penetration resolution*; *Post Stabilization* (Baumgarte tradeoffs, general
    community framing, cross-checked against Chou's and Havok's own explanations).
    https://pybullet.org/Bullet/phpBB3/viewtopic.php?t=9082 · https://pybullet.org/Bullet/phpBB3/viewtopic.php?t=86
17. Wolfpld. Tracy Profiler documentation and repository (zones, `FrameMark`, nanosecond-resolution instrumentation).
    https://github.com/wolfpld/tracy
18. Wikipedia / general HPC sources. *Roofline model*. https://en.wikipedia.org/wiki/Roofline_model
19. General property-based testing sources (Kotest docs, LambdaClass blog) on QuickCheck-style invariant testing;
    no physics-specific proptest+conservation-law published case found (noted as a gap, §3.7).
    https://kotest.io/docs/proptest/property-based-testing.html · https://blog.lambdaclass.com/what-is-property-based-testing/
20. `docs/architecture.md`, `docs/research/physics-engines.md`, `docs/research/continuous-time.md`,
    `docs/research/driving-physics.md`, `docs/plans/physics-and-vehicles.md`, `games/traffic/PERF.md`,
    `tools/perf.py`, `docs/evals.md` — this repository's own ground truth, not repeated above.

---

**Epistemological note.** This is a snapshot as of 2026-09-30. The build-order sources (§1–§2) are strongly
convergent testimony from independently-written material spanning nearly two decades (Fiedler 2004 to Catto 2024)
and are high confidence. The performance numbers cited for data-oriented design (§3.2, Acton's qualitative "10x")
and for islands (§3.5, Box2D's measured 0.69 ms → 0.01 ms) come from different evidentiary classes — the first is
the speaker's own framing in a talk, the second is a published before/after benchmark on a named scene — and are
labelled accordingly rather than treated as equally strong. §3.7 (property-based conservation testing) is this
document's own synthesis of two separately well-evidenced ideas (property-based testing in general; conservation
checks in physics engines); no source found combines them with published results, and this gap is stated rather
than papered over. §3.8 and §3.9 (roofline, Amdahl's law) are well-evidenced as general techniques but have no
physics-engine-specific application found in this research; they are recommended as deferred, not adopted, for
exactly that reason. Re-evaluate this document once `sim-physics`'s first benchmark-scene results exist — at that
point several of §3's "evidence it works" claims should be replaced with simcraft's own measured numbers, the way
`games/traffic/PERF.md` already replaced general physics-engine folklore with its own measured steps.
