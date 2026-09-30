# Generated sources

Made with the Higgsfield CLI (`nano_banana_pro`, 1k, 2 credits each) on 2026-09-28, then turned into pixel art with
`simcraft-pixelate` (key out magenta, crop, scale, reduce colours). The large source PNGs are not committed
(`.gitignore`); regenerate them with the prompts below, or use the job ids.

Style suffix on every backdrop prompt: *16-bit pixel art game background layer, side-scrolling platformer style,
crisp pixels, limited palette, no text, no characters, the layer is isolated on a completely flat solid magenta
#FF00FF background, nothing but magenta above and around it.*

| Asset | Prompt | Aspect | Job | Pixelate |
|---|---|---|---|---|
| `far_mountains` | Distant blue-grey mountain range silhouette with gentle snowless peaks spanning the full width, lighter toward the top, bottom edge flat and filling the lower third | 21:9 | 3bfef15b-b5a5-45ba-89ef-f61502c2344b | `--height 26 --colors 5` |
| `hills` | Rolling green grassy hills spanning the full width, soft shading, a few small shrubs, bottom edge flat and filling the lower half | 21:9 | bac8600e-676b-40d8-a056-3df6455d8b23 | `--height 16 --colors 8` |
| `treeline` | A row of leafy deciduous trees and bushes spanning the full width, varied heights, dark green with light highlights, trunks visible, bottom edge flat | 21:9 | 1f4d31ee-94b1-4f4f-8dcb-0c0aa193dc83 | `--height 14 --colors 8` |
| `soil` | Seamless tileable 16-bit pixel art texture of underground brown soil with small pebbles, grains and tiny roots, top-down flat texture, crisp pixels, limited earthy palette, no border (no style suffix, no key) | 1:1 | bbdc9312-b8d1-441c-b91d-1ae03e9901e4 | `--height 32 --colors 8 --no-key` |

```bash
cargo run --release -p sim-render --bin simcraft-pixelate -- assets/src/hills.png assets/pixel/hills.png --height 18 --colors 8
```

## HD pack (`assets/hd/`, `assets/nature_hd.ron`) — 2026-09-28

Natural-history illustration for the GPU stage. Higgsfield `nano_banana_pro` (reported back as `nano_banana_2`),
backgrounds removed with Higgsfield's image background remover for objects, a white key (`simcraft-import --white`)
for scenery (the remover erased whole landscapes), then `simcraft-import` (see the flags there). Sources are not
committed; regenerate with the prompts, or use the job ids.

Style, in every prompt: *realistic natural-history illustration / scientific illustration for a 2.5D side-scrolling
game, painterly but accurate, soft diffuse daylight from the upper left*, and for objects *isolated on a plain flat
pure white background, no shadow, no ground, no text*.

| Asset | Prompt (short) | Job | Import |
|---|---|---|---|
| `sky` | summer sky, cerulean to warm haze, a few cumulus (21:9, 2k) | 30fa3c24-7ea0-4524-9754-95dd5d1a4611 | `--max 2048` |
| `mountains` | distant range through haze, blue-grey (21:9, 2k) | 1c2c5e84-a3b5-4c04-96fd-e84bd84508cb, cut c792d6dc-4750-49bd-872c-328f6924d1fe | cut, `--bottom --tile 20` |
| `forest` | far spruce, pine and birch edge (21:9, 2k) | e05b3389-35ed-43ec-b9ff-abcec62f6544 | `--white --bottom --tile 20` |
| `meadow` | rolling meadow, clover, shrubs (21:9, 2k) | a5d1d571-90ae-45fe-ada8-b2bc9fb86ef4 | `--white --bottom --tile 20` |
| `grass_front` | ant-scale grass, clover, leaf, pebble, dandelion clumps (21:9, 2k) | f9f8a382-35fb-42b4-962b-6b79e11e92d2 | `--white --bottom --tile 20` |
| `soil` | ant-farm soil profile, tileable (1:1, 2k) | f1a90132-66c1-4e3a-9ef0-e361ec00b835 | `--max 1024` |
| `hollow` | nest tunnel wall texture (1:1) | eb843023-28ed-4cb5-b27a-524e4c4739d3 | `--max 512` (unused: hollows are blobs) |
| `ant` | *Formica rufa* worker, lateral, tripod gait, accurate anatomy (1:1) | 3c1be446-007c-4570-ac99-1473dbcc12c0, cut 797c561f-3d95-417f-bc9b-9983d734132f | cut, `--crop --max 512` |
| `ant_b` | same ant (reference), opposite gait phase | 9c3198df-173f-4ed2-9fc6-05076060b2b0, cut 5469d129-12f6-47b2-824a-ff8ccb377918 | cut, `--crop --max 512` |
| `ant_carry` | same ant carrying a raspberry | dfa06879-772a-4606-a759-4ea184cdb22c, cut 7282cf8a-96ef-4f2a-8de2-e04548d6ead3 | cut, `--crop --max 512` |
| `ant_dormant` | same ant folded, winter rest | 98ea3635-9a05-4b33-ae7f-b3c78fc295d3, cut 579d2cc0-bd14-492d-90ea-ca8b7da42108 | cut, `--crop --max 512` |
| `ant_callow` | young pale callow worker | f96d4f54-468f-43d6-8ad7-c88487605bda, cut 4b8233c3-f071-48f7-921e-4466142e8c5c | cut, `--crop --max 512` |
| `bush_ripe` / `bush_bare` | wild raspberry shrub with / without berries | 296ff3c8-5508-4775-a569-8a1083ce14ab / 3231e989-8197-4e3a-882f-a151ad1c39a9 (cuts c516b0cb… / 1de5cf77…) | cut, `--crop --max 512` |
| `mound` | red wood ant nest mound, entrance | d7771bca-49df-460f-9297-2438dc76da2d, cut 4a91fb3a-7ca1-459b-a32a-44a7526bce19 | cut, `--crop --max 512` |
| `granary` | heap of stored seeds | 88794b44-d09f-4428-9a27-fe98e0f15dcb, cut a078cfca-3504-4985-9fb9-e5b3632cc7f6 | cut, `--crop --max 512` |
| `card_summer` / `card_autumn` / `card_winter` | serif title card with a botanical sprig (16:9, 2k; the winter card is the reference for the others) | 04a5f861-3fa0-4c47-9bd3-b4bf4835c5db / 09cfad4f-cba6-4702-841d-a3c5299c5f70 / e758e078-d64b-4003-90c1-8af74b162b34 | `--white --crop --max 1400` |

