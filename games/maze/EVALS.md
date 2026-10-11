# MAZE: eval log

Every level is played by the autopilot (`crates/sim-mobile/src/maze/pilot.rs`) through the same input the hand
uses: it plans the shortest way from S to G that never crosses a hole, keeps the corners, and tilts the board to
roll towards the next corner at a speed it can stop from (top speed 1.2 m/s, braking 1 m/s²). A level passes when it
is won in 90 s without a fall and the marble never sinks 1 cm into the wood.
Run: `cargo test -p sim-mobile --release maze_eval -- --nocapture`. Pictures: `cargo run --release -p sim-mobile
--bin simcraft-maze -- shot OUT_DIR --level N --at 0,2,4`.

| Step | Change | Won (s) per level 1–5 | Falls | Knocks per level | Hardest knock (m/s) | Pen (mm) | Learned |
|---|---|---|---|---|---|---|---|
| 000 | First playable with the solver's default rolling resistance (0.02 m) | (not measured) | – | – | – | – | A unit test caught it before the eval ran: a 0.2 tilt moved the marble 3.7 cm in half a second instead of ~17. A rolling-resistance arm of 0.02 m on a 15 cm ball is a decelerating pull of ≈1.3 m/s², as strong as a 12° slope: SMASH's value is for stones on sand |
| 001 | Rolling resistance 0.001 m (steel on hard wood), in `maze.ron` | 7.6 · 18.2 · 8.8 · 15.8 · 10.6 | 0 | 1 · 0 · 0 · 5 · 3 | 0.61 | 0.1 | Every level won, none fell. NARROWS knocks most (5): its corners are one cell wide, so the braking marble still taps the far wall. ZIGZAG is the slowest (18 s): long corridors and many turns, each braked to a stop |

Every knock above `knock_min` (0.35 m/s) is a haptic thud, stronger with speed; a marble rolling along a wall is not
felt. Holes are spots on one flat floor (no seams): the marble never caught on an edge in any run.

## Still to measure on the phone

- How the tilt feels in the hand: `max_tilt` 14°, `tilt_lag` 0.06 s, how the phone is held as level (re-centred by a
  tap). The desktop preview has no tilt sensor (a dragged finger tilts instead), so these are a judgement for the
  designer on a real iPhone.
- Frame time with the shadowed 3D board (the stats line streams it over `devicectl ... --console`).
