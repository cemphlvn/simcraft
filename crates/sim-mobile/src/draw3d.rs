//! A small 3D renderer for the phone: unit meshes (cube, sphere, cylinder, cone) drawn as instances, lit by a sun
//! with a shadow map and a sky/ground hemisphere, with distance fog and 4× MSAA where the GPU offers it. The look
//! is a casual game's toy look: soft wrap lighting, a bright chamfer on every box edge (computed in the shader from
//! the box's size, so a block looks bevelled without a bevelled mesh), and a rim of sky light.
//!
//! A [`Scene3`] is plain data (a camera, a sun, instances); the 2D layers are drawn over it afterwards.

use sim_physics::rigid::{Quat, V3};

/// The meshes every instance picks from; each spans −1..1 on its local axes (scale is the half size).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mesh {
    Cube,
    Sphere,
    /// Round in x and z, height along y.
    Cylinder,
    /// Base at y = −1, tip at y = +1.
    Cone,
}

const MESHES: [Mesh; 4] = [Mesh::Cube, Mesh::Sphere, Mesh::Cylinder, Mesh::Cone];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inst3 {
    pub mesh: Mesh,
    pub pos: V3,
    pub rot: Quat,
    /// Half size along the local axes.
    pub half: V3,
    /// Linear RGB.
    pub color: [f32; 3],
    /// Width of the bright chamfer on box edges (m); 0 for none.
    pub bevel: f32,
    /// Glow: 0 lit normally, 1 its own colour regardless of light.
    pub glow: f32,
    /// Casts a shadow.
    pub shadow: bool,
}

impl Inst3 {
    pub fn new(mesh: Mesh, pos: V3, half: V3, color: [f32; 3]) -> Inst3 {
        Inst3 { mesh, pos, rot: Quat::IDENTITY, half, color, bevel: 0.0, glow: 0.0, shadow: true }
    }

    pub fn rot(mut self, rot: Quat) -> Inst3 {
        self.rot = rot;
        self
    }

    pub fn bevel(mut self, w: f32) -> Inst3 {
        self.bevel = w;
        self
    }

    pub fn glow(mut self, g: f32) -> Inst3 {
        self.glow = g;
        self
    }

    pub fn no_shadow(mut self) -> Inst3 {
        self.shadow = false;
        self
    }
}

/// A perspective camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub eye: V3,
    pub target: V3,
    /// Roll comes from tilting `up`.
    pub up: V3,
    /// Vertical field of view (radians).
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
}

impl Camera {
    pub fn view_proj(&self, aspect: f32) -> M4 {
        M4::perspective(self.fov_y, aspect, self.near, self.far).mul(&M4::look_at(self.eye, self.target, self.up))
    }

    /// Where world point `p` lands on a `w` × `h` screen (pixels, y down), and whether it is in front.
    pub fn project(&self, p: V3, w: f32, h: f32) -> (f32, f32, bool) {
        let c = self.view_proj(w / h).mul_v4([p.x, p.y, p.z, 1.0]);
        let inv = 1.0 / c[3].abs().max(1e-6);
        ((c[0] * inv * 0.5 + 0.5) * w, (0.5 - c[1] * inv * 0.5) * h, c[3] > 0.0)
    }

    /// The ray from the eye through screen point (`x`, `y`) of a `w` × `h` screen: its unit direction.
    pub fn ray(&self, x: f32, y: f32, w: f32, h: f32) -> V3 {
        let f = (self.target - self.eye).normalized();
        let r = f.cross(self.up).normalized();
        let u = r.cross(f);
        let t = (self.fov_y * 0.5).tan();
        let nx = (x / w * 2.0 - 1.0) * t * w / h;
        let ny = (1.0 - y / h * 2.0) * t;
        (f + r * nx + u * ny).normalized()
    }
}

