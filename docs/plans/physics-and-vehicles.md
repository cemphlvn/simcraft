# Plan: a physics layer and a vehicle simulator (draft, 2026-09-30)

Status: **decided, being built.** The model layer, number formats, step rate and vehicle schema come from
`docs/research/driving-physics.md`; how we build and measure the physics layer itself from
`docs/research/building-a-physics-engine.md` (in progress). Designer's decisions so far: realistic driving first *inside the traffic game*, on a
keyboard with assists; "like an actual engine simulator, so we can build **any** car with it" (a car is data).
Once decided, this moves into `docs/architecture.md` (the single source of truth) and this file is deleted.

## Principles (each from evidence already in the repo)

1. **Physics is a library behind a narrow boundary** (`docs/research/physics-engines.md` §1: Unity, Unreal and
   Godot all do this). A new crate, `sim-physics`, knows nothing about games, rules or the world's entities: plain
   data in, a fixed step, plain data out. `sim-core` calls it; rules never see its internals.
2. **Integers, deterministic** (`docs/architecture.md`, Determinism; physics research §4: Photon Quantum is the
   precedent). A fixed-point number type inside `sim-physics`, converted to `FINE` units at the boundary. Same
   inputs, same hash on every machine and core count.
3. **The rules decide, the physics computes.** Rules write *intent* (throttle, brake, steer, gear) as props; the
   physics writes *state* (speed, rpm, gear, slip, yaw) back as props. The designer's file never contains a force.
4. **A vehicle is data.** `assets/vehicles/<name>.ron` (engine torque curve, gears, mass, geometry, tyres, brakes,
   aero), named from `game.ron` like an asset pack. A truck, a hatchback and a bus differ by file, not by code.
5. **Measured, like everything else**: unit tests against known physics (skidpad, braking distance, 0–100 against
   the real car's spec), determinism tests, `tools/perf.py` (cars × sub-steps), `--feel` for how it drives.

## Where things live

```
crates/sim-physics/            no dependency on sim-core, sim-rules or any game
  src/fixed.rs                 the fixed-point number, deterministic sqrt / sin / cos / atan2 (tables or CORDIC)
  src/geom.rs                  vectors, oriented boxes, separating-axis overlap, AABBs
  src/broad.rs                 broadphase: today's sweep and prune (world.rs), generalised to oriented footprints
  src/vehicle/mod.rs           VehicleDef (the data), VehicleState, step(def, state, controls, dt)
  src/vehicle/tyre.rs          tyre force models, simplest first (research picks the first layer)
  src/vehicle/drivetrain.rs    engine torque curve, gears, differential, brakes
  src/vehicle/assists.rs       ABS, traction control, speed-sensitive steering (keyboard play)
assets/vehicles/<name>.ron     cars as data (+ a README: where each number comes from on a spec sheet)
crates/sim-core/src/world.rs   `motion` gains heading and yaw rate; a kind with `vehicle` is stepped natively
crates/sim-rules               `vehicle: "<name>"` on a kind; loads and validates the vehicle file
crates/sim-gpu                 draws heading (a car turns), later splines for curved roads
input                          analog axes (keyboard keys ramped into an axis; gamepad later), recorded in replays
```

## The boundary, concretely

```
kind "car": ( motion: (size: (..)), vehicle: "hatchback" )     // game.ron: which car
props written by rules:   throttle, brake, steer (-1000..1000), gear_request        // intent
props written by physics: speed, rpm, gear, yaw, yaw_rate, slip_front, slip_rear   // state
```

Each tick the core gives `sim-physics` every vehicle's controls and state, `sim-physics` sub-steps them (the
research gives the rate; Assetto Corsa is reported to run its physics far above the frame rate), and writes the new
state back. Collisions stay queries (`touching`, `ahead`) plus rules until a game needs impulses.

## Order of work (pending the research)

