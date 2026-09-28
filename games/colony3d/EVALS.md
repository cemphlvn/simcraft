# Colony 3D: eval log

The colony (games/colony, 17 eval steps in 2D) rebuilt for 3D worlds and the simcraft renderer.
Same seeds, same metrics (minus the 2D-only trail count). Re-run the last step: `tools/eval.py games/colony3d --check`.

| Step | Change | Survived (mean ticks) | Deliveries | Loop closed | Learned |
|---|---|---|---|---|---|
| 000 | Rebuilt in 3D: nest underground (shaft, granary at level 3, deep chamber at level 5); temperature and scent are fields; ants feel the temperature of their voxel; winter ants climb the warmth gradient; body heat | 620 | 23.4 | 0/5 | Soil physics emerges: the deep chamber lags the seasons by ~a quarter year (60° in early winter while the air is 1°). Foraging is weak: in the viewer, scouts pin themselves in the corners (heading away from the nest) while ripe bushes go unpicked |
| 001 | **Engine:** salts by identity (as colony step 018). **Game:** the player layer (actions `lay_egg`, `rally`, `warm` on the nest; spoilage with `spoil_every = 0`, i.e. off; props `spoiled`, `rally`) | 428.4 | 15.8 | 0/5 | The player layer is neutral when unused: under identity salts the old and new `game.ron` give identical evals on every metric. The drop (620 → 428) is the reseed alone: the balance was fitted to the old dice on 5 seeds |

## Next

- **Correlated random walk** for searchers (keep a heading, turn when blocked). Tried once: the lever was not neutral
  when off, because new rules shifted the salts of later rules. Fixed at step 001 (salts by identity): it can go in now.
- More seeds (10+): step 001 showed the balance moves with the dice alone.
- Temperature-dependent evaporation of scent (pheromone is less volatile in the cold).
