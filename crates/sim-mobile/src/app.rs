//! The shell: a winit application that owns the window, the GPU surface and the loop. The same code runs on iOS,
//! Android and in the desktop preview (a phone-sized window; the mouse is one finger).
//!
//! Lifecycle: on suspend the surface is dropped (Android destroys it; iOS goes inactive), on resume it is made
//! again; the scene and the GPU device survive both. The loop runs the scene at a fixed tick and draws between ticks.

use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, Touch, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Window, WindowId};

use crate::draw::Renderer;
use crate::gesture::{Px, Recognizer, Tuning};
use crate::haptics::{self, Haptics, Pulse};
use crate::layer::{Color, Frame};
use crate::playground::{Layout, Phase, Playground, TICK_RATE};
use crate::sensors::Motion;
use crate::stats::Stats;

/// A phone's size in points for the desktop preview (an iPhone 15).
#[cfg(not(any(target_os = "ios", target_os = "android")))]
const PREVIEW: (f64, f64) = (393.0, 852.0);
/// The mouse's finger id.
const MOUSE: u64 = u64::MAX;

struct Surface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

pub struct App {
    instance: wgpu::Instance,
    window: Option<Arc<Window>>,
    surface: Option<Surface>,
    renderer: Option<Renderer>,
    playground: Playground,
    gestures: Recognizer,
    haptics: Box<dyn Haptics>,
    motion: Motion,
    started: Instant,
    last: Instant,
    /// Unspent time towards the next tick (in ticks).
    clock: f32,
    mouse: Px,
    mouse_down: bool,
    /// The last second, printed as one JSON line (streamed from a phone by `devicectl ... --console`).
    stats: Stats,
}

impl Default for App {
    fn default() -> App {
        App::new()
    }
}

impl App {
    pub fn new() -> App {
        let now = Instant::now();
        App {
            instance: wgpu::Instance::default(),
            window: None,
            surface: None,
            renderer: None,
            playground: Playground::new(),
            gestures: Recognizer::new(Tuning::for_scale(1.0)),
            haptics: haptics::platform(),
            motion: Motion::new(),
            started: now,
            last: now,
            clock: 0.0,
            mouse: Px::default(),
            mouse_down: false,
            stats: Stats::default(),
        }
    }

    fn ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    /// Where everything is on the screen now.
    fn layout(&self) -> Option<Layout> {
        let (s, w) = (self.surface.as_ref()?, self.window.as_ref()?);
        Some(Layout::new(s.config.width as f32, s.config.height as f32, w.scale_factor() as f32))
    }

    fn play(&mut self, pulses: Vec<Pulse>) {
        for p in pulses {
            self.stats.pulse();
            self.haptics.play(p);
        }
    }

    /// A finger (or the mouse): to the playground as a raw touch, and to the gesture recognizer.
    fn finger(&mut self, id: u64, phase: Option<Phase>, at: Px) {
        let Some(layout) = self.layout() else { return };
        let ms = self.ms();
        let mut pulses = Vec::new();
        let gs = match phase {
            Some(Phase::Down) => self.gestures.down(id, at, ms),
            Some(Phase::Move) => self.gestures.moved(id, at, ms),
            Some(Phase::Up) => self.gestures.up(id, at, ms),
            None => self.gestures.cancel(id),
        };
        self.playground.touch(id, phase.unwrap_or(Phase::Up), at, &layout, &mut pulses);
        for g in &gs {
            self.stats.gesture();
            self.playground.input(g, &layout, &mut pulses);
        }
        self.play(pulses);
    }

    fn touch(&mut self, t: Touch) {
        let at = Px::new(t.location.x as f32, t.location.y as f32);
        let phase = match t.phase {
            TouchPhase::Started => Some(Phase::Down),
            TouchPhase::Moved => Some(Phase::Move),
            TouchPhase::Ended => Some(Phase::Up),
            TouchPhase::Cancelled => None,
        };
        self.finger(t.id, phase, at);
    }

    fn resize(&mut self, w: u32, h: u32) {
        if let (Some(s), Some(r)) = (self.surface.as_mut(), self.renderer.as_ref()) {
            s.config.width = w.max(1);
            s.config.height = h.max(1);
            s.surface.configure(&r.device, &s.config);
        }
    }

