# Colony 3D: game feel eval log

How the game *looks in motion*, measured the way games/colony3d/EVALS.md measures how it *plays*.
`simcraft-feel games/colony3d --view games/colony3d/views/generated.ron` plays 40 s at 60 fps and 10 ticks/s on
seeds 1–3, following one ant with the viewer's own code, headless. Raw results: `feel-evals/NNN-*.json`.

| Metric | Goal | Meaning |
|---|---|---|
| `cam_jump` | min | largest camera move in one frame (px): lurches |
| `cam_jerk` | min | change of camera velocity per frame: smoothness |
| `cam_lag` | min | how far the camera trails its goal (px): responsiveness |
| `sprite_step_p95` | min | a walking sprite's step per frame (px): hop vs glide |
| `stutter` | min | walking, but frozen on screen (share of frames) |
| `frame_ms` | info | cost of building and drawing a frame |

| Step | Change | cam_jump | cam_jerk | cam_lag | sprite step | stutter | Learned |
|---|---|---|---|---|---|---|---|
| 000 | Baseline: what the player saw (no interpolation, the camera snaps) | 17.7 | 0.50 | 0 | 8 | 84 % | "It feels strange": ants freeze for 5 frames, then hop a whole cell; the camera lurches with them |
| 001 | Interpolate between ticks | 13.7 | 0.18 | 0 | 2 | 14 % | Ants glide. The camera still lurches when the followed ant changes |
| 002 | Spring camera (stiffness 40, critically damped) | 1.7 | 0.15 | 2.1 | 2 | 14 % | Probed 10–160: 10 is softest but trails 3.5 px, 160 steps 2 px a frame; 40 chosen for a softer follow (80 is a fine, more responsive alternative) |
| 003 | Metric: stutter counts only motion the side view shows (x, levels) | 1.7 | 0.15 | 2.1 | 2 | 6.5 % | Half the "stutter" was ants walking in depth, which a side view cannot show. The rest is one still frame when an ant sets off (interpolation starts at where it stood). **Not comparable with 000–002** |
| 004 | Walk bob (1 px) | 1.7 | 0.15 | 2.1 | 2 | 3.8 % | The bob hides the set-off frame and gives walking a rhythm; 2 px measured the same but is too much for a 4 px ant |

## Next candidates (each one measured the same way)

- Camera look-ahead in the walking direction (lag vs. seeing where the ant goes).
- Ease the camera when the followed ant changes (Tab), instead of the same spring.
- Squash and settle when an ant picks up or drops a berry (a feel event, not a rule).
- Frame rate on the real display: the terminal path re-sends a whole picture per frame; the GPU window removes that.