/// What a frame shows in 3D.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene3 {
    pub camera: Camera,
    /// Towards the sun (unit).
    pub sun: V3,
    pub sun_color: [f32; 3],
    pub sky_top: [f32; 3],
    pub sky_horizon: [f32; 3],
    /// Light bounced up from the ground (the hemisphere's lower half).
    pub ground: [f32; 3],
    pub fog: [f32; 3],
    /// Fog starts and is full at these distances from the eye (m).
    pub fog_near: f32,
    pub fog_far: f32,
    /// The shadow map covers a ball of `shadow_radius` about `shadow_center`.
    pub shadow_center: V3,
    pub shadow_radius: f32,
    /// A flash over the whole picture (impact), 0..1, and its colour.
    pub flash: f32,
    pub instances: Vec<Inst3>,
}

/// A 4 × 4 matrix by columns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct M4(pub [[f32; 4]; 4]);

impl M4 {
    pub fn mul(&self, o: &M4) -> M4 {
        let mut r = [[0.0; 4]; 4];
        for (c, col) in r.iter_mut().enumerate() {
            *col = self.mul_v4(o.0[c]);
        }
        M4(r)
    }

    pub fn mul_v4(&self, v: [f32; 4]) -> [f32; 4] {
        let m = &self.0;
        let mut r = [0.0; 4];
        for (i, out) in r.iter_mut().enumerate() {
            *out = m[0][i] * v[0] + m[1][i] * v[1] + m[2][i] * v[2] + m[3][i] * v[3];
        }
        r
    }

    /// Right-handed, looking down −z in view space.
    pub fn look_at(eye: V3, target: V3, up: V3) -> M4 {
        let f = (target - eye).normalized();
        let s = f.cross(up).normalized();
        let u = s.cross(f);
        M4([[s.x, u.x, -f.x, 0.0], [s.y, u.y, -f.y, 0.0], [s.z, u.z, -f.z, 0.0], [-s.dot(eye), -u.dot(eye), f.dot(eye), 1.0]])
    }

    /// Depth 0 (near) to 1 (far), as wgpu wants it.
    pub fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> M4 {
        let f = 1.0 / (fov_y * 0.5).tan();
        let r = far / (near - far);
        M4([[f / aspect, 0.0, 0.0, 0.0], [0.0, f, 0.0, 0.0], [0.0, 0.0, r, -1.0], [0.0, 0.0, r * near, 0.0]])
    }

    /// A box `±w × ±h`, depth `near..far`, depth 0..1.
    pub fn ortho(w: f32, h: f32, near: f32, far: f32) -> M4 {
        let r = 1.0 / (near - far);
        M4([[1.0 / w, 0.0, 0.0, 0.0], [0.0, 1.0 / h, 0.0, 0.0], [0.0, 0.0, r, 0.0], [0.0, 0.0, r * near, 1.0]])
    }

    fn flat(&self) -> [f32; 16] {
        let mut o = [0.0; 16];
        for c in 0..4 {
            o[c * 4..c * 4 + 4].copy_from_slice(&self.0[c]);
        }
        o
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 3],
    normal: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Inst {
    /// Rotation times scale, by columns, and the position.
    m0: [f32; 4],
    m1: [f32; 4],
    m2: [f32; 4],
    pos: [f32; 4],
    /// rgb, bevel.
    color: [f32; 4],
    /// Half size, glow.
    half: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    view_proj: [f32; 16],
    light: [f32; 16],
    inv_view_proj: [f32; 16],
    eye: [f32; 4],
    sun: [f32; 4],
    sun_color: [f32; 4],
    sky_top: [f32; 4],
    sky_horizon: [f32; 4],
    ground: [f32; 4],
    /// rgb, flash.
    fog: [f32; 4],
    /// near, far, shadow texel, 0.
    params: [f32; 4],
}

const SHADOW: u32 = 2048;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

const SHADER: &str = r#"
struct G {
    view_proj: mat4x4<f32>,
    light: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    sun: vec4<f32>,
    sun_color: vec4<f32>,
    sky_top: vec4<f32>,
    sky_horizon: vec4<f32>,
    ground: vec4<f32>,
    fog: vec4<f32>,
    params: vec4<f32>,
};
@group(0) @binding(0) var<uniform> g: G;
@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_cmp: sampler_comparison;

struct I {
    @location(2) m0: vec4<f32>,
    @location(3) m1: vec4<f32>,
    @location(4) m2: vec4<f32>,
    @location(5) pos: vec4<f32>,
    @location(6) color: vec4<f32>,
    @location(7) size: vec4<f32>,
};

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) local: vec3<f32>,
    @location(3) color: vec4<f32>,
    @location(4) size: vec4<f32>,
};

