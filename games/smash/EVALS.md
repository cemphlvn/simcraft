# SMASH: eval log

The core mechanic (pull, release, smash) tuned one change at a time. Every step plays the same five scripted shots
(`eval.toml`: center full, mid, soft, aimed left, aimed right) on fresh towers through the same calls a finger
makes, and is compared with the step before. Raw results: `evals/NNN-*.json`.
Run: `cargo build --release -p sim-mobile --bin simcraft-smash && target/release/simcraft-smash eval [--save NAME]
[--set path=value]`; re-run the last step with `--check`. Pictures of a shot: `simcraft-smash shot OUT_DIR`.

| Step | Change | Pouch in view | Pulses/s peak | Pen (mm) | Cam jerk | Aim error (m) | Settle (s) | Cleared % | Learned |
|---|---|---|---|---|---|---|---|---|---|
| 000 | Baseline: first playable (solver defaults: 6 substeps, 60 Hz contacts) | 0.00 | 26.8 | 46.0 | 14097 | 0.111 | 5.81 | 42.0 | Works end to end and looks like the genre at once. But you can't see what you're pulling (the pouch is under the screen's edge), the hand gets 27 pulses in a second, something sinks 46 mm into something, and the camera lurches between poses |
| 001 | Camera back and lower (eye 2.55/11.4 → 2.3/12.6), picked from a probe of five levers | **1.00** | 26.8 | 46.0 | 16540 | 0.111 | 5.81 | 42.0 | Probes: z 12.6 alone 0.79, z 13.4 1.00, fov 52 0.27, a shorter pouch travel 0.00. The lower eye keeps the tower big and looks up at it. Timings moved though the physics didn't: they are machine noise, verdicts now need a 30 % move |
| 002 | Camera path without jumps: the spring follows a smoothed goal; one point of interest (the stone, then where it hit) and an eased push-in amount, instead of three poses switching | 1.00 | 26.8 | 46.0 | **667** | 0.111 | 5.81 | 42.0 | −96 % jerk. The smoothing stage alone gave 6107; the chase pose was the rest (417 without it). New guard `fg_clear` (added after this step, no game change): the slingshot stays under the pedestal's foot 97.6 % of the time; by eye the push-in *looked* crowded but doesn't cover the smash, and without it only 31 % |
| 003 | Haptics mark big moments: one channel (≥ 0.1 s apart), a tumble buzzes only at ≥ 25 % of the shot's hardest hit | 1.00 | **9.2** | 46.0 | 667 | 0.111 | 5.81 | 42.0 | 129 → 21 pulses a shot. Pull ticks were never the storm (4 or 8 changed nothing); impacts and per-break taps were |
| 004 | Debris layer (solver: collision layers): shards touch only the ground and pedestal; shards are wall plates that can't overlap, 5 cm thick | 1.00 | 9.4 | **12.8** | 658 | 0.111 | **5.26** | 35.3 | Found by instrumenting (stats name the deepest pair): thin light shards crushed under heavy wood (≈100:1 mass), not overlap at spawn. Two hypotheses rejected on the way (spawn overlap: 55 mm; shards inside the stone: no change, code removed) |
| 005 | **Measure:** aim error across the hit surface (where on the target), not centre to centre a tick apart. Definition change: not comparable with 000–004 | 1.00 | 9.4 | 12.8 | 658 | **0.063** | 5.26 | 35.3 | Contact to contact first read 0.20 m: the solver's impact point is an anchor from the step's start, in the air for a speculative contact. Across the surface: centre shots 1 cm. Aimed shots 11 cm: the stone strikes a seam (wood row over glass jars at the same depth); preview and solver pick different sides, under a stone's radius. Eight rim rays instead of four changed nothing (reverted) |
| 006 | Look pass: thin glint trail only once clear of the lens, no dots in the first metre, sun from the side. **Measure** `fg_share` | 1.00 | 9.4 | 12.8 | 658 | 0.063 | 5.26 | 35.3 | Every metric identical (`--check`), as a look change must be. `fg_share` 0.22. Probes overturned a reading by eye: the push-in *lowers* it (0.39 without), the culprit is a stone resting high in the pouch. Lowering the slingshot trades it against the pouch in view (y 1.05: 0.19 / 0.78; y 0.9: 0.16 / 0.58), **a designer's call** |
| 007 | **Measure:** solver time as the 95th percentile, not the worst step (the worst moved 3.7 → 18 ms run to run with no change: the OS scheduler). SMASH becomes the app's first card | 1.00 | 9.4 | 12.8 | 658 | 0.063 | 5.26 | 35.3 | Play identical. Timings keep a ±30 % noise band |
| 008 | **Measure:** aiming precision (`precision.rs`): cm per px, linearity, reach, release roll, swim, parallax | 1.00 | 9.4 | 12.8 | 658 | 0.063 | 5.26 | 35.3 | The designer's "I can't put it where I want", in numbers: a third of the tower out of reach (64 %), the hit climbing 16.8× unevenly, sideways 4× coarser than up/down, the mark drifting 20 px after the finger stops, 3.2 cm of roll on release |
| 009 | Linear aim: the pull picks a height on the tower (evenly, low arc solved), sideways a point across it. The preview is an exact sphere (`World::sphere_cast`). Solver: a speculative contact on a fast sphere is kept only if its sweep really reaches (no ghost collisions) | 1.00 | 8.6 | 14.4 | 622 | 0.065 | 6.28 | 21.3 | Reach 64 → **100 %**, linearity 16.8 → **1.13**, sideways and up/down now equal (0.37 / 0.35 cm/px). Found on the way: the stone hit cans it flew 13 cm over (the solver made up the contact) and the preview's 5 rays missed grazes. Cleared % not comparable: the scripted pulls aim elsewhere now |
| 010 | Release lock: a release shoots the aim from 80 ms before the lift | 1.00 | 8.6 | 14.4 | 622 | 0.065 | 6.28 | 21.3 | Release roll 4.3 → **0 cm**; every other metric identical |
| 011 | Parallax round the target: the eye slides with the aim, the look stays on the tower (lean 0.035 → 0.06) | 1.00 | 8.6 | 14.4 | 635 | 0.065 | 6.28 | 21.3 | Swim 20.6 → **4.5 px**, parallax 46 → **104 px** (probes: 0.035 → 61, 0.09 → 157). Tilt parallax added after (the phone's tilt slides the eye, baseline adapts in 2 s): no gyro in the evals, so identical; a unit test checks the beach moves ≥ 4× more than the tower |
| 012 | Levels as data: 8 towers (`=`/`#` beams, centred rows), stones per level, stars, next level on a clear, retry when out of stones; HUD | 1.00 | 8.6 | 14.4 | 635 | 0.065 | 6.28 | 21.3 | The evals keep playing FORTRESS: identical. The pedestal sizes itself (minimum 1.55 m, so no tower's table changed) |

## On the phone (iPhone 14 Pro, step 007, 14 shots by hand, the app's stats line)

| | Value |
|---|---|
| Frame rate | 119.9 fps throughout (ProMotion 120 Hz), collapses included |
| Frame work | ≤ 4.4 ms of the 8.3 ms budget |
| Solver step | ≈ 25 µs with the tower asleep; ≤ 1.4 ms with 57 bodies awake |
| Haptics | Core Haptics, no failed pulses |

## Open for the designer

- **Slingshot height** (step 006): a lower slingshot covers less of the smash but its pulled pouch leaves the screen.
- **Feel by hand**: pull distance for a full shot (30 % of the screen), stone speed range (13–25 m/s), hit-stop
  (60 ms), slow motion trigger, how much the camera pushes in. The evals keep these honest; only a hand can say
  whether they feel right.
