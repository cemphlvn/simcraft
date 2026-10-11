# GUMBALL: eval log

Every step plays five whole jars (dice seeds 1 to 5) with the autopilot (`crates/sim-mobile/src/gumball/pilot.rs`:
for each of 17 places across the jar, where would the ball in the hand come to rest; the lowest wins, landing on a
ball of its own size preferred) and the same five with a player who drops at random, for scale. Both go through the
same calls a finger makes. One jar is one chaotic run: a single jar moved every number by 30 % between runs that
differed only in an unrelated setting, so a step is judged on five.
Run: `cargo test -p sim-mobile --release gumball_eval -- --nocapture`. Pictures: `cargo run --release -p sim-mobile
--bin simcraft-shot -- gumball OUT_DIR --at 3,15,28,40`.

Columns are means over the five jars, except the overlap (the worst moment of any jar). **Resting overlap**: how far
two balls that are both still (under 5 cm/s) and settled (over half a second old) sit inside each other.
**Squashed ticks**: ticks in a jar where some pair of settled balls is more than 2 cm inside each other for a moment
(a landing, a pop).

| Step | Change | Player | Drops | Merges | Score | Biggest size | Resting overlap (mm) | Squashed ticks | Learned |
|---|---|---|---|---|---|---|---|---|---|
| 000 | First playable: 11 sizes ×1.26 from 0.14 m, a 3.0 × 4.2 m jar, real sugar density, 6 substeps | autopilot | 74 | 49 | 457 | 7.8 | 29.2 | 197 | Looking before dropping beats dropping at random by 38 % in score and lasts 10 % longer; neither gets past size 9 of 11 in five jars, so a jar fills after about 70 drops. Whether that is too short is a designer's call on the phone |
| | | random | 67 | 35 | 332 | 7.6 | 25.4 | 113 | |

## Rejected on the way

- **Overlap as the solver's deepest penetration, any moment:** 313 to 392 mm. It counted the merge's new ball,
  which appears inside its neighbours on purpose. Measured instead as above, which splits what rests from what
  passes.
- **Density falling with size** (`density_falloff`, so a big ball does not crush small ones, the mass ratio that
  SMASH's step 004 found the solver handles worst): resting overlap 45 mm against 29 over the same five jars, and
  a lower score. **10 substeps** (measured with a looser rest test, under 20 cm/s): 69 mm against 61 for 6. Neither
  helps reliably; real sugar density and 6 substeps stay. The density lever stays in the data at 0.

## Still to judge on the phone

- How long a jar should last (sizes, growth, jar width), and whether a drop-cooldown of 0.45 s feels quick.
- Frame time with 30 or more shadowed spheres.