fn model(i: I) -> mat3x3<f32> {
    return mat3x3<f32>(i.m0.xyz, i.m1.xyz, i.m2.xyz);
}

@vertex
fn vs(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, i: I) -> Out {
    let m = model(i);
    let w = m * p + i.pos.xyz;
    var o: Out;
    o.clip = g.view_proj * vec4<f32>(w, 1.0);
    o.world = w;
    // The inverse transpose of rotation × scale: n / scale², then turned.
    let s = max(i.size.xyz, vec3<f32>(1e-4));
    o.normal = normalize(m * (n / (s * s)));
    o.local = p * i.size.xyz;
    o.color = i.color;
    o.size = i.size;
    return o;
}

@vertex
fn vs_shadow(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, i: I) -> @builtin(position) vec4<f32> {
    return g.light * vec4<f32>(model(i) * p + i.pos.xyz, 1.0);
}

fn shadow(world: vec3<f32>, n: vec3<f32>) -> f32 {
    let texel = g.params.z;
    // Pushed out along the normal so a face never shadows itself.
    let lp = g.light * vec4<f32>(world + n * texel * 1.5, 1.0);
    let uv = vec2<f32>(lp.x * 0.5 + 0.5, 0.5 - lp.y * 0.5);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || lp.z > 1.0) {
        return 1.0;
    }
    let d = lp.z - 0.0015;
    let px = 1.0 / f32(textureDimensions(shadow_map).x);
    var sum = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            sum += textureSampleCompareLevel(shadow_map, shadow_cmp, uv + vec2<f32>(f32(x), f32(y)) * px, d);
        }
    }
    return sum / 9.0;
}

fn sky(dir: vec3<f32>) -> vec3<f32> {
    let up = clamp(dir.y, 0.0, 1.0);
    var c = mix(g.sky_horizon.rgb, g.sky_top.rgb, pow(up, 0.6));
    let sd = max(dot(dir, g.sun.xyz), 0.0);
    c += g.sun_color.rgb * (pow(sd, 400.0) * 3.0 + pow(sd, 12.0) * 0.18);
    if (dir.y < 0.0) {
        c = mix(g.sky_horizon.rgb, g.fog.rgb, clamp(-dir.y * 8.0, 0.0, 1.0));
    }
    return c;
}

@fragment
fn fs(i: Out) -> @location(0) vec4<f32> {
    let n = normalize(i.normal);
    var albedo = i.color.rgb;
    let bevel = i.color.a;
    if (bevel > 0.0) {
        // Distance to the nearest edge: on a face, the smaller of the two in-face distances to its border.
        let d = i.size.xyz - abs(i.local);
        let lo = min(d.x, min(d.y, d.z));
        let hi = max(d.x, max(d.y, d.z));
        let mid = d.x + d.y + d.z - lo - hi;
        let e = clamp(mid / bevel, 0.0, 1.0);
        albedo = albedo * mix(1.35, 1.0, smoothstep(0.0, 1.0, e)) * mix(0.82, 1.0, smoothstep(0.0, 0.25, e));
    }
    let v = normalize(g.eye.xyz - i.world);
    let l = g.sun.xyz;
    let ndl = dot(n, l);
    let wrap = clamp((ndl + 0.35) / 1.35, 0.0, 1.0);
    let sh = shadow(i.world, n);
    let hemi = mix(g.ground.rgb, g.sky_top.rgb * 0.9 + g.sky_horizon.rgb * 0.3, n.y * 0.5 + 0.5);
    let h = normalize(l + v);
    let spec = pow(max(dot(n, h), 0.0), 48.0) * 0.35 * sh * step(0.0, ndl);
    let rim = pow(1.0 - max(dot(n, v), 0.0), 3.0) * 0.35;
    var c = albedo * (hemi * 0.55 + g.sun_color.rgb * wrap * mix(0.35, 1.0, sh)) + g.sun_color.rgb * spec + g.sky_horizon.rgb * rim * 0.6;
    c = mix(c, i.color.rgb * 1.2, i.size.w);
    let dist = length(g.eye.xyz - i.world);
    let f = smoothstep(g.params.x, g.params.y, dist);
    c = mix(c, g.fog.rgb, f);
    c = mix(c, vec3<f32>(1.0, 0.97, 0.9), g.fog.w);
    return vec4<f32>(c, 1.0);
}

