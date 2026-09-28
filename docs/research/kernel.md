# Research: tick rate, frame rate and a compute kernel (2026-09-28)

Question: raise the simulation tick rate (slower per-tick speeds per game type), keep sim ticks and visual
frames separate, and move the hot loop into a `kernel/` that can run on CPU, wasm and the GPU. Which language?

## 1. Tick vs frame: what we already have

- The split exists: `sim_render::feel` interpolates between the last two ticks; the viewer's clock is
  `sim_clock += dt * speed` (default 10 ticks/s), frames run at display rate. That is the "fix your timestep"
  accumulator pattern. Cost: interpolation shows the past, one tick of latency.
- Alternative without interpolation: a "render tick" that copies the state and simulates it forward by the
  leftover time (Jakub Tomšů). Not for us: our tick is a discrete rule step, it cannot be advanced by 0.4 of a tick.
- Reference points by tick rate (Tomšů): ~10 tps is unusable for real-time action (latency, jitter);
  30–60 tps is typical; at ~120+ tps interpolation stops mattering.

**Framing.** Ticks are keyframes, frames are in-betweens. The tick stream is the compressed signal and the
renderer decompresses it. The GPU form of that: upload `prev` and `curr` positions **once per tick**; the vertex
shader lerps with a single `alpha` uniform per frame. Per-frame CPU→GPU traffic drops to one uniform.

**Raising the tick rate with slower speeds.** Today speed is a probability per tick
(`rand(100) < warmth`). At a higher tick rate that gives Poisson-like gaps: same average speed, jerky cadence.
A deterministic integer accumulator (Bresenham) gives an even cadence: `acc += speed; if acc >= 1000 { acc -= 1000; step }`.
If the engine owns it, a game declares speed in cells/second and the tick rate, and the engine converts.

Suggested tick rates by game type (to be confirmed by feel evals, not assumed):

| Game type | tps | Why |
|---|---|---|
| Ecosystem / colony (colony, wolf_sheep) | 10–20 | slow motion, interpolation hides the steps |
| RTS lockstep | 10–30 | classic RTS lockstep; input goes through the tick |
| Action / platformer | 60 | input latency ≤ 1 tick = 16 ms |
| Economy / management (market, gamedev) | 1–5 | nothing moves; ticks are days/hours |

## 2. Measured cost today (release, M-series Mac)

`simcraft-agent` stepping to the end, load included:

| Game | ticks | entities | wall time | ≈ per tick |
|---|---|---|---|---|
| colony | 486 | ~570 | 301 ms | ≤ 0.6 ms |
| colony3d | 627 | small | 177 ms | ≤ 0.3 ms |
| wolf_sheep | 500 | ~30 | 119 ms | ≤ 0.2 ms |

About 1 µs per entity per tick (Rhai, `me` map rebuilt per entity, world cloned per tick). At 60 tps colony
needs ~36 ms of CPU per second: fine. **The wall is entity count, not tick rate**: 10k entities ≈ 10 ms/tick,
which already breaks 60 tps on one wasm thread. So a kernel is justified by scale, and by tick rate × scale.

## 3. Computational limits

**wasm (browser).** 60–95 % of native for compute. SIMD128 is everywhere. Threads need SharedArrayBuffer, which
needs COOP/COEP headers (itch.io and portals do not always allow them), so assume **one thread** in the browser.
Linear memory 2–4 GB (Memory64 arriving).

**WebGPU (default limits, what every device must give).** Storage buffer binding 128 MB, workgroup shared memory
16 KB, 256 invocations per workgroup. **Readback is the expensive part**: `mapAsync` takes 5–15 ms in browsers
even when the GPU work took 0.1 ms. So state must stay resident on the GPU; read back rarely (hash every N ticks,
double-buffered staging), render straight from the GPU buffers.

**WGSL integers.** Only `i32`/`u32` (no 64-bit), two's complement. Integer ops are exact, so integer kernels
give the same answer on every GPU. Floats do not: WGSL lets drivers reassociate and fuse f32 math.
Our world uses `i64` (`sim-core/src/world.rs`), so a GPU path needs i32 props (range-checked at load) or
emulated i64 (two u32).

**Determinism on the GPU.** Order-independent atomics (`atomicAdd`, `atomicMin`, `atomicMax` on integers) are
deterministic because the operations commute. Order-dependent ones (append counters, exchange, "first writer wins")
are not. Our "first Move wins" and atomic groups map to: every entity proposes, then `atomicMin` on a packed key
`(priority << k) | entity_id` per target cell; the winner is the same on every run. Stencils (field diffusion,
decay) read the old buffer and write a new one: deterministic by construction.

