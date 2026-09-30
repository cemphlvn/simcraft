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
use crate::gesture::{Gesture, Px, Recognizer, Tuning};
use crate::haptics::{self, Haptics};
use crate::layer::{Color, Fit, Frame, Insets};
use crate::scene::{BOARD, Scene, TICK_RATE};
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
    scene: Scene,
    gestures: Recognizer,
    haptics: Box<dyn Haptics>,
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
            scene: Scene::new(),
            gestures: Recognizer::new(Tuning::for_scale(1.0)),
            haptics: haptics::platform(),
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

    /// Where the board is on the screen now: fitted into the safe area.
    fn fit(&self) -> Option<(Fit, crate::layer::Rect)> {
        let (s, w) = (self.surface.as_ref()?, self.window.as_ref()?);
        let safe = Insets::phone(w.scale_factor() as f32).safe(s.config.width as f32, s.config.height as f32);
        let margin = safe.w * 0.04;
        let area = crate::layer::Rect::new(safe.x + margin, safe.y + margin * 3.0, safe.w - margin * 2.0, safe.h - margin * 4.0);
        Some((Fit::new(BOARD.0, BOARD.1, area), safe))
    }

    fn gestures(&mut self, gs: &[Gesture]) {
        let Some((fit, _)) = self.fit() else { return };
        let mut pulses = Vec::new();
        for g in gs {
            self.stats.gesture();
            self.scene.input(g, &fit, &mut pulses);
        }
        for p in pulses {
            self.stats.pulse();
            self.haptics.play(p);
        }
    }

    fn touch(&mut self, t: Touch) {
        let at = Px::new(t.location.x as f32, t.location.y as f32);
        let ms = self.ms();
        let gs = match t.phase {
            TouchPhase::Started => self.gestures.down(t.id, at, ms),
            TouchPhase::Moved => self.gestures.moved(t.id, at, ms),
            TouchPhase::Ended => self.gestures.up(t.id, at, ms),
            TouchPhase::Cancelled => self.gestures.cancel(t.id),
        };
        self.gestures(&gs);
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
        let mut pulses = Vec::new();
        while self.clock >= 1.0 {
            let t = Instant::now();
            self.scene.step(&mut pulses);
            self.stats.tick(t.elapsed().as_secs_f32() * 1e6);
            self.clock -= 1.0;
        }
        for p in pulses {
            self.stats.pulse();
            self.haptics.play(p);
        }
        let Some((fit, safe)) = self.fit() else { return };
        let (Some(s), Some(r)) = (self.surface.as_ref(), self.renderer.as_mut()) else { return };
        let (w, h) = (s.config.width, s.config.height);
        let mut frame = Frame::default();
        self.scene.draw(self.clock, &fit, safe, &mut frame);
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
            let haptics = format!("\"haptics\":\"{}\",\"haptics_failed\":{}", self.haptics.name(), self.haptics.failed());
            println!("{}", report.json(self.ms(), (w, h), &format!("{haptics},{}", self.scene.observe())));
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
        el.set_control_flow(ControlFlow::Poll);
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
                    let gs = self.gestures.moved(MOUSE, self.mouse, self.ms());
                    self.gestures(&gs);
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                let ms = self.ms();
                let gs = if state == ElementState::Pressed {
                    self.mouse_down = true;
                    self.gestures.down(MOUSE, self.mouse, ms)
                } else {
                    self.mouse_down = false;
                    self.gestures.up(MOUSE, self.mouse, ms)
                };
                self.gestures(&gs);
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
