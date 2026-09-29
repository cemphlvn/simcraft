# Mound: game feel eval log

How moving as a termite *feels*, measured the way `EVALS.md` measures how the colony *builds*.
`simcraft-play games/mound --feel` runs a scripted player through the real controller (`sim_gpu::walker`, the same
code the window uses) at 60 frames a second, headless: start, stop, a mouse sweep, a walk, a hop, a wall climb.
The feel is data (`roam.ron` → `feel`); raw results: `feel-evals/NNN-*.json`.

| Metric | Goal | Meaning |
|---|---|---|
| `start_s` / `stop_s` | info | seconds to walking speed / to a stop: the glide of the body |
| `look_jerk` | min | change of the view's turning speed between frames (degrees): smooth turns |
| `look_lag` | info | how far the view trails the mouse at most (degrees): the glide of the look |
| `eye_jerk` | min | largest jolt of the eye between frames (voxels): lurches |
| `jump_h` / `air_s` | info | a hop: height (voxels), time in the air |
| `climb_s` | info | seconds to climb a wall three voxels high by walking into it |

| Step | Change | start | stop | look_jerk | look_lag | eye_jerk | hop | climb | Learned |
|---|---|---|---|---|---|---|---|---|---|
| 000 | Baseline: velocity eases (accel 9), the view eases after the mouse (16/s), the eye bolted to the body | 0.27 | 0.33 | 0.56 | 7.9° | 0.162 | 0.74 / 0.65 s | 2.35 | Moving and turning glide; the eye does not: take-off, landing and the bob switching off in the air jolt it |
| 001 | The eye glides after the body (`eye_glide` 18/s), the bob fades in and out | 0.27 | 0.33 | 0.56 | 7.9° | **0.042** | 0.74 / 0.65 s | 2.35 | 4× smoother eye; nothing else moved. "Camera movement gliding", measured |

## Next candidates (each measured the same way)

- A metric for how far the eye trails the body (`eye_lag`), before raising `eye_glide` further.
- Walls: the view tilts toward the wall while climbing (a crawler's view), or stays level.
- Running: a longer glide (lower `accel` at run speed) for momentum.
