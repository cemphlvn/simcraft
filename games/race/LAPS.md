# Race: the lap benchmark

The dogfooding benchmark for the physics layer: a real car on a real track has a real lap time. The 2024 Coca-Cola
600 pole at Charlotte Motor Speedway was **29.355 s (183.955 mph)**, Ty Gibbs, Next Gen car, 670 hp package.

    cargo build --release -p sim-physics
    target/release/simcraft-physics-bench lap games/race/tracks/charlotte.ron assets/vehicles/stock_car.ron
    SIMCRAFT_TRACE=1 ...                              # twice a second: speed, plan, steering, pedals, slip
    SIMCRAFT_PILOT="reach=0.4,gain=1,pace=1" ...      # probe the autopilot's knobs

- `lap_s`: the autopilot's flying lap with the full dynamics (second of three laps). The number to beat.
- `plan_s`: the quasi-steady-state lap simulation's prediction for the same car and line (no driver).
- `worst_off_line_m`: how far the car strayed from its line (the track is 18 m wide: more than 9 m is off it).
- `max_lat_g`: the most the tyres pushed sideways (what the driver feels).

The car's grip, aero and torque curve are estimates (`assets/vehicles/stock_car.ron` marks each); the track is
Charlotte's published length, radii and banking on an approximate plan (`gen_track.py`). Tuning an estimate
toward the real lap time is setup, not proof: say which number moved and why.

| step | change | lap_s | plan_s | off line | lat g | learned |
|---|---|---|---|---|---|---|
| 0 | first run: layer 1 speeds, pilot at full throttle on any deficit | 30.20 | 26.19 | 31.3 | 5.05 | the lap time looked close by accident: the car ran 31 m off the line; lateral g counted the slope's pull |
| 1 | plan: each axle's grip (the weaker sets the limit); g from tyre forces | 30.50 | 26.71 | 27.9 | 3.83 | still off the line: the plan asks for more than the car can hold |
| 2 | pilot aims from the direction of travel, plus Stanley | never | 26.71 | — | — | positive feedback: turning left, the car slides right of its heading, the error grows; spun in 3 s |
| 3 | pilot aims from the heading again | never | 26.71 | — | — | power oversteer: full throttle at 78 m/s left the rear no grip to turn; realistic, and the plan ignored it |
| 4 | plan: the driven axle's grip is shared between cornering and holding speed (friction circle) | never | 30.56 | — | — | the plan is now 4 % off the pole; the pilot still spins (all-or-nothing pedals) |
| 5 | pilot: feed-forward throttle, lifts as the driven tyres near their peak slip | never | 30.56 | — | — | still spins in the dogleg: the car slid 6.5° at 1.8 g |
| 6 | car: slick cornering stiffness 18 → 30 %/° (grip peaks near 5° of slip); track: dogleg radius 300 → 500 m (est) | never | 29.85 | — | — | plan 1.7 % off; the pilot now swings: Stanley's angle term asks for ~4 g per degree at 80 m/s |
| 7 | pilot: no Stanley term (pure pursuit only) | 31.45 | 29.85 | 19.4 | 2.79 | first clean laps; the car runs wide: 1 s of look-ahead is 80 m |
| 8 | pilot: look-ahead 0.4 s, pedal gain 1 (probed: 0.3–1 s × 0.1–1) | **30.35** | 29.85 | 4.2 | 3.25 | 3.4 % off the pole; the plan is a little conservative (pace 1.02 laps 29.95) |
| 9 | driver aids (traction control, ABS) for the pilot; standing starts (`standing=1`) | 30.37 | 29.85 | 4.2 | 3.25 | from rest the pilot's lift was outvoted (applied before clamping) and 670 hp on the rear spun the car; with TC it launches cleanly |
| 10 | drivetrain: torque curve at the gear's rpm, optimal-point automatic gearbox, engine braking | 30.43 | 29.85 | 4.1 | 3.23 | the stand-in (peak power at every speed) cost only 0.06 s here: an oval is flat out in 5th at ~8,500 rpm |
