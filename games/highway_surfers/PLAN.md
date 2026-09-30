# Highway Surfers: the .ron plan

How the [GDD](GDD.md) becomes `game.ron` + `engine.toml` + `track.ron` (+ `input.ron`), what the engine already
does, and the walls we expect to hit. Every wall that turns out real goes into `docs/emergence.md` (symptom → need →
refactor → evidence) when it is fixed. Draft 0, 2026-09-29.

## Files

| File | Role | Holds |
|---|---|---|
| `game.ron` | designer | kinds, the surfer's state machine, riding / hop / land / hit rules, actions, score, end |
| `engine.toml` | operator | seed, `tick_rate`, run length, `controllable = ["surfer"]`, tuned params |
| `track.ron` | feel | road (4 lanes), chase camera, vehicle sprites and roof heights, `switch` (the hop), `fx`, HUD |
| `input.ron` | controls | swipes (mobile) and keys (desktop) → the same actions |

## World model (the grid)

- `x` = lane 0..3 (0 = fast lane, left). `y` = along the road. 1 cell = one car length (~4.5 m).
- **Speeds are periods on one shared clock**: a vehicle moves one cell every `period` ticks, on ticks where
  `tick % period == 0`. The surfer reads the period of the roof under it and moves on the same ticks, so rider
  and roof never drift apart. (`pace()` has a per-entity phase, so it cannot be used for this.)
- `tick_rate = 30`. Periods: fast lane 2 (15 cells/s ≈ 240 km/h, arcade), then 3, 4, 5; on foot 8.
- Nothing is `solid`: the surfer shares a cell with its roof. What a shared cell *means* is decided by `level`.
- The surfer has a `level` prop: 0 = asphalt, 1..3 = standing on a roof that high, and the airborne states.

## `game.ron` sketch

```ron
Game(
    name: "highway_surfers",
    perception: Senses,
    kinds: {
        "surfer": (glyph: 'S', fsm: "surfer",
                   props: { "lane_to": -1, "level": 1, "from_level": 1, "timer": 0, "lives": 3,
                            "coins": 0, "combo": 0, "dist": 0, "jumped": 0, "over": 0 },
                   // What it feels under its feet and around it, once per tick.
                   senses: {
                       "t": "tick",
                       "roof": r#"if around("truck", 0) > 0 { 3 } else if around("van", 0) > 0 { 2 }
                                  else if around("car", 0) > 0 { 1 } else { 0 }"#,
                       "roof_period": r#"if around("truck", 0) > 0 { nearest_prop("truck", "period", 0, 8) } ..."#,
                       "clearance": r#"nearest_prop("bridge", "clearance", 0, 99)"#,
                   }),
        "car":    (glyph: 'c', props: { "period": 3 }),   // roof height 1
        "van":    (glyph: 'v', props: { "period": 4 }),   // roof height 2
        "truck":  (glyph: 'T', props: { "period": 5 }),   // roof height 3
        "bridge": (glyph: '=', props: { "clearance": 3 }),// hits anyone at level >= clearance (not ducking)
        "coin":   (glyph: '$'),
        "finish": (glyph: '*', hidden: true),
    },
    params: {
        "lanes": 4, "hop_ticks": 9, "jump_ticks": 15, "duck_ticks": 12, "wobble_ticks": 20,
        "reach": 1,              // a hop climbs at most this many roof levels; a jump climbs any
        "foot_period": 8, "ground_grace": 60, "coin_score": 10, "combo_bonus": 5,
    },
    fsms: { "surfer": (initial: "Riding", states: {
        "Riding":  (),           // on a roof: carried by it
        "Hopping": (),           // one lane over, low arc (swipe left/right)
        "Jumping": (),           // high arc, any roof height (swipe up)
        "Ducking": (),           // under bridges (swipe down)
        "OnFoot":  (),           // on the asphalt: slow; traffic from behind hits you
        "Wobbly":  (),           // after a hit: invulnerable for a moment, blinks
    }, transitions: [
        // Landing: whatever is under you when the arc ends decides where you are.
        (from: "Hopping", to: "Riding", when: "me.timer == 0 && sense.roof > 0 && sense.roof <= me.from_level + p.reach",
         then: [ Set("level", "sense.roof"), Emit("landed") ]),
        (from: "Hopping", to: "OnFoot", when: "me.timer == 0 && sense.roof > me.from_level + p.reach",
         then: [ Set("level", "0"), Emit("bonked") ]),        // hopped into a side too tall to reach
        (from: "Hopping", to: "OnFoot", when: "me.timer == 0 && sense.roof == 0",
         then: [ Set("level", "0"), Emit("fell") ]),
        // Jumping: the same three, without the reach limit. Ducking → Riding when the timer ends.
    ]) },
    rules: [
        // Traffic: one cell forward on its own beat, the same beat the rider reads.
        (name: "drive_car", for: "car", when: "tick_on(me.period)", then: [ Move("0", "1") ]),   // see wall W2
        // The surfer: carried forward by its roof (or on foot), and across towards lane_to, in ONE move.
        (name: "ride", for: "surfer", when: "me.over == 0",
         then: [ MoveBy("toward(me.x, me.lane_to)", "if sense.t % period_here == 0 { 1 } else { 0 }") ]),
        // Losing the roof: it drove away from under you (or you from it).
        (name: "roof_gone", for: "surfer", state: "Riding", when: "sense.roof == 0", then: [ Goto("OnFoot"), ... ]),
        // Hits: a vehicle drives into you on foot; a bridge lower than your level.
        (name: "run_over", for: "surfer", state: "OnFoot", when: "sense.roof > 0", then: [ /* life, Wobbly, crashed */ ]),
        (name: "bridge",   for: "surfer", state: "Riding", when: "me.level >= sense.clearance", then: [ /* … */ ]),
        (name: "coin", for: "surfer", target: Nearest("coin"), when: "it.dist == 0", then: [ Despawn(Nearest("coin")), ... ]),
        (name: "wreck", for: "surfer", when: "me.lives <= 0", then: [ Emit("wrecked"), Despawn(Me) ]),
    ],
    actions: [
        (name: "hop",  for: "surfer", args: ["dir"], when: r#"in_state("Riding") || in_state("OnFoot")"#,
         then: [ Set("lane_to", "me.x + arg.dir"), Set("from_level", "me.level"), Set("timer", "p.hop_ticks"),
                 Goto("Hopping"), Emit("hopped") ]),
        (name: "jump", for: "surfer", when: ..., then: [ ..., Goto("Jumping"), Emit("jumped") ]),
        (name: "duck", for: "surfer", when: r#"in_state("Riding")"#, then: [ ..., Goto("Ducking"), Emit("ducked") ]),
    ],
    score: "me.dist + me.coins * p.coin_score + me.combo * p.combo_bonus",
    end: [ (when: "count.surfer == 0", result: "wrecked"), (when: "count.finish == 0", result: "finished") ],
    layout: ( legend: { 'S': "surfer", 'c': "car", 'v': "van", 'T': "truck", '=': "bridge", '$': "coin" },
              rows: [ /* authored highway, ~600 rows for the first playable; the surfer starts on a car */ ] ),
)
```

