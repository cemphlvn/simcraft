# Research: many walkers, 3D models, and animation driven by state machines

Asked (2026-09-29): how many frames can we render if the termites work and walk all the time; would walking need 3D
models to stay efficient; how Unity and Unreal combine 3D models with their own state machines, and how that could
connect to our engine; whom to take after. Our measurements first, then the four topics, then a proposal to decide.

## 1. What we render today (measured)

`simcraft-play games/mound --bench 400 --ticks 600 --size 1280x720 --panel <spawn N, crawl C>`, headless, M-series
Mac, 10 cores. `crawl = 20` is a step every tick (walking non-stop); 4 is the game's default. Milliseconds per frame.

| Termites | crawl | sim | frame build | GPU column | ≈ fps (uncapped) | worst frame |
|---|---|---|---|---|---|---|
| 120 | 20 | 0.20 | 0.18 | 1.61 | ~500 | 4.8 |
| 480 | 20 | 0.31 | 0.59 | 2.12 | ~330 | 7.9 |
| 960 | 20 | 0.46 | 1.05 | 3.03 | ~220 | 16.1 |
| 1500 | 20 | 0.64 | 1.58 | 3.88 | ~165 | 18.2 |
| 1500 | 4 | 0.44 | 1.46 | 3.75 | ~175 | 18.2 |

- Walking all the time costs the *simulation* little (+0.2 ms at 1500); the cost is drawing the bodies.
- Each body is ~1300 vertices built on the CPU every frame (3 ellipsoids, 6 two-segment legs), converted and
  uploaded: CPU work and bandwidth grow with the count. 1500 termites fill a 40x40 ground (1600 surface voxels).
- The average stays far above 60 fps; the worst frames (16–18 ms) at ~1000+ are what a player would feel.

## 2. Walking cheaply: what engines do

