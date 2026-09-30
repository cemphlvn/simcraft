# Research: a mobile template: ontology, a game-feel library, physics tiers

2026-09-30. Question: what would a template need so that a mobile game developer can bootstrap a game on simcraft,
with a ready library of mobile game-feel elements and a choice of physics types (stacking and others)? Builds on
`mobile-games.md` (industry, evals) and `legendary-mobile-games.md` (how the landmark games are built).

## 1. Describing games formally: ontologies that exist

| Work | What it is | What we take |
|---|---|---|
| **Game Ontology Project** (Zagal, Mateas et al., DiGRA 2005) | A hierarchy of concepts abstracted from many games. Top level: **interface, rules, goals, entities, entity manipulation** | A check on `game.ron`'s vocabulary: we have entities (kinds), rules, goals (`end:`) and manipulation (actions); *interface* is what the mobile layer adds (gestures, feel) |
| **Ludii ludemes** (Browne et al., Digital Ludeme Project) | Games composed from *ludemes*, units of game concept, separating **form** (rules, equipment) from **function** (emergent behaviour). Its grammar is generated from the engine's class hierarchy: a 1:1 mapping between code and language | The strongest precedent for simcraft's "a game is data composed from engine primitives". The 1:1 mapping idea fits `simcraft-check`: every word in `game.ron` is a real engine constructor, so nothing silently means nothing |
| **Boardgame mechanics ontology** (SBGames 2017) | Mechanics catalogued from BoardGameGeek | A model for cataloguing *mobile* mechanic families (tap-away, sort, slots, stack and topple...) |
| **VideOWL** (CEUR 2023) | An OWL ontology of the video game environment | – |
| **Anderson et al. 2018** (arXiv 1805.09012) | OWL context ontology plus micro-services on Android; reasoning over more than ~6,000 axioms struggled on phones | Author as an ontology, ship it compiled (see `legendary-mobile-games.md` §4) |

## 2. The game-feel library: a vocabulary that already exists

Pichlmair & Johansen, *Designing Game Feel: A Survey* (2020, 200+ sources), define game feel as "the intentional design of the affective impact of moment-to-moment interaction". It splits into three domains, each with its own polishing practice:

- **Physicality → tuning**
- **Amplification → juicing**
- **Support → streamlining**

Their catalogue, sorted by where each technique lives in simcraft:

| Group | Techniques in the survey | In simcraft today | For the mobile template |
|---|---|---|---|
| **Movement and actions** (tuning, support) | Basic movement, gravity, terminal velocity, **coyote time**, invincibility frames, corner correction, collision shapes, **button caching** (input buffering), **spring-locked modes** (the survey's example: Angry Birds' slingshot), **assisted aiming** | Movement and gravity per game; nothing named for support | Support as declared, tunable parameters: `buffer` (ms), `grace` (coyote), `aim_assist` (snap radius), `forgive` (a touch radius larger than the visible shape; thumbs are fat) |
| **Event signification** (juicing) | **Screen shake** (eased, not random), recoil, **one-shot particles**, cooldown display, ragdoll, **colour flash**, impact markers, **hit stop**, **audio feedback**, **haptic feedback** | `fx.rs` (touchdown jolt, hurt effect), the audio mixer | One `feel:` block mapping bus events to effects, the same way `sound:` maps events to buses: `on: "hit.glass" → shake(0.3), flash, particles("shards"), haptic(sharp), hitstop(40ms)` |
| **Time manipulation** | Freeze frames, slow motion, bullet time, **instant replay** | Deterministic replays (`R`, `--replay`) | Instant replay is almost free for us: re-simulate the last seconds at a slower tick. Clash of Clans' method, and the source of ad clips |
| **Persistence** | Trails, decals and debris, follow-through, fluid interfaces (Apple 2018), idle animations | Gone animations, walk bob, spring camera (`feel.rs`) | Debris that *stays* (the Smash Fest table after the shot); follow-through on UI |
| **Scene framing** | Points of interest, dynamic camera | Spring camera | A camera that frames the whole stack, then the impact |

Findings on *how much* juice:
- Too little and too much both lower player experience.
- Visual juice raised aesthetic appeal in every study, but had no effect on usability or performance.

So the library needs a master intensity setting, and an eval should be able to turn all feel off (a "dry" run) and compare.

## 3. Haptics: a vocabulary both platforms share

| | iOS (Core Haptics, AHAP files) | Android (`VibrationEffect`) |
|---|---|---|
| Unit | An **event**: *transient* (a click) or *continuous* (up to 30 s) | Predefined effects and **composition primitives** (click, tick, thud, spin, quick rise, slow rise, quick fall, low tick) |
| Parameters | **Intensity** (like volume), **sharpness** (on continuous events 0 → 1 maps to ~80–230 Hz; on transients it darkens or brightens) | Scale per primitive; amplitude on capable devices |
| Guidance | Keep haptics in sync with audio (Core Haptics can play both together) | Prefer primitives (consistent across devices); few unique effects; avoid long or jarring buzzes |

**A common core:** `haptic(kind: tap|thud|tick|rise|fall|buzz, intensity, sharpness, ms)`. It maps to AHAP on iOS and to the nearest composition primitive on Android. This is what Nice Vibrations (the Unity asset) does; we'd declare it as data.

