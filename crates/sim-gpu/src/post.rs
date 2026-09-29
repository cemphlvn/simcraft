//! The lens: depth of field, as a macro lens has it. The sky and the world are drawn into an offscreen picture
//! with its depth; each pixel's distance gives its circle of confusion (how blurred a thin lens focused at `focus`
//! draws it); a golden-angle disk of samples blurs it. A sample counts only where its own blur reaches this pixel,
//! so sharp things in front do not smear over the blur behind them.

use crate::gpu::Lens;

const SHADER: &str = r#"
struct P { focus: f32, aperture: f32, a: f32, b: f32, size: vec2<f32>, maxr: f32, pad: f32 };
@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var depth_t: texture_depth_2d;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var<uniform> p: P;

struct VOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex
fn vs(@builtin(vertex_index) i: u32) -> VOut {
    let xy = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VOut;
    o.pos = vec4<f32>(xy * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(xy.x, 1.0 - xy.y);
    return o;
}

// Distance along the view from the stored depth (the projection: depth = a + b / distance).
fn dist(px: vec2<i32>) -> f32 {
    let d = textureLoad(depth_t, px, 0);
    return p.b / min(d - p.a, -1e-6);
}

// Blur radius in pixels for something at distance z.
fn coc(z: f32) -> f32 {
    return clamp(p.aperture * abs(z - p.focus) / max(z, 1e-3) * p.size.y * 0.5, 0.0, p.maxr);
}

@fragment
fn fs(v: VOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(v.pos.xy);
    let r0 = coc(dist(px));
    var sum = textureSampleLevel(scene, samp, v.uv, 0.0);
    if (r0 < 0.5) {
        return sum;
    }
    var wsum = 1.0;
    let hi = vec2<i32>(p.size) - vec2<i32>(1, 1);
    for (var k = 1; k < 32; k = k + 1) {
        let rr = sqrt(f32(k) / 32.0) * r0;
        let ang = f32(k) * 2.39996;
        let q = v.uv + vec2<f32>(cos(ang), sin(ang)) * rr / p.size;
        let qp = clamp(vec2<i32>(q * p.size), vec2<i32>(0, 0), hi);
        let w = clamp(coc(dist(qp)) - rr + 1.0, 0.0, 1.0);
        sum = sum + textureSampleLevel(scene, samp, q, 0.0) * w;
        wsum = wsum + w;
    }
    return sum / wsum;
}
"#;

pub struct Post {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    format: wgpu::TextureFormat,
    scene: Option<(u32, u32, wgpu::TextureView)>,
}

impl Post {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Post {
        let shader = device
            .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("lens"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let tex = |binding: u32, sample_type: wgpu::TextureSampleType| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lens"),
            entries: &[
                tex(0, wgpu::TextureSampleType::Float { filterable: true }),
                tex(1, wgpu::TextureSampleType::Depth),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("lens"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lens"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("lens"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lens"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Post { pipeline, layout, sampler, uniform, format, scene: None }
    }

    /// The offscreen picture the sky and the world are drawn into (made again when the size changes).
    pub fn scene(&mut self, device: &wgpu::Device, w: u32, h: u32) -> wgpu::TextureView {
        if self.scene.as_ref().is_none_or(|(sw, sh, _)| (*sw, *sh) != (w, h)) {
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("scene"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.scene = Some((w, h, t.create_view(&wgpu::TextureViewDescriptor::default())));
        }
        self.scene.as_ref().expect("made above").2.clone()
    }

    /// Blurs the scene by distance onto `target`.
    #[allow(clippy::too_many_arguments)]
    pub fn apply(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        w: u32,
        h: u32,
        lens: Lens,
    ) {
        let Some((_, _, scene)) = &self.scene else { return };
        let (n, f) = (lens.near, lens.far);
        let a = f / (f - n);
        let b = -n * f / (f - n);
        let maxr = (h as f32 / 90.0).clamp(4.0, 16.0);
        let u: [f32; 8] = [lens.focus, lens.aperture, a, b, w as f32, h as f32, maxr, 0.0];
        queue.write_buffer(&self.uniform, 0, bytemuck::cast_slice(&u));
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("lens"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(scene) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(depth) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: self.uniform.as_entire_binding() },
            ],
        });
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("lens"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bg, &[]);
        pass.draw(0..3, 0..1);
    }
}