struct SkyOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_sky(@builtin(vertex_index) vi: u32) -> SkyOut {
    let xy = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u)) * 2.0 - 1.0;
    var o: SkyOut;
    o.clip = vec4<f32>(xy, 1.0, 1.0);
    o.ndc = xy;
    return o;
}

@fragment
fn fs_sky(i: SkyOut) -> @location(0) vec4<f32> {
    let far = g.inv_view_proj * vec4<f32>(i.ndc, 1.0, 1.0);
    let dir = normalize(far.xyz / far.w - g.eye.xyz);
    let c = mix(sky(dir), vec3<f32>(1.0, 0.97, 0.9), g.fog.w);
    return vec4<f32>(c, 1.0);
}
"#;

/// Where each mesh sits in the shared vertex and index buffers.
#[derive(Clone, Copy, Debug)]
struct Range {
    first: u32,
    count: u32,
    base: i32,
}

pub struct Renderer3 {
    pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    sky_pipeline: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    bind: wgpu::BindGroup,
    shadow_bind: wgpu::BindGroup,
    shadow_view: wgpu::TextureView,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    ranges: [Range; 4],
    instances: wgpu::Buffer,
    capacity: usize,
    samples: u32,
    /// The multisampled colour and the depth targets, remade when the size changes.
    targets: Option<(u32, u32, Option<wgpu::TextureView>, wgpu::TextureView)>,
    format: wgpu::TextureFormat,
}

