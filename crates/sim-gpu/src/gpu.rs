//! The wgpu backend: every image becomes one mipmapped texture (premultiplied alpha, sRGB), every quad one instance
//! of a 4-vertex strip. Quads that share a texture are drawn in one call. A frame uploads only the instance list.

use std::collections::BTreeMap;

use sim_render::image::Image;

use crate::stage::{BLOB, Quad, WHITE, Wrap};

const SHADER: &str = r#"
struct Globals { screen: vec2<f32>, pad: vec2<f32> };
@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct Inst {
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) top: vec4<f32>,
    @location(3) bottom: vec4<f32>,
    @location(4) fx: vec4<f32>,
};
struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) fx: vec4<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32, i: Inst) -> VOut {
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u));
    // Rotation (fx.z, radians) about the quad's centre.
    let half = i.rect.zw * 0.5;
    let d = (corner - vec2<f32>(0.5, 0.5)) * i.rect.zw;
    let c = cos(i.fx.z);
    let s = sin(i.fx.z);
    let p = i.rect.xy + half + vec2<f32>(d.x * c - d.y * s, d.x * s + d.y * c);
    var o: VOut;
    o.pos = vec4<f32>(p.x / g.screen.x * 2.0 - 1.0, 1.0 - p.y / g.screen.y * 2.0, 0.0, 1.0);
    o.uv = mix(i.uv.xy, i.uv.zw, corner);
    o.color = mix(i.top, i.bottom, corner.y);
    o.fx = i.fx;
    return o;
}

@fragment
fn fs(v: VOut) -> @location(0) vec4<f32> {
    // Textures hold premultiplied colour; blur is a mip bias (out of focus), desat pulls towards grey (winter).
    let c = textureSampleBias(tex, samp, v.uv, v.fx.x);
    let grey = dot(c.rgb, vec3<f32>(0.299, 0.587, 0.114));
    let rgb = mix(c.rgb, vec3<f32>(grey), v.fx.y) * v.color.rgb;
    return vec4<f32>(rgb, c.a) * v.color.a;
}
"#;

const SHADER3: &str = r#"
struct Globals3 {
    view_proj: mat4x4<f32>,
    fog: vec4<f32>,
    eye: vec4<f32>,
    range: vec4<f32>,
    light_vp: mat4x4<f32>,
    // on, strength (how dark), texel, bias
    shadow: vec4<f32>,
    // towards the sun (xyz), haze (w)
    sun: vec4<f32>,
    sun_col: vec4<f32>,
};
@group(0) @binding(0) var<uniform> g: Globals3;
@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_samp: sampler_comparison;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) fog: f32,
};
struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) fog: f32,
    @location(3) world: vec3<f32>,
};

@vertex
fn vs(v: VIn) -> VOut {
    var o: VOut;
    o.pos = g.view_proj * vec4<f32>(v.pos, 1.0);
    o.uv = v.uv;
    o.color = v.color;
    // Fog by distance from the eye when the frame asks for it (range.y > 0): meshes kept on the GPU need no
    // per-frame fog of their own. Otherwise the vertex's own fog (tracks set it).
    var f = v.fog;
    if (g.range.y > 0.0) {
        f = max(f, clamp((distance(v.pos, g.eye.xyz) - g.range.x) / g.range.y, 0.0, 1.0));
    }
    o.fog = f;
    o.world = v.pos;
    return o;
}

// Depth from the sun (shadow maps).
@vertex
fn vs_shadow(v: VIn) -> @builtin(position) vec4<f32> {
    return g.light_vp * vec4<f32>(v.pos, 1.0);
}

// How much sun reaches a point: the shadow map, 3x3 filtered (soft edges).
fn sunlit(p: vec3<f32>) -> f32 {
    let c = g.light_vp * vec4<f32>(p, 1.0);
    let uv = vec2<f32>(c.x * 0.5 + 0.5, 0.5 - c.y * 0.5);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || c.z > 1.0) {
        return 1.0;
    }
    var sum = 0.0;
    for (var i = -1; i <= 1; i = i + 1) {
        for (var j = -1; j <= 1; j = j + 1) {
            sum = sum + textureSampleCompareLevel(shadow_map, shadow_samp, uv + vec2<f32>(f32(i), f32(j)) * g.shadow.z, c.z - g.shadow.w);
        }
    }
    return sum / 9.0;
}

