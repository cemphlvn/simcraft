//! sim-model: models from the best tools (glTF 2.0) or built in code, made ready for crowds. `model` loads or builds
//! a model, bakes its clips into palettes and poses it live on the CPU; `skin` draws its instances with wgpu
//! (skinned, PBR-lit, one instanced draw per part). Nothing of simcraft: any wgpu 29 host can use it.

pub mod model;
pub mod skin;
