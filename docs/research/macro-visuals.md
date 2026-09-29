# Research: how Empire of the Ants pushed its visuals, and what our engine can take

Asked (2026-09-29) after `animation.md`: how did Empire of the Ants (Tower Five / Microids, Unreal 5, 2024) push its
visuals, and which of those paradigms could our engine support natively? Sources: the studio and publisher, a tech
Q&A, reviews, and the five official Steam screenshots (looked at, not only read about).

## What they did (sourced)

- **A documentary look:** "something akin to the BBC documentaries on wildlife"; a lead artist: "the techniques I used
  in advertising and cinema are now in real-time"
  ([tech Q&A](https://wccftech.com/empire-of-the-ants-tech-qa-pushing-visuals-with-ue5-in-an-rts-game/),
  [Microids](https://www.microids.com/empire-of-the-ants-how-tower-five-studio-created-photorealistic-graphics/)).
- **Reference from the real place:** a week in the Fontainebleau forest photographing; roots, stumps and a landmark
  rock 3D-scanned; insect specimens set in resin to study "from every angle"; a large photo library
  ([PlayStation Blog](https://blog.playstation.com/2024/07/24/empire-of-the-ants-marches-to-ps5-november-7-details-on-crafting-a-realistic-microscopic-world/)).
- **Detail:** the hero ant 103,683rd has over 50,000 triangles. Nanite: "getting rid of most LODs and not having to
  handle mesh transitions". Texture streaming and virtual texturing, "empowered by NVMe drives".
- **Light:** Lumen (dynamic global illumination), lighting kept consistent "regardless of the time of day or weather";
  reflections on water emphasised.
- **Performance:** 30 fps on PS5, a single 60 fps mode on PS5 Pro, DLSS 3 frame generation on PC.
- **Play:** a third-person RTS; a "small legion" of workers, gunners, warriors and support units, a combat triangle;
  macro photography where "pebbles are boulders and a beetle is an elephant"; reviewers found the camera the weak
  point ([PC Gamer](https://www.pcgamer.com/games/rts/empire-of-the-ants-review/),
  [TheSixthAxis](https://www.thesixthaxis.com/2024/11/04/empire-of-the-ants-review/),
  [Game8](https://game8.co/articles/reviews/empire-of-the-ants-review)).
- **Not published:** army sizes, how the armies are animated or instanced, per-unit costs.

## What the screenshots show ([Steam](https://store.steampowered.com/app/2287330/Empire_of_the_Ants/))

1. **Scale told by familiar things.** A worn soccer ball, pine cones, a fallen twig, a daisy: the ant is 2–3% of the
   frame; human-sized objects make it small without any number.
2. **Light is most of the look.** Low sun, long soft shadows, shafts of light through ferns (volumetric fog), haze
   that pales the distance (aerial perspective), golden-hour warmth; an autumn palette in another scene.
3. **A camera lens.** Foreground grass out of focus, background soft: depth of field like a macro lens.
4. **Clutter everywhere.** Pine needles, dead leaves, moss, grains of sand, small stones: dense scatter of small
   props on every surface, never a bare plane.
5. **Water.** Still pools reflecting sky and trees, clear enough to show the bed.
6. **Armies as groups.** Dozens of beetles and many small ants on screen; each group is a hovering round badge;
   the minimap is a graph of chambers; abilities are pheromones ("Dash pheromones: +60% move speed for 20 s").

## Opportunities: what our engine can do natively that theirs had to author

| Their paradigm | Ours, native | What to build |
|---|---|---|
| Pheromones are buttons (a buff with a timer) | Pheromones are physics: fields that spread and fade; stigmergy builds the mound | Show them as they are: the smell as volumetric haze lit by the sun (it already lives per voxel), trails you can see forming |
| A legion follows orders | A colony follows traces: you lead by leaving marks (your mud already recruits builders) | A player "command" is a mark in a field; the eval shows whether the colony took it |
| Lumen: dynamic GI, consistent at any hour | The world *is* voxels: light, sky visibility and AO can be computed per voxel (a field) and change only where the world changes | A light field (sky visibility and sun shadow per voxel, updated where terrain changed), sampled by the renderer; seasons and time of day already exist as environments (colony3d) |
| Nanite + authored LODs | The terrain is generated from the world; LOD is ours to decide | Instanced meshes with distance LOD (the crawler plan), greedy voxel meshing |
| Photogrammetry and scans | Assets from Higgsfield (images, `generate_3d` → GLB) | glTF import (the animation plan), macro-photographed textures: laterite mud, grass stalks |
| Hand-placed clutter | Everything is seeded and deterministic | Scatter of small props (needles, grains, leaves) by seed, instanced; a familiar giant (a bottle cap, a ball) as a landmark |
| Cinematic camera, called its weak point | We know, deterministically, where the story happens (events, hashes, replays) | An auto-director: shots chosen from events (the first pillar, a collapse), like our stage shots; a replayable "documentary mode" |
| 30/60 fps targets, frame generation | Frame costs measured and stressed (`--stress`, `--sweep`), drops rebuilt by hash | Budgets per phase in the gate, like the work profiles |
| Depth of field, haze, grading | A post pass is missing | Macro depth of field, aerial perspective, sun shafts, grading per season |

The strongest ones are those only a simulation engine can do: pheromones as visible physics, orders as marks, light
as a field of the voxel world, a director that knows where the story is.
