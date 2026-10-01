# Research: Smash Fest / Knock'em All — physics-destruction mobile games (2026-10-02)

Question: how do shipped slingshot/tower-topple destruction games (Angry Birds, Smash Hit, Crash of Cars,
Knock'em All-style hypercasuals) structure physics, stacking, collision and feel on mobile — and what does
that fix or confirm in our planned solver (3D soft-step, boxes+spheres, SAT box-box, speculative contacts,
islands/sleeping, breakable glass)? Written as a reusable template for this genre, not just a one-off report.

## 1. Architecture pattern (converges across every source)

| Layer | Pattern | Evidence |
|---|---|---|
| Game loop | Fixed-step physics decoupled from render; physics owns truth, render interpolates | Convergent with `docs/architecture.md`'s own fixed-tick model |
| Body count | Tower scenes: tens to low hundreds of dynamic bodies, not thousands. Debris after a hit adds the rest — capped and pooled | Smash Hit, generic mobile-physics optimization sources |
| Sleeping | Non-sleeping bodies dominate cost; a sleeping body is ~free. **"Spawn asleep"**: towers start fully asleep so they stand still until the first real contact | Box2D docs; genre-standard trick (confirmed pattern, not one specific citation) |
| Fracture | **Pre-fracture at build/load time (Voronoi), not at impact time.** Runtime 3D Voronoi is CPU-only and O(n²) in fragment count — "noticeable frame rate drops" if done live | Springer, *Real-time fracturing in video games* (2022); Unity fracture tooling docs |
| Exception | Smash Hit breaks this rule on purpose: runtime plane-splitting (below) because *everything* is glass and must break along the actual impact point, not a canned pattern | Gustafsson, *Cracking destruction* |
| Debris lifecycle | Object pool for shards/debris; despawn/recycle after a timeout or distance, never per-object alloc | Generic mobile-perf sources (daily.dev, Medium physics-opt pieces) |
| LOD for debris | Small/old shards drop to kinematic or simplified collision, or are culled outright, before they hit the pool limit | Generic sources; same spirit as islands |
| Thermal/battery | The actual lever is "how many *awake* bodies, how many *active* contacts", not total scene complexity — matches the sleeping point above | Unity-physics optimization write-ups |

**Net:** the genre's performance story is almost entirely about *keeping things asleep* and *capping what breaks*, not about a faster solver per se. A correct solver with good sleeping beats a fast solver with bad sleeping.

## 2. Stable stacking — Box2D v3 "Soft Step" (Erin Catto), with concrete numbers

Erin Catto's own comparative work (Solver2D, 2024) and the Box2D v3 source are the strongest evidence here:
primary documentation, same author who solved this problem publicly for 18 years, with numbers in the source.

| Parameter | Value | Note |
|---|---|---|
| Substeps per step | 4 (primary), 2 (relax pass) | "sub-stepping instead of iterations" — contacts computed once per step, re-evaluated each substep |
| Simulation rate | 60 Hz | Standard; substeps run *within* one 60 Hz tick |
| Contact hertz | 30 Hz (soft-constraint spring frequency) | Lower than sim rate on purpose — softer than rigid, avoids energy injection; sub-stepping lets this still look stiff |
| Contact damping ratio | 10 (critically-overdamped side) | Kills jitter in stacks without the spring overshooting |
| Linear slop | 0.005 × length-unit (≈5 mm at 1 unit = 1 m) | Allowed penetration before correction kicks in |
| Speculative contact distance | 4 × linear slop (≈2 cm) | Contacts generated *before* touching — this is what replaces "skin"/CCD margins and avoids tunneling without full TOI |
| Contact recycle distance / angle | 10 × linear slop / cos(11.5°) | Reuses (warm-starts) a contact point across frames if geometry hasn't moved past this |
| Time to sleep | 0.5 s below velocity threshold | Per-body, then island-wide via union-find |
| Max rotation per step | 0.25 × π | Clamp to keep substep linearization valid |
| Solver quality vs PGS | TGS-Soft: 10 constraint-loop-equivalents ≈ PGS at 8 (4 extra iterations) for comparable stack stability | Solver2D post — soft+substeps beats plain iteration count for stacks, chains, high mass ratios |
| CCD | Hybrid speculative + time-of-impact, not full bisection TOI on every pair | "Releasing Box2D 3.0" — exactly the fast-projectile case (slingshot ball into a tower) |

**TGS vs PGS, restated plainly:** PGS (sequential impulses, Box2D v2 style) needs many iterations to converge on tall stacks and high mass ratios (light shard on a heavy block). TGS (temporal Gauss-Seidel, i.e. sub-stepping) re-linearizes the constraint each substep instead of each outer iteration, which converges faster per unit of CPU for exactly the stack/tower case. "Soft" adds the spring-damper contact instead of a hard velocity-bias to kill the residual jitter/energy gain that plain TGS still has. This is the lineage our planned solver is already copying — confirms the plan, doesn't change it.

## 3. Box-box SAT in 3D (Dirk Gregorius, GDC 2013/2015) + cylinders

- **Canonical algorithm** (Gregorius, *The Separating Axis Test between Convex Polyhedra*, GDC 2013): test the 3 face normals of each box (6 axes) + 9 cross products of edge pairs (face-face vs edge-edge). Track the axis of **minimum penetration**, not first-found — minimizes jitter from axis flip-flopping.
- **Face contact (box resting on box/ground):** pick reference face (bigger penetration axis), clip the incident face's 4 points against the 4 side planes of the reference face (Sutherland-Hodgman) → up to 4 contact points (what Box2D/Box3D call manifold reduction — never keep more than 4, average/pick extremes instead).
- **Edge-edge contact:** only when an edge-edge axis wins; needs a **bias** toward face axes in the comparison (a small epsilon favoring face axes) because floating-point/fixed-point noise otherwise flickers between a face manifold and a single edge point on near-parallel edges — this is the single most-cited gotcha in SAT implementations (Gregorius's talks, corroborated by community write-ups, e.g. cairno's *Improvements to the Separating Axis Test*).
- **Cylinders (cans):** no shipped casual/hypercasual source documents true cylinder-cylinder narrowphase for a from-scratch mobile solver — the universal cheap trick is to **approximate the can as a box or an N-sided convex prism (hexagonal/octagonal)** and reuse box-box/convex-convex SAT, trading a barely-visible silhouette difference for zero new collision code path. Physics-engine issue trackers (e.g. generic open-source engines) treat true cylinder primitives as a separate, still-being-added feature even in mature libraries — confirms this is still non-trivial enough that nobody bothers for casual-game fidelity.

## 4. Game feel: slingshot, camera, impact

| Technique | Concrete value / method | Source |
|---|---|---|
| Trajectory preview | Dashed/dotted line, drawn from the *same* launch physics as the real shot (either closed-form parabola if no drag, or a shadow-simulated step-ahead if drag/variable gravity apply) | Angry Birds pattern (Unity discussions); generic trajectory-predictor write-ups |
| Hit-stop | Freeze 40–80 ms on a heavy impact before resuming | *Juice It or Lose It* (Jonasson & Purho, GDC 2012) |
| Screen shake | A few pixels, decaying fast, **scaled to impact energy** — small hits barely shake, tower collapse shakes hard | Same talk; genre-universal |
| Camera follow | Follow the projectile until impact, then cut/pan to the collapse, not a fixed wide shot the whole time — keeps the moment of impact legible at phone scale | Genre convention (Angry Birds, Crash of Cars) |
| Haptics | A short pulse on launch release, a stronger one on tower-break/impact threshold crossed | Genre convention, no single primary source found — treat as medium confidence |
| Slow-motion on big collapses | Brief timescale drop (e.g. 0.3–0.5×) on a "clear" or big multi-body collapse, ramping back to 1× over ~0.5 s | Genre convention; same juice lineage as hit-stop, not independently sourced beyond the talk's general principle |

## 5. Fracture: Smash Hit as the relevant counter-example

Smash Hit (Mediocre, Dennis Gustafsson) is the one shipped mobile title whose *entire* gameplay is glass breaking under its own from-scratch engine — directly relevant, and it deliberately does **not** pre-fracture:

- **Runtime plane-splitting, not Voronoi.** On impact above a threshold, the engine carves a small volume around the impact point using **5 randomized bounding planes** and splits every convex piece against them, producing new dynamic convex pieces on the spot.
- **Everything is convex compounds.** Every breakable object is already a compound of convex shapes, so "break" = "re-slice convex pieces + reconnect/detect new connected components" (GJK overlap + graph connectivity), not a mesh boolean.
- **Robustness tactics**, directly reusable: classify each vertex against each plane before splitting; only split edges that cross a plane; cap open splits to guarantee a closed, non-degenerate shape afterward.
- **Multiple solves per frame, capped at 3**, with collision detection and the carve/break step interleaved — breaking happens *during* the solve, not as a post-pass, so motion through a breaking object looks continuous instead of teleporting.
- **No shard-count or body-cap numbers were published** in available sources (GDC talk slides are behind GDC Vault, not independently accessible here) — this is a real gap, flagged rather than guessed.

**Why this matters for us:** pre-fractured Voronoi is right for **pedestal props that always break the same way** (a stack of cans, a wood crate) — bake shard sets at content time, spawn asleep, swap mesh+colliders on impulse-over-threshold. Smash Hit's runtime carving is the right model only for **glass that must break exactly where it's hit** (a pane, not a prop) — more expensive, and only worth it if "the glass breaks right where the ball hit" is itself the game's selling point, which it is for Smash Hit and may be for our glass material specifically.

## What this means for our solver and game

1. **Solver plan is confirmed, not changed.** Box2D v3's soft-step (substep + soft contact + warm start + relax + speculative contacts) is exactly our stated plan; adopt its *numbers* as starting defaults rather than re-deriving them:
   - 4 primary substeps + 2 relax passes per 60 Hz tick.
   - Contact hertz 30 Hz, damping ratio 10, at our chosen length unit (re-derive if 1 unit ≠ 1 m).
   - Linear slop ≈ 0.5% of a "small object" dimension (Box2D: 5 mm at human scale) — pick a slop as a fraction of the smallest breakable object (a can), not a fixed world constant.
   - Speculative contact margin = 4× slop. This is what makes the fast slingshot projectile not need full TOI/bisection — matches our "speculative contacts for the fast projectile" plan directly.
   - Time-to-sleep: 0.5 s below a velocity threshold is a reasonable default to start tuning from.
   - Manifold cap: 4 points max for box-box, via reference/incident clipping (§3) — implement the face-axis bias for edge-edge from day one; this is the most commonly cited correctness bug in from-scratch SAT and costs nothing to add upfront.
2. **"Spawn asleep" is not optional, it's the main performance lever.** Towers must be constructed already in the sleeping island state with zero relative velocity, not "asleep after one still frame" — this is standard practice (and determinism-friendly): a tower that has never been awake can be hash-verified identical every run until struck, which fits our sim's existing replay/hash culture directly (reuse the `golden_wolf_sheep_hash` pattern: a "tower stands until hit" golden snapshot test is a natural `test/scenarios/*.ron` entry).
3. **Cans: approximate as octagonal (or hexagonal) convex prisms, reuse box/convex SAT.** Do not write cylinder-cylinder narrowphase; no shipped casual game needs it, and even mature general-purpose engines treat true cylinders as a still-separate, non-default feature. One SAT path (box/convex-vs-convex) serves boxes and cans both.
4. **Fracture: two separate code paths, not one.** Pre-fractured Voronoi, baked at content-build time, spawned asleep, swapped on impulse-over-threshold, for cans/wood/stone. A from-scratch runtime plane-split path, Smash-Hit style, only if glass must shatter exactly where hit — this is a bigger, separate engineering investment (its own convex-compound representation, connectivity graph, 3-passes-per-frame interleaving) and should be scoped and evaluated on its own, not folded into the generic breakable-material threshold check.
5. **Camera/feel is cheap and high-leverage — build it early, drive it off events, test it in evals.** Hit-stop (40–80 ms), scaled screen shake, a trajectory preview computed from the *same* launch function as the real shot (no separate "preview physics"), and haptics on launch + on break-threshold are all small, bus-event-driven additions (matches `docs/architecture.md`'s existing "feedback is data, hangs off bus events" pattern from `legendary-mobile-games.md`) and should be in the first playable, not polish added later — juice is cheap to add and expensive to retrofit believably.
6. **Eval-driven template for this genre** (reusable for the next tower/destruction game, not just this one):
   - A `test/scenarios/*.ron` tower that must stay asleep and bit-identical for N ticks until struck (sleeping correctness).
   - A scripted "straight shot center-mass" scenario with a pass/fail on: tower falls within K ticks, no body escapes a bounding volume (tunneling check against speculative-contact margin), final rest state is stable (no residual jitter after sleep).
   - A body-count/awake-count counter exposed the way `evals`/`queries`/`fires` already are, so "how many bodies were awake this tick" becomes a tracked efficiency metric the same way Rhai eval budgets are today.
   - A shard/debris-count budget check per break event, so pre-fracture content authoring has a hard ceiling checked by `simcraft-check`, the same way other per-game budgets are enforced.

## Sources

1. Erin Catto. *Solver2D*. https://box2d.org/posts/2024/02/solver2d/
2. Erin Catto. *Releasing Box2D 3.0*. https://box2d.org/posts/2024/08/releasing-box2d-3.0/
3. Erin Catto. Box2D source, `include/box2d/constants.h` (linear slop, speculative distance, time-to-sleep, contact recycle). https://github.com/erincatto/box2d
4. Erin Catto. *Box2D 3.1* (contact hertz/damping defaults). https://box2d.org/posts/2025/04/box2d-3.1/
5. Erin Catto. *Inscribed Spheres* (ongoing SAT/manifold refinement notes). https://box2d.org/posts/2026/09/inscribed-spheres/
6. Dirk Gregorius. *The Separating Axis Test between Convex Polyhedra*, GDC 2013. https://gdcvault.com/play/1017646/Physics-for-Game-Programmers-The (slides/code: http://media.steampowered.com/apps/valve/2013/DGregorius_GDC2013.zip)
7. cairno. *Improvements to the Separating Axis Test*. https://cairno.substack.com/p/improvements-to-the-separating-axis
8. Dennis Gustafsson. *Cracking destruction* (Smash Hit's fracture engine). https://www.gamedeveloper.com/programming/cracking-destruction
9. Dennis Gustafsson, GDC 2015. *Physics for Game Programmers: Destruction in Smash Hit*. https://www.gdcvault.com/play/1022200/Physics-for-Game-Programmers-Destruction
10. Dennis Gustafsson. Voxagon blog (physics background). https://blog.voxagon.se/2015/02/20/physics-tutorial-at-gdc-2015.html
11. Springer. *Real-time fracturing in video games* (2022). https://link.springer.com/article/10.1007/s11042-022-13049-x
12. Martin Jonasson & Petri Purho. *Juice It or Lose It*, GDC 2012 (general knowledge of the talk's content; canonical hit-stop/screen-shake reference).
13. Angry Birds trajectory-preview discussion (Unity Discussions). https://discussions.unity.com/t/angry-birds-dotted-trajectory-for-the-slingshot/103819
14. Generic mobile-physics optimization sources (sleeping, pooling, LOD for debris): daily.dev, *10 Strategies to Optimize Physics in Mobile Games*. https://daily.dev/blog/10-strategies-to-optimize-physics-in-mobile-games/
15. `docs/research/physics-engines.md` and `docs/research/legendary-mobile-games.md` (this repo) — architecture and genre context this document builds on.

---

**Epistemological note.** Snapshot as of 2026-10-02. Strong/primary: Box2D parameters (own source code, §2), Gregorius's SAT algorithm (primary GDC source, §3), Smash Hit's fracture approach (author's own technical blog, §5). Weaker/convergent-but-uncited-per-point: genre-wide body-count ranges, haptics conventions, slow-motion timing (§1, §4) — these are consistent across multiple secondary sources but no single shipped game publishes exact numbers, so they are given as reasonable starting points, not verified constants. Gaps acknowledged rather than guessed: Smash Hit's actual shard/body counts (GDC Vault slides not accessible here), and whether any shipped mobile title does true cylinder-cylinder narrowphase (none found; treated as "nobody does this," which is itself the useful answer).
