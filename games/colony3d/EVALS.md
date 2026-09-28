# Colony 3D: eval log

The colony (games/colony, 17 eval steps in 2D) rebuilt for 3D worlds and the simcraft renderer.
Same seeds, same metrics (minus the 2D-only trail count). Re-run the last step: `tools/eval.py games/colony3d --check`.

| Step | Change | Survived (mean ticks) | Deliveries | Loop closed | Learned |
|---|---|---|---|---|---|
| 000 | Rebuilt in 3D: nest underground (shaft, granary at level 3, deep chamber at level 5); temperature and scent are fields; ants feel the temperature of their voxel; winter ants climb the warmth gradient; body heat | 620 | 23.4 | 0/5 | Soil physics emerges: the deep chamber lags the seasons by ~a quarter year (60° in early winter while the air is 1°). Foraging is weak: in the viewer, scouts pin themselves in the corners (heading away from the nest) while ripe bushes go unpicked |

## Next

- **Correlated random walk** for searchers (keep a heading, turn when blocked). Tried once: the lever is not neutral
  when off, because new rules shift the salts of later rules; it needs to go in as a measured step of its own.
- Temperature-dependent evaporation of scent (pheromone is less volatile in the cold).