impl Renderer3 {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, samples: u32) -> Renderer3 {
        let shader = device
            .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("3d"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("3d"),
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
        // The shadow pass sees only the uniforms: a pass may not sample the texture it writes.
        let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("3d shadow"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("3d globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let shadow_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow map"),
            size: wgpu::Extent3d { width: SHADOW, height: SHADOW, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_view = shadow_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let cmp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("3d"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&shadow_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&cmp) },
            ],
        });
        let shadow_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("3d shadow"),
            layout: &shadow_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });
        let main_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("3d"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shadow_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("3d shadow"),
            bind_group_layouts: &[Some(&shadow_layout)],
            immediate_size: 0,
        });
        let vattrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
        let iattrs =
            wgpu::vertex_attr_array![2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4];
        let buffers = [
            wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &vattrs,
            },
            wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Inst>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &iattrs,
            },
        ];
        let ms = wgpu::MultisampleState { count: samples, ..Default::default() };
        let depth = |write: bool, cmp: wgpu::CompareFunction, bias: wgpu::DepthBiasState| wgpu::DepthStencilState {
            format: DEPTH,
            depth_write_enabled: Some(write),
            depth_compare: Some(cmp),
            stencil: wgpu::StencilState::default(),
            bias,
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("3d"),
            layout: Some(&main_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &buffers,
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(depth(true, wgpu::CompareFunction::Less, wgpu::DepthBiasState::default())),
            multisample: ms,
            multiview_mask: None,
            cache: None,
        });
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("3d shadow"),
            layout: Some(&shadow_pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_shadow"),
                buffers: &buffers,
                compilation_options: Default::default(),
            },
            fragment: None,
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(depth(
                true,
                wgpu::CompareFunction::Less,
                wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 },
            )),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sky_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky"),
            layout: Some(&main_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_sky"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_sky"),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(depth(false, wgpu::CompareFunction::LessEqual, wgpu::DepthBiasState::default())),
            multisample: ms,
            multiview_mask: None,
            cache: None,
        });
        let (verts, idx, ranges) = meshes();
        use wgpu::util::DeviceExt;
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("3d meshes"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("3d indices"),
            contents: bytemuck::cast_slice(&idx),
            usage: wgpu::BufferUsages::INDEX,
        });
        let capacity = 512;
        let instances = Renderer3::instance_buffer(device, capacity);
        Renderer3 {
            pipeline,
            shadow_pipeline,
            sky_pipeline,
            globals,
            bind,
            shadow_bind,
            shadow_view,
            vertices,
            indices,
            ranges,
            instances,
            capacity,
            samples,
            targets: None,
            format,
        }
    }

    fn instance_buffer(device: &wgpu::Device, n: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("3d instances"),
            size: (n * std::mem::size_of::<Inst>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn targets(&mut self, device: &wgpu::Device, w: u32, h: u32) {
        if matches!(self.targets, Some((tw, th, ..)) if tw == w && th == h) {
            return;
        }
        let tex = |format, label| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: self.samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let color = (self.samples > 1).then(|| tex(self.format, "3d msaa"));
        self.targets = Some((w, h, color, tex(DEPTH, "3d depth")));
    }

    /// Draws `scene` into `target` (`w` × `h`), replacing what was there.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        w: u32,
        h: u32,
        scene: &Scene3,
    ) {
        self.targets(device, w, h);
        let mut sorted: Vec<&Inst3> = scene.instances.iter().collect();
        sorted.sort_by_key(|i| (i.mesh, !i.shadow));
        let inst: Vec<Inst> = sorted.iter().map(|i| pack(i)).collect();
        if inst.len() > self.capacity {
            self.capacity = inst.len().next_power_of_two();
            self.instances = Renderer3::instance_buffer(device, self.capacity);
        }
        if !inst.is_empty() {
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&inst));
        }
        // Draw calls: per mesh, the instances that cast shadows and all of them.
        let mut calls: Vec<(Range, u32, u32, u32)> = Vec::new();
        let mut i = 0;
        for m in MESHES {
            let start = i;
            while i < sorted.len() && sorted[i].mesh == m && sorted[i].shadow {
                i += 1;
            }
            let casters = i;
            while i < sorted.len() && sorted[i].mesh == m {
                i += 1;
            }
            if i > start {
                calls.push((self.ranges[m as usize], start as u32, casters as u32, i as u32));
            }
        }
        let cam = &scene.camera;
        let vp = cam.view_proj(w as f32 / h as f32);
        let r = scene.shadow_radius;
        let light_eye = scene.shadow_center + scene.sun * (r * 2.0);
        let up = if scene.sun.y.abs() > 0.95 { V3::Z } else { V3::Y };
        let light = M4::ortho(r, r, 0.1, r * 4.0).mul(&M4::look_at(light_eye, scene.shadow_center, up));
        let c3 = |c: [f32; 3], a: f32| [c[0], c[1], c[2], a];
        let g = Globals {
            view_proj: vp.flat(),
            light: light.flat(),
            inv_view_proj: invert(&vp).flat(),
            eye: [cam.eye.x, cam.eye.y, cam.eye.z, 1.0],
            sun: [scene.sun.x, scene.sun.y, scene.sun.z, 0.0],
            sun_color: c3(scene.sun_color, 1.0),
            sky_top: c3(scene.sky_top, 1.0),
            sky_horizon: c3(scene.sky_horizon, 1.0),
            ground: c3(scene.ground, 1.0),
            fog: c3(scene.fog, scene.flash.clamp(0.0, 1.0)),
            params: [scene.fog_near, scene.fog_far, 2.0 * r / SHADOW as f32, 0.0],
        };
        queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&g));
        {
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
            pass.set_bind_group(0, &self.shadow_bind, &[]);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.set_vertex_buffer(1, self.instances.slice(..));
            pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
            for &(r, a, casters, _) in &calls {
                if casters > a {
                    pass.draw_indexed(r.first..r.first + r.count, r.base, a..casters);
                }
            }
        }
        let Some((_, _, msaa, depth)) = &self.targets else { return };
        let (view, resolve) = match msaa {
            Some(m) => (m, Some(target)),
            None => (target, None),
        };
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("3d"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: resolve,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Discard },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &self.bind, &[]);
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_vertex_buffer(1, self.instances.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        for &(r, a, _, b) in &calls {
            pass.draw_indexed(r.first..r.first + r.count, r.base, a..b);
        }
        // The sky last, only where nothing was drawn (depth 1).
        pass.set_pipeline(&self.sky_pipeline);
        pass.draw(0..3, 0..1);
    }
}