@fragment
fn fs(v: VOut) -> @location(0) vec4<f32> {
    let c = textureSample(tex, samp, v.uv);
    if (c.a < 0.02) {
        discard;
    }
    var rgb = c.rgb * v.color.rgb;
    if (g.shadow.x > 0.5) {
        rgb = rgb * mix(1.0 - g.shadow.y, 1.0, sunlit(v.world));
    }
    // Haze: the fog is brighter towards the sun (light scattered in the air).
    let toward = pow(max(dot(normalize(v.world - g.eye.xyz), g.sun.xyz), 0.0), 6.0) * g.sun.w;
    let fogc = mix(g.fog.rgb, g.sun_col.rgb, clamp(toward, 0.0, 1.0));
    // Premultiplied: fog blends towards the horizon colour in proportion to coverage.
    rgb = mix(rgb, fogc * c.a, v.fog);
    return vec4<f32>(rgb, c.a) * v.color.a;
}
"#;

/// The sun's shadow map is this many texels a side.
pub const SHADOW_SIZE: u32 = 2048;

/// A vertex of the 3D world: position, texture coordinate, colour (sRGB, multiplied), fog amount 0..1.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vert3 {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
    pub fog: f32,
}

/// Triangles that share one texture and wrap mode (drawn in order; the depth buffer sorts what overlaps).
#[derive(Clone, Debug)]
pub struct Mesh {
    pub image: String,
    pub wrap: Wrap,
    pub verts: Vec<Vert3>,
}

/// The 3D part of a frame: camera matrix, fog colour (sRGB), meshes; and optionally fog by distance from `eye`
/// (`fog_range`: starts at, fully fogged this much farther; 0 = off) and meshes kept on the GPU (`Gpu::keep`),
/// drawn first.
pub struct World3<'a> {
    pub view_proj: [[f32; 4]; 4],
    pub fog: [f32; 3],
    pub meshes: &'a [Mesh],
    pub eye: [f32; 3],
    pub fog_range: [f32; 2],
    pub kept: &'a [u32],
    /// Skinned, instanced models (`Gpu::upload_model`), lit by `light`.
    pub models: &'a [crate::skin::ModelDraw],
    pub light: crate::skin::Light,
    /// Sun shadows over a region (None = no shadows).
    pub shadow: Option<Shadow>,
    /// A lens: depth of field (None = everything sharp).
    pub lens: Option<Lens>,
    /// Light scattered towards the sun in the fog (0 = none).
    pub haze: f32,
}

/// Sun shadows over a square `radius` around `center` (world units); `strength`: how dark the shade (0..1).
#[derive(Clone, Copy, Debug)]
pub struct Shadow {
    pub center: [f32; 3],
    pub radius: f32,
    pub strength: f32,
}

/// A lens focused `focus` units away; `aperture`: how fast things blur away from the focus (0 = pinhole).
/// `near`/`far`: the camera's (to read depth back as distance).
#[derive(Clone, Copy, Debug)]
pub struct Lens {
    pub focus: f32,
    pub aperture: f32,
    pub near: f32,
    pub far: f32,
}

