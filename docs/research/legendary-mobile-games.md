# Research: how the legendary mobile games are built, and what our engine should learn

2026-09-30. Question: how were the landmark mobile games (all-time, and Türkiye's top five of the last five years)
built inside? Which techniques (stacking, 2.5D, physics tiers, level pipelines) recur, and what does that say about
simcraft's architecture? Continues `mobile-games.md`.

## 1. Ten landmark games, and one stacking reference

| Game (year, studio) | Built on | The technique that made it | Evidence |
|---|---|---|---|
| **Snake** (1997, Nokia) | Phone firmware | A grid, a discrete tick, one input: the smallest possible deterministic game | general knowledge |
| **Tower Bloxx** (2005, Digital Chocolate, J2ME) | Feature phones | *Stacking by timing:* a swinging crane (a pendulum), and a tower whose sway grows with misalignment. Fake physics, fully authored | Wikipedia |
| **Angry Birds** (2009, Rovio) | Custom C++ with **Box2D** and SDL | *Rigid-body destruction:* a slingshot aimed into wood, stone and ice structures; the physics does the level's storytelling | Wikipedia, Box2D |
| **Cut the Rope** (2010, ZeptoLab) | Custom engine; **Verlet** points and constraints | Details below | port source |
| **Fruit Ninja** (2010, Halfbrick) | Custom engine | *Swipe as a blade:* the finger's trail is a line tested against objects (the same test as apelann's catch-by-swipe) | general knowledge |
| **Candy Crush Saga** (2012, King) | **Fiction Factory**, King's in-house C++ engine, editor and toolchain | *Levels at industrial scale:* 4+ new levels a day since 2012; bots play each level thousands of times before release (difficulty, shuffles); 95% fewer manual tweaks; an AI "tweak co-pilot" whose suggestions designers accept or reject *with a written reason*, and it learns from those reasons | King and GDC |
| **Clash of Clans** (2012, Supercell) | **Titan**, in-house C++ (client in C++/Objective-C, server in Java and Rust) | *Deterministic simulation:* a replay is not a video but the taps plus the game logic, re-simulated; the server re-runs battles to catch cheats | Supercell's own post |
| **Subway Surfers** (2012, SYBO) | Unity | *Lanes and a bent world:* a vertex shader curves the horizon at draw time only, with "zero impact on physics, colliders, AI, gameplay" | Curved World docs |
| **Monument Valley** (2014, ustwo) | Unity, one orthographic camera | *Impossible geometry:* "visual alignment equals physical connection". Navigation connects nodes that touch *on screen*, ordered by depth from the camera. ustwo wrote its own navigation, route-planning and locomotion, because hand-marking connections was unrealistic | Game Developer |
| **Crossy Road** (2014, Hipster Whale) | Unity; models made in Qubicle | *Voxel art:* flat, low-poly, readable at thumb size, cheap to author | Pocket Gamer |
| *Stacking reference:* **Stack** (2016, Ketchapp) | Unity | *Stacking without physics:* a tap stops a sliding slab; the overhang is **sliced off** geometrically; a perfect drop keeps the size; a streak of perfect drops plays rising piano tones | store page, reviews |

### Cut the Rope, read from source

Source: `yell0wsuit/cuttherope-dx`, a C# port of the PC version.