- **Instancing:** one mesh uploaded once, drawn many times from a small per-instance record (position, orientation,
  animation time). Draw calls and uploads stop growing with the count.
  [GPU Gems 3, ch. 2](https://developer.nvidia.com/gpugems/gpugems3/part-i-geometry/chapter-2-animated-crowd-rendering):
  ~10,000 independently animated characters at 30 fps on 2007 hardware (instanced palette skinning, bones in a texture).
- **Vertex animation textures (VAT):** each frame's vertex positions (and normals) baked into a texture; the vertex
  shader reads them. No skeleton at run time; cheapest for many identical animated characters.
  [chenjd/Render-Crowd-Of-Animated-Characters](https://github.com/chenjd/Render-Crowd-Of-Animated-Characters): 10,000
  soldiers in ~20 draw calls. Unreal's AnimToTexture (below) is the same idea.
- **Instanced skinning:** bone matrices (all frames of all clips) in a texture, skinning in the vertex shader, one
  instanced draw per mesh; a compute pass can blend clips per joint first
  ([GPU-based large-scale skeletal animation, arXiv 2505.06703](https://arxiv.org/html/2505.06703v1)). More flexible
  than VAT (blending, IK on top), a little more costly.
- **Procedural legs for many-legged creatures:** baked clips cannot cover every slope, wall and overhang; games move
  the body and solve each foot with closed-form two-bone IK onto the surface, legs in alternating groups (tripods
  for insects). [Wall-walking spider, Unity](https://github.com/PhilS94/Unity-Procedural-IK-Wall-Walking-Spider),
  [Three.js spider: walks, climbs walls, no rig or clips](https://github.com/majidmanzarpour/threejs-procedural-spider),
  [Godot 4.5 spider](https://80.lv/articles/ik-driven-procedural-spider-locomotion-in-godot-4-5). Closed-form IK is a
  few lines of arithmetic: it can run in the vertex shader, per instance.
- **Level of detail** in all of them: far characters get fewer triangles, cheaper animation or none.

So: walking does not require authored 3D models to be cheap. It requires that the per-character cost be a small
record, not a mesh. Authored models (glTF) become worth it when the look needs them.

## 3. Unity and Unreal: models moved by state machines

**Unity.** An Animator Controller is a state machine of animation states and transitions; *parameters* (float, int,
bool, trigger) are set from scripts and drive the transitions
([Animation Parameters](https://docs.unity3d.com/6000.1/Documentation/Manual/AnimationParameters.html),
[Animation state machine](https://docs.unity3d.com/Manual/AnimationStateMachines.html)). One controller serves many
models. For crowds: ECS/DOTS animation (e.g. [Rukhanka](https://docs.rukhanka.com/), on Entities Graphics); Unity's
announced new animation system has a hierarchical, layered state machine meant to scale to thousands of characters
([2025 roadmap](https://www.cgchannel.com/2025/03/unity-unveils-its-2025-product-roadmap/)).

**Unreal.** An Animation Blueprint has two halves: the Event Graph gathers gameplay values on the game thread, the
Anim Graph (state machines, blend spaces) reads only those cached values and can run on worker threads ("thread-safe
update", Property Access) ([Animation Blueprint guide](https://mocaponline.com/blogs/mocap-news/unreal-engine-5-animation-blueprint),
[animation optimization](https://dev.epicgames.com/documentation/en-us/unreal-engine/animation-optimization-in-unreal-engine)).
For crowds: the [Animation Sharing plugin](https://dev.epicgames.com/documentation/unreal-engine/animation-sharing-plugin-in-unreal-engine)
evaluates one animation per *state* (an enum set from gameplay) and copies it to every character in that state;
[Mass](https://vrealmatic.com/unreal-engine/crowds) (Unreal's ECS, from the AI team) with
[AnimToTexture](https://dev.epicgames.com/community/learning/tutorials/3xKm/unreal-engine-animtotexture-plugin-how-to-use-it-to-make-vertex-animation-textures-for-crowds)
VATs and instanced static meshes animates the City Sample crowds (a reported 18,000 characters, three LODs).

**Empire of the Ants** (Unreal 5, 2024): 50,000+ triangle hero ant, Nanite and Lumen
([Microids](https://www.microids.com/empire-of-the-ants-how-tower-five-studio-created-photorealistic-graphics/));
how its armies are animated is not published ([tech Q&A](https://wccftech.com/empire-of-the-ants-tech-qa-pushing-visuals-with-ue5-in-an-rts-game/)).

**The shared pattern, and how we fit it.** Both split *deciding* (gameplay sets a state and a few numbers) from
*animating* (a graph turns them into a pose, possibly shared by everyone in the same state). Our engine already
owns the deciding half: every entity has a state-chart state and props, deterministic. So the contract is:

- the game's state (e.g. `Carrying`, `Searching`) → an animation state; props (speed, `carrying`) → parameters;
- our renderer plays it (instanced, shared per state like Animation Sharing);
- the Unity and Unreal adapters (`adapters/`) set the same names on an Animator Controller / an Animation Sharing
  state enum. One mapping, written once as data, three renderers.

## 4. Formats and whom to take after

- **glTF 2.0** is the interchange: skins (joints + inverse bind matrices), animation clips (channels targeting a
  node's translation/rotation/scale or morph weights, keyframe samplers), and
  [`EXT_mesh_gpu_instancing`](https://github.com/KhronosGroup/glTF/tree/main/extensions/2.0/Vendor/EXT_mesh_gpu_instancing)
  ([spec](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html)). Blender, Unity and Unreal all read it, and
  Higgsfield's `generate_3d` produces GLB.
- **Bevy** (Rust, on wgpu like us): `AnimationGraph` is a blend DAG over glTF clips
  ([docs](https://docs.rs/bevy/latest/bevy/animation/index.html)); `bevy_animation_graph` adds state machines as
  graph nodes, each state playing its own graph ([crate](https://crates.io/crates/bevy_animation_graph)). Closest
  code to read; we would not take the ECS, only the shape.
- **Unreal's split** (gather / evaluate on workers / share per state) is the architecture to copy: it is what our
  deterministic states already give us for free.
- **Crowd techniques:** GPU Gems 3 ch. 2 (instanced skinning), VAT (AnimToTexture), procedural IK spiders (legs).

## Proposal (to decide together: it is a stack choice)

1. **Instanced procedural crawlers** (no new assets): the body as one mesh on the GPU; per termite a record (feet,
   surface normal, heading, gait phase, carrying); legs by closed-form IK in the vertex shader; far ones coarser.
   Expected: the frame cost stops growing with the count; the limit becomes the simulation and the world's room.
   Measured with `--stress` / `--bench` before and after.
2. **An animation contract as data** in the view: kind state/props → animation state/parameters, with sharing per
   state; the same names exported through `sim-ffi` for the Unity and Unreal adapters.
3. **glTF models when the look asks for them:** load `.glb` (skins, clips, `EXT_mesh_gpu_instancing`), bake clips
   to bone or vertex textures at load, play them instanced under the same contract.
