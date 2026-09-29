//! Models on the GPU, for crowds: a model's mesh, textures and baked clip palettes are uploaded once; each character
//! is then one `Instance` (where it is, which two frames it blends, a tint). The vertex shader skins with the
//! palette; the fragment shader lights with the model's PBR maps (base colour, normal, occlusion-roughness-metal),
//! a sun, a sky and ground ambient, wrap lighting and a transmitted back-light (thin, pale bodies glow against the sun).

use std::collections::BTreeMap;

use crate::model::{Model, Texture};

const SHADER: &str = r#"
struct G {
    view_proj: mat4x4<f32>,
    fog: vec4<f32>,
    eye: vec4<f32>,
    range: vec4<f32>,
    sun_dir: vec4<f32>,
    sun: vec4<f32>,
    sky: vec4<f32>,
    ground: vec4<f32>,
    light_vp: mat4x4<f32>,
    shadow: vec4<f32>,
};
@group(0) @binding(0) var<uniform> g: G;
@group(0) @binding(1) var<storage, read> palette: array<vec4<f32>>;
@group(0) @binding(2) var shadow_map: texture_depth_2d;
@group(0) @binding(3) var shadow_samp: sampler_comparison;

struct M { base: vec4<f32>, pbr: vec4<f32> };
@group(1) @binding(0) var base_t: texture_2d<f32>;
@group(1) @binding(1) var normal_t: texture_2d<f32>;
@group(1) @binding(2) var orm_t: texture_2d<f32>;
@group(1) @binding(3) var samp: sampler;
@group(1) @binding(4) var<uniform> mat: M;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) joints: vec4<u32>,
    @location(4) weights: vec4<f32>,
    @location(5) m0: vec4<f32>,
    @location(6) m1: vec4<f32>,
    @location(7) m2: vec4<f32>,
    @location(8) frames: vec4<u32>,
    @location(9) params: vec4<f32>,
    @location(10) tint: vec4<f32>,
};
struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tint: vec4<f32>,
    @location(4) fog: f32,
};

fn apply(r0: vec4<f32>, r1: vec4<f32>, r2: vec4<f32>, p: vec4<f32>) -> vec3<f32> {
    return vec3<f32>(dot(r0, p), dot(r1, p), dot(r2, p));
}

fn skinned(v: VIn) -> array<vec3<f32>, 2> {
    var p = vec3<f32>(0.0);
    var n = vec3<f32>(0.0);
    let t = v.params.x;
    for (var k = 0u; k < 4u; k = k + 1u) {
        let w = v.weights[k];
        if (w > 0.0) {
            let a = v.frames.x + v.joints[k] * 3u;
            let b = v.frames.y + v.joints[k] * 3u;
            let r0 = mix(palette[a], palette[b], t);
            let r1 = mix(palette[a + 1u], palette[b + 1u], t);
            let r2 = mix(palette[a + 2u], palette[b + 2u], t);
            p = p + w * apply(r0, r1, r2, vec4<f32>(v.pos, 1.0));
            n = n + w * apply(r0, r1, r2, vec4<f32>(v.normal, 0.0));
        }
    }
    return array<vec3<f32>, 2>(apply(v.m0, v.m1, v.m2, vec4<f32>(p, 1.0)), apply(v.m0, v.m1, v.m2, vec4<f32>(n, 0.0)));
}

@vertex
fn vs_shadow(v: VIn) -> @builtin(position) vec4<f32> {
    return g.light_vp * vec4<f32>(skinned(v)[0], 1.0);
}

@vertex
fn vs(v: VIn) -> VOut {
    let s = skinned(v);
    let world = s[0];
    var o: VOut;
    o.clip = g.view_proj * vec4<f32>(world, 1.0);
    o.world = world;
    o.normal = normalize(s[1]);
    o.uv = v.uv;
    o.tint = v.tint;
    o.fog = select(0.0, clamp((distance(world, g.eye.xyz) - g.range.x) / g.range.y, 0.0, 1.0), g.range.y > 0.0);
    return o;
}

// A normal map without stored tangents: the tangent frame from screen-space derivatives (Schüler, 2013).
fn perturb(n: vec3<f32>, p: vec3<f32>, uv: vec2<f32>, tn: vec3<f32>) -> vec3<f32> {
    let dp1 = dpdx(p);
    let dp2 = dpdy(p);
    let duv1 = dpdx(uv);
    let duv2 = dpdy(uv);
    let dp2perp = cross(dp2, n);
    let dp1perp = cross(n, dp1);
    let t = dp2perp * duv1.x + dp1perp * duv2.x;
    let b = dp2perp * duv1.y + dp1perp * duv2.y;
    let invmax = inverseSqrt(max(dot(t, t), dot(b, b)) + 1e-12);
    return normalize(mat3x3<f32>(t * invmax, b * invmax, n) * tn);
}