- **A level is a data file.** `content/maps/1_1.xml` holds a 320×480 map with a small object vocabulary: `candy`, `grab` (a rope anchor with `length`, `wheel`, `moveLength`, `spider`...), `target`, `star` with a `timeout`, plus tutorials. The port has 425 of these files.
- **Points and constraints** (`ConstrainedPoint.cs`):
  - Verlet integration (position and previous position).
  - Each point has a **weight** (`invWeight`), and a correction is split by weight, so the heavy candy barely moves while the light rope does.
  - **Three constraint types:** `DISTANCE` (a rod), **`NOT_MORE_THAN` (a real rope: it can go slack, it can't stretch)**, `NOT_LESS_THAN` (a strut).
- **The rope** (`Bungee.cs`):
  - 30 relaxation passes per step.
  - Cutting removes a point's constraints, and the candy keeps its velocity.
  - Ropes lengthen or shorten by adding or removing points.
  - The rope is drawn as a Bezier curve through the points.
- **What this teaches apelann's ropes:** his only constraint is `DISTANCE`, split 50/50 between the two points. `NOT_MORE_THAN` plus weights is the difference between a chain of sticks and a rope with a heavy load on it.

## 2. Türkiye's top five of the last five years

| Game (launch, studio) | Revenue | Built on | What's distinctive to build |
|---|---|---|---|
| **Royal Match** (2021, Dream Games) | $7B+ since launch; $1.46B in 2024 | Unity | Match-3 with very polished juice (every match and power-up animates); a deterministic board core with Unity drawing what the core decides (industry teardowns); a level builder so designers test without programmers |
| **Royal Kingdom** (2024, Dream Games) | $301M in its first year, beating Royal Match; $750M+ | Unity (same studio) | The same machine applied a second time: the first proof that a Turkish studio's hit can be repeated |
| **Match Factory!** (2022, Peak) | $188M+ a year | Not documented | "Match 3D": a **pile of 3D physics objects**; you tap to collect triples. Zynga calls its physics "market-leading". The closest top hit to Smash Fest's needs |
| **Color Block Jam** (2024, Gybe Games for Rollic) | $148M+ | Not documented | Deterministic, handcrafted levels built from a **modular set of obstacles and board shapes**; "no randomness, strategy through trial and error"; reached $300K a day |
| **Pixel Flow!** (2025, Loom Games) | $100M+ IAP; ~$550K a day at peak | Not documented | **Fully deterministic** ("no randomness bails players out"); **a picture is the level** (the art is the goal, so the content pipeline is image in, level out); dexterity under pressure (chain up to 10 units past the tray limit); ~10 people at launch, founders from Crescive Games, who made **Twisted Tangle**, a rope puzzle, with Rollic |

The engine is only documented for Dream Games (Unity). For the others I found no primary source, and I haven't guessed.

The pattern:
- Every one is **deterministic** at its core.
- Every one is **content-heavy** (hundreds to thousands of levels).
- Every one adds **one twist** to a known family.
- None of them won on engine technology. They won on the rule, the feel and the level pipeline.

## 3. Ways of building, as layers

| Layer | The options seen | Who |
|---|---|---|
| Engine | In-house C++ (Titan, Fiction Factory, ZeptoLab's, Rovio's first); Unity (Royal Match, Monument Valley, Crossy Road, Subway Surfers); Cocos, Defold, Godot (see `mobile-games.md`) | The biggest long-running live games own their engine; most hits license Unity |
| Physics tier | **Kinematic / geometric** (Stack's slice, Tower Bloxx's authored sway) → **constraints / Verlet** (Cut the Rope) → **rigid bodies** (Angry Birds with Box2D, Match Factory) | Pick the *cheapest tier that gives the feel* |
| 2.5D view | Orthographic isometric (Monument Valley); curved-world vertex bend (Subway Surfers); voxels (Crossy Road, Pixel Flow's cubes); toy-like 3D renders with a fixed camera (Smash Fest, Block Out) | A *view* choice, except Monument Valley, where the projection *is* the rule |
| Levels | A data file with a small object vocabulary (Cut the Rope XML, Color Block Jam modules); images as levels (Pixel Flow); an editor for designers (Royal Match) | Always data, never code |
| Verification | Bots at scale with designer-set criteria (King); deterministic re-simulation (Supercell) | |
| Gestures | Tap (Stack), swipe-cut (Fruit Ninja, Cut the Rope), pull and release (Angry Birds), swap-drag (Candy Crush), tap to place (Clash of Clans) | |
| Feedback | Rising pitch on a streak (Stack), animation on every action (Royal Match), haptics | |

## 4. The ontology paper (arXiv 1805.09012)

Anderson, Suarez, Xu, David (2018): *An Ontology-Based Reasoning Framework for Context-Aware Applications*. It's a 6-page workshop paper about context-aware Android apps, not games.

What it proposes:
- An OWL ontology split into terms (TBox: classes and relations) and facts (ABox: instances).
- Micro-services for sensing, classification and prediction, talking to a core over inter-process calls.
- A reasoner (Pellet, HermiT) that derives complex contexts from simple ones.

**Its measured limit:** reasoning over ontologies above ~6,000 axioms ran into memory and time problems on Nexus 4–6 phones.

Mapped onto simcraft:

| Paper | simcraft |
|---|---|
| TBox (the terms) | `game.ron`: kinds, props, states, rules, actions |
| ABox (the facts) | The world: entities and their props at a tick |
| Sensing micro-services | Input sources; on mobile these become touch, tilt (gyroscope), shake (accelerometer) |
| Derived contexts via a reasoner | `senses` and rules computed each tick |
| Reasoning cost on phones | We already avoid it: rules are **compiled ahead of time** into deterministic native code, never interpreted by a general reasoner at run time |

**The lesson we can use:** keep the ontology in the *authoring* layer (readable, checkable by `simcraft-check`) and ship it compiled. Phone sensors are just more input sources feeding the same action bindings.

## 5. What this means for the engine

1. **Deterministic core, thin renderer.** Clash of Clans, Pixel Flow and Royal Match's Unity-draws-what-the-core-decides all converge here. simcraft already is this. Keep the mobile shell *outside* `sim-core`, like the curved-world shader stays outside gameplay.
2. **A level layer.** Every hit ships hundreds to thousands of levels made from a small object vocabulary. simcraft has one layout per game. It needs `games/<name>/levels/*.ron`: a level lists objects from the game's kinds, like Cut the Rope's XML. Images as levels (Pixel Flow) is one importer on top.
3. **Evals per level, with the designer's reasons.** King's loop (bots play thousands of times, then the designer accepts or rejects with a reason) is `tools/eval.py` plus `EVALS.md`, run *per level* instead of per game. Add a per-level sweep and a pass-rate metric from scripted players (the best ~5% of bot runs predicts humans best; see `mobile-games.md`).
4. **Physics as tiers, chosen per game:**
   - Kinematic slicing needs no solver.
   - Constraints: integer Verlet with Cut the Rope's three constraint types and weights. This is small and serves apelann's rope.
   - Rigid bodies: resting stacks, as in `building-a-physics-engine.md` steps 3–5.

   Smash Fest needs the top tier. Match Factory proves that tier sells.
5. **Gestures as declared inputs.** The recurring set is tap, swipe-across (a line test against shapes), drag, and pull-and-release. Each should be one binding in `input.ron`. The swipe-across test is shared by Fruit Ninja, Cut the Rope and apelann's catch.
6. **2.5D stays a view,** with one exception to support: a *screen-space adjacency* query, if a game makes the projection a rule (Monument Valley).
7. **Feedback is data,** like sound already is. Rising pitch on streaks, haptic patterns and animation hooks all hang off bus events.

## Sources

[Supercell on replays](https://x.com/ClashofClans/status/1737801293370884157?lang=en) ·
[The technology behind Clash of Clans](https://macsources.com/the-technology-behind-clash-of-clans/) ·
[Angry Birds (Wikipedia)](https://en.wikipedia.org/wiki/Angry_Birds_(video_game)) ·
[Box2D (Wikipedia)](https://en.wikipedia.org/wiki/Box2D) ·
[Cut the Rope DX source](https://github.com/yell0wsuit/cuttherope-dx) ·
[Cut the Rope tribute (Verlet)](https://github.com/Haddley/cuttherope) ·
[Candy Crush Saga (Wikipedia)](https://en.wikipedia.org/wiki/Candy_Crush_Saga) ·
[mobilegamer.biz: King's human and AI design](https://mobilegamer.biz/how-king-balances-human-and-ai-powered-design-in-candy-crush-saga/) ·
[Neurohive: AI in King's level design](https://neurohive.io/en/ai-apps/how-ai-helped-king-studio-develop-13-755-levels-for-candy-crush-saga/) ·
[GDC: Level Design Saga](https://gdcvault.com/play/1023799/Level-Design-Saga-Creating-Levels) ·
[Curved World docs](https://amazing-assets.gitbook.io/curved-world) ·
[Making the impossible possible in Monument Valley](https://www.gamedeveloper.com/design/making-the-impossible-possible-in-i-monument-valley-i-) ·
[The making of Crossy Road](https://www.pocketgamer.biz/feature/60837/making-of-crossy-road/) ·
[Tower Bloxx (Wikipedia)](https://en.wikipedia.org/wiki/Tower_Bloxx) ·
[Stack (App Store)](https://apps.apple.com/us/app/stack/id1080487957) ·
[Royal Match (Wikipedia)](https://en.wikipedia.org/wiki/Royal_Match) ·
[Royal Kingdom passes $750M](https://www.pocketgamer.biz/royal-kingdom-surpasses-750m-with-42-of-all-revenue-made-in-h1-2026/) ·
[Zynga: Match Factory launch](https://www.zynga.com/blog/zynga-and-peak-launch-match-factory-creating-3d-puzzle-adventure-fun-on-an-industrial-scale/) ·
[Color Block Jam at $300K a day](https://www.gamigion.com/color-block-jam-by-rollic-scaled-to-300k-a-day/) ·
[Color Block Jam: evolution of the puzzle genre](https://medium.com/@elifecekusku/color-block-jam-evolution-of-puzzle-genre-e51c517d8a21) ·
[Deconstructor of Fun: Pixel Flow](https://www.deconstructoroffun.com/blog/2026/2/13/pixel-flow-the-publishers-dream) ·
[Pixel Flow hits $100M](https://mobilegamer.biz/data-digest-pixel-flow-hits-100m-mays-top-games-neverness-to-everness-pokemon-go-more/) ·
[AppMagic Türkiye 2026 report](https://mobidictum.com/appmagic-turkiye-mobile-gaming-landscape-2026/) ·
[Anderson et al., arXiv 1805.09012](https://arxiv.org/abs/1805.09012)
