//! Drawing a [`Frame`](crate::layer::Frame) with wgpu: every shape one instance of one pipeline. A capsule or a
//! rounded box is a signed distance in the fragment shader, so edges stay smooth at any size without meshes or
//! multisampling (cheap on a phone's GPU). Metal on iOS, Vulkan (or GLES) on Android, the desktop's API in the preview.

use crate::draw3d::{Renderer3, Scene3};
use crate::layer::Shape;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Inst {
    /// A capsule: its ends. A box: its centre and half size.
    a: [f32; 2],
    b: [f32; 2],
    /// Radius (capsule) or corner radius (box); kind 0 = capsule, 1 = box.
    r_kind: [f32; 2],
    _pad: [f32; 2],
    color: [f32; 4],
}

const SHADER: &str = r#"
struct Globals { size: vec2<f32>, _pad: vec2<f32> };
@group(0) @binding(0) var<uniform> g: Globals;

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) p: vec2<f32>,
    @location(1) a: vec2<f32>,
    @location(2) b: vec2<f32>,
    @location(3) rk: vec2<f32>,
    @location(4) color: vec4<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32, @location(0) a: vec2<f32>, @location(1) b: vec2<f32>,
      @location(2) rk: vec2<f32>, @location(3) pad: vec2<f32>, @location(4) color: vec4<f32>) -> Out {
    // The shape's bounds plus a pixel for the soft edge.
    var lo: vec2<f32>;
    var hi: vec2<f32>;
    if (rk.y < 0.5) {
        lo = min(a, b) - vec2<f32>(rk.x + 1.0);
        hi = max(a, b) + vec2<f32>(rk.x + 1.0);
    } else {
        lo = a - b - vec2<f32>(1.0);
        hi = a + b + vec2<f32>(1.0);
    }
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u));
    let p = mix(lo, hi, corner);
    var o: Out;
    o.pos = vec4<f32>(p.x / g.size.x * 2.0 - 1.0, 1.0 - p.y / g.size.y * 2.0, 0.0, 1.0);
    o.p = p;
    o.a = a;
    o.b = b;
    o.rk = rk;
    o.color = color;
    return o;
}

@fragment
fn fs(i: Out) -> @location(0) vec4<f32> {
    var d: f32;
    if (i.rk.y < 0.5) {
        let pa = i.p - i.a;
        let ba = i.b - i.a;
        let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
        d = length(pa - ba * h) - i.rk.x;
    } else {
        let q = abs(i.p - i.a) - i.b + vec2<f32>(i.rk.x);
        d = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - i.rk.x;
    }
    let cover = clamp(0.5 - d, 0.0, 1.0);
    return i.color * cover;
}
"#;

pub struct Renderer {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    instances: wgpu::Buffer,
    capacity: usize,
    three: Renderer3,
}

impl Renderer {
    pub async fn new(instance: &wgpu::Instance, surface: &wgpu::Surface<'_>) -> Result<Renderer, String> {
        let adapter = Renderer::adapter(instance, Some(surface)).await?;
        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats.iter().copied().find(|f| f.is_srgb()).unwrap_or(caps.formats[0]);
        Renderer::with_adapter(&adapter, format).await
    }

    /// Without a window: draws into textures of `format` (screenshots, tests).
    pub async fn headless(instance: &wgpu::Instance, format: wgpu::TextureFormat) -> Result<Renderer, String> {
        let adapter = Renderer::adapter(instance, None).await?;
        Renderer::with_adapter(&adapter, format).await
    }

    async fn adapter(instance: &wgpu::Instance, surface: Option<&wgpu::Surface<'_>>) -> Result<wgpu::Adapter, String> {
        instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                // A phone's battery matters more than the last frame.
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: surface,
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| format!("no GPU adapter: {e}"))
    }

    async fn with_adapter(adapter: &wgpu::Adapter, format: wgpu::TextureFormat) -> Result<Renderer, String> {
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("sim-mobile"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| format!("no GPU device: {e}"))?;
        let shader = device
            .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("shapes"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
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
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shapes"),
            bind_group_layouts: &[Some(&globals_layout)],
            immediate_size: 0,
        });
        let attrs = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x2, 3 => Float32x2, 4 => Float32x4];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shapes"),
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
        let capacity = 1024;
        let instances = Renderer::instance_buffer(&device, capacity);
        // 4× MSAA where the format allows it: nearly free on a phone's tiled GPU, and edges are what a toy look shows.
        let samples = if adapter.get_texture_format_features(format).flags.sample_count_supported(4) { 4 } else { 1 };
        let three = Renderer3::new(&device, format, samples);
        Ok(Renderer { device, queue, format, pipeline, globals, globals_bg, instances, capacity, three })
    }

    fn instance_buffer(device: &wgpu::Device, n: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shapes"),
            size: (n * std::mem::size_of::<Inst>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Draws `scene` (if any, else `clear`), then `shapes` (back to front) over it, into `target` of `w` × `h` pixels.
    pub fn draw(&mut self, target: &wgpu::TextureView, w: u32, h: u32, clear: [f32; 4], scene: Option<&Scene3>, shapes: &[Shape]) {
        let inst: Vec<Inst> = shapes
            .iter()
            .map(|s| match *s {
                Shape::Capsule { a, b, r, color } => {
                    Inst { a: [a.x, a.y], b: [b.x, b.y], r_kind: [r, 0.0], _pad: [0.0; 2], color: color.0 }
                }
                Shape::Box { rect, r, color } => {
                    let c = rect.center();
                    Inst {
                        a: [c.x, c.y],
                        b: [rect.w / 2.0, rect.h / 2.0],
                        r_kind: [r.min(rect.w / 2.0).min(rect.h / 2.0), 1.0],
                        _pad: [0.0; 2],
                        color: color.0,
                    }
                }
            })
            .collect();
        if inst.len() > self.capacity {
            self.capacity = inst.len().next_power_of_two();
            self.instances = Renderer::instance_buffer(&self.device, self.capacity);
        }
        self.queue.write_buffer(&self.globals, 0, bytemuck::cast_slice(&[w as f32, h as f32, 0.0, 0.0]));
        if !inst.is_empty() {
            self.queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&inst));
        }
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("shapes") });
        if let Some(scene) = scene {
            self.three.draw(&self.device, &self.queue, &mut enc, target, w, h, scene);
        }
        {
            let [r, g, b, a] = clear.map(f64::from);
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shapes"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if scene.is_some() { wgpu::LoadOp::Load } else { wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }) },
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
            pass.draw(0..4, 0..inst.len() as u32);
        }
        self.queue.submit([enc.finish()]);
    }
}
