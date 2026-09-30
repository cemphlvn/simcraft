# Research: can simcraft build a driving game in the style of Assetto Corsa? (2026-09-30)

Question (Cem): what would it take to make simcraft's traffic game feel like a realistic driving simulator —
not "physics for its own sake," but what generalizable engine features that requires, so the engine grows the
way it always has: a game hits a wall, the wall gets named, the engine grows to cross it (`docs/emergence.md`).

Ground truth, not repeated here: `docs/architecture.md` ("Product direction", "Determinism", "Continuous
motion", "First person: tracks"); `docs/research/continuous-time.md` (fixed step + render interpolation is
already the right shape — nothing here argues otherwise); `docs/research/physics-engines.md` (no mainstream
physics engine is fixed-point; simcraft's own kinematics + a broadphase, not an embedded rigid-body engine, is
the recommended shape — this document reaches the same conclusion independently for vehicle dynamics
specifically). Today's `games/traffic/game.ron`: IDM car-following + a light MOBIL lane-change, straight
four-lane road, `motion: (size: (600, 900))` with `px, py, vx, vy` in `FINE = 1000` fine units, **no heading** (a
car's "ahead" is always +y), footprints are axis-aligned boxes, a per-lane-column sweep-and-prune broadphase
(`games/traffic/PERF.md`: 3.44 ms wall / 1600 cars after five optimization steps). `track.ron`'s road is X across
(lane index), Y forward, Z into the screen — a straight strip, not a spline.

**The honest framing, before any evidence.** Assetto Corsa is not one thing to copy; it is a stack of models,
each adding a specific feel at a specific cost, that Kunos built up over roughly a decade (netKar Pro → AC → ACC).
"Build the traffic game in the style of AC" is really: which layers of that stack does a *traffic* game need
(cars driving themselves and one player among them, not a licensed-track time-trial sim), and which layers are
AC's alone and not simcraft's problem to solve.

## 1. What makes Assetto Corsa feel real

**Physics rate.** Community and developer-adjacent sources converge on **333 Hz** for Assetto Corsa's physics
step (raised toward roughly double that in later builds, "still way below 1K"), and **400 Hz** for Assetto Corsa
Competizione's console physics as of a recent update. Racer.nl (Ruud van Gaal's open-source-adjacent sim,
one generation before Kunos, whose Pacejka reference page and car-physics documentation Kunos-era developers
cite) runs its physics **internally at 1000 Hz**. iRacing's tire model went to **double-precision floats** in its
NTMv10 revision specifically because single-precision rounding error compounded over a session. None of this is
about the tyre *force curve* needing 333 Hz to stay accurate — it is about the **suspension**: independent
springs, dampers, anti-roll bars and geometry per wheel are a genuinely stiff mass-spring-damper system (§2, §3),
and stiff ODEs need either a high fixed rate or an implicit/semi-implicit solver to stay stable. This is the
single most important fact this research found for simcraft's fit question: **the high physics rate is the cost
of suspension springs, not of tyre grip.** A game that skips modeled suspension springs does not need 333 Hz.

**Tyre model, and what Kunos actually said about it.** Stefano Casillo (Kunos' co-founder and lead physics
programmer, later left and rejoined) iterated through **four tyre models across netKar Pro's releases alone**:
v1.0–1.0.2 on Pacejka's Magic Formula, v1.0.3 on a "similarity" model, v1.1 on the brush model, v1.2 on a new
model of his own — before Assetto Corsa and Assetto Corsa Competizione shipped with tyre models described as
"completely original... derived from meticulous work... you won't find in scientific papers." Read plainly: the
textbook models (Pacejka, brush) were each tried and each replaced, not because they were wrong, but because
none alone gave the *feel* Casillo wanted; ACC's later "5-point tyre model" (2020) is not a suspension-geometry
change — it replaced a **single tyre-road contact point** with **five collision points across the contact patch**
specifically so tyres climb kerbs and sausage kerbs realistically (a curb-contact fix, not a cornering-force
fix). The throughline in every source found: Kunos treats the tyre model as the one component worth rebuilding
from scratch repeatedly, because it is the single largest lever on "does this feel like a real car," and they
prioritize *player-perceptible feel*, arrived at by iteration against real drivers, over fidelity to any one
published formula.

**Suspension.** No source found gives Assetto Corsa's suspension model a name as specific as "5-point" (that
term, confirmed above, is the tyre contact model). What is well documented in the vehicle-dynamics literature and
in Racer.nl's own suspension tutorial: independent springs, dampers, anti-roll bars, roll centre and anti-pitch
geometry per wheel, with rebound stops — i.e., a genuine multi-body suspension, one 2nd-order stiff ODE per
wheel (§2 layer 5), which is the layer that both explains and justifies AC's high physics rate.

**Drivetrain.** Standard for the genre: an engine torque curve (torque as a function of RPM) scaled by gear
ratio × differential ratio × drivetrain efficiency, divided by tyre radius, to get wheel force; two coupled
first-order ODEs (engine RPM, wheel angular velocity) tie the engine to the tyre's rotational state (the
Wassimulator source below gives this explicitly, §2).

**Force feedback.** Self-aligning torque (SAT) — the tyre's own tendency to steer itself straight, proportional
to lateral force times the pneumatic trail (the distance behind the contact-patch centre where that force
effectively acts) — is the physical quantity a wheel's motor reproduces. It falls directly out of the tyre
model's lateral-force output; nothing extra needs to be simulated once slip angle and lateral force exist,
which is why force feedback is presented in every driving-sim architecture as a *readout* of the tyre model, not
a separate system.

**Contrast: arcade (Need for Speed).** NFS explicitly optimizes for something else: speed-sensitive, twitchy
steering response tunable per player, deliberate "drift assist" and "drift stability" systems, and (in *Shift*,
the one NFS built as an explicit sim experiment) a conscious departure from the rest of the series toward AC's
end of the spectrum. The franchise's own marketing language ("purest arcade racer") confirms the genre is a
deliberate design choice, not a fidelity shortfall — arcade handling is its own target, tuned for controller
feel and drift, not for tyre-physics accuracy.

**Contrast: soft-body (BeamNG.drive).** BeamNG's mesh of nodes-and-beams (each node a point mass, each beam a
spring with a breaking threshold) answers a different question entirely: "what happens when metal bends and
suspension breaks," not "how does a tyre find grip at the limit." The chassis itself is the simulated object in
BeamNG; in Assetto Corsa the chassis is rigid and (per the sources above) "almost all the engineering effort"
goes into the tyre contact patch. This is the cleanest confirmation that "realistic driving feel" and "realistic
damage/deformation" are orthogonal engineering investments — simcraft's traffic game wants the former, not the
latter.

## 2. The layered models of vehicle dynamics: simplest to hardest

