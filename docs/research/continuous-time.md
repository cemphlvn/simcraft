# Research: continuous time in a fixed-tick, integer-only engine (2026-09-29)

Question: `highway_surfers` moves one whole cell every N ticks (speed via `pace`), so a rider or a car looks
like it teleports instead of gliding. Before building a motion layer, survey who has worked on continuous time
in simulation and rendering, and what they were each trying to achieve, against simcraft's constraints:
determinism across platforms (replays, lockstep, bot evals), no floats in the core (fixed-point is fine),
efficiency with a few dozen visible entities and hundreds simulated, rules as data, rendering at 60–120 Hz.

Today's engine (`docs/architecture.md`, "Tick loop", "Determinism", "First person: tracks"): fixed ticks,
`i64` everywhere, positions are whole grid cells, one move per entity per tick, `sim_render::feel` interpolates
positions between the last two ticks for display. Speed is "one cell every N ticks" (a `pace`-gated event), not
a continuously integrated quantity — the actual bug: for N−1 ticks `prev == curr` (nothing to interpolate, the
entity is dead still), then on the Nth tick it snaps a whole cell and the interpolator stretches that one jump
over a single frame. That reads as a teleport, not as the "fix your timestep" glide it is supposed to be.

## 1. Functional Reactive Programming: continuous time as a semantic

**Idea.** Model a time-varying value (a *behavior*) as a literal mathematical function of time, `Behavior a =
Time -> a`, with a denotational semantics: what a behavior *means* does not depend on any sampling rate,
frame rate, or event loop. Discrete happenings are separate first-class *events* (time-tagged occurrence
streams). Composition (map, integrate, switch between behaviors) is defined on these semantic objects, then
implemented underneath however is efficient.

**Who, when.** Conal Elliott and Paul Hudak, *Functional Reactive Animation* ("Fran"), ICFP 1997, Microsoft
Research. Elliott's later *Push-Pull Functional Reactive Programming* (Haskell Symposium 2009) and
*Denotational design with type class morphisms* (2009) refine the same programme.

**North star (their words).** Fran's motivation: multimedia animation authoring "has long been a complex and
tedious job," suffering "from the lack of sufficiently high-level abstractions, and in particular from the
failure to clearly distinguish between modeling and presentation" — i.e., resolution independence: define the
motion once, sample it at any resolution. The 2009 follow-up names the cost of the first approach directly:
"FRP has [...] simple and powerful semantics, but has resisted efficient implementation" because naive
(demand-driven/polling) FRP recomputes values "even when inputs don't change, and reaction latency can be as
high as the sampling period." Push-pull FRP combines data-driven and demand-driven evaluation so "values are
recomputed only when necessary, and reactions are nearly instantaneous."

**Efficiency evidence.** Qualitative, implementation-level (Haskell laziness/thunks): the 2009 paper's entire
point is that the naive model is wasteful and push-pull fixes it; no cross-language benchmark numbers.

**Verdict for simcraft.** Not adopt the machinery: Fran/push-pull FRP lives in a lazy, garbage-collected,
float-native host, exactly the properties simcraft's core forbids (determinism, no floats, no shared mutable
event graph). Borrow the semantic idea directly, though: define an entity's motion as a pure function of time
rather than as a sequence of steps. That is precisely the "closed-form trajectory" alternative in Cem's
question, and it is a useful frame for the *renderer* side of the design (see §7).

## 2. Discrete-event and quantized-state simulation: DEVS, QSS

**Idea.** Instead of stepping time forward in fixed increments and recomputing every state variable at every
step, quantize the *state*: a variable only produces an event when it crosses to the next quantum level
`q`, at a time computed in closed form from its (piecewise-linear or -quadratic) trajectory. Nothing is
recomputed between crossings. QSS1/2/3 differ in the order of the polynomial approximating each variable's
derivative.

**Who, when.** Bernard Zeigler formalized DEVS (Discrete Event System Specification) in 1976; the
quantized-state idea (Zeigler & Lee) was turned into the QSS family by Ernesto Kofman and Junco (~2001) and
developed with François Cellier (*Continuous System Simulation*, 2006), implemented in PowerDEVS.