## `track.ron` sketch (the feel lives here)

```ron
Track(
    assets: ["highway"], follow: "surfer", sky: "sunset_sky", skyline: (image: "city", height: 0.22, drift: 0.3),
    road: (image: "asphalt", lanes: 4, repeat: 2.5, shoulder: "barrier", shoulder_width: 6.0, paint: (240, 236, 222)),
    // Portrait chase camera: behind and above, the surfer in the lower third.
    camera: (height: 2.2, back: 3.4, fov: 64.0, pitch: 14.0, rumble: 0.004, bank: 0.7, max_bank: 8.0,
             lane: (stiffness: 90.0, damping: 1.0)),
    views: [ (name: "low", height: 1.2, back: 2.2, fov: 72.0, pitch: 6.0, body: ("surfer_back", 0.9)) ],
    kinds: {
        "car":   (frames: ["car_rear"],   height: 0.9),
        "van":   (frames: ["van_rear"],   height: 1.5),
        "truck": (frames: ["truck_rear"], height: 2.4),
        "bridge":(frames: ["gantry"],     height: 3.2),
        "coin":  (frames: ["coin"], height: 0.4, lift: 0.3, spin: 0.8, bob: 0.06, push: 1.8, gone: (...)),
    },
    switch: ( prop: "lane_to", positions: [-1.5, -0.5, 0.5, 1.5], base: 0.10, per_lane: 0.10, ease: back_out,
              hop: 0.35,                          // a real arc now, not a nudge
              start: (tracks: { "fov": [...], "roll": [...] }), arrive: (tracks: { "pitch": [...], "shake": [...] }) ),
    fx: { "landed": (...), "bonked": (...), "fell": (...), "ducked": (...), "crashed": (...), "near_miss": (...) },
    meters: [ (prop: "lives", icon: "heart", max: 3, at: (0.05, 0.04), size: 0.06) ],
    blink: "Wobbly",
)
```

