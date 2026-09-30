# Highway Surfers: game feel eval log

How the game *looks in motion*, measured the way `EVALS.md` measures how it *plays*.
`simcraft-play games/highway_surfers --feel --replay feel-evals/NNN-run.jsonl` replays a run headless at 60 frames a
second through the same code a window uses (`sim_gpu::track::feel_probe`) and prints the numbers; raw results:
`feel-evals/NNN-*.json`. Motion is judged as drawn: positions on screen, frame by frame.

| Metric | Goal | Meaning |
|---|---|---|
| `stall` | min | of the frames a moving thing near the camera is on screen, the share in which it did not move: stop-and-go |
| `step_ratio` | min (1 is perfect) | a moving thing's largest step between frames over its average step: N = it covers N frames of road in one (teleporting) |
| `eye_jerk` | min | largest change of the eye's velocity between frames (world units a frame): camera jolts |
| `rider_jerk` | min | the same for the surfer's position across the road: snaps and hard starts |
| `tick_ms` / `frame_ms` | info | cost of a tick (simulation) and of composing a frame (no GPU); wall clock, not deterministic |
| `entities` | min | entities simulated to show a few dozen |
| `frames` | info | frames measured (a run that ends early is measured only until it ends) |

| Step | Change | stall | step_ratio | eye_jerk | rider_jerk | tick ms | entities | Learned |
|---|---|---|---|---|---|---|---|---|
| 000 | Baseline: cells and ticks (30/s), a vehicle moves a cell every 2–5 ticks | 0.683 | 3.04 | 0.049 | 0.016 | 0.93 | 1526 | The playtest's "teleporting", measured: a nearby vehicle stands still in 68% of frames and then covers three frames of road in one |
| 001 | Continuous motion (engine `motion`: fine positions and velocities integrated every tick; the renderer draws fine positions) | 0.062 | 1.27 | 0.056 | 0.016 | 0.57 | 1529 | Stop-and-go gone. Ticks got cheaper (vehicles no longer evaluate a beat every tick). The rest was the probe counting frozen frames (below) |
| 002 | 60 ticks a second (speeds halve, gravity quarters: same speeds and airtimes in seconds) | **0.000** | **1.001** | 0.051 | 0.016 | 1.13 | 1524 | Even motion. A first measurement said 0.499: the old inputs wrecked the surfer at frame 302 and the probe kept counting a stopped world. The probe now stops at the end of a run and skips hitstops (`frames` says how far it got); 000–001 were measured before that fix (their runs did not end early) |
| 003 | The simulation draws the rider: its move across is physics, not the renderer's curve; eased steering to the lane (no snaps) | 0.000 | 1.001 | 0.051 | **0.008** | 1.17 | 1527 | Drawing the simulated position first showed a jolt (0.267): landing snapped the surfer to the lane's centre while it was still a quarter lane short. Steering instead of snapping: half the jerk of the renderer's old curve, and now it is the truth |
| 004 | Traffic only around the surfer (directors), not a whole road laid out | 0.000 | 1.001 | 0.048 | 0.008 | **0.38** | **478** | A third of the entities (most of the rest are idle coins and bridges); frames 1.7 → 0.24 ms. New traffic, so a new run (`004-run.jsonl`) |
| 005 | (measured after the two-way road and themes; new map, new run) | 0.000 | 1.000 | 0.057 | 0.049 | 3.18 | 832 | **Stutter**: frames 31 ms average, 90 ms worst, all 600 over 8 ms (new metrics `worst_ms`, `p99_ms`, `over_8ms`). Cause (profiled): button lit-state asked every frame + unbounded nearest search |
| 006 | Buttons asked once a tick; bounded nearest search | 0.000 | 1.000 | 0.057 | 0.049 | **0.47** | 832 | frame 0.08 ms, worst 1.0 ms, none over 8 ms |

Runs: 000–001 replay `000-run.jsonl` (the bot at 30 ticks/s); from 002 on, `002-run.jsonl` (the bot at 60).

## Next candidates

- `eye_jerk` 0.051 is now the largest jolt: find which effect or spring makes it before tuning.