**Where the GPU wins.** FLAME GPU 2 runs up to ~200M agents on one V100; spatially partitioned messages scale
far better than brute force (10× agents → 5–7× time instead of 10.5×). Below ~10k agents dispatch and upload
overhead dominate and the CPU wins. Fields (voxel heat, pheromones) are the first clear GPU fit.

## 4. Kernel language options

| Option | Runs on | Integers / determinism | Risk | Fit |
|---|---|---|---|---|
| **Rust, CPU** (SoA, rules compiled to a small integer IR, SIMD, rayon on native) | native, wasm, every FFI host | i64, exact, same as today | low | reference backend; must exist anyway |
| **WGSL via `wgpu`** (already a dependency of `sim-gpu`) | every GPU + browser | i32 exact | low; hand-written or generated text | fields, senses, move proposals |
| **CubeCL** (Rust-embedded kernels → WGSL/CUDA/Metal/SPIR-V, JIT, autotune) | wgpu, CUDA, HIP, Metal, CPU | ints supported | medium: young, API churn, heavy deps | if kernels multiply |
| rust-gpu | SPIR-V | — | archived Oct 2025 | no |
| **ONNX** (tensor graph; via `burn-onnx` → Rust code, or onnxruntime-web WebGPU EP) | everywhere ONNX runs | int64 weak on WebGPU EP (cast to i32/u32); runtimes are float-first, no bit-exact guarantee | medium | **agent brains** (policy networks), not the tick |

Why ONNX is not the tick kernel: the tick is scatter with conflict resolution, per-entity branching and state
charts. ONNX expresses that only through `ScatterND` reductions and `If`/`Loop` subgraphs, which runtimes
optimise poorly, and nothing guarantees bit-identical results across execution providers. It is a good format
for learned policies that read an observation tensor and return an action: that sits beside the agent protocol,
outside the deterministic core (or inside it only with integer/quantised models and a conformance test).

## 5. Proposal (for discussion)

- `kernel/` = a crate that owns the hot loop over **SoA integer columns**, with the rule language lowered to a
  small integer IR. Backends: CPU (reference, runs everywhere) and WGSL (generated, i32). Sim-core stays
  game-agnostic; the kernel knows the IR, not any game.
- Every backend is proven with the existing conformance pattern: same hash as the `.ron` reference on every tick.
  The GPU backend reads back the hash every N ticks.
- Order: (1) engine-owned speed accumulator + `tick_rate` in `engine.toml`, (2) CPU kernel (SoA, no per-entity
  map, no world clone), (3) fields on WGSL, (4) agents on WGSL only when a game needs >10k entities.
- Per `CLAUDE.md`, each step starts from a symptom in a real game and is recorded in `emergence.md`.

## Sources

- Fixed timestep without interpolation: https://jakubtomsu.github.io/posts/fixed_timestep_without_interpolation/
- Reliable fixed timestep & inputs: https://jakubtomsu.github.io/posts/input_in_fixed_timestep/
- Taming time in game engines: https://andreleite.com/posts/2025/game-loop/fixed-timestep-game-loop/
- WGSL spec: https://www.w3.org/TR/WGSL/ ; f32 reassociation issue: https://github.com/typeshade/typeshade/issues/378
- WebGPU limits: https://webgpufundamentals.org/webgpu/lessons/webgpu-limits-and-features.html
- mapAsync latency: https://github.com/gpuweb/gpuweb/issues/4432
- FLAME GPU 2 (Richmond et al., 2023): https://onlinelibrary.wiley.com/doi/full/10.1002/spe.3207
- CubeCL: https://github.com/tracel-ai/cubecl ; Rust GPU ecosystem: https://nvlabs.github.io/cuda-oxide/appendix/ecosystem.html
- ONNX Runtime WebGPU EP: https://onnxruntime.ai/docs/execution-providers/WebGPU-ExecutionProvider.html ; int64 issue: https://github.com/microsoft/onnxruntime/issues/28029
- Burn (ONNX → Rust): https://github.com/tracel-ai/burn
- wasm limits: https://qouteall.fun/qouteall-blog/2025/WebAsembly%20Limitations ; wasm vs native: https://ar5iv.labs.arxiv.org/html/1901.09056
