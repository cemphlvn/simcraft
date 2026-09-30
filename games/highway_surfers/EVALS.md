# Highway Surfers: eval log

What "better" means is in `eval.toml`: a scripted player (`bot.py`, skill 80: it notices a danger in time 80% of the
time) plays five seeds for a minute of game time. Raw results: `evals/NNN-*.json`. How it looks in motion: `FEEL.md`.

| Step | Change | survived s | hits | landings/min | falls | coins | vehicles | Learned |
|---|---|---|---|---|---|---|---|---|
| 000 | Baseline: cells and ticks, lane-locked traffic | 60 | 0.2 | 53.4 | 0 | 27.6 | 1155 | The playtest in numbers: a good player never loses. The traffic is a fixed pattern that a program reads exactly: no risk, no fun |
| 001 | Continuous motion (fine positions, velocities; landing where the falling height crosses a roof; physics decides reach) | 60 | 0 | 48.4 | 0 | 20.2 | 1155 | Same game, now physical: a hop that does not clear a truck hits its side. `points` changed units (fine distance), not comparable from here |
| 002 | 60 ticks a second; the bot's rhythm in seconds; it decides every 2 ticks | 60 | 0.4 | 52.0 | 0 | 20.8 | 1154 | Same game at twice the rate, as intended |
| 003 | The simulation draws the rider; eased steering across | 60 | 0.6 | 51.2 | 0 | 18.0 | 1158 | Steering takes the lane's full 21 ticks: a few moves the bot made before no longer fit its checks |
| 004 | Traffic only around the surfer: directors send vehicles where there is room; 80 rows laid out | 54.4 | 1.6 | 49.4 | 0 | 25 | **84** | 14× fewer vehicles. Traffic now differs by seed, and with it risk appears: one seed wrecked the good player at 32 s |

## Open

- Nothing here yet measures risk the way the designer asked for (traffic from behind, time to react, dynamic):
  the next steps change the traffic, and add metrics for it (reaction time left, near misses).