1. `sim-physics` with the fixed-point type and its tests (the base everything stands on).
2. Heading in `motion`: yaw and yaw rate, oriented footprints, the index over their bounding boxes (same answers
   for axis-aligned kinds: existing games keep their hashes).
3. The vehicle step, first model layer only; one real car as data; the player's car in `traffic` uses it.
4. Keyboard axes with assists; controls in the replay log.
5. More cars from spec sheets; traffic AI steering (path following) while IDM keeps setting its speed.
6. Only if a game needs it: more model layers, curved roads, impulses between cars.

## Decided (from `docs/research/driving-physics.md`)

- **Model layers, in order, each measured before the next:**
  1. Kinematic bicycle: a car that turns like a car.
  2. Dynamic bicycle: a linear, saturating tyre with algebraic load transfer. The car can slide.
  3. Drivetrain (layer 6): torque curve, gears, clutch, RPM, differential, brakes. Throttle and gears matter.
  4. Four wheels with suspension (layer 5, the designer's call, 2026-09-30: "we are building a full car and
     engine simulator"). Each corner gets a spring and a damper, plus anti-roll bars. The body gains heave, pitch
     and roll, and each tyre's load comes from its spring instead of from the algebraic formula.

  Not built: tyre heat and wear, lab-fitted Pacejka coefficients, soft bodies.
- **Step rate: the game ticks at 60 Hz; suspension sub-steps inside the physics pass.** Suspension is the one
  stiff part. The wheel mass bouncing on the tyre's stiffness (about 40 kg on 200 kN/m) oscillates near 11 Hz,
  which is too close to 60 Hz for explicit integration. The pass therefore runs `substeps` per tick: vehicle
  data, default 8 (480 Hz, inside Assetto Corsa's 333–1000 Hz). Sub-steps stay integers and deterministic, and
  are invisible to rules, the hash cadence and replays. The number is measured, not guessed: the lowest one at
  which a bump test settles the same way. If fixed point starts drifting at small sub-steps (Q16 of a metre per
  1/480 s is coarse at walking speed), the energy and settling tests will show it, and the fix is more
  fractional bits.
- **Tests that keep the layers honest:**
  - at low lateral acceleration, the four-wheel car's steady-state turn matches the bicycle model's;
  - a car dropped on its springs settles at the static sag, `m·g/k`;
  - an undamped spring conserves energy over a long run;
  - the damping ratio from the data matches the measured decay.
- **Road surface:** flat for now, so suspension shows through dive, squat, roll and their settling. Bumps and
  kerbs need a surface height the physics can query, added with spline tracks.
- **Numbers:** positions keep `FINE`. Inside `sim-physics`, a Q48.16 fixed-point type (Photon Quantum's format)
  with i128 intermediates. Angles are binary angles (a full turn is a power of two, so wraparound is free), and
  sin/cos come from a quarter-wave table generated at compile time with integer maths, interpolated. The slip
  angle uses the small-angle ratio vy/vx instead of atan2, and square roots use integer Newton iteration.
- **A vehicle is data:** §6.2's schema (mass, CG height, wheelbase, track, weight split, torque curve as points,
  gears, final drive, differential, drivetrain, brakes, tyre, steer lock, aero), with `0` meaning "derive it"
  (§6.3's rules of thumb, e.g. yaw inertia = mass × (0.45 × wheelbase)²). Trucks, buses and sports cars are just
  different numbers. A motorbike is not (lean is a missing state), so it is deferred.
- **The schema gains** per-axle `spring_rate`, `damper_bump`, `damper_rebound`, `antiroll`, `ride_height`,
  `unsprung_mass`, `tyre_rate`, and the vehicle-level `substeps`. As with the other fields, `0` means "derive it"
  (e.g. spring rate from a target ride frequency).
- **After the car:** analog input axes (in parallel with the vehicle), oriented-box SAT on top of the sweep and
  prune, and spline tracks with `track_dist`/`track_offset` later.