fn pack(i: &Inst3) -> Inst {
    let m = i.rot.mat();
    let s = i.half;
    let col = |v: V3, k: f32| [v.x * k, v.y * k, v.z * k, 0.0];
    Inst {
        m0: col(m.c[0], s.x),
        m1: col(m.c[1], s.y),
        m2: col(m.c[2], s.z),
        pos: [i.pos.x, i.pos.y, i.pos.z, 1.0],
        color: [i.color[0], i.color[1], i.color[2], i.bevel],
        half: [s.x, s.y, s.z, i.glow],
    }
}

/// The inverse of a 4 × 4 matrix (cofactors); the identity if it has none.
fn invert(m: &M4) -> M4 {
    let a = m.flat();
    let mut inv = [0.0f32; 16];
    inv[0] = a[5] * a[10] * a[15] - a[5] * a[11] * a[14] - a[9] * a[6] * a[15] + a[9] * a[7] * a[14] + a[13] * a[6] * a[11]
        - a[13] * a[7] * a[10];
    inv[4] = -a[4] * a[10] * a[15] + a[4] * a[11] * a[14] + a[8] * a[6] * a[15] - a[8] * a[7] * a[14] - a[12] * a[6] * a[11]
        + a[12] * a[7] * a[10];
    inv[8] =
        a[4] * a[9] * a[15] - a[4] * a[11] * a[13] - a[8] * a[5] * a[15] + a[8] * a[7] * a[13] + a[12] * a[5] * a[11] - a[12] * a[7] * a[9];
    inv[12] = -a[4] * a[9] * a[14] + a[4] * a[10] * a[13] + a[8] * a[5] * a[14] - a[8] * a[6] * a[13] - a[12] * a[5] * a[10]
        + a[12] * a[6] * a[9];
    inv[1] = -a[1] * a[10] * a[15] + a[1] * a[11] * a[14] + a[9] * a[2] * a[15] - a[9] * a[3] * a[14] - a[13] * a[2] * a[11]
        + a[13] * a[3] * a[10];
    inv[5] = a[0] * a[10] * a[15] - a[0] * a[11] * a[14] - a[8] * a[2] * a[15] + a[8] * a[3] * a[14] + a[12] * a[2] * a[11]
        - a[12] * a[3] * a[10];
    inv[9] = -a[0] * a[9] * a[15] + a[0] * a[11] * a[13] + a[8] * a[1] * a[15] - a[8] * a[3] * a[13] - a[12] * a[1] * a[11]
        + a[12] * a[3] * a[9];
    inv[13] =
        a[0] * a[9] * a[14] - a[0] * a[10] * a[13] - a[8] * a[1] * a[14] + a[8] * a[2] * a[13] + a[12] * a[1] * a[10] - a[12] * a[2] * a[9];
    inv[2] =
        a[1] * a[6] * a[15] - a[1] * a[7] * a[14] - a[5] * a[2] * a[15] + a[5] * a[3] * a[14] + a[13] * a[2] * a[7] - a[13] * a[3] * a[6];
    inv[6] =
        -a[0] * a[6] * a[15] + a[0] * a[7] * a[14] + a[4] * a[2] * a[15] - a[4] * a[3] * a[14] - a[12] * a[2] * a[7] + a[12] * a[3] * a[6];
    inv[10] =
        a[0] * a[5] * a[15] - a[0] * a[7] * a[13] - a[4] * a[1] * a[15] + a[4] * a[3] * a[13] + a[12] * a[1] * a[7] - a[12] * a[3] * a[5];
    inv[14] =
        -a[0] * a[5] * a[14] + a[0] * a[6] * a[13] + a[4] * a[1] * a[14] - a[4] * a[2] * a[13] - a[12] * a[1] * a[6] + a[12] * a[2] * a[5];
    inv[3] =
        -a[1] * a[6] * a[11] + a[1] * a[7] * a[10] + a[5] * a[2] * a[11] - a[5] * a[3] * a[10] - a[9] * a[2] * a[7] + a[9] * a[3] * a[6];
    inv[7] =
        a[0] * a[6] * a[11] - a[0] * a[7] * a[10] - a[4] * a[2] * a[11] + a[4] * a[3] * a[10] + a[8] * a[2] * a[7] - a[8] * a[3] * a[6];
    inv[11] =
        -a[0] * a[5] * a[11] + a[0] * a[7] * a[9] + a[4] * a[1] * a[11] - a[4] * a[3] * a[9] - a[8] * a[1] * a[7] + a[8] * a[3] * a[5];
    inv[15] = a[0] * a[5] * a[10] - a[0] * a[6] * a[9] - a[4] * a[1] * a[10] + a[4] * a[2] * a[9] + a[8] * a[1] * a[6] - a[8] * a[2] * a[5];
    let det = a[0] * inv[0] + a[1] * inv[4] + a[2] * inv[8] + a[3] * inv[12];
    if det.abs() < 1e-20 {
        return M4([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]);
    }
    let k = 1.0 / det;
    let mut r = [[0.0; 4]; 4];
    for c in 0..4 {
        for row in 0..4 {
            r[c][row] = inv[c * 4 + row] * k;
        }
    }
    M4(r)
}