**North star.** Exploit *heterogeneous activity*: in most continuous systems, at any instant only a few state
variables are actually changing fast while the rest sit near-constant; simulate exactly the variables that are
moving, with a mathematically bounded (quantum-sized) error instead of a fixed step's unprovable one.

**Efficiency evidence.** "More than one order of magnitude faster than the most efficient classic [fixed-step]
solvers" on stiff/heterogeneous systems (Castro, Bergonzi, Pecker Marcosig, Fernández & Kofman, 2024 review);
some reported cases reach three orders of magnitude; stand-alone QSS solvers also beat DEVS-hosted (PowerDEVS)
implementations by more than 10×.

**Verdict for simcraft.** Not for entity motion: QSS's numerical guarantees are stated over real-valued ODEs
with a floating-point error bound, which fights the "no floats" constraint and buys nothing for
rule-driven, designer-authored movement (a lane change is a discrete decision, not an ODE). The *idea* —
quantize state, fire only on threshold crossing — is a better match for simcraft's continuous **fields**
(diffusion/decay, already ticked every voxel every tick): a future field could skip voxels whose value hasn't
moved past a quantum since the last diffusion pass. Not proposed here; noted for `docs/research/` if fields
ever become the bottleneck.

## 3. Event-driven exact simulation: molecular dynamics, kinetic data structures