const PI: f32 = 3.14159265;

fn sunlit(p: vec3<f32>) -> f32 {
    if (g.shadow.x < 0.5) {
        return 1.0;
    }
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
fn fs(v: VOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let texel = textureSample(base_t, samp, v.uv);
    let base = texel.rgb * mat.base.rgb * v.tint.rgb;
    var n = normalize(v.normal);
    if (!front) {
        n = -n;
    }
    if (mat.pbr.z > 0.5) {
        let tn = textureSample(normal_t, samp, v.uv).xyz * 2.0 - 1.0;
        n = perturb(n, v.world, v.uv, tn);
    }
    var ao = 1.0;
    var rough = mat.pbr.y;
    var metal = mat.pbr.x;
    if (mat.pbr.w > 0.5) {
        let orm = textureSample(orm_t, samp, v.uv);
        ao = mix(1.0, orm.r, mat.base.a);
        rough = rough * orm.g;
        metal = metal * orm.b;
    }
    rough = clamp(rough, 0.05, 1.0);
    let l = normalize(g.sun_dir.xyz);
    let view = normalize(g.eye.xyz - v.world);
    let h = normalize(l + view);
    let ndl = dot(n, l);
    let ndv = max(dot(n, view), 1e-3);
    let ndh = max(dot(n, h), 0.0);
    // Wrap lighting: light reaches a little past the terminator, as through a thin cuticle.
    let wrap = max((ndl + 0.3) / 1.3, 0.0);
    // GGX with Smith visibility and Schlick Fresnel.
    let a = rough * rough;
    let a2 = a * a;
    let d = a2 / (PI * pow(ndh * ndh * (a2 - 1.0) + 1.0, 2.0));
    let k = (rough + 1.0) * (rough + 1.0) / 8.0;
    let vis = 1.0 / ((max(ndl, 0.0) * (1.0 - k) + k) * (ndv * (1.0 - k) + k) + 1e-4);
    let f0 = mix(vec3<f32>(0.04), base, metal);
    let fres = f0 + (1.0 - f0) * pow(1.0 - max(dot(h, view), 0.0), 5.0);
    let spec = d * vis * fres * max(ndl, 0.0) * 0.25;
    let diffuse = base * (1.0 - metal) * wrap;
    // Light through the body: seen against the sun, pale tissue glows.
    let through = base * pow(max(dot(view, -l), 0.0), 4.0) * (1.0 - max(ndl, 0.0)) * 0.35;
    let ambient = mix(g.ground.rgb, g.sky.rgb, n.y * 0.5 + 0.5) * base * (1.0 - metal * 0.5) * ao;
    let lit = mix(1.0 - g.shadow.y, 1.0, sunlit(v.world));
    var rgb = ambient + (diffuse + spec + through) * g.sun.rgb * lit;
    let toward = pow(max(dot(normalize(v.world - g.eye.xyz), l), 0.0), 6.0) * g.sun_dir.w;
    rgb = mix(rgb, mix(g.fog.rgb, min(g.sun.rgb, vec3<f32>(1.0)), clamp(toward, 0.0, 1.0)), v.fog);
    return vec4<f32>(rgb, 1.0);
}
"#;

/// One character: its world transform (the top three rows of an affine matrix), the palette offsets of the two
/// frames it blends (in vec4s), the blend, and a tint (rgb, 1 = none).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    pub m: [[f32; 4]; 3],
    pub frames: [u32; 4],
    pub params: [f32; 4],
    pub tint: [f32; 4],
}

/// Characters of one model to draw this frame.
#[derive(Clone, Debug, Default)]
pub struct ModelDraw {
    pub model: String,
    pub instances: Vec<Instance>,
}

/// The light of a scene: where the sun is (towards it), its colour, the sky and the ground (linear light).
#[derive(Clone, Copy, Debug)]
pub struct Light {
    pub sun_dir: [f32; 3],
    pub sun: [f32; 3],
    pub sky: [f32; 3],
    pub ground: [f32; 3],
    /// Light scattered towards the sun in the fog (0 = none).
    pub haze: f32,
}