## 4. Physics as tiers: choose the cheapest that gives the feel

| Tier | Solver | Games | Determinism cost | Stacking? |
|---|---|---|---|---|
| 0. **Kinematic / geometric** | None: intersections and slicing | Stack (the slab is cut), Tower Bloxx (authored sway), Block Out, Arrows | Free | *Precision stacking* (tap to drop, slice the overhang) |
| 1. **Constraints (Verlet)** | Points plus `DISTANCE` / `NOT_MORE_THAN` / `NOT_LESS_THAN`, weights, N relaxation passes | Cut the Rope, apelann's ropes, Twisted Tangle | Integers and a fixed pass count; only needs an integer square root | *Soft stacking* (chains, bridges, hanging loads) |
| 2. **Rigid bodies** | Sequential impulses, warm starting, slop, islands and sleeping | Angry Birds (Box2D), Match Factory (a pile), Smash Fest (stack and topple) | The hard one; `building-a-physics-engine.md` steps 3–5 | *Physical stacking* (towers that stand, then fall) |

The three are different game families, not quality levels. Stack is tier 0 and loses nothing by it. A template should let a game **declare its tier** (`physics: kinematic | constraints | bodies`), so a designer never pays for tier 2 when tier 0 is the game.

## 5. What templates offer today, and the gap

- **Unity starter kits** (Asset Store "hyper-casual starter kits", Runner Clash and similar) ship code for one genre: a level manager, UI and a runner controller.
- **Publisher SDKs** (Homa Belly, Voodoo's) ship the *business* plumbing as one SDK:
  - Ads, analytics and A/B tests.
  - Level-attempt events (`homa_level_attempts`).
  - D1, D7 and D28 retention.

None of them ship game feel as a vocabulary, a physics tier as a choice, or *evidence* (evals, bots, deterministic replays) before launch. That's the gap simcraft fits.

## 6. The template, as layers

```
games/<name>/          the designer's: game.ron, engine.toml, levels/*.ron, feel.ron, input.ron, eval.toml
  ↑ uses
sim-feel (new)         the event → effect vocabulary: shake, flash, particles, hitstop, slowmo, replay, haptic
sim-physics            tiers: kinematic helpers, Verlet constraints (new), rigid bodies (growing)
sim-core / rules       deterministic world, levels, bus events (unchanged; knows nothing of phones)
  ↓ runs inside
sim-mobile (new)       the shell: lifecycle, safe areas, embedded assets, touch → input.ron gestures,
                       haptics bridge (Core Haptics / VibrationEffect), audio session; later live-ops behind one interface
simcraft-build         --target ios | android | web (the web build doubles as a playable ad)
```

It stays true to the rules in `CLAUDE.md`:
- `sim-core` learns nothing about phones.
- Feel and haptics hang off bus events, outside the simulation, so they can't change a hash.
- Each new word (a gesture, a feel effect, a physics tier) goes into `docs/architecture.md` first, and `simcraft-check` learns to catch its misuse.

## Sources

[Game Ontology Project, DiGRA 2005](https://eis.ucsc.edu/papers/OntologyDIGRA2005.pdf) ·
[Ludii and RBG evaluation](https://arxiv.org/pdf/1907.00244) ·
[Foundations of Digital Archæoludology (ludemes)](https://arxiv.org/pdf/1905.13516) ·
[Ontology of boardgame mechanics, SBGames 2017](https://www.sbgames.org/sbgames2017/28939arw2923/ARTES_E_DESIGN/FULL_PAPERS/175272_2_versao_preliminar.pdf) ·
[VideOWL](https://ceur-ws.org/Vol-3579/paper15.pdf) ·
[Pichlmair & Johansen, Designing Game Feel: A Survey](https://arxiv.org/abs/2011.09201) ·
[Hicks et al., an empirically grounded framework for juicy design](https://dl.digra.org/index.php/dl/article/download/936/936/933) ·
[How does juicy game feedback motivate? (CHI 2024)](https://dl.acm.org/doi/fullHtml/10.1145/3613904.3642656) ·
[Impact feel in action games](https://arxiv.org/pdf/2208.06155) ·
[Apple: Introducing Core Haptics (WWDC19)](https://developer.apple.com/videos/play/wwdc2019/520/) ·
[Apple: Practice audio haptic design (WWDC21)](https://developer.apple.com/videos/play/wwdc2021/10278/) ·
[Lofelt: everything about Core Haptics](https://lofelt.com/blog/everything-you-ever-wanted-to-know-about-core-haptics) ·
[Nice Vibrations: transient and continuous](https://nice-vibrations-docs.moremountains.com/transient-continuous-haptics.html) ·
[Android haptics design principles](https://developer.android.com/develop/ui/views/haptics/haptics-principles) ·
[Android VibrationEffect.Composition](https://developer.android.com/reference/android/os/VibrationEffect.Composition) ·
[Unity Hyper-Casual Starter Kit](https://assetstore.unity.com/packages/templates/systems/hyper-casual-starter-kit-229396) ·
[Homa SDK docs](https://sdk.homagames.com/docs/tutorials/analytics/custom-events.html) ·
[Homa's LaunchOps](https://www.gameanalytics.com/blog/how-homas-launchops-team-helps-you-transform-your-prototypes-into-monster-hits)
