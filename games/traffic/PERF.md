# Traffic: performance log

How the engine scales with moving things that ask about each other. `tools/perf.py games/traffic --kind car
--counts 100,400,1600 --ticks 300` runs the same seed at growing car counts and times 300 ticks after a warm-up of
60 (milliseconds a tick, wall clock on all cores, and CPU milliseconds a tick, every core added up). The world's hash
at the end must not change between steps: an optimisation that changes a single tick is a bug. Raw: `perf/NNN-*.json`.
Guided by `docs/research/physics-engines.md` §6 (the order in which physics engines' techniques pay off here).

| Step | Change | 100 cars | 400 | 1600 | 1600 CPU | World | Learned |
|---|---|---|---|---|---|---|---|
| 000 | Baseline: every footprint question (`ahead`, `behind`, `touching`, `under`) scans every car | 1.47 ms | 12.6 | 257 | 1851 | - | Quadratic: 4× the cars costs 20× the time. At 1600 cars a tick is 15× a 60 Hz budget |
| 001 | Broadphase (§6.1): sweep and prune per column, rebuilt once a tick after motion | 0.75 | 2.90 | 47.0 | 358 | same | 5.5× at 1600, but still 16× per 4× cars: something else is quadratic |
| 002 | The column walk stops when nothing further out can be nearer (a shrinking fixed-radius search), columns under the question first; positions kept in the index (no string lookups per candidate, a first step of §6.4) | 0.52 | 1.52 | **6.44** | 40.6 | same | 40× faster than the baseline at 1600; now about 4.2× per 4× cars (near linear). A profile had shown columns with no overlapping car walked to their end, and `memcmp` from `props.get("px")` |
| 003 | Compiled rules (§6.3; closure compilation, Feeley and Lapalme 1987): the common subset of rule expressions (arithmetic, comparisons, `if`, `let`, `me/p/sense/it`, engine functions, script effect lists) is parsed once and evaluated natively; the rest runs in Rhai | 0.45 | 1.07 | **4.97** | 18.3 | same | CPU per tick 40.6 → 18.3 ms (2.2× less work) but wall only 1.3×: the work moved. Profile now: parallel rule evaluation 56% (Rhai scopes still built even when every expression of a kind is compiled), the world's hash every tick 13%, `integrate_motion` 11%, `apply` 7% |
| 004 | Rebaseline: the game got a player car (a new prop, so a new world); 003's engine measured on it (`SIMCRAFT_NO_BARE=1`) | 0.47 | 1.31 | 5.36 | 19.2 | new game | Steps compare the same game: a change to the game is a new baseline, never a speed-up |
| 005 | Bare kinds: a kind whose every expression is compiled builds no interpreter scope (`me`, `sense`, `it` maps); a fallback dresses the scope for that one expression | 0.47 | 1.06 | **3.44** | 14.7 | same | 1.6× at 1600; 75× since the baseline. In colony a kind became bare too: 0.2 fewer maps a tick, same world |
| 006 | Hash on demand: the engine can skip hashing every tick (`Engine::hash_every_tick(false)`); the agent protocol hashes once per step, a window never (bus subscribers still get every tick's hash) | 0.29 | 0.71 | **2.72** | 13.9 | same | 1.3× at 1600, 1.6× at 100; 94× since the baseline. The hash function itself cannot change (the golden hashes), so the fix is not hashing what nobody reads |

Correctness: `the_broadphase_answers_exactly_what_a_scan_answers` (40 random roads, both directions, sideways
shifts, riders): every question gets the same answer with and without the index; the hashes above agree.
Compiled rules: `SIMCRAFT_NATIVE_CHECK=1` asks the interpreter too for every compiled answer and stops on a difference
(it found a slot bug in nested `let`s on its first run: 0 differences since, in all 12 games); `fast_paths_change_nothing`
runs every game with and without the fast paths for 400 ticks and compares the hash of every tick.

## Next candidates (from the profile at step 003)

- `integrate_motion` and the index read motion props by name: pack them (§6.4), then integrate in parallel (§6.5).