**Idea.** When an object's future position is known in closed form (e.g., constant velocity until the next
collision), don't step through time at all — compute the exact time of the next event and jump the clock
straight to it. A *certificate* is a condition that provably holds continuously (e.g., "these two particles do
not overlap") until some root of a low-degree polynomial in `t` makes it false; that root **is** the next event
time.

**Who, when.** Berni Alder and Tom Wainwright, event-driven hard-sphere molecular dynamics, *Studies in
Molecular Dynamics I* (J. Chem. Phys., 1959, following their 1957 hard-sphere phase-transition work). Kinetic
data structures generalize this to computational geometry: Julien Basch, Leonidas Guibas, John Hershberger,
*Data Structures for Mobile Data* (SODA 1997).

**North star.** The KDS framework names four explicit design goals: *responsiveness* (cheap to update on an
event), *compactness* (few certificates, near-linear in n), *locality* (each certificate touches few objects,
so failures don't cascade), and *efficiency* — the ratio of actual events processed to the theoretical minimum
number of "external" events should be small. The shared north star with Alder & Wainwright: pay only for what
changes; a system that is mostly still should cost almost nothing.

**Efficiency evidence.** Exact by construction (no discretization error) and event count-proportional, not
time-proportional; the KDS literature argues this beats periodic "polling" (recomputing everything on a fixed
clock) whenever the structure is stable for long stretches — the common case for physical and geometric
systems.

**Verdict for simcraft.** This is the closest formal cousin of Cem's alternative design (closed-form
trajectory, core computes only at events). Worth borrowing the *shape* of it for entities with simple motion:
store `(t0, x0, velocity, target)` and evaluate `x(t)` at render time; recompute only at lane changes, contacts,
spawns. But KDS's efficiency case is built for thousands to millions of moving points where pairwise
certificate checking pays for itself; simcraft's regime is a few dozen visible, hundreds simulated, where the
existing per-tick cost is already far under budget (§ Synthesis). Adopt the trajectory-as-data idea narrowly,
not the certificate machinery.

## 4. Game physics in continuous time: timewarp, continuous collision, fixed timestep, rollback

**Mirtich's Timewarp rigid body simulation.** Apply Jefferson's optimistic parallel Time Warp (§6) to a
single-processor rigid body simulator: bodies advance independently and optimistically; a late-discovered
collision only rolls back the bodies actually involved, not the whole world. Brian Mirtich, *Timewarp Rigid
Body Simulation*, SIGGRAPH 2000. North star, in the paper's framing: conventional simulators impose
"unnecessary synchronization" — every body waits for every other body at every step even when they never
interact — which "scale[s] poorly to systems of hundreds or more moving, interacting bodies." Reported result:
"significant performance improvements over traditional [...] algorithms [...] with systems of hundreds of
bodies," though the source pages found here do not give the exact multiplier.

**Continuous collision detection / conservative advancement.** Compute the earliest time of impact (TOI)
between two moving shapes analytically or by bisection, instead of only checking overlap at tick boundaries
(which lets fast, thin objects "tunnel" through each other). Conservative advancement traces to Mirtich's PhD
thesis; Erin Catto's Box2D version deliberately trades exactness for guaranteed-fast convergence — classic
conservative advancement can take "hundreds of iterations" to converge in bad cases, so Catto's variant accepts
a slightly conservative answer in a bounded number of steps (Erin Catto, *Continuous Collision*, GDC 2013).

**"Fix Your Timestep."** The accumulator pattern: run the simulation at a fixed `dt` regardless of the display's
variable frame time, taking as many (or as few) fixed steps as needed to catch up, then interpolate the last
two simulation states for display using the leftover fraction as the blend factor. Glenn Fiedler,
*gafferongames.com*, 2004 (`Fix Your Timestep!`). North star: decouple the simulation's determinism and
stability from the display's refresh rate, "without an entire category of platform-specific bugs" — this is,
almost verbatim, simcraft's current `sim_render::feel` design.

**Deterministic lockstep and rollback (GGPO).** Classic lockstep keeps every client in perfect sync by having
them wait for confirmed input before simulating — perfectly consistent, but adds input delay. GGPO (Tony
Cannon, 2006) runs the deterministic simulation optimistically ahead of confirmed remote input, predicting it,
and rolls back to resimulate when a prediction is wrong. North star: make online play "feel like offline" —
instant response to input — without giving up the exact determinism lockstep needs to stay synchronized.

**Efficiency evidence.** Fiedler's accumulator "costs almost nothing" to implement (his own framing).
Mirtich's timewarp wins specifically once body count is high enough that synchronization, not computation,
dominates. GGPO's cost is proportional to how often predictions are wrong (rollback depth × resimulation cost);
it works at all only because the underlying simulation is bit-exact and cheap to resimulate.

**Verdict for simcraft.** Fixed step + interpolation (Fiedler) is not a research gap here — it is already the
architecture, and this whole thread converges on it being the right answer for exactly simcraft's kind of game
(discrete, rule-driven, needs to look continuous). Borrow narrowly: (a) Catto's conservative-advancement
framing is useful if a fast mover (a car, a thrown object) can cross a whole thin obstacle within one tick at a
low tick rate — a bounded-iteration TOI check between the mover's cell path and the obstacle's cell, done in
integers, would catch it without changing the tick model. (b) GGPO's precondition — bit-exact determinism from
any resimulated point — is exactly what `snapshot`/`restore`/replay already guarantee, so rollback netcode is a
networking feature to add later on top of whatever motion layer ships, not a reason to change it now.

## 5. Variational / asynchronous integrators: each element its own time step

**Idea.** Derive a numerical integrator from a discrete variational (Lagrangian) principle over a
*spacetime* mesh, so different elements can advance with different time steps, in arbitrary integer ratios to
their neighbors, while still exactly respecting momentum conservation (and nearly exactly, energy) — because
the discretization step is spacetime itself, not a global time axis with one step size.

**Who, when.** Adrian Lew, Jerrold Marsden, Michael Ortiz, Matthew West, *Asynchronous Variational
Integrators*, Archive for Rational Mechanics and Analysis 167(2), 2003.

**North star.** Let stiff, fine mesh elements take small steps and soft, coarse elements take large steps in
one simulation, without forcing a wasteful global fine step and without losing the long-run stability
(symplectic/geometric conservation) that makes variational integrators trustworthy over many steps.

**Efficiency evidence.** Built and evaluated for finite-element elastodynamics (deformable solids, wave
propagation through inhomogeneous meshes), where required step size varies by orders of magnitude across a
mesh; not benchmarked against fixed-step integration for discrete-agent simulation.

**Verdict for simcraft.** Not for us. AVI targets continuum mechanics (mesh elements, stress waves), not
discrete, rule-governed entities, and lives in floating-point PDE land — orthogonal to "rules as data" and to
integer determinism. The transferable idea — different things legitimately need different time granularity —
already exists in simcraft as per-game `tick_rate` and the `pace()` helper; AVI's machinery would be a large
detour to reinvent something already present in simpler form.

## 6. Parallel/optimistic simulation: Time Warp

**Idea.** In a parallel discrete-event simulation, let each process run ahead on its own "virtual time"
without waiting for global synchronization. If a message with an earlier timestamp than something already
processed arrives (a "straggler"), roll back to before that time (via saved state or anti-messages) and
re-execute forward. Global progress is tracked as "Global Virtual Time," the earliest unprocessed message
anywhere in the system.

**Who, when.** David Jefferson, *Virtual Time*, ACM TOPLAS 7(3), 1985; implemented as the Time Warp Operating
System (TWOS).

**North star.** Get the speed of unsynchronized, asynchronous parallel execution while still guaranteeing
exactly the result a strictly sequential, causally ordered simulation would have produced — optimism as a
substitute for expensive up-front synchronization.

**Efficiency evidence.** Wins when synchronization overhead dominates computation and rollbacks are rare
relative to useful work forward-progress (see Mirtich's adaptation, §4, for a concrete case).

**Verdict for simcraft.** The idea already exists in miniature: within one tick, groups (proposed effects) are
computed in parallel, optimistically, and reconciled at `apply()` — a group touching a since-changed entity is
dropped (`short`/`conflict`) rather than rolled back across many ticks. That is a single-tick, no-rollback
cousin of Time Warp's optimism-then-reconcile pattern. Full multi-tick rollback is unneeded complexity at
simcraft's scale (a few dozen to a few hundred entities, sub-millisecond ticks); worth naming in
`docs/emergence.md` as the formal ancestor of the group-conflict mechanism if a future feature (e.g.,
speculative networking) needs real rollback.

## 7. Rendering continuous time: distributed ray tracing

**Idea.** Treat time as one more dimension to stochastically sample per pixel, alongside lens position (depth
of field) and light-source area (soft shadows): each ray samples the scene at a slightly different time `t`
within the frame's shutter interval; the pixel color is the Monte Carlo average. An object's position at time
`t` is evaluated analytically — motion is never discretized into sub-frames to get blur.

**Who, when.** Robert Cook, Thomas Porter, Loren Carpenter (Pixar/Lucasfilm), *Distributed Ray Tracing*,
SIGGRAPH 1984 (Computer Graphics 18(3), pp. 137–145).

**North star.** Reproduce physically continuous camera/film effects (motion blur, depth of field, soft
shadows, glossy reflection) by sampling continuous parameters directly, rather than approximating each with a
fixed number of discrete sub-steps — "motion blurred pixels [are] calculated by averaging over many different
samples of t," integrated with the ordinary visible-surface calculation rather than bolted on afterward.

**Efficiency evidence.** None quantitative in the sources found here; the technique's cost scales with
samples-per-pixel, independent of how many discrete motion steps an object "really" has, because position is a
closed-form function of `t`, not a stored sequence.

**Verdict for simcraft.** This is the renderer-side sibling of the closed-form-trajectory idea (§3). If the
motion layer stores each entity's motion as a function of time — even something as simple as a linear segment
between two ticks, or the arc already used for the rider's `air` state and the lane `switch`'s ease curve in
`track.ron` — the renderer can evaluate that function at the *exact* display time instead of only linearly
interpolating two fixed snapshots. Adopt the principle (evaluate a stored trajectory function at arbitrary t)
for the renderer; it costs nothing extra today and leaves room for true motion blur or 120 Hz phones later
without touching the core.

## 8. Hybrid automata: the formal model for discrete rules + continuous motion

**Idea.** A hybrid automaton pairs a finite automaton's discrete states and guarded transitions with, inside
each state, a continuous flow (an ODE, or in the simplest "linear hybrid automaton" case, a constant rate)
governing how continuous variables evolve while the system stays in that state. A transition fires when a
guard on the continuous variables (or an external input) becomes true, and may reset those variables.

**Who, when.** Thomas Henzinger, *The Theory of Hybrid Automata*, LICS 1996 (reprinted in a NATO ASI volume,
2000).

**North star (his words, paraphrased from the paper's stated goal).** Show that "concepts from the theory of
discrete concurrent systems can give insight into partly continuous systems," and that finite-state
verification techniques extend to certain systems with uncountably infinite (continuous) state spaces, by
finding finite bisimulation quotients of them.

**Efficiency evidence.** Not an efficiency result at all — a decidability/verification result (which classes
of hybrid automata admit algorithmic model checking).

**Verdict for simcraft.** Adopt as *naming*, not as new machinery: simcraft's motion layer already wants to be
exactly this — a discrete, rule-governed state (an FSM state: `Wander`, `Air`, a lane `switch`) that carries a
continuous flow (constant velocity, or a parametrized arc) with guards that fire discrete transitions (landing
on the tick the timer prop reaches zero, arriving at a lane, a contact). The rider's `air` state ("flies an arc
[...] lasting as long as its `timer` prop counts down, so it lands on the tick the game does") and the lane
`switch`'s `ease` curve are already hybrid automata in miniature, just not named as such. Writing the motion
layer explicitly in these terms — *state defines a flow function of (entry tick, elapsed time); a transition
is a guarded event* — is the cleanest way to generalize what `track.ron` already does ad hoc, without pulling
in Henzinger's verification apparatus.

## Comparison

| # | Thread | Who / when | North star (one line) | Efficiency evidence | Determinism/integer fit | Verdict |
|---|---|---|---|---|---|---|
| 1 | FRP (Fran, push-pull) | Elliott & Hudak 1997; Elliott 2009 | Resolution-independent semantics, then efficient implementation without losing it | Qualitative (push-pull avoids wasted recompute) | Poor: lazy, GC'd, float-native host | Borrow the semantic idea only |
| 2 | DEVS / QSS | Zeigler 1976; Kofman & Cellier ~2001–2006 | Quantize state, not time; simulate only what's actually changing | >10× vs classic solvers; up to 3 orders of magnitude in some cases | Poor for motion (real-valued ODE error bound); plausible later for fields | Not for motion; note for fields |
| 3 | Event-driven exact sim (MD, KDS) | Alder & Wainwright 1957–59; Basch, Guibas, Hershberger 1997 | Pay only for what changes; exact, event-proportional cost | Exact by construction; event-count proportional, beats polling when stable | Neutral: root-finding can be done in fixed-point, but adds real complexity | Borrow trajectory-as-data idea narrowly, not at our scale for full adoption |
| 4a | Mirtich timewarp | Mirtich 2000 | Remove unnecessary synchronization between unrelated bodies | "Significant" gains at hundreds of bodies (no exact multiplier found) | Good in principle; single-tick version already exists (groups) | Name the ancestor; don't build full multi-tick version |
| 4b | Continuous collision / Catto | Mirtich (PhD); Catto, GDC 2013 | Guaranteed-fast, bounded-iteration time-of-impact, not exact-but-slow | Avoids "hundreds of iterations" of classic conservative advancement | Good: bounded-iteration root-finding is integer-friendly | Borrow narrowly if fast movers can tunnel |
| 4c | Fix Your Timestep | Fiedler, gafferongames.com, 2004 | Decouple sim determinism from display refresh rate | "Costs almost nothing" | Excellent — already the architecture | Already adopted; the fix is applying it correctly, not replacing it |
| 4d | GGPO rollback | Tony Cannon, 2006 | Feel like offline play without losing lockstep determinism | Proportional to misprediction rate; needs cheap resimulation | Excellent precondition already met (snapshot/restore/replay) | Orthogonal networking feature, not a motion-layer change |
| 5 | Asynchronous variational integrators | Lew, Marsden, Ortiz, West 2003 | Per-element time steps without losing conservation | Built for FEM elastodynamics, not agents | Poor: float PDE land | Not for us |
| 6 | Time Warp | Jefferson 1985 | Optimistic parallelism, roll back only stragglers | Wins when sync overhead dominates and rollbacks are rare | Good in principle; single-tick cousin already exists (groups) | Note the lineage; no adoption needed at this scale |
| 7 | Distributed ray tracing | Cook, Porter, Carpenter 1984 | Sample continuous camera/time parameters directly, not via sub-steps | Cost scales with samples/pixel, not motion granularity | Good: purely a display-side, read-only evaluation | Adopt: renderer evaluates a stored trajectory function at exact display time |
| 8 | Hybrid automata | Henzinger 1996 | Discrete states carry continuous flows; transitions are guarded events | N/A (a verification result, not an efficiency one) | Good: exactly matches FSM-state + fixed-point-flow model | Adopt as the naming/framing for the motion layer |

## Synthesis: the recommended design for simcraft's motion layer

**Cem's proposal — fixed-point positions and velocities integrated every tick at a fixed rate, with render
interpolation — is the field's converged answer for this kind of game, and every source in §4 confirms it
rather than contradicts it.** Fiedler's accumulator, Mirtich's and Catto's collision work, and GGPO's rollback
all assume, and build on top of, exactly this base: a fixed, deterministic step plus a display-side
presentation layer that never feeds back into the simulation. Nothing found here argues for replacing it.

The actual bug is narrower than "continuous vs. discrete time": `highway_surfers` doesn't integrate a
continuous position at all — it *teleports a whole cell on a `pace`-gated event* and asks the interpolator to
paper over that with one frame of lerp between two states that were identical for the previous N−1 ticks. The
fix, in order:

1. **Give moving entities a genuinely continuous, fixed-point sub-cell position and a fixed-point velocity,
   integrated every tick** (not gated behind a multi-tick `pace` event). This alone removes the teleport,
   because `prev` and `curr` now actually differ every tick, giving the interpolator something real to blend.
   No exotic idea needed — this is Fiedler's model (§4c) applied where it was missing, not a new mechanism.
2. **Raise the tick rate for action games**, as already flagged in `docs/research/kernel.md` (60 tps for
   action, vs. 10–20 for ecosystems): at 10 tps, "one cell" is a large jump even with correct interpolation;
   at 60 tps, each cell is a small enough sub-step that a one-tick interpolation window is imperceptible. This
   is cheap: kernel.md measures colony3d/mound/wolf_sheep at 0.17–0.6 ms per tick with hundreds of entities, far
   under a 16 ms (60 Hz) or 8 ms (120 Hz) budget.
3. **Name the flow explicitly, per FSM state (Henzinger, §8).** Every moving state already declares, or should
   declare, a closed-form position function of (entry tick, elapsed ticks): constant velocity for `Wander`,
   a timed arc for the rider's `Air` state, an eased curve for a lane `switch`. This generalizes what
   `track.ron` already special-cases for `air` and `switch` into one concept, rather than inventing motion
   handling per feature.
4. **Let the renderer evaluate that flow function at the exact display time (Cook/Porter/Carpenter, §7;
   the FRP semantic idea, §1)**, instead of only `lerp(prev, curr, alpha)`. This is a strict generalization —
   linear interpolation is the flow function for the common case — and it is what buys resolution independence
   (60–120 Hz, arbitrary phone refresh rates) for free, entirely on the read-only display side.
5. **Do not adopt** a fully event-driven exact-simulation core (§3) or QSS-style quantized-state motion (§2):
   their entire efficiency case is "pay only for what changes, at whatever scale," and simcraft's actual scale
   (a few dozen visible, hundreds simulated, sub-millisecond ticks already) never reaches the regime where that
   trade is worth its cost — moving time-of-impact math and certificate bookkeeping into the deterministic,
   integer, rules-as-data core. The same argument rules out asynchronous variational integrators (§5, wrong
   domain entirely) and full multi-tick Time Warp (§6, no evidence of a synchronization bottleneck to remove).

**What would change this recommendation.** If a future game needs thousands of simultaneously visible movers
(not "a few dozen"), or needs fast, thin objects that can tunnel through obstacles within one tick even at a
raised tick rate, revisit §3/§4b (event-driven exact contact resolution) — bounded-iteration, integer
conservative advancement, not full KDS. If simcraft ever needs speculative online play, GGPO's rollback (§4d)
is nearly free to add given `snapshot`/`restore` already exist. If fields (not entities) become the
performance bottleneck, QSS's quantized-state idea (§2) is the one to revisit first.

## Sources

1. Elliott, C. & Hudak, P. *Functional Reactive Animation*. ICFP 1997.
   https://dl.acm.org/doi/10.1145/258948.258973 · summary: https://blog.acolyer.org/2015/12/07/fran/
2. Elliott, C. *Push-Pull Functional Reactive Programming*. Haskell Symposium 2009.
   http://conal.net/papers/push-pull-frp/ · PDF: http://conal.net/papers/push-pull-frp/push-pull-frp.pdf
3. Elliott, C. *Denotational design with type class morphisms*. 2009. http://conal.net/papers/type-class-morphisms/
4. Zeigler, B. DEVS formalism (1976); Kofman, E. & Junco, S. Quantized State Systems.
   https://journals.sagepub.com/doi/10.1177/0037549703038881
5. Castro, R., Bergonzi, M., Pecker Marcosig, E., Fernández, J. & Kofman, E. *Discrete-event simulation of
   continuous-time systems: evolution and state of the art of quantized state system methods*. Simulation,
   2024. https://journals.sagepub.com/doi/10.1177/00375497241230985
6. Alder, B. J. & Wainwright, T. E. *Studies in Molecular Dynamics. I. General Method*. J. Chem. Phys. 31(2),
   1959. https://gibbs.ccny.cuny.edu/teaching/s2021/labs/HardDiskSimulation/Alders&Wainwright1959.pdf
7. Basch, J., Guibas, L. J. & Hershberger, J. *Data Structures for Mobile Data*. SODA 1997.
   https://graphics.stanford.edu/courses/cs268-11-spring/notes/kinetic.pdf · handbook chapter:
   https://geometry.stanford.edu/lgl_2024/papers/g-KDS_DS-Handbook-04/g-KDS_DS-Handbook-04.pdf
8. Mirtich, B. *Timewarp Rigid Body Simulation*. SIGGRAPH 2000.
   https://history.siggraph.org/learning/timewarp-rigid-body-simulation-by-mirtich/ · MERL TR2000-17:
   https://merl.com/publications/TR2000-17
9. Catto, E. *Continuous Collision*. GDC 2013. https://box2d.org/files/ErinCatto_ContinuousCollision_GDC2013.pdf
10. Fiedler, G. *Fix Your Timestep!*. gafferongames.com, 2004. https://gafferongames.com/post/fix_your_timestep/
11. GGPO / Tony Cannon, rollback netcode. https://en.wikipedia.org/wiki/GGPO ·
    https://www.snapnet.dev/blog/netcode-architectures-part-2-rollback/
12. Lew, A., Marsden, J. E., Ortiz, M. & West, M. *Asynchronous Variational Integrators*. Archive for Rational
    Mechanics and Analysis 167(2), 2003. https://link.springer.com/article/10.1007/s00205-002-0212-y
13. Jefferson, D. *Virtual Time*. ACM TOPLAS 7(3), 1985. https://lasr.cs.ucla.edu/lasr-members/reiher/Time_Warp.html
14. Cook, R., Porter, T. & Carpenter, L. *Distributed Ray Tracing*. SIGGRAPH 1984 (Computer Graphics 18(3)).
    https://history.siggraph.org/learning/distributed-ray-tracing-by-cook-porter-and-carpenter/ · PDF:
    https://artis.inrialpes.fr/Enseignement/TRSA/CookDistributed84.pdf
15. Henzinger, T. A. *The Theory of Hybrid Automata*. LICS 1996.
    http://pub.ist.ac.at/~tah/Publications/the_theory_of_hybrid_automata.html

---

**Epistemological note.** This is a snapshot as of 2026-09-29. The efficiency claims for QSS and KDS are
well-evidenced for their native domains (stiff ODEs; large moving-point sets) but do not transfer as measured
numbers to simcraft's regime — that transfer judgment (§ Synthesis) is this document's own analysis, not a
cited result, and should be revisited if entity counts or tick rates change by an order of magnitude.
