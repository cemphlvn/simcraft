# Research: mobile games, Türkiye, and where simcraft fits

2026-09-30. Questions: what makes the Turkish mobile games industry unique (Istanbul, Anatolia, indies, performance
marketing); which genres and mechanics are alive; how mobile games get built; and how a `sim-mobile` template can
connect to eval-driven development so it makes the people with game intuition worth more, not replace them.

Context: the next game is a Smash Fest-like (stack and topple), inspired by apelann's Verlet ropes.

## 1. Türkiye: why it works

| Factor | What the sources say |
|---|---|
| **Scale** | Istanbul: ~757 active studios, second games hub in EMEA after London; ~24 funding rounds a year; $3.6B raised since 2009; funding $35M (2023) → $129M (2024) → $181M (2025). |
| **The "mafia" effect** | Exits (Gram 2018, Peak $1.8B 2020, Rollic 2020, all Zynga) seeded the next wave: Peak alumni staff Dream, Spyke, Bigger, Ace. Mechanics born in Istanbul spread globally (Royal Match's Lava Quest is in over half of the top casual games). |
| **Hypercasual as the training ground** | The hypercasual years taught prototype → CPI test → kill or scale as a routine. That skill moved into hybrid-casual and puzzle. |
| **Small, product-driven teams** | Loom Games: 20 people, one game (Pixel Flow, late 2025), top-20 grossing in the US, Scopely majority stake at $1B+ in Feb 2026. Pocket Gamer's Game of the Year 2026. |
| **Performance marketing as a core skill** | "Looking at numbers on the decimal point every single day." Royal Match got 61.5% of its downloads from paid channels (Candy Crush: 15–25%). Creative testing is the biggest UA lever; proven hooks get cloned within months. Now also moving to OEM stores as a third UA channel. |
| **State support** (unified 27 Feb 2026) | 50% of marketing (up to 25M TL), digital product marketing (up to 50M TL), Apple/Google commissions (up to 20M TL), cloud (5M TL), analytics (2.5M TL), staff; 70% for target countries; max 5 years. Teknokent: game sales income exempt from corporate tax; outside a teknokent, 80% of software export profit is exempt. |
| **Risks named** | Talent is the bottleneck; genre myopia (puzzle and casual only; RPG, strategy, shooters underdeveloped); hit dependency (a studio's second hit is unproven). |

### Anatolia

- **Ankara:** Panteon (ODTÜ Teknokent, 2012, two hits in two months), TaleWorlds (Mount & Blade), and the ATOM accelerator at METU Teknokent.
- **Eskişehir:** Mobge. Oddmar won an Apple Design Award in 2018, a first for Türkiye: a *craft* win, not a UA win.
- **İzmir:** Ruby Games (Ege University Technopark, TÜBİTAK-backed, Hunter Assassin; acquired by Rovio).
- **Growing:** Denizli, and university towns in general.
- **Their advantages:**
  - The tax regime is tied to the teknokent, not to Istanbul. The advantage travels.
  - They sit next to universities with game design programmes (this is where apelann's profile comes from).
  - Lower costs than Istanbul. No source measured this, so it's an inference.
  - Less poaching. Also an inference: Istanbul's talent war is named as a risk above.

### Indies

- **Arrows – Puzzle Escape** (Lessmore UG, a small German studio in Eschelbronn): one rule, no art, "thousands of handcrafted levels". 21M+ downloads, 18M in 8 months with no IP, brand or trend. Top of US Google Play Free a month after launch.
- **Takeaway:** a clean rule plus a large supply of good levels can beat art budgets. The level supply is the bottleneck, and tools can carry it (§5).

## 2. What's alive: genres and mechanic families

- The H1 2026 casual market is ~$11B IAP, with downloads down 7.2%.
- Puzzle is $4.9B (44%).
- **Hybrid-casual was the only segment to grow** (+20%, $4.2B), led by Color Block Jam, Pixel Flow, Screwdom, Magic Sort and All in Hole.

New subgenres appear as **a remix of a known mechanic plus one real twist**. Examples: Screwdom took the screw puzzle from 2D to 3D physics with camera rotation; Color Block Jam added rotating circular blocks.

Market shape decides strategy:
- **Block:** winner-take-all (the leader has 75%).
- **Screw:** concentrated (~45%).
- **Sort:** a "democratized middle class" (the leader has 18%), where fast followers and creative iterators win.

The same mechanics, read as building blocks, and what each would need from simcraft:

| Family | Examples | The core | What simcraft has | What's missing |
|---|---|---|---|---|
| **Stack and topple** | Smash Fest, Knock'em All | Rigid bodies at rest, a projectile, chain collapse | 2D box contact with sequential impulses (`sim-physics/contact.rs`) | 3D bodies, resting stacks, warm starting, sleeping, fracture (`building-a-physics-engine.md` steps 3–5) |
| **Tap-away / extraction** | Arrows, Block Out, Color Block Jam | Pieces leave along a path, in dependency order | Grid, entities, rules, `end:` conditions | Path pieces, a solver for "is it solvable" |
| **Sort** | Magic Sort | Containers with capacity, move legality | Rules and props | Little |
| **Slots and queues** | Pixel Flow (5 slots, conveyor capacity), Screwdom (5 holders) | A limited buffer that creates a "stuck" state, which is also the monetization lever | Rules, ticks | Little |
| **Screw / pin** | Screwdom | 3D physics, joints, camera rotation | – | Joints, 3D |
| **Rope / constraint** | apelann's Verlet ropes | Position constraints, tension as stored energy | – | An integer Verlet solver |
| **Idle shooter + merge** | Cat Gunner | Numbers that grow, meta layers | Rules, props | Big-number display, the meta layer |

Read plainly:
- Most current hits are **discrete and deterministic**, and simcraft's grid core already fits them.
- **Smash Fest is the family that needs the most new engine work** (resting 3D stacks). It's the right choice if the aim is to push the physics engine, not the cheapest first mobile game.

Retention differs a lot between families:
- Screwdom D30 is ~17%, Magic Sort ~8%, Color Block Jam ~4%.
- The new subgenres are still well behind the legacy leaders (Royal Match).

## 3. AI-made games: what's scarce now

- Mobile releases rose 77% between Dec 2025 and Feb 2026; new Android publishers rose 82%.
- **~97% of those games got under 1,000 downloads.** Successful games (over $20k) rose only 14%.
- Games that lean heavily on AI average 15–20% lower review scores and 2–3× the refund rate.
- 52% of developers (GDC 2026) say generative AI is harming games, up from 30%.
- Naavik names what stays scarce: **design intuition and feel, genuine new mechanics (not remixes), and taste**. "You cannot out-execute China, even with AI... it's even more important to become tastemakers." Sid Meier's rule: put a third of the effort into genuinely new mechanics.

This is simcraft's thesis in someone else's data. Making a game becomes cheap for everyone. What stays expensive, and what wins, is the judgement of the people who decide what the game should feel like. The tool's job is to make that judgement *cheaper to exercise and easier to verify*, not to replace it.

### The designer profile this serves: apelann

- Digital Game Design student. Strong in gameplay, systems and UX; by his own words "a newbie at art and level design". Working on physics, multiplayer and procedural generation.
- Recent GitHub stars: Godot (29 Sep 2026), `unity-mcp` (AI assistants driving the Unity editor), `archify`, `video-use`. He's exploring AI-assisted engine workflows himself.
- His rope repo shows the kind of intuition that's scarce: the "grab by swiping across" gesture, and the finger as a sliding ring.
- **Implication:** the template should take on exactly what he says he's weak at:
  - **Level supply:** generated and then *verified* by evals.
  - **Art:** asset packs, themes.
- It should leave the rule and the feel in his hands.

## 4. How mobile games get built

- **Engine share** among active mobile developers: Unity ~48% (about 70% of top-grossing games), Godot ~14% and the fastest-growing, Cocos Creator ~9% (Asia, WeChat mini games), Defold ~4% (King's former engine, open source since 2023).
- Studios increasingly pick an engine per project.
- **Rust (wgpu + winit) does ship to iOS and Android.** Tilarium does it: "Xcode is a one-time setup. Android is a single build script." Nobody documents the hard parts well (lifecycle, surface loss, touch, audio session, IAP, safe areas). We'd be writing that playbook.
- **The publishing pipeline** (the time budget a Turkish studio works to):
  1. Playable prototype: 5–10 days.
  2. Creative and CPI test: 3–7 days.
  3. Soft launch and retention read: 2–4 weeks.
  4. Scale or shelve.
- **The gates publishers use:**
  - CPI $0.30–0.50 (under $0.30 is strong; Magic Sort got $0.50 on Android).
  - D1 retention 35–40%+.
  - D7 retention ~10–25%.
  - D30 retention 8–12%.
  - Sessions of 8–12 minutes to start a publisher conversation, 25+ minutes for best in class.

## 5. The bridge: the publishing pipeline as evals

Each gate in the pipeline has a question simcraft can answer *before* a real player sees the game. Determinism is what makes each answer cheap and repeatable.

| Pipeline stage | The question | How simcraft answers it | Exists? |
|---|---|---|---|
| Prototype | Is the rule fun, and does it read? | `game.ron` plus `engine.toml` switches; days, not weeks | yes |
| Level supply | Is every level solvable? Does difficulty rise smoothly? | Generator plus solver per level; **bot pass rate per level** (Rovio/Aalto and King research: an agent's *best ~5% of runs* correlates best with human pass and churn rates); the eval flags spikes and dead levels | the eval runner and scripted players exist (`docs/evals.md`, `[player]`); a per-level sweep doesn't |
| The "stuck" lever | How often does a fair player hit the slot limit, and how late? | A `stuck` event and a first-stuck metric per level | rule language yes, metric no |
| Feel | Does the hit land? Is the haptic in time with the break? | `simcraft-feel` evals (arrival, overshoot); haptics declared as data, driven by bus events | feel evals yes, haptics no |
| Creative / CPI | Which hook converts? | Deterministic replays rendered as **ad videos**, and the **web build as an HTML5 playable ad**. This turns Türkiye's strongest skill (creatives and UA) into a native output of the engine | replays yes, web build and capture no |
| Soft launch | D1/D7, session length | Real players only. Later: telemetry comes back in the same metric format, so an eval can compare "the bots predicted" with "players did" | no |

A studio's gut calls stay gut calls. The evals make each one faster to test and leave a record (`EVALS.md`), so a designer's intuition compounds instead of being re-argued in every meeting.

## 6. What a `sim-mobile` template would be

One shell, every game (the rule in `architecture.md` "one game, every platform" stays):

- **Lifecycle:**
  - Suspend and resume.
  - Android surface loss.
  - Portrait lock.
  - Safe areas.
  - Assets embedded in the app instead of read from the repo.
- **Touch as input bindings:** `Tap`, `Drag`, `Swipe`, `Pinch`, `Hold` in `input.ron` (today: keys, mouse, d-pads). Smash Fest needs drag-to-aim and release; apelann's rope needs a swipe *across* a line.
- **Haptics as data:** a `haptics:` block mapping bus events to patterns (`hit.glass → sharp, 0.6`), the same way sound maps events to buses. iOS Core Haptics or `UIImpactFeedbackGenerator`; Android `VibrationEffect`.
- **Audio:** the existing mixer and voice lines, plus the iOS audio-session category (respect the silent switch).
- **Build:** `simcraft-build --target ios|android` produces a thin Xcode project or Gradle script around the static library. The web target from the same code doubles as the playable-ad output.
- **Later:** live-ops bridges (ads, IAP, analytics, remote config) behind one small interface, so a game never calls an SDK directly.

The option still open: a Rust-native shell (above) or the Unity adapter as the mobile shell (discussed 2026-09-30). The research doesn't settle it. Rust ships but is under-documented; Unity carries the live-ops SDKs for free.

## 7. Two papers that bear on this

**Difficulty before players exist** (Kristensen & Burelli, arXiv 2401.17436, Lily's Garden by Tactile Games):
- **The case:** predicting attempts per level for a player or a cohort.
- **The hard part is the cold start**, meaning new levels with no player data:
  - Player and level features alone don't beat a constant average.
  - Only an agent's features (from one PPO agent trained *per level*, 8 random seeds, up to 100 moves) do. They matter 5–6× more than level features; the most important are minimum game length and completion rate.
  - Even then, predictions **drift back to the mean**: hard levels are underestimated and easy ones overestimated.
  - Attempts are long-tailed (hard levels take 30+), because luck (the seed) and skill are tangled together.
  - Pass-rate error: 8.1% MAE (King reports 4.0–6.6%).
  - What no model answers: whether a level is fun and its solution clear.

**Where simcraft is a real answer, and where it isn't:**
- **Seeds are cheap and controlled.** A deterministic, headless game can run a level on thousands of seeds, not 8. That gives the *distribution* of attempts, not just a mean.
- **Luck and skill can be separated,** because the seed is fixed and the player's skill varies: the same level × N seeds × a ladder of scripted players (`[player]` with a skill argument). The result is a pass-rate-by-skill *curve* per level, a richer feature than one agent's rate (King: the best ~5% of runs predicts best). It's also cheaper than training an RL agent per level.
- **Engine events become features for free:** stuck, shuffle, dead end, wasted moves, counted by the eval.
- **Human attempts use the same format:** replays and bus logs come from real players too, so "cohort statistics + simulated data" (the paper's best model) is one schema.
- **Not solved:** the gap between agents and humans still needs calibrating against real players; fun and clarity stay the designer's call.

**Behaviour across sources on the device** (Yang et al., arXiv 2609.01057, OPPO Research and the Chinese Academy of Sciences; kept in mind, not built):
- **What it does:** pre-trains user embeddings from device-level logs, predicting first *which source* the next behaviour comes from, then the action within it. Sources are system apps with fine detail (App Store, Game Center, browser) and third-party apps with coarse detail (install, launch, uninstall).
- **Evidence:** 1.9M users, 83K apps, online A/B tests on game recommendation.
- **Why it matters here:**
  - It is the OEM channel's view of the funnel (ad → store → search → install → launch). OEM stores are where Türkiye's marketers are moving (§1).
  - A game built on simcraft already logs every act on a bus. If the telemetry keeps **source** and **granularity** as fields, the same funnel continues inside the game (install → level attempts → stuck → purchase), in a shape these models consume.
  - Replays give creatives, and scripted players give behaviour traces. That is where the engine could meet the country's performance-marketing talent later.

## Sources

[Pathfounders: how Turkey's sector got so big](https://pathfounders.com/p/analysis-how-the-hell-did-turkey-s-mobile-gaming) ·
[Deconstructor of Fun: how Türkiye became the capital of mobile gaming](https://www.deconstructoroffun.com/blog/2025/3/10/how-trkiye-became-the-new-capital-of-mobile-gaming) ·
[Pocket Gamer: Top 30 Türkiye game makers 2026](https://www.pocketgamer.biz/the-top-30-turkiye-game-makers-of-2026/) ·
[Tech.eu: Grand Games $70M Series B](https://tech.eu/2026/05/11/grand-games-raises-70m-series-b-to-scale-hybrid-casual-mobile-games/) ·
[Game Developer: Scopely acquires Loom Games](https://www.gamedeveloper.com/business/scopely-acquires-majority-stake-in-pixel-flow-developer-loom-games) ·
[Pocket Gamer: Loom's rise](https://www.pocketgamer.biz/loom-games-rapid-rise-to-trkiyes-next-unicorn/) ·
[Pixel Flow Game of the Year 2026](https://www.pocketgamer.biz/pixel-flow-wins-game-of-the-year-at-the-pocket-gamer-mobile-games-awards-2026/) ·
[Webrazzi: Panteon](https://webrazzi.com/2017/08/13/panteon-teknasyon/) ·
[Mobidictum: Panteon's two hits](https://mobidictum.com/2-hit-games-in-2-months-from-panteon/) ·
[Daily Sabah: Mobge Apple Design Award](https://www.dailysabah.com/technology/2018/06/08/eskisehir-based-game-maker-mobge-takes-home-apple-design-award-2018) ·
[Rovio acquires Ruby Games](https://www.rovio.com/articles/rovio-entertainment-acquires-hyper-casual-game-studio-ruby-games/) ·
[Gamigion: Turkish incentives 2026](https://www.gamigion.com/incentives-in-turkey-new-rules-now-for-the-gaming-studios/) ·
[Teknokent tax for game studios](https://www.ifasturk.com.tr/teknokent-oyun-yazilimi-vergi-avantajlari-rehberi) ·
[Mobidictum: Türkiye's marketers pivot to OEMs](https://mobidictum.com/2026-turkiyes-mobile-marketers-are-pivoting-to-oems/) ·
[Capermint: Arrows numbers](https://www.capermint.com/blog/develop-game-like-arrows-puzzle-escape/) ·
[AppMagic: Casual Games Report H1 2026](https://appmagic.rocks/research/casual-report-H12026/?hl=en) ·
[Naavik: niche puzzle subgenres](https://naavik.co/digest/how-niche-subgenres-are-reshaping-the-mobile-puzzle-market/) ·
[Naavik: is the era of mobile AI slop here?](https://naavik.co/digest/is-the-era-of-mobile-ai-slop-games-here/) ·
[GDC 2026 on generative AI](https://www.gianty.com/gdc-2026-report-about-generative-ai/) ·
[Predicting game difficulty and engagement using AI players](https://arxiv.org/abs/2107.12061) ·
[Completion rate in mobile puzzle games with RL](https://arxiv.org/abs/2306.14626) ·
[App Radar: mobile engines 2026](https://appradar.com/blog/mobile-game-engines-development-platforms) ·
[VoxBooster: engine statistics 2026](https://voxbooster.com/blog/game-engine-statistics-2026/) ·
[Tilarium tech stack (Rust, winit, wgpu)](https://natehardt.itch.io/tilarium/devlog/1596545/tilarium-tech-stack-rust-winit-wgpu-egui) ·
[Hybrid-casual KPIs 2026](https://gamegrowthadvisor.com/blog/2026-04-16-hybrid-casual-game-design-strategy-2026/) ·
[Supersonic: prototype KPIs](https://supersonic.com/learn/blog/the-3-most-important-kpis-for-testing-your-hyper-casual-prototype)