impl Default for Light {
    fn default() -> Self {
        Light { sun_dir: [0.4, 0.8, 0.3], sun: [3.0, 2.8, 2.5], sky: [0.45, 0.55, 0.7], ground: [0.25, 0.18, 0.12], haze: 0.0 }
    }
}

/// Where each clip's frames start in the palette buffer (in vec4s), and how many slots a frame has.
#[derive(Clone, Debug)]
pub struct Frames {
    pub slots: usize,
    pub clips: BTreeMap<String, (u32, usize, f32)>,
}

impl Frames {
    /// The palette layout of a model: every clip's frames one after another (in name order), each matrix as three
    /// vec4 rows. `Skin::upload` writes it; views use it to pick frames. One definition, so they cannot disagree.
    pub fn of(m: &Model) -> Frames {
        let mut at = 0u32;
        let clips = m
            .clips
            .iter()
            .map(|(name, c)| {
                let start = at;
                at += (c.frames.len() * m.slots * 3) as u32;
                (name.clone(), (start, c.frames.len(), c.fps))
            })
            .collect();
        Frames { slots: m.slots, clips }
    }

    /// Like `at`, as frame numbers of the clip (for reading its matrices on the CPU: sockets).
    pub fn frame_numbers(&self, clip: &str, t: f32) -> (usize, usize, f32) {
        let Some(&(_, count, fps)) = self.clips.get(clip).or_else(|| self.clips.get("rest")) else { return (0, 0, 0.0) };
        if count <= 1 {
            return (0, 0, 0.0);
        }
        let f = (t * fps).rem_euclid((count - 1) as f32);
        (f.floor() as usize, (f.floor() as usize + 1) % count, f.fract())
    }

    /// The two frames (palette offsets) and the blend for `clip` at time `t` seconds, looping. Unknown → rest.
    pub fn at(&self, clip: &str, t: f32) -> ([u32; 2], f32) {
        let Some(&(start, count, fps)) = self.clips.get(clip).or_else(|| self.clips.get("rest")) else { return ([0, 0], 0.0) };
        if count <= 1 {
            return ([start, start], 0.0);
        }
        // The last frame equals the first in a looping clip: loop over count - 1.
        let span = (count - 1) as f32;
        let f = (t * fps).rem_euclid(span);
        let (i, blend) = (f.floor() as usize, f.fract());
        let stride = (self.slots * 3) as u32;
        ([start + i as u32 * stride, start + ((i + 1) % count) as u32 * stride], blend)
    }
}

struct GpuPart {
    first: u32,
    count: u32,
    base_vertex: i32,
    material: wgpu::BindGroup,
}

struct GpuModel {
    verts: wgpu::Buffer,
    indices: wgpu::Buffer,
    parts: Vec<GpuPart>,
    palette: wgpu::Buffer,
    globals: wgpu::BindGroup,
    shadow_globals: wgpu::BindGroup,
}

pub struct Skin {
    pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    shadow_view: wgpu::TextureView,
    shadow_sampler: wgpu::Sampler,
    shadow_globals_layout: wgpu::BindGroupLayout,
    globals: wgpu::Buffer,
    globals_layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    models: BTreeMap<String, GpuModel>,
    pub frames: BTreeMap<String, Frames>,
    instances: wgpu::Buffer,
    capacity: usize,
    white: wgpu::TextureView,
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

/// The mip chain of an RGBA8 image (box filter).
fn mip_chain(t: &Texture) -> Vec<(u32, u32, Vec<u8>)> {
    let mut out = vec![(t.w, t.h, t.rgba.clone())];
    while out.last().is_some_and(|(w, h, _)| *w > 1 || *h > 1) {
        let (w, h, px) = out.last().expect("not empty");
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let at = |xx: u32, yy: u32| px[((yy.min(h - 1) * w + xx.min(w - 1)) * 4 + c) as usize] as u32;
                    let s = at(2 * x, 2 * y) + at(2 * x + 1, 2 * y) + at(2 * x, 2 * y + 1) + at(2 * x + 1, 2 * y + 1);
                    next[((y * nw + x) * 4 + c) as usize] = (s / 4) as u8;
                }
            }
        }
        out.push((nw, nh, next));
    }
    out
}