/// Every mesh in one vertex list and one index list (counter-clockwise from outside).
fn meshes() -> (Vec<Vertex>, Vec<u32>, [Range; 4]) {
    let mut v: Vec<Vertex> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let mut ranges = [Range { first: 0, count: 0, base: 0 }; 4];
    let begin = |v: &Vec<Vertex>, idx: &Vec<u32>| (v.len(), idx.len());
    let mut end = |m: Mesh, (vb, ib): (usize, usize), v: &Vec<Vertex>, idx: &mut Vec<u32>| {
        let _ = v;
        ranges[m as usize] = Range { first: ib as u32, count: (idx.len() - ib) as u32, base: vb as i32 };
    };
    // Cube: four vertices a face so every face has its own normal.
    let s = begin(&v, &idx);
    for axis in 0..3 {
        for sign in [1.0f32, -1.0] {
            let mut n = [0.0f32; 3];
            n[axis] = sign;
            let (u, w) = ((axis + 1) % 3, (axis + 2) % 3);
            let base = (v.len() - s.0) as u32;
            for (a, b) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let mut p = [0.0f32; 3];
                p[axis] = sign;
                p[u] = a;
                p[w] = b * sign;
                v.push(Vertex { pos: p, normal: n });
            }
            idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    end(Mesh::Cube, s, &v, &mut idx);
    // Sphere: latitude and longitude.
    let s = begin(&v, &idx);
    let (rings, segs) = (14u32, 24u32);
    for r in 0..=rings {
        let th = std::f32::consts::PI * r as f32 / rings as f32;
        for k in 0..=segs {
            let ph = std::f32::consts::TAU * k as f32 / segs as f32;
            let p = [th.sin() * ph.cos(), th.cos(), -th.sin() * ph.sin()];
            v.push(Vertex { pos: p, normal: p });
        }
    }
    for r in 0..rings {
        for k in 0..segs {
            let a = r * (segs + 1) + k;
            let b = a + segs + 1;
            idx.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    end(Mesh::Sphere, s, &v, &mut idx);
    // Cylinder: a side with smooth normals and two flat caps.
    let s = begin(&v, &idx);
    let segs = 28u32;
    for k in 0..=segs {
        let ph = std::f32::consts::TAU * k as f32 / segs as f32;
        let (x, z) = (ph.cos(), -ph.sin());
        v.push(Vertex { pos: [x, -1.0, z], normal: [x, 0.0, z] });
        v.push(Vertex { pos: [x, 1.0, z], normal: [x, 0.0, z] });
    }
    for k in 0..segs {
        let a = k * 2;
        idx.extend_from_slice(&[a, a + 2, a + 1, a + 1, a + 2, a + 3]);
    }
    for (y, ny) in [(1.0f32, 1.0f32), (-1.0, -1.0)] {
        let c = (v.len() - s.0) as u32;
        v.push(Vertex { pos: [0.0, y, 0.0], normal: [0.0, ny, 0.0] });
        for k in 0..=segs {
            let ph = std::f32::consts::TAU * k as f32 / segs as f32;
            v.push(Vertex { pos: [ph.cos(), y, -ph.sin()], normal: [0.0, ny, 0.0] });
        }
        for k in 0..segs {
            if ny > 0.0 {
                idx.extend_from_slice(&[c, c + 1 + k, c + 2 + k]);
            } else {
                idx.extend_from_slice(&[c, c + 2 + k, c + 1 + k]);
            }
        }
    }
    end(Mesh::Cylinder, s, &v, &mut idx);
    // Cone: a side whose normals lean out by the slope, and a base.
    let s = begin(&v, &idx);
    let segs = 24u32;
    let slope = 0.5f32; // radius 1 over height 2
    for k in 0..=segs {
        let ph = std::f32::consts::TAU * k as f32 / segs as f32;
        let (x, z) = (ph.cos(), -ph.sin());
        let n = V3::new(x, slope, z).normalized().to_array();
        v.push(Vertex { pos: [x, -1.0, z], normal: n });
        v.push(Vertex { pos: [0.0, 1.0, 0.0], normal: n });
    }
    for k in 0..segs {
        let a = k * 2;
        idx.extend_from_slice(&[a, a + 2, a + 1]);
    }
    let c = (v.len() - s.0) as u32;
    v.push(Vertex { pos: [0.0, -1.0, 0.0], normal: [0.0, -1.0, 0.0] });
    for k in 0..=segs {
        let ph = std::f32::consts::TAU * k as f32 / segs as f32;
        v.push(Vertex { pos: [ph.cos(), -1.0, -ph.sin()], normal: [0.0, -1.0, 0.0] });
    }
    for k in 0..segs {
        idx.extend_from_slice(&[c, c + 2 + k, c + 1 + k]);
    }
    end(Mesh::Cone, s, &v, &mut idx);
    (v, idx, ranges)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_triangle_faces_out() {
        let (v, idx, ranges) = meshes();
        for m in MESHES {
            let r = ranges[m as usize];
            for t in idx[r.first as usize..(r.first + r.count) as usize].chunks(3) {
                let p = |i: u32| {
                    let q = v[(r.base as u32 + i) as usize].pos;
                    V3::new(q[0], q[1], q[2])
                };
                let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
                let n = (b - a).cross(c - a);
                if n.length() < 1e-6 {
                    continue;
                }
                let centre = (a + b + c) * (1.0 / 3.0);
                assert!(n.dot(centre) > 0.0, "{m:?}: a triangle faces in at {centre:?}");
            }
        }
    }

    #[test]
    fn a_point_in_front_of_the_camera_projects_to_the_middle() {
        let cam = Camera { eye: V3::new(0.0, 1.0, 5.0), target: V3::new(0.0, 1.0, 0.0), up: V3::Y, fov_y: 1.0, near: 0.1, far: 100.0 };
        let (x, y, front) = cam.project(V3::new(0.0, 1.0, 0.0), 400.0, 800.0);
        assert!(front && (x - 200.0).abs() < 0.01 && (y - 400.0).abs() < 0.01);
        let (_, y2, _) = cam.project(V3::new(0.0, 2.0, 0.0), 400.0, 800.0);
        assert!(y2 < 400.0, "up is up on the screen");
        let d = cam.ray(200.0, 400.0, 400.0, 800.0);
        assert!((d - V3::new(0.0, 0.0, -1.0)).length() < 1e-5);
    }

    #[test]
    fn the_inverse_undoes_a_view_projection() {
        let cam = Camera { eye: V3::new(1.0, 2.0, 7.0), target: V3::new(0.0, 1.0, 0.0), up: V3::Y, fov_y: 0.9, near: 0.1, far: 100.0 };
        let vp = cam.view_proj(0.5);
        let id = vp.mul(&invert(&vp));
        for c in 0..4 {
            for r in 0..4 {
                let want = if c == r { 1.0 } else { 0.0 };
                assert!((id.0[c][r] - want).abs() < 1e-3, "{id:?}");
            }
        }
    }
}