    fn frame(&mut self) {
        let now = Instant::now();
        let interval = now.duration_since(self.last).as_secs_f32();
        self.last = now;
        self.clock += interval.min(0.25) * TICK_RATE as f32;
        let sense = self.motion.sense();
        let mut pulses = Vec::new();
        while self.clock >= 1.0 {
            let t = Instant::now();
            self.playground.step(&sense, &mut pulses);
            self.stats.tick(t.elapsed().as_secs_f32() * 1e6);
            self.clock -= 1.0;
        }
        self.play(pulses);
        self.playground.age(interval * 1000.0);
        let Some(layout) = self.layout() else { return };
        let (Some(s), Some(r)) = (self.surface.as_ref(), self.renderer.as_mut()) else { return };
        let (w, h) = (s.config.width, s.config.height);
        let mut frame = Frame::default();
        self.playground.draw(self.clock, &layout, &mut frame);
        let shapes = frame.sorted();
        match s.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(tex) | wgpu::CurrentSurfaceTexture::Suboptimal(tex) => {
                let view = tex.texture.create_view(&wgpu::TextureViewDescriptor::default());
                r.draw(&view, w, h, Color::hex(0x1b1440).0, &shapes);
                tex.present();
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {}
            _ => s.surface.configure(&r.device, &s.config),
        }
        self.stats.frame(interval * 1000.0, now.elapsed().as_secs_f32() * 1000.0);
        if let Some(report) = self.stats.take(1000.0) {
            self.playground.fps = report.fps;
            let haptics = format!("\"haptics\":\"{}\",\"haptics_failed\":{}", self.haptics.name(), self.haptics.failed());
            println!("{}", report.json(self.ms(), (w, h), &format!("{haptics},{}", self.playground.observe())));
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        let window = match &self.window {
            Some(w) => w.clone(),
            None => {
                let attrs = Window::default_attributes().with_title("simcraft");
                #[cfg(not(any(target_os = "ios", target_os = "android")))]
                let attrs = attrs.with_inner_size(winit::dpi::LogicalSize::new(PREVIEW.0, PREVIEW.1)).with_resizable(false);
                let w = Arc::new(el.create_window(attrs).expect("a window"));
                self.gestures = Recognizer::new(Tuning::for_scale(w.scale_factor() as f32));
                self.window = Some(w.clone());
                w
            }
        };
        let surface = self.instance.create_surface(window.clone()).expect("a surface");
        if self.renderer.is_none() {
            match pollster::block_on(Renderer::new(&self.instance, &surface)) {
                Ok(r) => self.renderer = Some(r),
                Err(e) => {
                    eprintln!("sim-mobile: {e}");
                    el.exit();
                    return;
                }
            }
        }
        let r = self.renderer.as_ref().expect("a renderer");
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: r.format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        surface.configure(&r.device, &config);
        self.surface = Some(Surface { surface, config });
        self.last = Instant::now();
        // Wait, not Poll: on iOS Poll spins the run loop without ever sleeping (a whole core at 94 % on an idle
        // screen, measured with Instruments); each frame is asked for instead and paced by the display.
        el.set_control_flow(ControlFlow::Wait);
        window.request_redraw();
    }

    fn suspended(&mut self, el: &ActiveEventLoop) {
        // Android destroys the surface as this returns: nothing may draw into it any more.
        self.surface = None;
        el.set_control_flow(ControlFlow::Wait);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(size) => self.resize(size.width, size.height),
            WindowEvent::Touch(t) => self.touch(t),
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse = Px::new(position.x as f32, position.y as f32);
                if self.mouse_down {
                    self.finger(MOUSE, Some(Phase::Move), self.mouse);
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                self.mouse_down = state == ElementState::Pressed;
                let phase = if self.mouse_down { Phase::Down } else { Phase::Up };
                self.finger(MOUSE, Some(phase), self.mouse);
            }
            WindowEvent::RedrawRequested => self.frame(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if self.surface.is_some()
            && let Some(w) = &self.window
        {
            w.request_redraw();
        }
    }
}