impl<'a> World3<'a> {
    pub fn new(view_proj: [[f32; 4]; 4], fog: [f32; 3], meshes: &'a [Mesh]) -> World3<'a> {
        World3 {
            view_proj,
            fog,
            meshes,
            eye: [0.0; 3],
            fog_range: [0.0; 2],
            kept: &[],
            models: &[],
            light: crate::skin::Light::default(),
            shadow: None,
            lens: None,
            haze: 0.0,
        }
    }
}

/// Meshes uploaded once and drawn every frame until their version changes (terrain that rarely changes).
struct Kept {
    version: u64,
    buf: Option<wgpu::Buffer>,
    draws: Vec<(String, Wrap, u32, u32)>,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Inst {
    rect: [f32; 4],
    uv: [f32; 4],
    top: [f32; 4],
    bottom: [f32; 4],
    fx: [f32; 4],
}

/// One bind group per wrap mode (the sampler is part of the group).
struct Tex([wgpu::BindGroup; 4]);

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    tex_layout: wgpu::BindGroupLayout,
    samplers: [wgpu::Sampler; 4],
    textures: BTreeMap<String, Tex>,
    instances: wgpu::Buffer,
    capacity: usize,
    pipeline3: wgpu::RenderPipeline,
    globals3: wgpu::Buffer,
    globals3_bg: wgpu::BindGroup,
    verts: wgpu::Buffer,
    vcapacity: usize,
    kept: BTreeMap<u32, Kept>,
    /// Models (made on the first upload).
    pub skin: Option<crate::skin::Skin>,
    shadow_view: wgpu::TextureView,
    shadow_sampler: wgpu::Sampler,
    shadow_pipeline: wgpu::RenderPipeline,
    shadow_bg: wgpu::BindGroup,
    /// The lens (depth of field), made when a frame first asks for one.
    post: Option<crate::post::Post>,
    /// The depth buffer the last world pass used (the lens reads it), and one per size drawn (a frame and a mirror
    /// in it do not rebuild each other's).
    depth: Option<(u32, u32, wgpu::TextureView)>,
    depths: BTreeMap<(u32, u32), wgpu::TextureView>,
    /// Pictures rendered on the GPU that quads and meshes can name (`render_into`): size and target.
    offscreen: BTreeMap<String, (u32, u32, wgpu::TextureView)>,
    /// Texture sizes (for the composer's aspect ratios).
    pub sizes: BTreeMap<String, (u32, u32)>,
}

/// Every mesh's vertices in order, colours to linear light; on all cores (the per-frame cost of a 3D frame).
fn to_linear(meshes: &[Mesh]) -> Vec<Vert3> {
    use rayon::prelude::*;
    let total: usize = meshes.iter().map(|m| m.verts.len()).sum();
    let mut out = vec![Vert3 { pos: [0.0; 3], uv: [0.0; 2], color: [0.0; 4], fog: 0.0 }; total];
    let mut rest = out.as_mut_slice();
    let mut jobs = Vec::with_capacity(meshes.len());
    for m in meshes {
        let (head, tail) = rest.split_at_mut(m.verts.len());
        jobs.push((head, &m.verts));
        rest = tail;
    }
    jobs.into_par_iter().for_each(|(dst, src)| {
        dst.par_chunks_mut(4096).zip(src.par_chunks(4096)).for_each(|(d, s)| {
            for (d, v) in d.iter_mut().zip(s) {
                *d = Vert3 { color: linear(v.color), ..*v };
            }
        });
    });
    out
}

/// Quad colours are written in sRGB (as picked by eye); the shader blends in linear light.
fn linear(c: [f32; 4]) -> [f32; 4] {
    [to_lin(c[0]), to_lin(c[1]), to_lin(c[2]), c[3]]
}

/// x^2.2 from a table (interpolated; error under 1e-4): the per-vertex cost of a 3D frame without `powf`.
fn to_lin(x: f32) -> f32 {
    const N: usize = 4096;
    static LUT: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
    if !(0.0..1.0).contains(&x) {
        return x.max(0.0).powf(2.2);
    }
    let lut = LUT.get_or_init(|| (0..=N).map(|i| (i as f32 / N as f32).powf(2.2)).collect());
    let f = x * N as f32;
    let i = f as usize;
    lut[i] + (lut[i + 1] - lut[i]) * (f - i as f32)
}

/// Premultiplies alpha and builds the mip chain (box filter on premultiplied values: no dark fringes).
fn mips(img: &Image, max_side: usize) -> Vec<(u32, u32, Vec<u8>)> {
    let mut cur = Image { w: img.w, h: img.h, px: img.px.iter().map(|p| premul(*p)).collect() };
    while cur.w.max(cur.h) > max_side {
        cur = half(&cur);
    }
    let mut out = vec![(cur.w as u32, cur.h as u32, cur.px.iter().flatten().copied().collect())];
    while cur.w > 1 || cur.h > 1 {
        cur = half(&cur);
        out.push((cur.w as u32, cur.h as u32, cur.px.iter().flatten().copied().collect()));
    }
    out
}

fn premul(p: [u8; 4]) -> [u8; 4] {
    let a = p[3] as u32;
    [(p[0] as u32 * a / 255) as u8, (p[1] as u32 * a / 255) as u8, (p[2] as u32 * a / 255) as u8, p[3]]
}

fn half(img: &Image) -> Image {
    let (w, h) = ((img.w / 2).max(1), (img.h / 2).max(1));
    let mut px = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let mut s = [0u32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let p = img.get((x * 2 + dx).min(img.w - 1), (y * 2 + dy).min(img.h - 1));
                for c in 0..4 {
                    s[c] += p[c] as u32;
                }
            }
            px.push([(s[0] / 4) as u8, (s[1] / 4) as u8, (s[2] / 4) as u8, (s[3] / 4) as u8]);
        }
    }
    Image { w, h, px }
}