Lessons: ask for a *white* ground, not "transparent-looking" (the model paints a checkerboard); name the word
"exactly once" on title cards; a background remover keeps objects but erases landscapes.

### Walk cycle from video (`ant_walk_0..7`)

1. Image-to-video from the still ant (job 3c1be446…): Kling 3.0 Turbo, 1:1, 5 s, start frame only, prompt *"the ant
   walks in place on the spot with a natural tripod gait … body stays at exactly the same position and size … static
   locked-off camera, plain pure white background"* → ff20b635-3357-4c47-a617-59a08615be99. (MiniMax H3 with the
   same image as start and end frame, fed64805…, loops but the ant drifts and turns: worse for a sprite.)
2. Higgsfield's video background remover (529a261b-4e79-4570-936a-48b0f7d7449b) returns the ant over **black**,
   H.264, no alpha. Keying black would eat the dark gaster, so: **two-background matte**. The same frame exists over
   cream (original) and over black (cut): `alpha = 1 − (original − cut) / cream`, `colour = cut / alpha`. Exact
   edges, antennae and hairs kept.
3. The loop: the segment whose last frame is closest to its first **relative to how much the legs move in between**
   (a still segment closes perfectly but does not walk): frames 80..111 at 24 fps; 8 frames evenly across it, all
   cropped to one shared box (no jitter), 512 px wide.

## Road pack (`assets/road/`, `assets/road.ron`) — 2026-09-29

First-person desert highway for `games/lanes`. Higgsfield `nano_banana_pro`; objects cut with the background remover
(white parts such as cone bands, wall stripes, arrows and the checkered banner survive it), scenery with
`simcraft-import --white --tile`, then `simcraft-import`.

| Asset | Prompt (short) | Job |
|---|---|---|
| `sky` | sunset desert sky, orange to violet, streaky clouds (21:9, 2k) | 4c84d79d-5809-46a5-b3a7-d569d9c9e02f |
| `mesas` | distant mesas and buttes at sunset, haze (21:9, 2k; `--white --bottom --tile 20`) | 0b1a3dd3-b98e-49fc-89a0-d7a0c6f0fb1a |
| `asphalt` / `sand` | tileable worn asphalt / desert roadside ground, from above | 00ec9476… / b35723e2… |
| `cone`, `barrier`, `coin`, `nitro`, `fuelpad`, `gate` | eye-level objects in warm sunset light, white ground | 1e1d4d0d…, 5ef2c2e8…, a8d4c65a…, 321b69bd…, e55b3be1…, 4bb667f0… |
| `oil` | iridescent oil slick from above | 9d0b7b0b… (cut 0142aaa2…) |
| `hood` | red muscle car hood from the driver's seat | 33b80deb-4a15-4f5f-8bf8-04efbb9211a4 |
| `car_rear` | the same car from behind (hood as reference) | 52491bb4-3101-43e5-bad2-76f78242d183 |
| `cactus`, `rock`, `post` | roadside scenery | 00545f81…, 45e8647d…, 063572f0… |
| `btn_*` | chrome-rimmed midnight-blue badges: arrows, flame (dash), brake, jump, tap; `btn_pos1..4`: four lane stripes, one lit (the jump badge as reference) | 7aee37e7…, 3031f285…, aae2514c…, 218cfbc0…, 1b052772…, a3ba137d…, 434c2384…, 3aac2707…, f985158b…, 2f96debb… |