## `input.ron` sketch

```ron
Input(
    scheme: "touch",
    schemes: {
        "touch": { "hop_left": Swipe(left), "hop_right": Swipe(right), "jump": Swipe(up), "duck": Swipe(down) },
        "keys":  { "hop_left": Key("a"), "hop_right": Key("d"), "jump": Key("w"), "duck": Key("s") },
    },
    contexts: [ (name: "ride", actions: {
        "hop_left":  Game(do: "hop", args: { "dir": -1 }), "hop_right": Game(do: "hop", args: { "dir": 1 }),
        "jump": Game(do: "jump"), "duck": Game(do: "duck"),
    }) ],
)
```

## Expected walls (to confirm, then record in `emergence.md`)

| # | Symptom (what the game will say) | Need | Likely change | Blocks |
|---|---|---|---|---|
| W1 | "Trucks and buses are longer than one car" | multi-cell vehicles: a roof that spans cells | a kind `length` / footprint: the entity covers `length` cells behind its head (queries, `around`, rendering) | trucks as long platforms; v0 uses 1-cell trucks |
| W2 | "The rider and its roof must move on the same tick" | a shared clock in rules; `tick` is sense-only and `pace` has a per-entity phase | `tick_on(n)` (or `pace` with a shared phase), or vehicles read `sense.t` | riding; can prototype with senses on every vehicle |
| W3 | "Traffic moves, and it steps cell by cell on screen" | the track draws moving entities between ticks, not only the followed one | interpolate every entity's position in `track` compose (prev → current cell) | anything moving looks smooth |
| W4 | "The camera must stand on the roof I am on" | camera height and the body's height from a prop (`level`), on a spring, with arcs for hop / jump | `camera.stand: (prop: "level", heights: [...])`; `switch.hop` as a real arc | the whole height game |
| W5 | "Swipe to hop" | touch gestures as bindings | `Swipe(dir)` in `input.ron`; winit touch events → gestures (threshold, time), testable with `--press swipe_left@F` | mobile |
| W6 | "Portrait" | a 9:16 layout: HUD, FOV and framing per aspect | HUD anchors per aspect; FOV as vertical or fitted | mobile |
| W7 | "Run it on my iPhone" | `simcraft-play` on iOS: winit iOS lifecycle, Metal surface, bundled assets, Xcode project | an iOS target for `sim-gpu` (spike first: a blank wgpu window on the device) | shipping |
| W8 | "An endless highway" | traffic spawned ahead of and behind the surfer forever | spawner entities riding with the surfer (maybe `Spawn` at an offset) | endless mode; v0 is an authored highway |
| W9 | "Vehicles as 3D shapes" | billboards of a truck's rear look flat when you hop onto its side | the `model` path (glb, used by `roam`) in the track pass | look, later |

**Status (2026-09-29, step 1–2 done in greybox):** W2 needed no engine change (a sense reads `tick`; the rider
moves on its roof's beat, H.1 in `emergence.md`). W3 was not a wall: the track already draws every entity between
ticks. W4 is done as the track's `rider` and chase camera (H.3, H.4). W6 is partly done (`screen`, framing). Open:
W1, W5, W7, W8, W9.

W2 and W3 come first: without them nothing can be ridden or looked at. W1 and W4 make it Highway Surfers rather
than a lanes variant. W5–W7 are the mobile stretch; W7 is spiked early in parallel because it is the big unknown.

## Steps

1. **Core ride (desktop, v0).** 1-cell vehicles, shared-clock riding (W2), hop / jump / duck / fall / on-foot,
   authored ~600-row highway. Check: `simcraft-check`; a scenario in `test/scenarios/highway_surfers.ron`
   (a ride carries you; a hop lands; a car→truck hop bonks; a jump makes it; a bridge hits a level-3 rider).
2. **See it.** `track.ron` with sprite traffic; interpolation (W3); camera on the roof (W4). Record runs.
3. **Feel pass.** `FEEL.md` + `feel-evals` as in `mound` / `colony3d`: measure camera jerk and lag, landing dips,
   input-to-motion latency on recorded hops; tune `switch` and `fx`.
4. **Long roofs (W1)** and buses; scoring by lane and combo; `eval.toml` for difficulty (runs survived per seed).
5. **Mobile.** Swipes (W5), portrait (W6), iOS build (W7; spike starts at step 1).
6. **Later.** Endless highway (W8), 3D vehicles (W9), lane-changing traffic, power-ups, sound.