| Layer | Adds to *feel* | State (beyond `px,py,vx,vy`) | Equation shape | Integration rate needed | Cost / car / step | Fixed-point fit |
|---|---|---|---|---|---|---|
| 1. Kinematic bicycle | Turns follow the wheel angle (Ackermann geometry); no drift, no slide — still reads as "a car," not "a puck" | heading `θ`, speed `v` | Algebraic: `θ += (v / wheelbase) * tan(steer) * dt`; `x += v·cos θ`, `y += v·sin θ` | Any (no stiffness; stable at 60 Hz) | Trivial: 2 trig lookups + a few multiplies | Excellent — this is `docs/research/continuous-time.md`'s hybrid-automaton flow model with one more state variable |
| 2. Dynamic bicycle, linear tyre | Understeer/oversteer, drift *onset*, the car feels like it has lateral inertia, not just a heading | + lateral velocity `vy` (car frame), yaw rate `r` | Slip angle ≈ `vy/vx` (small-angle, no `atan` needed); lateral force `F = -C_α · slip` (linear); `m·v̇y = ΣF − m·vx·r`, `I·ṙ = Σ(F·a)` | 60 Hz is fine for road-car cornering-stiffness values (not stiff unless `C_α` is set unrealistically high) | Low: a handful of multiplies per axle, no trig in the hot path | Good — **the recommended starting layer** |
| 3. Nonlinear tyre (Pacejka Magic Formula / brush, friction circle) | The saturating, then-*decreasing* grip curve at the limit; combined braking+cornering (friction ellipse) | same state; force is a nonlinear function of slip, evaluated via curve or table | Magic Formula: `F = D·sin(C·atan(B·slip − E·(B·slip − atan(B·slip))))`; brush: force derived from contact-patch pressure distribution | Not itself stiff (still algebraic per tick) | Moderate: a lookup table (cf. simcraft's own sin/cos table, §3) avoids the trig/atan cost | Good via table; **tuning** the B/C/D/E coefficients (or a brush model's contact-patch parameters) without lab data is the real cost, not the arithmetic |
| 4. Load transfer (longitudinal/lateral) | Nose dive under braking, squat under acceleration, outside tyres loading up in a corner — directly changes how much grip layer 2/3 gives each wheel | + per-wheel normal load `N_i` | Algebraic, not integrated: `ΔN ∝ (a_x or a_y) × CG height / wheelbase or track width` | N/A (no ODE) | Very low: a proportional formula per wheel | Excellent — plain integer proportionality, same shape as the IDM/MOBIL math already in `traffic/game.ron` |
| 5. Four-wheel + suspension | Pitch/roll dynamics, kerb response, camber gain from geometry | + per-wheel suspension travel and velocity (spring-damper state, ×4) | 2nd-order spring-damper ODE per wheel: `m·ẍ = −k·x − c·ẋ + F_tyre` | **Genuinely stiff** at road-car spring rates — this is where AC's 333–1000 Hz comes from | High: 4 extra 2nd-order integrations + suspension-geometry lookups per car | Hardest — stiffness makes explicit/semi-implicit Euler unstable at 60 Hz without sub-stepping (§3) |
| 6. Drivetrain (engine, gears, differential) | Torque delivery character, engine braking, wheelspin, the sound of a gearbox | + engine RPM, gear, wheel angular velocity | Two coupled first-order ODEs (Wassimulator, §2 sources): `dR/dt = k₀T + k₁(R_ex − R_t) − k₂R_t`; `dω/dt = k₀(ω_ex − ω_t) − k₁B − k₂ω_t`; wheel torque `= (T_engine·gear·diff·η) / r_tyre` | Not stiff; fine at 60 Hz | Low–moderate: a torque-curve lookup + a few multiplies | Good |
| 7. Tyre thermal/wear | Grip changes over a stint/lap — race-strategy depth, not lap-to-lap feel | + per-tyre (or per-contact-patch-zone) temperature, wear | First-order thermal ODEs, slow time constant (seconds, not ticks) | Not stiff at 60 Hz (slow dynamics) | Moderate arithmetic, **high tuning cost** (TRT/TRT EVO-style models are a research paper's worth of coupled sub-models) | Feasible in fixed-point, but lowest feel-per-effort of every layer for a *traffic* game |

**Reading the table for simcraft:** layers 1–4 are cheap, not stiff, and directly produce "feels like a car, not
a puck" (1), "feels like it can slide" (2), "feels like it's at the limit of grip" (3), and "feels like braking
hard dives the nose" (4) — everything a player consciously notices about handling. Layer 5 is the one that costs
AC its physics rate, and layer 7 is the one that costs AC (and iRacing, and ACC) the most *tuning* effort for the
least moment-to-moment feel. Neither is warranted for "cars that drive themselves realistically, with one
player among them."

## 3. Determinism and integers: what fixed-point buys and where it breaks

**Nobody else does this.** `docs/research/physics-engines.md` already established that every mainstream rigid-body
engine (Box2D, Jolt, PhysX, Rapier, Avian, Chaos) is float-based; this research adds that the *tyre-model*
end of the genre is float too, and moving toward *more* float precision, not less — iRacing's NTMv10 moved from
single- to **double**-precision specifically because rounding error compounded across a long stint. Photon
Quantum, built for deterministic networked lockstep, is the closest existing analogue to simcraft's choice, and
even it is fixed-point-*shaped* floating math (`FP` = Q48.16, still a fractional binary format), not
simcraft's plain integer fine units. simcraft's own `FINE = 1000` (no fractional bits, `i64` throughout) is one
step stricter than anything found in this survey, in either research thread.

**What needs care, concretely, for a bicycle-model car:**

- **Heading and rotation.** simcraft has no angle unit today. The standard trick for integer/fixed-point angles
  (used across CORDIC-based systems and fixed-point game math) is a **binary angle measure (BAM)**: represent a
  full turn as `2^16 = 65536` units, so wraparound is free (`u16` overflow) and a precomputed sin/cos table
  indexed by the low bits needs no modulo. At `FINE`-consistent scaling (outputs as parts-per-10000, matching
  how `traffic/game.ron` already expresses IDM's `a`/`b` in thousandths), a 4096-entry quarter-wave table with
  linear interpolation gives roughly 0.0055°/unit resolution — an order of magnitude finer than the 0.1° a slip
  angle near the grip limit needs to be stable.
- **Avoid literal inverse trig in the hot path.** Slip angle is conventionally `atan(vy/vx)`, but for the small
  angles road cars actually run (a handful of degrees before grip saturates), `slip ≈ vy/vx` is the standard
  small-angle approximation used in the dynamic bicycle model (the arXiv sources above use exactly this form),
  which sidesteps needing an integer `atan`/`atan2` entirely — a ratio, scaled to the same fixed-point
  convention as everything else in `sim-core`. Pure pursuit's steering angle (§5) has the same trick available:
  its curvature form (`2·sin(α)/L_d`) only needs the sin table already built for heading, not a raw atan2.
  Where a real `atan2` is unavoidable (a nonlinear tyre model's exact slip angle at high speed), CORDIC's
  vectoring mode computes it with only shifts and adds, in fixed-point, deterministically — a known, bounded-
  iteration algorithm, not a new research problem.
- **Integer square root.** Needed for any true vector magnitude (e.g. combined-slip friction-ellipse magnitude,
  §2 layer 3). Newton-Raphson integer sqrt converges in a handful of iterations and is exact-per-input
  deterministic (same bits on every platform, unlike a platform's native `sqrt` on doubles) — already the
  standard recommendation in general fixed-point-physics writeups.
- **Where it actually breaks: stiff forces, not small ones.** The precision risk in this whole stack is not
  "slip angles are small so they lose bits" (BAM16 gives more than enough headroom, as above) — it is layer 5's
  suspension springs. A stiff mass-spring-damper system has a stability condition (oscillation frequency must
  stay under roughly half the sampling frequency for semi-implicit Euler) that is exactly why AC needs 333+ Hz:
  road-car spring/damper rates are stiff enough that 60 Hz semi-implicit Euler would either blow up or need to
  be so heavily damped it stops feeling like a real suspension. **This is precisely why layer 5 is the one this
  research recommends not building** (§7): it is the layer that would force simcraft to choose between breaking
  its 60 Hz tick-rate convention (sub-stepping inside `integrate_motion`, still deterministic, still integer,
  but a real engine-rate change) or accepting a suspension that cannot be stiff enough to feel real.

**Recommendation: fixed-point format and step rate.**

- **Format:** keep `FINE = 1000` for position/velocity exactly as today (proven, `i64`, no change). Add
  **`ANGLE = 65536`** units per full turn (BAM16, `i64`) for heading and yaw rate, with a checked-in sin/cos
  lookup table (a few KB, like the deterministic-math helpers already in the architecture: `clamp`, `pct`,
  `ramp`, `triangle`) — no floats, no trig at runtime beyond a table read and a linear interpolation, both
  integer. Slip-angle-equivalent quantities stay as velocity-component *ratios* in existing fine units, not
  literal angles, wherever the small-angle approximation holds (layers 2–4; the overwhelming majority of what a
  traffic/driving game needs).
- **Step rate: keep 60 Hz.** Every stiffness concern found in this research traces to spring-damper suspension
  (layer 5), which this document recommends against building (§7). Layers 1–4 and 6 are algebraic or
  non-stiff first-order ODEs; nothing in the survey suggests they need a rate above what
  `docs/research/kernel.md`'s existing 60 tps recommendation for action games already provides, and
  `docs/research/continuous-time.md`'s conclusion (60 tps makes each cell-fraction of motion small enough for
  interpolation to look continuous) applies unchanged to heading and lateral velocity. If layer 5 is ever
  revisited, the fix is **sub-stepping inside the native integration pass** (run the suspension ODE 2–4× per
  60 Hz tick, still integer, still invisible to Rhai and to the hash's tick cadence) rather than raising the
  game's tick rate — the same "flow function per FSM state, evaluated more finely where it's stiff" idea
  `docs/research/continuous-time.md` §5 (asynchronous variational integrators) already named and declined to
  adopt wholesale, applied narrowly instead.

## 4. Input and feel: what matters most per dollar

Ranked by (perceived realism gained) ÷ (engineering cost), using the evidence above:

1. **The tyre force curve itself (model layers 2–4).** This is not an "input" item, but it dominates every
   other line below: no amount of analog input, camera work or audio sells a car that corners like a puck.
   Already covered in §2; restated here because it is the actual top of this list.
2. **Analog input (steering/throttle/brake as continuous axes, not digital taps).** Cheap — an input-layer
   change (`input.ron`'s binding types, §7), not a core change — and without it, even a perfect tyre model feels
   binary: a player can only ask for full-lock or nothing. NFS's own steering-response tuning knobs exist
   precisely because analog steering *response curve* is where "twitchy" vs "controlled" lives.
3. **Load transfer, visible through camera pitch/roll cues even without modeled suspension.** Algebraic (layer
   4), nearly free, and pairs with the camera item below: a chase camera that dips forward under braking
   *reads* as suspension even when none is simulated.
4. **Camera (chase with look-ahead, cockpit).** Confirmed across sources as a presentation-layer concern
   (spring/damper-smoothed follow, speed-scaled look-ahead offset, roll a fraction of the car's yaw) — exactly
   the kind of read-only, camera-only feature `sim_render::feel` already hosts for `track.ron` (`docs/architecture.md`,
   "First person: tracks"). Sells weight and speed cheaply; simcraft already has the render-side hooks (camera
   effects, springs) this would extend.
5. **Engine RPM audio synthesis.** Two real approaches found: additive synthesis (physically motivated,
   harder to get right) and granular synthesis (splice pre-recorded engine samples by RPM/load/gear — "complex
   analysis done offline, synthesis efficient in real time," the practical industry default). Meaningfully sells
   "power" but costs more than input or camera and is invisible to players who play muted.
6. **Force feedback self-aligning torque.** Physically "free" once a tyre model with lateral force exists (§1) —
   but only pays off for the fraction of players with a wheel, and requires exposing per-wheel lateral force at
   a useful rate through `sim-ffi`'s C API to a host that owns the actual FFB device — real integration cost for
   a narrow audience.
7. **Suspension/thermal/wear/5-point contact.** Highest engineering cost (§2 layers 5, 7; §1's ACC 5-point
   detail) for the smallest marginal *feel* gain once layers 1–4 already exist. Do not build first (§7).

## 5. The world: tracks, free motion, oriented collision, and traffic AI without lanes

**Splines and open-source track representations.** Racer.nl smooths a track's raw polygon mesh (imported from
VRML) into a **spline surface**, with lateral subdivision to keep cornered/cambered road segments smooth to
drive on — the spline is a *rendering and contact-surface* concept there, not the vehicle's coordinate system.
TORCS/Speed Dreams' track XML is described (with only moderate confidence — public documentation on the exact
waypoint/spline encoding is thin, per the sources found) as segment-based (straight/curve pieces combined by a
`trackgen`/`accc` toolchain) with spline smoothing applied on top, the same shape as Racer's approach. The
common thread across every open-source sim surveyed: **a track's centreline + width is track-specific data, fed
into a spline for smooth geometry, kept separate from the vehicle's own state** — never a redefinition of the
world's coordinate system.

**This matters directly for simcraft's fit.** `track.ron`'s road today is X = lane index, Y = distance along —
an implicit assumption that "ahead" is always +y, which both the rule language (`ahead(kind)`, Chebyshev
distance, `near`) and the cell grid (`x = px / FINE`) lean on. A curving track should **not** mean redefining
simcraft's coordinate system to follow the road; it should mean the world stays an ordinary Cartesian (x, y)
plane (already true — continuous motion's `px, py` are already free 2D positions, not lane-locked, per
`docs/architecture.md` "Continuous motion") and a **track's centreline + width becomes game data**, read by two
new query functions in the spirit of `ahead`/`behind`/`touching`: something like `track_dist()` (progress along
the nearest spline segment) and `track_offset()` (signed perpendicular distance from the centreline, i.e. "how
far off-line," the direct generalization of today's `lane_to`). This is additive — straight-road games
(`traffic`, `highway_surfers`) never need it — and keeps the Cartesian `(px, py)` the one true position, matching
how every open-source sim surveyed treats the spline as auxiliary geometry, not the coordinate frame itself.

**Oriented-box collision (SAT).** For two-dimensional oriented rectangles specifically (simcraft's footprints,
even once they carry a heading), the Separating Axis Theorem reduces to testing **4 candidate axes** (each
box's two face normals) — not the 15 axes SAT needs for general 3D oriented boxes, a distinction worth being
precise about since sources describing SAT default to the 3D case. Each axis test is a handful of `i64` dot
products and a min/max comparison — cheap, exact, and deterministic, and a natural second stage after the
existing sweep-and-prune broadphase (`games/traffic/PERF.md` step 002) culls candidates by position; the
broadphase does not need to become exact-aware of heading, only conservative (it already is, since an
axis-aligned bound around an oriented box is a valid, if loose, cull).

**Traffic AI once cars can steer freely.** The clean split found across both the autonomous-driving and
racing-AI literature: **speed stays IDM's problem** (how far behind the car ahead, how hard to brake) — nothing
about steering changes that decision. **Steering becomes a path-following controller** reading a look-ahead
point on the track/lane centreline (§5's `track_dist`/`track_offset`, or today's per-lane `x` if no spline
exists yet): **pure pursuit** first — one geometry formula (curvature `= 2·sin(α)/L_d`, expressible with the same
sin table §3 recommends for heading, no literal atan2 needed), simple to tune (one look-ahead-distance
parameter), and exactly the shape of `traffic/game.ron`'s existing `steer` rule (`steer_k`, `steer_acc`,
`steer_max`) — pure pursuit is the natural generalization of what that rule already approximates for discrete
lane changes. **Stanley** (cross-track error *and* heading error, referenced from the front axle) is reported as
more accurate and more stable at higher speed in every comparison found, at the cost of needing the car's exact
heading error to the path, not just a look-ahead point — worth adopting once cars are fast enough (highway
speeds, tight track sections) that pure pursuit's look-ahead-distance tradeoff (too short → oscillation, too
long → corner-cutting) becomes a felt problem, not before.

## 6. A vehicle as data — so the core can drive *any* car, not just `traffic`'s

Cem's addition, stated as the actual goal: not "make `traffic` feel like Assetto Corsa" but **build the engine
so a `vehicle` is data, the way a `kind`'s `props`, `fsm` and `rules` already are** — a hot hatch, a truck, a bus
and a motorbike-shaped two-wheeler should all be the same native component reading different numbers, never
different Rust. This section looks at how the real sims already do this (they all converged on "a car is a
folder of parameter files," independently), proposes simcraft's own schema, and asks what a designer can fill
in from a spec sheet versus what has to be guessed or measured.

### 6.1 How real sims describe a car as data

Every sim surveyed — commercial and open-source alike — splits a vehicle into the same handful of files, which
is itself evidence about where the *real* degrees of freedom in "car feel" live:

| Sim | Files | What's in them |
|---|---|---|
| **Assetto Corsa** | `car.ini` (mass, centre-of-gravity offset, sprung inertia), `engine.ini` (power/torque curve, `[TURBO_x]` boost, limiter/rev limit), `drivetrain.ini` (gear ratios, final drive, differential type/preload), `tyres.ini` (per-compound `DY0` peak lateral grip coefficient, `LS_EXPY` load-sensitivity exponent, `DCAMBER_0/1` camber-grip dependency, thermal/wear coefficients), `suspensions.ini` (spring rates, geometry per corner), `brakes.ini`, `aero.ini` | One `.ini` file per subsystem, in the car's own folder; the same subsystem boundaries this document's §2 model-layer table already found independently |
| **rFactor / rFactor 2** | `.hdv` ("High-Detail Vehicle": general — mass, CG height — plus suspension, aero, AI, pit behaviour), `Engine.ini` (torque curve, RPM limits), `.tbc` (tyre compound parameters), `Gearbox.ini` | Same split, different file extensions; `.hdv`'s `[GENERAL]` section literally has `Mass` and `CGHeight` as top-level keys, in kg and metres |
| **BeamNG.drive (jbeam)** | One JSON-like `.jbeam` per part: `nodes` (point masses, position relative to a reference frame), `beams` (spring/damper connections with stiffness and a breaking threshold), an inline or CSV-included `[rpm, torque]` array for the engine curve | The odd one out on purpose (§1): because the chassis itself is the simulated object, "mass distribution" is emergent from where nodes are placed, not a single CG-height number — the cost of soft-body realism is that even *mass* is not a scalar parameter anymore |
| **TORCS / Speed Dreams** | One XML per car: engine section (`mass`, `max-power`, `peak-engine-rpm`, `rpm-limit`, `inertia`, `idle`, `torque-curve-00..NN` as explicit `(rpm, torque)` pairs), wheel sections (dynamic-friction tyre parameters), aerodynamics section | Explicit torque-curve *points*, not a formula — the same representation this section recommends for simcraft (§6.2) |
| **VDrift** | One car-parameters file per vehicle (mass, wheelbase, differential, tyre parameters) | Confirms the same split a fourth time in a fully open-source, from-scratch engine |

**What this convergence says:** four independently-built simulators (one commercial and secretive about its
exact tyre formula, three open enough to read in full) landed on the *same* file boundaries — mass/CG/inertia,
engine curve, drivetrain (gears/diff), tyres, suspension, aero, brakes — because those are the genuine
independent degrees of freedom in how a car feels, not an accident of any one codebase's history. That is
strong evidence simcraft's own schema (§6.2) should follow the same boundaries rather than invent new ones.

**Which parameters actually drive the feel** (cross-referencing §2's model-layer table): the tyre parameters
(`DY0`/cornering stiffness, load sensitivity) and the engine torque curve are what a player feels every second
of driving; mass, CG height and wheelbase set the *character* (how much load transfer, how much yaw inertia) but
rarely need per-lap attention; gear ratios and differential matter on acceleration and out of corners; aero and
brakes matter only near a car's limits (top speed, hard braking) — exactly the priority order §4 already argued
for from a different angle (feel-per-dollar), now confirmed from the file-format side: nobody splits tyres and
engine into more than one file each, but everyone bundles "the rest" more coarsely.

### 6.2 A proposed schema for simcraft: `vehicle:` on a kind, or `vehicles/<name>.ron`

Following the architecture's existing convention — `game.ron` names a `kind`'s `motion`, and heavy shared
definitions (state machines, environments) live in their own files loaded by name — a vehicle is either an
inline `vehicle:` block on a `kind` (small games, one or two car types) or a named file in `vehicles/<name>.ron`
that a kind references (a game with many vehicle types: cars, trucks, bikes in one traffic sim), the same
shape `envs/<name>.ron` already has. Every field is `i64`, in `FINE`/`ANGLE`-consistent fixed-point units (§3),
never a float:

```ron
"car": (glyph: 'c', fsm: "car", motion: (size: (600, 1700)),  // footprint still fine units, across × along
        vehicle: (
            // Mass and geometry (fine units: mm for length/height, kg for mass — designer's choice, documented once)
            mass: 1350, cg_height: 480, wheelbase: 2600, track_width: 1500,
            weight_dist_front: 60,               // % of mass on the front axle (static)
            yaw_inertia: 0,                       // 0 = derive from mass × (0.45 × wheelbase)², §6.3; or set explicitly

            // Engine: RPM -> torque (Nm × 10, so 250.0 Nm = 2500), as data points, not a formula (TORCS' approach, §6.1)
            engine: (idle: 900, redline: 6500, limiter: 6700,
                     torque_curve: [(1000, 1200), (2000, 2100), (4000, 2500), (5500, 2400), (6500, 1900)]),

            // Drivetrain
            gears: [3910, 2140, 1430, 1030, 810, 660],   // ratios × 1000 (3.910, 2.140, ...)
            final_drive: 3420,                              // × 1000
            differential: "locked",                         // "open" | "locked" | "limited_slip" (a % lock-up if so)
            drivetrain: "rwd",                              // "fwd" | "rwd" | "awd" (+ a front/rear torque split for awd)

            // Brakes
            brake_torque_max: 3200, brake_bias_front: 62,   // % of braking on the front axle

            // Tyres: linear-with-saturation by default (§2 layer 2/3); Pacejka-fit compounds are an opt-in escalation
            tyre: (cornering_stiffness: 850,   // lateral force per degree of slip, per mille of static load (§6.3)
                   peak_slip_angle: 900,        // hundredths of a degree where grip peaks before saturating
                   friction_coeff: 1000),       // × 1000 (1.000 = a typical dry road tyre; lower for wet/gravel)
            // Escalation, opt-in, same shape either way (a kind's expressions never see which one is active):
            // tyre: (model: "pacejka", b: 10000, c: 1300, d: 1000, e: 970),

            // Steering and aero (aero optional; 0 = ignored, cheapest default)
            steer_lock: 4800,      // hundredths of a degree, wheel-to-wheel (e.g. 48.00°)
            drag_coeff: 0, downforce_coeff: 0, frontal_area: 0,
        )),
```

**Design choices, and why:**

- **Torque curve as data points, not a fitted formula** — TORCS' `torque-curve-00..NN` and jbeam's inline
  `[rpm, torque]` array both do this, and it is the right call for simcraft too: a designer (or a script that
  reads a spec sheet, §6.3) can paste published dyno points directly; the engine linearly interpolates between
  them (same integer-lerp already used elsewhere, e.g. `ramp`/`triangle`).
- **`differential`/`drivetrain` as short strings, not booleans** — matches the rule language's existing taste
  for readable `state` strings over magic numbers, and a `simcraft-check`-style validator can catch a typo
  (`"lockd"`) at load, the same way an unknown FSM state is caught today.
- **The tyre block has two shapes on purpose**, chosen by `model:` — a cheap linear-with-saturation default
  (§2 layer 2/3's recommended starting point, three numbers) and an opt-in Pacejka-coefficient escalation (four
  numbers, `B C D E`, §1's Magic Formula) — a kind's rules never see which one is active; the native `vehicle`
  evaluator (like `integrate_motion` today) picks the force law from which fields are present, exactly how
  `motion`'s own optional `gravity` field already works (present → falls; absent → doesn't).
- **`yaw_inertia: 0` means "derive it"** — the formula in §6.3 (`mass × (0.45 × wheelbase)²`) is a good enough
  default that most designers should never need to fill it in, mirroring how a kind's engine-owned motion props
  are created automatically today; a nonzero value overrides it for a designer who measured or found a real
  figure.
- **Everything a rule can already do to a `kind`'s `props` still works** — `vehicle` is a sibling of `motion`,
  not a replacement; a car's `v0`, `human`, `lane_to` props in today's `traffic/game.ron` are untouched, and the
  IDM/MOBIL rules keep writing a *desired* speed and lane the same way — what changes is that the native
  `vehicle` step (like `integrate_motion`, run natively after `apply`, §2/§6.4 of the fit table) turns "desired
  speed and steering intent" into an actual slip-based force and a heading change, instead of directly setting
  `vx`/`vy`.

### 6.3 From a public spec sheet to these numbers: what's published, what's guessed

A manufacturer's spec sheet (or a car-review site's numbers table) publishes some of §6.2's fields directly and
implies the rest only through rules of thumb — the same gap every sim above closes with either measurement
(a real dyno, a tri-filar pendulum for yaw inertia) or an accepted approximation. For a design tool that must
work from public numbers alone, not a test rig:

| Field | Usually published? | If not: rule of thumb | Source |
|---|---|---|---|
| `mass`, `wheelbase`, `track_width` | Yes (curb weight, wheelbase, track are standard spec-sheet lines) | — | manufacturer spec |
| `cg_height` | Rarely | Estimate 40–50% of the car's overall height for a sedan/hatch, lower (30–35%) for a low sports car, higher (55–60%+) for a truck/SUV/bus with a raised chassis | vehicle-dynamics texts (general knowledge; no single citation found with a clean number, moderate confidence) |
| `weight_dist_front` | Sometimes (enthusiast reviews quote it; base spec sheets often don't) | FWD hatch ≈ 60/40 front-biased; RWD sports car ≈ 50/50 to 45/55; truck ≈ 55–65% front unladen, shifts rearward loaded | industry convention, moderate confidence |
| `yaw_inertia` | Almost never | `mass × (0.45 × wheelbase)²` (radius of gyration ≈ 45% of wheelbase, "on average," per vehicle-dynamics references); a more precise German rule-of-thumb formula from SAE #840561: `(0.1269 to 0.1468) × wheelbase × overall_length × mass` | eng-tips.com vehicle-dynamics discussion citing SAE #840561; Mitostile Prototipo, *Calculating vehicle inertia* |
| `engine.torque_curve` | Peak torque + peak power + their RPMs are always published; the full curve almost never is | Interpolate a smooth curve through (idle, ~0), (peak torque RPM, peak torque), (peak power RPM, power/RPM-derived torque), (redline, a modest fall-off) — a rough shape, not a dyno trace, but good enough for §2's non-stiff drivetrain ODE | this document's own synthesis of §2's drivetrain model, not a cited source |
| `gears`, `final_drive` | Yes, almost always (a full gear-ratio table is standard spec-sheet content) | — | manufacturer spec |
| `differential` type | Sometimes named ("limited-slip standard/optional"); rarely a lock-up percentage | Open for an economy FWD car, limited-slip for anything marketed as sporty, locked only for genuine race/off-road builds | general industry convention |
| `tyre.cornering_stiffness` | Never (tyre-rig lab data, not a spec-sheet field) | **≈ 16–17% of the tyre's static load, per degree of slip** — e.g. a 1350 kg car, 60/40 front-biased: front axle load ≈ 810 kg ≈ 7950 N, ÷2 wheels ≈ 3975 N per front tyre, × 0.165 ≈ 656 N/degree per front tyre | eng-tips.com / mchenrysoftware.com tire-cornering-stiffness worked examples (moderate confidence: a widely repeated engineering rule of thumb, not a single authoritative source) |
| `friction_coeff` | Never directly; tyre compound and weather are named | 1.0–1.1 for a summer tyre on dry tarmac, 0.7–0.8 wet, 0.4–0.6 gravel/snow — standard driving-dynamics ballpark figures, not sim-specific | general knowledge, moderate confidence |
| `drag_coeff`, `frontal_area`, `downforce_coeff` | Cd sometimes published for cars marketed on efficiency; frontal area almost never; downforce essentially never outside racing cars | Cd 0.28–0.35 hatch/sedan, 0.35–0.45 SUV/truck/bus; frontal area ≈ track_width × height × 0.85 (a rough silhouette-fill factor); downforce 0 unless the vehicle is an explicit race car | general knowledge, low-moderate confidence — flagged as the least evidenced row in this table |
| `brake_torque_max`, `brake_bias_front` | Rotor/caliper size sometimes published; torque figure never | Bias 60–70% front (weight transfers forward under braking, front tyres do most of the work) is a safe default across nearly every road car; torque itself matters less than bias for *feel*, since §2's load-transfer layer already governs how much grip each end has | general driving-dynamics convention |

**Honest uncertainty, stated once for the whole table:** every "rule of thumb" row above is a design-tool
convenience, not a measured fact about any specific real car — the same gap Milliken's *Race Car Vehicle
Dynamics* and every sim's own setup-guide culture fills with either real measurement or an accepted
approximation. A designer typing in a real car's name should expect these defaults to be *plausible*, not
*correct*; §2's whole point (layers 1–4 first) is that simcraft's target is "feels like a real car of this
general shape," not "reproduces this exact car's telemetry," so this level of approximation is the right
amount of rigor for the job, not a shortcut that undermines it.

### 6.4 One core, many vehicle shapes: what's data, what's still code

The point of §6.2's schema is that a **truck**, a **bus**, a **sports car** and a **motorbike-like two-wheeler**
should all run through the *same* native `vehicle` evaluator — the one proposed in §7's engine-feature order —
by varying only the numbers:

- **A truck or bus**: same schema, different numbers — higher `cg_height` (more load transfer, more body roll
  *feel* even without simulated suspension, §2 layer 4), lower `weight_dist_front` when laden, a much higher
  `yaw_inertia` (long wheelbase, heavy), a flatter/lower-peak-RPM torque curve, more gears or a very tall final
  drive, a much lower `steer_lock`-to-turning-radius ratio. **Needs zero new code.**
- **A sports car**: same schema, tuned the other way — low `cg_height`, near-even `weight_dist_front`, high
  peak-RPM torque curve, tighter gear ratios, higher `tyre.cornering_stiffness` (stickier compound), nonzero
  `downforce_coeff`. **Needs zero new code.**
- **A motorbike-like two-wheeler**: this is the honest limit of the bicycle-model schema, and worth naming
  precisely rather than glossing over. The *longitudinal* half of §6.2 (engine, gears, differential-as-single-
  wheel-drive, brakes) transfers directly. The *lateral* half does not: a motorcycle's dominant lateral dynamic
  is **lean angle** (countersteering into a lean, cornering force coming from camber thrust as much as slip
  angle) — a third rotational degree of freedom (roll) that the two-wheel bicycle model (§2) has no state
  variable for at all. Representing a motorbike convincingly is not a parameter change to `vehicle:`; it is a
  **new model layer** (a lean-angle state and a camber-thrust force law, on top of — not instead of — the
  existing yaw/heading state), which is real new native code, not data. **This is the one vehicle "shape" on
  this list that would need engine work beyond §6.2's schema**, and this document recommends treating it as a
  distinct future feature (its own §7-style entry, only when a game actually wants motorbikes), not something
  to design speculative fields for now.
- **What always stays code, for every vehicle shape**: the force laws themselves (how a torque curve, a slip
  angle or a load produces a number) — those are `sim-core` native functions, evaluated the way
  `integrate_motion` is today, deterministic and Rhai-free in the hot path (§3's fixed-point recommendation).
  Data describes *this specific vehicle*; code describes *what a vehicle is*, the same essence/accident split
  this agent's own methodology names for every research question. A `vehicle:` schema that needed new Rust for
  every new car would have failed at being "an engine simulator that can build any car" — the test this whole
  section is answering is exactly that every *car-shaped* vehicle (hatch, truck, bus, sports car) is representable
  by §6.2 alone, and the one shape that fails that test (a motorbike) fails it for a precise, nameable reason
  (a missing state variable), not a vague one.


## 7. Fit for simcraft: engine features, in order, with effort — and what not to build

All of these are **engine** features (data-driven, game-agnostic), matching the standing rule ("a game hits a
wall, the wall gets named, the engine grows to cross it," `CLAUDE.md`). None of them make simcraft a rigid-body
physics engine — that path was already evaluated and declined in `docs/research/physics-engines.md` §5, and
nothing found in this research reopens that question.

Feature (2) below is exactly the native `vehicle` evaluator §6.2 proposed as data (`vehicle:` on a
kind, or `vehicles/<name>.ron`); this table sequences *when* to build the evaluator, §6 already specified
*what* it reads.

| Order | Feature | What it is | Depends on | Effort | Why this order |
|---|---|---|---|---|---|
| 1 | **Heading + angular velocity in `motion`, oriented footprints** | New engine-owned props (e.g. `heading`, `turn_rate`, in `ANGLE` units, §3) on a `motion` kind; footprint queries become oriented rectangles | §3's BAM16 format + sin/cos table | **Days to ~1.5 weeks** | Nothing else in this list is possible without a car that can face a direction other than +y; this is the one true prerequisite |
| 2 | **A native `vehicle` component, kinematic bicycle model first** | Evaluated in the core after `apply`, like `integrate_motion` today; parameters (wheelbase, max steer, etc.) are data in `game.ron`, the same way IDM's `a`/`b`/`headway` are today | (1) | **~3–5 days** for kinematic (layer 1, §2); **+1–2 weeks** to escalate to dynamic bicycle + linear tyre + algebraic load transfer (layers 2–4, §2) — the recommended stopping point for "feels real" | This alone, even at kinematic-only, is a bigger feel jump than anything else on this list: a car that turns like a car instead of strafing like a puck |
| 3 | **Analog input axes in the input layer** | `input.ron` binding types extended with continuous axes (gamepad sticks/triggers, held-key ramps for keyboard), plumbed to declared actions as continuous values instead of today's discrete `d: 1 / -1` | Independent of (1)/(2), but pointless without them | **~2–4 days** | Cheapest single item on this list with an outsized effect on "feels controllable, not toggled" (§4) — do this in parallel with (2), not after |
| 4 | **Oriented-box broadphase (SAT)** | The existing per-lane sweep-and-prune (`games/traffic/PERF.md`) generalizes: keep it as a conservative cull, add the 4-axis SAT exact test (§5) for candidates once cars are not lane-locked | (1) | **~3–5 days** | Needed the moment a car can leave its lane under its own steering — i.e., right after (2)/(3) ship, not before |
| 5 | **Spline tracks in the renderer + `track_dist`/`track_offset` core queries** | A track's centreline + width as game data; two new query functions in the spirit of `ahead`/`behind`; `track.ron` gains curve rendering and a camera that follows the spline, not a straight Z axis | (1)–(4), so there is a car worth putting on a curved track | **~2–3 weeks** | The single biggest item, and the one to defer: a heading-aware, analog-input, oriented-collision car is already a complete "drives like a car" experience on today's straight road or an open plane — curved circuits are the next investment, not a prerequisite for feeling real |

**What NOT to build, and when to revisit:**

- **Full multi-body suspension (layer 5, §2).** The one component directly responsible for Assetto Corsa's
  333–1000 Hz physics rate (§1, §3) and years of the genre's own engineering effort (Racer, rFactor, iRacing,
  ACC all treat it as a major, ongoing subsystem). Nothing in a traffic game needs kerb-climbing suspension
  geometry or spring-rate tuning; layer 4's algebraic load transfer plus camera pitch/roll cues (§4) buys most
  of the *perceived* effect for a tiny fraction of the cost. **Revisit only** if simcraft ever targets a
  dedicated sim-racing title where suspension setup is itself the gameplay (tuning for a specific track).
- **Nonlinear Pacejka Magic Formula with lab-fit coefficients.** Layer 3 (§2) is real and eventually worth
  having for the saturating grip curve, but the *B/C/D/E* coefficients Kunos, iRacing and academic sources all
  fit from tyre-rig lab data are not something simcraft can guess its way to; a hand-tuned nonlinear traction
  curve (the games-industry shortcut in the Wassimulator/Marco Monster tradition — same saturating shape,
  no lab-fit requirement) gets most of the feel at a fraction of the tuning cost. **Revisit** only if simcraft
  licenses or gathers real tyre data, or targets esports-grade accuracy.
- **Tyre thermal/wear modeling (layer 7, §2).** Meaningful for race-strategy games (stint length, tyre
  management as a decision), irrelevant to lap-to-lap or tick-to-tick driving feel. **Revisit** if a future game
  is explicitly about race strategy, not driving.
- **Physically modeled force-feedback hardware support.** Cheap in physics terms (§4) but real integration cost
  for a narrow, wheel-owning audience; simcraft's product direction (Unity/Unreal hosts, `docs/architecture.md`
  "Product direction") already puts hardware I/O on the host side, not the core. **Revisit** once a host adapter
  specifically wants it, not proactively.
- **Soft-body/BeamNG-style deformation.** A different genre of realism (damage, destruction) answering a
  different question than "drives like Assetto Corsa" (§1). Not on this path at all.
- **A lean-angle (motorbike) model.** §6.4 named this precisely: the two-wheel bicycle model (§2) has
  no roll/lean state at all, so a motorbike is not a parameter change to §6.2's schema, it is a new
  model layer (a third rotational degree of freedom and a camber-thrust force law). **Revisit** only
  when a game actually wants two-wheelers, as its own future engine feature, not speculatively now.

## Sources

1. r/OverTake.gg. *Assetto Corsa Competizione: The 5 Point Tyre Model Blog*.
   https://www.overtake.gg/threads/assetto-corsa-competizione-the-5-point-tyre-model-blog.171148/
2. Kunos Simulazioni forum. *PHYSICS - Introducing the 5 point tyre model for ACC!*
   https://www.assettocorsa.net/forum/index.php?threads%2Fintroducing-the-5-point-tyre-model-for-acc.59307%2F=
3. Ravsim (Race and Vehicle Simulations). *Stefano Casillo on netKar Pro* (parts 1 and 2) — netKar Pro's four
   tyre models (Pacejka → similarity → brush → Casillo's own). https://ravsim.com/2011/11/12/stefano-casillo-talks-about-netkar-pro-part-1/
   · https://ravsim.com/2012/08/27/stefano-casillo-on-netkar-pro-part-2/
4. OverTake.gg forum thread on Assetto Corsa physics rate; Kunos Simulazioni forum, *Physics and FF frequency*
   (333 Hz, raised toward ~2× in later builds). https://www.overtake.gg/threads/what-rate-do-physics-run-at-1000hz-500.159380/
   · https://www.assettocorsa.net/forum/index.php?threads%2Fphysics-and-ff-frequency.31453%2F=
5. Traxion.gg. *Assetto Corsa Competizione console update out now, adds new tyre model and 400Hz physics*.
   https://traxion.gg/assetto-corsa-competizione-console-update-out-now-adds-new-tyre-model-and-400hz-physics/
6. Racer.nl (Ruud van Gaal). *Car physics* (1000 Hz internal physics rate, Pacejka tyre formula); *Track
   Splines* (spline smoothing of polygon track data); *Defining car suspensions* (independent suspension,
   springs/dampers/anti-roll/rollcenter/anti-pitch, rebound stops). http://www.racer.nl/reference/carphys.htm ·
   http://www.racer.nl/reference/tracks_spline.htm · http://www.racer.nl/tutorial/suspensions.htm ·
   http://www.racer.nl/reference/pacejka.htm (direct fetch of these pages failed during this research —
   expired TLS certificate — findings here are from cached search-result summaries only; moderate confidence)
7. Wassimulator. *Programming Vehicles in Games* — engine/wheel coupled ODEs, tyre-slip-to-Pacejka pipeline,
   explicit "no game simulates all of this at full fidelity" framing. https://wassimulator.com/blog/programming/programming_vehicles_in_games.html
8. Miata.net (mirror). Brian Beckman. *The Physics of Racing*, parts 1–22 (grip/slip angle, the Magic Formula,
   weight transfer). https://www.miata.net/sport/Physics/phor.pdf · https://www.miata.net/sport/Physics/
9. Adam Sawicki (mirror of Marco Monster's tutorial). *Car Physics for Games* — slip ratio/traction curve,
   weight transfer, lateral force vs. load. https://www.asawicki.info/Mirror/Car%20Physics%20for%20Games/Car%20Physics%20for%20Games.html
10. Wikipedia. *Tire model* — brush model vs. Magic Formula, semi-empirical vs. physical.
    https://en.wikipedia.org/wiki/Tire_model
11. Wikipedia. *Self aligning torque*. https://en.wikipedia.org/wiki/Self_aligning_torque
12. GameDev.net forum. *Generating force feedback from slip angle data?* — SAT as a lateral-force readout,
    kingpin offset/caster/pneumatic trail for physically-based FFB. https://gamedev.net/forums/topic/711400-generating-force-feedback-from-slip-angle-data/
13. iRacing.com. *Physics Modeling: NTM V7 Info*; iRacerHUB. *iRacing NTMv10: What the New Tire Model Actually
    Changes* (physically-based tyre model, double-precision physics, multi-zone contact-patch temperature).
    https://www.iracing.com/physics-modeling-ntm-v7-info-plus/ · https://iracerhub.com/iracing-ntmv10-tire-model-explainer/
14. Farroni, F., Russo, M., Sakhnevych, A. & Timpone, F. *TRT EVO: Advances in real-time thermodynamic tire
    modeling for vehicle dynamics simulations*. Proc. IMechE Part D, 2019.
    https://journals.sagepub.com/doi/full/10.1177/0954407018808992
15. gamepressure.com. *NFS Most Wanted (2005): Car handling*; NFS Wiki (Fandom). *Need for Speed (2015)/Tuning*
    (speed-sensitive steering, drift/stability assists). https://www.gamepressure.com/needforspeedmostwanted/the-basics-car-handling/zb1fd
    · https://nfs.fandom.com/wiki/Need_for_Speed_(2015)/Tuning
16. Wikipedia. *Need for Speed: Shift* (the franchise's explicit sim departure).
    https://en.wikipedia.org/wiki/Need_for_Speed:_Shift
17. Rigs of Rods documentation and Wikipedia; DeepWiki (RigsOfRods/rigs-of-rods). Soft-body node/beam physics,
    Actor/GfxActor separation. https://www.rigsofrods.org/ · https://en.wikipedia.org/wiki/Rigs_of_Rods ·
    https://deepwiki.com/RigsOfRods/rigs-of-rods/1-overview
18. CrashMods. *BeamNG vs Assetto Corsa Physics* ("what happens when metal bends" vs. "how a tire finds grip").
    https://crashmods.com/news/beamng-vs-assetto-corsa-physics/
19. VDrift. Repository (drift-focused open-source driving sim). https://github.com/VDrift/vdrift
20. TORCS (sourceforge). Track file distribution and tutorial; ResearchGate. *TrackGen: An interactive track
    generator for TORCS and Speed-Dreams* (spline-smoothed polygon tracks; documentation on exact spline
    encoding is thin — moderate confidence). https://sourceforge.net/projects/torcs/files/torcs-tracks/ ·
    https://www.researchgate.net/publication/272381520_TrackGen_An_interactive_track_generator_for_TORCS_and_Speed-Dreams
21. Medium (Sachin Kundu, Roboquest). *Understanding Geometric Path Tracking Algorithms — Stanley Controller*;
    Medium (Yan Ding). *Three Methods of Vehicle Lateral Control: Pure Pursuit, Stanley and MPC* — pure
    pursuit vs. Stanley, rear-axle vs. front-axle reference, cross-track + heading error.
    https://medium.com/roboquest/understanding-geometric-path-tracking-algorithms-stanley-controller-25da17bcc219
    · https://dingyan89.medium.com/three-methods-of-vehicle-lateral-control-pure-pursuit-stanley-and-mpc-db8cc1d32081
22. ResearchGate. *An Analysis of the Vehicle Dynamics Behind Pure Pursuit and Stanley Controllers*.
    https://www.researchgate.net/publication/369942308_An_Analysis_of_the_Vehicle_Dynamics_Behind_Pure_Pursuit_and_Stanley_Controllers
23. arXiv (multiple). Kinematic vs. dynamic bicycle model formulations, slip angle equations, validity bounds
    (kinematic model valid to ~0.5 μg lateral acceleration). https://arxiv.org/pdf/2011.09612 ·
    https://arxiv.org/pdf/1804.08290 · https://arxiv.org/pdf/2306.04857
24. Envato Tuts+ (code.tutsplus.com). *Collision Detection Using the Separating Axis Theorem*; Geometric Tools
    (David Eberly). *Dynamic Collision Detection using Oriented Bounding Boxes* (general 3D OBB SAT, 15 axes —
    simcraft's 2D case needs only 4, this document's own derivation, not a cited result).
    https://code.tutsplus.com/collision-detection-using-the-separating-axis-theorem--gamedev-169t ·
    https://www.geometrictools.com/Documentation/DynamicCollisionDetection.pdf
25. Photon Engine docs. *Quantum 3 — ECS — Fixed Point* (FP = Q48.16, deterministic lockstep vehicle physics).
    https://doc.photonengine.com/quantum/current/manual/quantum-ecs/fixed-point
26. CppCat. *Deterministic Physics in C++: Fixed-Point Math and Reproducible Simulation* — lookup-table sin/cos
    at Q16.16, integer sqrt via Newton-Raphson, why native `sqrt`/`sin`/`cos` on doubles break determinism.
    https://cppcat.com/deterministic-physics-engine/
27. pennelynn.com (reprint). *The CORDIC Method for Faster sin and cos Calculations* (CUJ, Nov 1992); glasgow.ac.uk
    (J. Wilson). *Simple CORDIC*. http://www.pennelynn.com/Documents/CUJ/HTML/92HTML/1992024C.HTM ·
    https://www.dcs.gla.ac.uk/~jhw/cordic/
28. ResearchGate (Request PDF). *A Modified Implicit Euler Algorithm for Solving Vehicle Dynamic Equations*
    (stiffness from tyre forces and suspension-bushing compliance in vehicle dynamic equations, real-time
    integration under milliseconds). https://www.researchgate.net/publication/226140008_A_Modified_Implicit_Euler_Algorithm_for_Solving_Vehicle_Dynamic_Equations
29. pybullet.org forum. *Is semi-implicit Euler unconditionally stable?* (stability bound: oscillation
    frequency < half the sampling frequency). https://pybullet.org/Bullet/phpBB3/viewtopic.php?t=9449
30. DIVA portal (Harald af Malmborg). *Evaluation of Car Engine Sound Design Methods in Video Games* — additive
    vs. granular synthesis for engine audio. https://www.diva-portal.org/smash/get/diva2:1557027/FULLTEXT01.pdf
31. GitHub (search-summary sources; racing-camera rigs). Chase-camera spring/damper follow, speed-scaled boom
    offset, look-ahead. https://github.com/Skaruts/racing_cameras
32. GitLab (Fabrice Wong Kwok / MUR-AssettoCorsa-19E). `engine.ini`, `tyres.ini` examples from a real AC car
    folder. https://gitlab.eng.unimelb.edu.au/fwongkwok/mur-assettocorsa/blob/tyres/mur_2019/data/engine.ini ·
    https://gitlab.eng.unimelb.edu.au/fwongkwok/mur-assettocorsa/-/blob/9b5f58048ae9d8f1d66304324834186824dfabf9/mur_2019/data/tyres.ini
33. Assetto Corsa Mods forum. *tyres.ini explained* (`DY0`, `LS_EXPY`, `DCAMBER_0/1` parameter meanings).
    https://assettocorsamods.net/threads/tyres-ini-explained.1904/
34. Studio-397. *Updating HDV and TBC files*; tirewall.net (rFactor 2 Modding Handbook). *High-Detail Vehicle -
    HDV*; MotorLaps. *Car Physics | HDV Engine UltraChassis rFactor 2* (`.hdv` general/suspension sections,
    `Mass`, `CGHeight`; `.tbc` tyre compounds). https://www.studio-397.com/2016/05/updating-hdv-and-tbc-files/ ·
    http://www.tirewall.net/mh-rf2/vehicle/physics/hdv.html · https://motorlaps.com/car-physics-rfactor2.php
35. BeamNG Documentation. *JBeam Syntax*; *Nodes*; *Introduction to JBeam* (node/beam mass-spring structure,
    inline or CSV-included `[rpm, torque]` engine curve). https://docs.beamng.com/modding/vehicle/intro_jbeam/jbeamsyntax/
    · https://documentation.beamng.com/modding/vehicle/sections/nodes/ ·
    https://documentation.beamng.com/modding/vehicle/intro_jbeam/
36. arXiv. *Simulated Car Racing Championship Competition Software Manual*, 2013 (TORCS engine XML parameters:
    `mass`, `max-power`, `peak-engine-rpm`, `rpm-limit`, `inertia`, `idle`, explicit `torque-curve-NN` (rpm,
    torque) points). https://arxiv.org/pdf/1304.1672
37. VDrift wiki. *Car parameters for vdrift-2010-06-30* (a fourth independent car-parameter file format,
    confirming the same mass/wheelbase/differential/tyre split; direct fetch failed during this research —
    expired/self-signed certificate — cited from the page title and search-result context only, low
    confidence on specifics). http://wiki.vdrift.net/index.php?title=Car_parameters_for_vdrift-2010-06-30
38. Eng-Tips (engineering forum). *Moments of inertia* thread, citing SAE Paper #840561 (yaw radius of gyration
    ≈ 45% of wheelbase "on average"; German rule-of-thumb formula `(0.1269–0.1468) × wheelbase × length × mass`).
    https://www.eng-tips.com/threads/moments-of-inertia.357873/
39. Mitostile Prototipo. *Calculating vehicle inertia*. https://sites.google.com/site/mitostile/vehicle-dynamics-powertrain-basics/technical-articles/chassis-suspension-brakes/calculating-vehicle-inertia
40. mchenrysoftware.com. *Examples - Tire Cornering Stiffness Calculation*; Eng-Tips thread via the same query
    (cornering stiffness ≈ 16–17% of static tyre load, per degree of slip — a widely repeated engineering rule
    of thumb, not traced here to one original derivation). https://mchenrysoftware.com/medit32/readme/msmac/examplestirecorneringstiffnesscalculation1.htm

---

**Epistemological note.** This is a snapshot as of 2026-09-30. Several findings rest on search-engine-summarized
forum and community sources (Assetto Corsa's exact physics Hz, Racer.nl's documentation pages, TORCS' track
XML format) rather than a primary document read in full — those are flagged inline with "moderate confidence"
where the summarization is doing real interpretive work, and two Racer.nl pages could not be fetched directly
(expired TLS certificate on `racer.nl`) so are cited from cached search snippets only. The academic vehicle-
dynamics sources (bicycle-model formulations, Pacejka/brush comparison, Stanley/pure-pursuit analyses,
implicit-Euler stiffness) are drawn from arXiv preprints and peer-reviewed venues and carry higher confidence.
The 2D-SAT-is-4-axes derivation in §5 is this document's own reasoning from the general 3D case, not a
cited claim. §6.3's real-car estimation table is explicitly a design-tool convenience,
not a set of measured facts about any specific vehicle — every row not backed by a manufacturer spec sheet
is flagged low-to-moderate confidence in the table itself, and the VDrift wiki citation (§6.1) could not be
fetched directly (self-signed certificate) so rests on a page title and search-result context only. Re-evaluate this document if simcraft ever ships a track-based (non-straight-road) racing game, at
which point §5's spline-track design should move from proposal to implementation record, or if a future game's
tuning reveals the small-angle slip approximation (§3) breaking down at speeds/angles a traffic game does not
currently reach.