/// A soft round spot (white, alpha falling off to the edge).
fn blob(n: usize) -> Image {
    let px = (0..n * n)
        .map(|i| {
            let (x, y) = ((i % n) as f32 + 0.5, (i / n) as f32 + 0.5);
            let d = ((x - n as f32 / 2.0).powi(2) + (y - n as f32 / 2.0).powi(2)).sqrt() / (n as f32 / 2.0);
            // A solid middle and a soft rim: overlapping blobs read as one tunnel.
            [255, 255, 255, ((1.0 - d) / 0.45).clamp(0.0, 1.0).powf(1.5).mul_add(255.0, 0.0) as u8]
        })
        .collect();
    Image { w: n, h: n, px }
}

/// Transparent in the middle, opaque at the corners: multiply by a colour to darken the frame's edges.
fn vignette(n: usize) -> Image {
    let px = (0..n * n)
        .map(|i| {
            let (x, y) = ((i % n) as f32 / n as f32 - 0.5, (i / n) as f32 / n as f32 - 0.5);
            let d = (x * x + y * y).sqrt() / std::f32::consts::FRAC_1_SQRT_2;
            [255, 255, 255, ((d - 0.45).max(0.0) / 0.55).powf(1.6).clamp(0.0, 1.0).mul_add(255.0, 0.0) as u8]
        })
        .collect();
    Image { w: n, h: n, px }
}