## Termite model and mound textures (2026-09-29, `games/mound`)

Realistic worker termite, from a photograph-like reference to a rigged, animated game model:

1. Reference: Higgsfield `gpt_image_2_5`, two variants (jobs `88cec517-0c01-4a52-8373-18de17b0cc12`,
   `7b2b6322-d9ad-423b-8a8d-2f1a68f8f778`): "Photorealistic macro studio photograph of a single worker termite
   (Macrotermes), scientific specimen, three-quarter top view from front-left, ... all six legs spread outward and
   fully separated from the body, ... plain pure white seamless background". `termite/ref_a.png` was used (legs
   clearest).
2. Mesh: image-to-3D on ref A, two models compared: Tripo H3.1 (`tripo_h3_1_image_to_3d`, detailed geometry and
   texture, PBR, 60k faces; job `a7c4bdc6-11af-4d5f-a5bf-868302282288`) → 58k triangles, colour + ORM + normal maps
   at 4096², crisp segments and hairs; Meshy 7 ultra (`meshy_v7_image_to_3d`; job
   `abceb5e6-4018-4848-81a7-62d786a54791`) → 62k triangles, softer. Tripo kept (`termite/tripo.glb`, not in git).
   Higgsfield's auto-rigging is humanoid-only, so the rig is ours:
3. Rig, clips, levels of detail: `Blender -b --factory-startup --python tools/blender/rig_insect.py --
   assets/src/termite/tripo.glb assets/models termite` → `assets/models/termite.glb` (12k triangles),
   `_lod1` (3k), `_lod2` (800); 22 bones, the `carry` socket, clips `walk`, `carry`, `dig`, `idle`; textures 1024².
   Check with `simcraft-model assets/models/termite.glb --game games/mound --kind termite`.

Textures (`assets/mound/`, pack `assets/mound.ron`): Higgsfield `gpt_image_2_5`, then `tools/tileable.py` (1024²):
- `laterite.png` (job `becc10e3-d722-4272-b01a-4ae8ea0cefd2`): "Seamless tileable texture, straight top-down
  orthographic macro photograph of dry red-orange laterite savanna soil at insect scale: fine sand grains, tiny
  rounded quartz pebbles and dust, ...".
- `mud.png` (job `01a34889-d9a5-48b7-ac26-620b7aa8d37d`): "... a termite mound wall built from packed moist clay
  pellets: rounded mud balls pressed together, dark red-brown laterite with a faint wet sheen ...".

## Themes (`assets/themes/`, `assets/cyber.ron`, `assets/ocean.ron`, `assets/savanna.ron`) — 2026-09-29

Three worlds for `games/highway_surfers` (CYBERRUN, BLUE OCEAN, SAVANNA STAMPEDE). Higgsfield `nano_banana_pro`
(reported back as `nano_banana_2`) in the project "Highway Surfers themes"; objects cut with Higgsfield's image
background remover; skies `simcraft-import --max 2048`; horizon bands `--white --bottom --tile 20 --max 2048` (the
island band came out as two copies stacked: the upper half was kept); surfaces `tools/tileable.py --size 1024`;
cut objects `--crop --max 512`. Music: Higgsfield `sonilo_music`, 60 s loops, converted to MP3 (`ffmpeg -c:a
libmp3lame -q:a 5`).

Style in every prompt: *stylized 3D mobile game art, clean vivid colors, no text*; objects *isolated on a plain flat
pure white background, no shadow*; vehicles *straight-on rear view* / *straight-on front view*.

| Theme | Sky, horizon, lane, shoulder | Pickup, props, rider | Low / mid / tall (rear, front) | Music |
|---|---|---|---|---|
| cyber | 0d5ee6a5, 1d22ee83, 727de61d, d538176b | 8e5d2ed7, d17f08f1, dd3711fa, 94c1dcfe | packet 9fa60d13 / c50ed6b9, transport 1de422bf / 3211f5fd, rack 7a7b72db / 5031afb8 | b3888f8a (synthwave, 128 bpm) |
| ocean | 1b0db8fd, d5958192, 467ed617, 2e166ac8 | 430fdea7, 0c5457a2, 33079129, c6266cc9 | jet ski 436d4fb3 / f59bf759, speedboat 935898cf / 6e4cf4e7, ferry eba49104 / 10c850de | ce8247b9 (marimba, steel drums, 115 bpm) |
| savanna | 27c7001a, ae73c1ca, 4f5515f9, a42b0ebb | e46d321b, 6eace251, 8299ba76, ecf2d27f | zebra 3c0d9c33 / ba6fb9f2, rhino 85b70346 / dd20a3a2, elephant e7a52520 / 96f7b172 | 605d3181 (djembe, kalimba, 124 bpm) |

Known weak spots: the CYBERRUN runner is dark on a dark road (hard to see); the front elephant is cartoon-coloured
while its rear is realistic; the riders are side-on, not from behind.