fn upload_texture(device: &wgpu::Device, queue: &wgpu::Queue, t: &Texture, srgb: bool) -> wgpu::TextureView {
    let levels = mip_chain(t);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("model texture"),
        size: wgpu::Extent3d { width: t.w, height: t.h, depth_or_array_layers: 1 },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: if srgb { wgpu::TextureFormat::Rgba8UnormSrgb } else { wgpu::TextureFormat::Rgba8Unorm },
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, (w, h, data)) in levels.iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(*h) },
            wgpu::Extent3d { width: *w, height: *h, depth_or_array_layers: 1 },
        );
    }
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

impl Skin {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        shadow_view: &wgpu::TextureView,
        shadow_sampler: &wgpu::Sampler,
    ) -> Skin {
        let shader = device
            .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("skin"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("skin globals"),
            entries: &[
                uniform_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("skin material"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                texture_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                uniform_entry(4),
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("skin"),
            bind_group_layouts: &[Some(&globals_layout), Some(&material_layout)],
            immediate_size: 0,
        });
        let vert_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Uint32x4, 4 => Float32x4];
        let inst_attrs =
            wgpu::vertex_attr_array![5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Uint32x4, 9 => Float32x4, 10 => Float32x4];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("skin"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<crate::model::ModelVert>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &vert_attrs,
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Instance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &inst_attrs,
                    },
                ],
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
        // Models cast shadows too: the same skinning, depth only, from the sun.
        // The shadow pass writes the shadow map: its bind group holds only the globals and the palette.
        let shadow_globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("skin shadow globals"),
            entries: &[
                uniform_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("skin shadow"),
            bind_group_layouts: &[Some(&shadow_globals_layout)],
            immediate_size: 0,
        });
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("skin shadow"),
            layout: Some(&shadow_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_shadow"),
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<crate::model::ModelVert>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &vert_attrs,
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Instance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &inst_attrs,
                    },
                ],
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
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("skin globals"),
            size: 256,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("skin"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let white = upload_texture(device, queue, &Texture { w: 1, h: 1, rgba: vec![255; 4] }, false);
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instances"),
            size: 1024 * std::mem::size_of::<Instance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Skin {
            pipeline,
            shadow_pipeline,
            shadow_view: shadow_view.clone(),
            shadow_sampler: shadow_sampler.clone(),
            shadow_globals_layout,
            globals,
            globals_layout,
            material_layout,
            sampler,
            models: BTreeMap::new(),
            frames: BTreeMap::new(),
            instances,
            capacity: 1024,
            white,
        }
    }

    pub fn has(&self, name: &str) -> bool {
        self.models.contains_key(name)
    }

    /// Uploads a model under `name`: mesh, textures (a material's image can be replaced by one from the game's
    /// assets: `overrides` by material name), and every clip's palettes.
    pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, name: &str, m: &Model, overrides: &BTreeMap<String, Texture>) {
        use wgpu::util::DeviceExt;
        let mut verts = Vec::new();
        let mut indices = Vec::new();
        let mut spans = Vec::new();
        for p in &m.parts {
            spans.push((indices.len() as u32, p.indices.len() as u32, verts.len() as i32, p.material));
            verts.extend_from_slice(&p.verts);
            indices.extend_from_slice(&p.indices);
        }
        let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(name),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(name),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        // The palette, laid out as `Frames::of` says.
        let mut palette: Vec<[f32; 4]> = Vec::new();
        for c in m.clips.values() {
            for frame in &c.frames {
                for mat in frame {
                    palette.extend((0..3).map(|r| [mat[0][r], mat[1][r], mat[2][r], mat[3][r]]));
                }
            }
        }
        let pbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(name),
            contents: bytemuck::cast_slice(&palette),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let globals = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(name),
            layout: &self.globals_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.globals.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: pbuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.shadow_view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.shadow_sampler) },
            ],
        });
        let mut views: BTreeMap<(usize, bool), wgpu::TextureView> = BTreeMap::new();
        let mut view = |i: Option<usize>, srgb: bool, over: Option<&Texture>| -> Option<wgpu::TextureView> {
            if let Some(t) = over {
                return Some(upload_texture(device, queue, t, srgb));
            }
            let i = i?;
            Some(views.entry((i, srgb)).or_insert_with(|| upload_texture(device, queue, &m.textures[i], srgb)).clone())
        };
        let parts = spans
            .into_iter()
            .map(|(first, count, base_vertex, mi)| {
                let mat = &m.materials[mi];
                let base = view(mat.base_tex, true, overrides.get(&mat.name));
                let normal = view(mat.normal_tex, false, None);
                let orm = view(mat.mr_tex.or(mat.occlusion_tex), false, None);
                // base.a = how much of the ORM map's red is occlusion (only when the material says so).
                let ao = if mat.occlusion_tex.is_some() { 1.0 } else { 0.0 };
                let uniform: [f32; 8] = [
                    mat.base_color[0],
                    mat.base_color[1],
                    mat.base_color[2],
                    ao,
                    mat.metallic,
                    mat.roughness,
                    normal.is_some() as u8 as f32,
                    orm.is_some() as u8 as f32,
                ];
                let ubuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("material"),
                    contents: bytemuck::cast_slice(&uniform),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                let material = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("material"),
                    layout: &self.material_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(base.as_ref().unwrap_or(&self.white)),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(normal.as_ref().unwrap_or(&self.white)),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(orm.as_ref().unwrap_or(&self.white)),
                        },
                        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                        wgpu::BindGroupEntry { binding: 4, resource: ubuf.as_entire_binding() },
                    ],
                });
                GpuPart { first, count, base_vertex, material }
            })
            .collect();
        let shadow_globals = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(name),
            layout: &self.shadow_globals_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.globals.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: pbuf.as_entire_binding() },
            ],
        });
        self.models.insert(name.to_string(), GpuModel { verts: vbuf, indices: ibuf, parts, palette: pbuf, globals, shadow_globals });
        self.frames.insert(name.to_string(), Frames::of(m));
    }

    /// Writes this frame's globals and instances; call before the pass.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view_proj: [[f32; 4]; 4],
        fog: [f32; 4],
        eye: [f32; 3],
        range: [f32; 2],
        light: Light,
        draws: &[ModelDraw],
        light_vp: [[f32; 4]; 4],
        shadow: [f32; 4],
    ) {
        let mut g: Vec<f32> = view_proj.iter().flatten().copied().collect();
        g.extend_from_slice(&fog);
        g.extend_from_slice(&[eye[0], eye[1], eye[2], 0.0, range[0], range[1], 0.0, 0.0]);
        let d = light.sun_dir;
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-6);
        g.extend_from_slice(&[d[0] / len, d[1] / len, d[2] / len, light.haze]);
        for c in [light.sun, light.sky, light.ground] {
            g.extend_from_slice(&[c[0], c[1], c[2], 0.0]);
        }
        g.extend(light_vp.iter().flatten().copied());
        g.extend_from_slice(&shadow);
        queue.write_buffer(&self.globals, 0, bytemuck::cast_slice(&g));
        let all: Vec<Instance> = draws.iter().flat_map(|d| d.instances.iter().copied()).collect();
        if all.len() > self.capacity {
            self.capacity = all.len().next_power_of_two();
            self.instances = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("instances"),
                size: (self.capacity * std::mem::size_of::<Instance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !all.is_empty() {
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&all));
        }
    }

    /// Their depth from the sun (inside the shadow pass, after `prepare`).
    pub fn draw_shadow(&self, pass: &mut wgpu::RenderPass, draws: &[ModelDraw]) {
        pass.set_pipeline(&self.shadow_pipeline);
        pass.set_vertex_buffer(1, self.instances.slice(..));
        let mut at = 0u32;
        for d in draws {
            let n = d.instances.len() as u32;
            if let (Some(m), true) = (self.models.get(&d.model), n > 0) {
                pass.set_bind_group(0, &m.shadow_globals, &[]);
                pass.set_vertex_buffer(0, m.verts.slice(..));
                pass.set_index_buffer(m.indices.slice(..), wgpu::IndexFormat::Uint32);
                for p in &m.parts {
                    pass.draw_indexed(p.first..p.first + p.count, p.base_vertex, at..at + n);
                }
            }
            at += n;
        }
    }

    /// Draws them (inside the 3D pass, after `prepare`): one instanced draw per model part.
    pub fn draw(&self, pass: &mut wgpu::RenderPass, draws: &[ModelDraw]) {
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(1, self.instances.slice(..));
        let mut at = 0u32;
        for d in draws {
            let n = d.instances.len() as u32;
            if let (Some(m), true) = (self.models.get(&d.model), n > 0) {
                pass.set_bind_group(0, &m.globals, &[]);
                pass.set_vertex_buffer(0, m.verts.slice(..));
                pass.set_index_buffer(m.indices.slice(..), wgpu::IndexFormat::Uint32);
                for p in &m.parts {
                    pass.set_bind_group(1, &p.material, &[]);
                    pass.draw_indexed(p.first..p.first + p.count, p.base_vertex, at..at + n);
                }
                let _ = &m.palette;
            }
            at += n;
        }
    }
}