impl Gpu {
    /// A device for a surface (window) or none (offscreen).
    pub async fn new(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
        format: Option<wgpu::TextureFormat>,
    ) -> Result<Gpu, String> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: surface,
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| format!("no GPU adapter: {e}"))?;
        // WebGL2-safe limits, plus storage buffers where the GPU has them (skinned models need one: their palettes).
        let mut limits = wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits());
        let have = adapter.limits();
        limits.max_storage_buffers_per_shader_stage = have.max_storage_buffers_per_shader_stage;
        limits.max_storage_buffer_binding_size = have.max_storage_buffer_binding_size;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("simcraft"),
                required_features: wgpu::Features::empty(),
                required_limits: limits,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| format!("no GPU device: {e}"))?;
        let format = match (surface, format) {
            (_, Some(f)) => f,
            (Some(s), None) => {
                let caps = s.get_capabilities(&adapter);
                caps.formats.iter().copied().find(|f| f.is_srgb()).unwrap_or(caps.formats[0])
            }
            (None, None) => wgpu::TextureFormat::Rgba8UnormSrgb,
        };
        let shader = device
            .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("quads"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("quads"),
            bind_group_layouts: &[Some(&globals_layout), Some(&tex_layout)],
            immediate_size: 0,
        });
        let attrs = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("quads"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Inst>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &attrs,
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        // The 3D pipeline: same textures and samplers, a depth buffer, perspective-correct texturing, fog.
        let shader3 = device
            .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("world"), source: wgpu::ShaderSource::Wgsl(SHADER3.into()) });
        let globals3_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals3"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        // The sun's shadow map (depth from the sun), and its comparing sampler.
        let shadow_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow map"),
            size: wgpu::Extent3d { width: SHADOW_SIZE, height: SHADOW_SIZE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_view = shadow_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let layout3 = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("world"),
            bind_group_layouts: &[Some(&globals3_layout), Some(&tex_layout)],
            immediate_size: 0,
        });
        let attrs3 = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4, 3 => Float32];
        let pipeline3 = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("world"),
            layout: Some(&layout3),
            vertex: wgpu::VertexState {
                module: &shader3,
                entry_point: Some("vs"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vert3>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attrs3,
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader3,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let globals3 = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals3"),
            size: 224,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals3_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals3"),
            layout: &globals3_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals3.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&shadow_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&shadow_sampler) },
            ],
        });
        // Depth only, from the sun: the terrain kept on the GPU casts shadows (models cast theirs in `Skin`).
        // Its own bind group: the pass writes the shadow map, so it must not also bind it for reading.
        let shadow_globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let shadow_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow globals"),
            layout: &shadow_globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals3.as_entire_binding() }],
        });
        let shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow"),
            bind_group_layouts: &[Some(&shadow_globals_layout)],
            immediate_size: 0,
        });
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow"),
            layout: Some(&shadow_layout),
            vertex: wgpu::VertexState {
                module: &shader3,
                entry_point: Some("vs_shadow"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vert3>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attrs3,
                }],
                compilation_options: Default::default(),
            },
            fragment: None,
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 },
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let verts = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("verts"),
            size: 4096 * std::mem::size_of::<Vert3>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });
        let sampler = |u: wgpu::AddressMode, v: wgpu::AddressMode| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                address_mode_u: u,
                address_mode_v: v,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                // Anisotropic: ground and walls seen at a grazing angle stay sharp along their length.
                anisotropy_clamp: 8,
                ..Default::default()
            })
        };
        use wgpu::AddressMode::{ClampToEdge, MirrorRepeat, Repeat};
        let samplers =
            [sampler(ClampToEdge, ClampToEdge), sampler(Repeat, Repeat), sampler(Repeat, ClampToEdge), sampler(MirrorRepeat, ClampToEdge)];
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instances"),
            size: 1024 * std::mem::size_of::<Inst>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut gpu = Gpu {
            device,
            queue,
            format,
            pipeline,
            globals,
            globals_bg,
            tex_layout,
            samplers,
            textures: BTreeMap::new(),
            instances,
            capacity: 1024,
            pipeline3,
            globals3,
            globals3_bg,
            verts,
            vcapacity: 4096,
            kept: BTreeMap::new(),
            skin: None,
            shadow_view,
            shadow_sampler,
            shadow_pipeline,
            shadow_bg,
            post: None,
            depth: None,
            depths: BTreeMap::new(),
            offscreen: BTreeMap::new(),
            sizes: BTreeMap::new(),
        };
        gpu.upload(WHITE, &Image { w: 1, h: 1, px: vec![[255; 4]] }, 1);
        gpu.upload(BLOB, &blob(64), 64);
        gpu.upload("__vignette", &vignette(256), 256);
        Ok(gpu)
    }

    /// Uploads an image as a texture (longest side capped at `max_side`), with its mip chain.
    pub fn upload(&mut self, name: &str, img: &Image, max_side: usize) {
        let levels = mips(img, max_side);
        let (w, h, _) = levels[0];
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(name),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, (lw, lh, data)) in levels.iter().enumerate() {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(lw * 4), rows_per_image: Some(*lh) },
                wgpu::Extent3d { width: *lw, height: *lh, depth_or_array_layers: 1 },
            );
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = |s: &wgpu::Sampler| {
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(name),
                layout: &self.tex_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(s) },
                ],
            })
        };
        let tex = Tex(std::array::from_fn(|i| bind(&self.samplers[i])));
        self.textures.insert(name.to_string(), tex);
        self.sizes.insert(name.to_string(), (img.w as u32, img.h as u32));
    }

    /// Draws the quads (in order) into `target`, clearing it first.
    pub fn draw(&mut self, target: &wgpu::TextureView, w: u32, h: u32, quads: &[Quad]) {
        self.render(target, w, h, quads, None, &[]);
    }

    /// A frame: 2D quads behind (sky), the 3D world with depth, 2D quads in front (hood, effects, HUD).
    pub fn render(&mut self, target: &wgpu::TextureView, w: u32, h: u32, back: &[Quad], world: Option<World3>, front: &[Quad]) {
        let insts: Vec<Inst> = back
            .iter()
            .chain(front)
            .map(|q| Inst {
                rect: [q.x, q.y, q.w, q.h],
                uv: q.uv,
                top: linear(q.top),
                bottom: linear(q.bottom),
                fx: [q.blur, q.desat, q.rot.to_radians(), 0.0],
            })
            .collect();
        if insts.len() > self.capacity {
            self.capacity = insts.len().next_power_of_two();
            self.instances = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("instances"),
                size: (self.capacity * std::mem::size_of::<Inst>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !insts.is_empty() {
            self.queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&insts));
        }
        self.queue.write_buffer(&self.globals, 0, bytemuck::cast_slice(&[w as f32, h as f32, 0.0, 0.0]));
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        // With a lens: the sky and the world are drawn into an offscreen picture, then blurred by distance onto the
        // target; the HUD (front) comes after, sharp.
        let lens = world.as_ref().and_then(|w| w.lens);
        if let Some(lens) = lens {
            let post = self.post.get_or_insert_with(|| crate::post::Post::new(&self.device, self.format));
            let scene = post.scene(&self.device, w, h);
            self.quad_pass(&mut enc, &scene, true, back, 0);
            if let Some(world) = world {
                self.world_pass(&mut enc, &scene, w, h, &world);
            }
            let depth = &self.depth.as_ref().expect("made by the world pass").2;
            let post = self.post.as_ref().expect("made above");
            post.apply(&self.device, &self.queue, &mut enc, target, depth, w, h, lens);
        } else {
            self.quad_pass(&mut enc, target, true, back, 0);
            if let Some(world) = world {
                self.world_pass(&mut enc, target, w, h, &world);
            }
        }
        if !front.is_empty() {
            self.quad_pass(&mut enc, target, false, front, back.len());
        }
        self.queue.submit(Some(enc.finish()));
    }

    fn quad_pass(&self, enc: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, clear: bool, quads: &[Quad], first: usize) {
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("quads"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: if clear { wgpu::LoadOp::Clear(wgpu::Color::BLACK) } else { wgpu::LoadOp::Load },
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.globals_bg, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        // Runs of quads that share a texture and a wrap mode: one draw call each.
        let mut i = 0;
        while i < quads.len() {
            let mut j = i + 1;
            while j < quads.len() && quads[j].image == quads[i].image && quads[j].wrap == quads[i].wrap {
                j += 1;
            }
            pass.set_bind_group(1, self.bind(&quads[i].image, quads[i].wrap), &[]);
            pass.draw(0..4, (first + i) as u32..(first + j) as u32);
            i = j;
        }
    }

    fn bind(&self, image: &str, wrap: Wrap) -> &wgpu::BindGroup {
        let tex = self.textures.get(image).or_else(|| self.textures.get(WHITE)).expect("white exists");
        let w = match wrap {
            Wrap::Clamp => 0,
            Wrap::Repeat => 1,
            Wrap::RepeatX => 2,
            Wrap::MirrorX => 3,
        };
        &tex.0[w]
    }

    fn world_pass(&mut self, enc: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, w: u32, h: u32, world: &World3) {
        if self.depth.as_ref().is_none_or(|(dw, dh, _)| (*dw, *dh) != (w, h)) {
            let view = self
                .depths
                .entry((w, h))
                .or_insert_with(|| {
                    let t = self.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("depth"),
                        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Depth32Float,
                        // Readable too: the lens reads depth back as distance.
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    });
                    t.create_view(&wgpu::TextureViewDescriptor::default())
                })
                .clone();
            // A window resized many times would keep every old size: keep only the last few.
            if self.depths.len() > 4 {
                let keep = [(w, h)];
                self.depths.retain(|k, _| keep.contains(k));
            }
            self.depth = Some((w, h, view));
        }
        let verts = to_linear(world.meshes);
        if verts.len() > self.vcapacity {
            self.vcapacity = verts.len().next_power_of_two();
            self.verts = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("verts"),
                size: (self.vcapacity * std::mem::size_of::<Vert3>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !verts.is_empty() {
            self.queue.write_buffer(&self.verts, 0, bytemuck::cast_slice(&verts));
        }
        let fog = linear([world.fog[0], world.fog[1], world.fog[2], 1.0]);
        // The sun's view over the shadowed region.
        let d = world.light.sun_dir;
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-6);
        let sun_dir = [d[0] / len, d[1] / len, d[2] / len];
        let (light_vp, shadow) = match world.shadow {
            Some(sh) => {
                let c = crate::math::V3(sh.center[0], sh.center[1], sh.center[2]);
                let eye = crate::math::Eye {
                    pos: c + crate::math::V3(sun_dir[0], sun_dir[1], sun_dir[2]).scale(sh.radius * 3.0),
                    target: c,
                    roll: 0.0,
                    fov: 0.0,
                    near: 0.1,
                    far: sh.radius * 6.0,
                };
                (eye.ortho(sh.radius), [1.0, sh.strength, 1.0 / SHADOW_SIZE as f32, 0.0015])
            }
            None => (crate::model::IDENTITY, [0.0; 4]),
        };
        let sun_col = linear([world.light.sun[0].min(1.0), world.light.sun[1].min(1.0), world.light.sun[2].min(1.0), 1.0]);
        if let (Some(skin), false) = (self.skin.as_mut(), world.models.is_empty()) {
            skin.prepare(
                &self.device,
                &self.queue,
                world.view_proj,
                fog,
                world.eye,
                world.fog_range,
                world.light,
                world.models,
                light_vp,
                shadow,
            );
        }
        let mut g: Vec<f32> = world.view_proj.iter().flatten().copied().collect();
        g.extend_from_slice(&fog);
        g.extend_from_slice(&[world.eye[0], world.eye[1], world.eye[2], 0.0, world.fog_range[0], world.fog_range[1], 0.0, 0.0]);
        g.extend(light_vp.iter().flatten().copied());
        g.extend_from_slice(&shadow);
        g.extend_from_slice(&[sun_dir[0], sun_dir[1], sun_dir[2], world.haze]);
        g.extend_from_slice(&sun_col);
        self.queue.write_buffer(&self.globals3, 0, bytemuck::cast_slice(&g));
        // Shadows first: depth from the sun, of the kept terrain and the models.
        if world.shadow.is_some() {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.shadow_pipeline);
            pass.set_bind_group(0, &self.shadow_bg, &[]);
            for slot in world.kept {
                let Some(Kept { buf: Some(buf), draws, .. }) = self.kept.get(slot) else { continue };
                let total: u32 = draws.iter().map(|d| d.3).sum();
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..total, 0..1);
            }
            if let (Some(skin), false) = (self.skin.as_ref(), world.models.is_empty()) {
                skin.draw_shadow(&mut pass, world.models);
            }
        }
        let keep_depth = world.lens.is_some();
        let depth = &self.depth.as_ref().expect("made above").2;
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("world"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: if keep_depth { wgpu::StoreOp::Store } else { wgpu::StoreOp::Discard },
                }),
                stencil_ops: None,
            }),
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline3);
        pass.set_bind_group(0, &self.globals3_bg, &[]);
        for slot in world.kept {
            let Some(Kept { buf: Some(buf), draws, .. }) = self.kept.get(slot) else { continue };
            pass.set_vertex_buffer(0, buf.slice(..));
            for (image, wrap, at, n) in draws {
                pass.set_bind_group(1, self.bind(image, *wrap), &[]);
                pass.draw(*at..*at + *n, 0..1);
            }
        }
        if let (Some(skin), false) = (self.skin.as_ref(), world.models.is_empty()) {
            skin.draw(&mut pass, world.models);
            pass.set_pipeline(&self.pipeline3);
            pass.set_bind_group(0, &self.globals3_bg, &[]);
        }
        pass.set_vertex_buffer(0, self.verts.slice(..));
        let mut at = 0u32;
        for m in world.meshes {
            let n = m.verts.len() as u32;
            if n > 0 {
                pass.set_bind_group(1, self.bind(&m.image, m.wrap), &[]);
                pass.draw(at..at + n, 0..1);
            }
            at += n;
        }
    }

    /// Renders a frame (2D behind, the 3D world) into a picture called `name` that later quads and meshes can use: a
    /// rear-view mirror, a screen in the world. Submitted at once, so a frame drawn after it sees it; the picture must
    /// not appear in its own `world` (a target cannot be read while it is drawn into).
    pub fn render_into(&mut self, name: &str, w: u32, h: u32, back: &[Quad], world: World3) {
        if self.offscreen.get(name).is_none_or(|t| (t.0, t.1) != (w, h)) {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(name),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind = |s: &wgpu::Sampler| {
                self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(name),
                    layout: &self.tex_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(s) },
                    ],
                })
            };
            let tex = Tex(std::array::from_fn(|i| bind(&self.samplers[i])));
            self.textures.insert(name.to_string(), tex);
            self.sizes.insert(name.to_string(), (w, h));
            self.offscreen.insert(name.to_string(), (w, h, view));
        }
        let view = self.offscreen[name].2.clone();
        self.render(&view, w, h, back, Some(world), &[]);
    }

    /// Keeps meshes on the GPU under `slot` until `version` changes: uploaded once, drawn by every frame that lists the
    /// slot in `World3::kept`. For terrain: rebuilt when it changes, not copied every frame.
    pub fn keep(&mut self, slot: u32, version: u64, meshes: &[Mesh]) {
        if self.kept.get(&slot).is_some_and(|k| k.version == version) {
            return;
        }
        let verts = to_linear(meshes);
        let buf = (!verts.is_empty()).then(|| {
            let b = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("kept"),
                size: std::mem::size_of_val(verts.as_slice()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.queue.write_buffer(&b, 0, bytemuck::cast_slice(&verts));
            b
        });
        let mut at = 0u32;
        let draws = meshes
            .iter()
            .map(|m| {
                let n = m.verts.len() as u32;
                at += n;
                (m.image.clone(), m.wrap, at - n, n)
            })
            .collect();
        self.kept.insert(slot, Kept { version, buf, draws });
    }

    /// Uploads a model for `World3::models` (textures of named materials may be replaced by `overrides`).
    /// Without storage buffers (WebGL2) there are no skinned models: the call is ignored (`models_supported`).
    pub fn upload_model(&mut self, name: &str, model: &crate::model::Model, overrides: &BTreeMap<String, crate::model::Texture>) {
        if !self.models_supported() {
            return;
        }
        let skin = self
            .skin
            .get_or_insert_with(|| crate::skin::Skin::new(&self.device, &self.queue, self.format, &self.shadow_view, &self.shadow_sampler));
        skin.upload(&self.device, &self.queue, name, model, overrides);
    }

    pub fn models_supported(&self) -> bool {
        self.device.limits().max_storage_buffers_per_shader_stage > 0
    }

    /// Renders offscreen and reads the picture back (screenshots, evals).
    pub fn shot(&mut self, w: u32, h: u32, quads: &[Quad]) -> Image {
        self.shot_scene(w, h, quads, None, &[])
    }

    /// `shot` of a whole frame (2D behind, 3D, 2D in front).
    pub fn shot_scene(&mut self, w: u32, h: u32, back: &[Quad], world: Option<World3>, front: &[Quad]) -> Image {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shot"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        self.render(&texture.create_view(&wgpu::TextureViewDescriptor::default()), w, h, back, world, front);
        let row = (w * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("readback") });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit(Some(enc.finish()));
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let data = slice.get_mapped_range();
        let bgr = matches!(self.format, wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Bgra8Unorm);
        let mut px = Vec::with_capacity((w * h) as usize);
        for y in 0..h as usize {
            for x in 0..w as usize {
                let p = &data[y * row as usize + x * 4..][..4];
                px.push(if bgr { [p[2], p[1], p[0], 255] } else { [p[0], p[1], p[2], 255] });
            }
        }
        Image { w: w as usize, h: h as usize, px }
    }
}
