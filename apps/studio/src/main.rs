mod app;
mod csg;
#[cfg(test)]
mod drag_test;
mod explorer;
mod extras;
mod icons;
mod menus;
mod metadata;
mod play;
mod plugins;
mod properties;
mod ribbon;
mod scripteditor;
mod theme;
mod viewport;
mod views;

use app::Studio;
use clap::Parser;
use rbx_platform::winit;
use rbx_render::{Renderer, wgpu};
use rbx_studio_ui::Document;
use std::path::PathBuf;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

#[derive(Parser)]
#[command(about = "Edits places")]
struct Args {
    place: Option<PathBuf>,
    #[arg(long)]
    assets: Option<PathBuf>,
    #[arg(long)]
    screenshot: Option<PathBuf>,
    #[arg(long, default_value_t = 90)]
    screenshot_frames: u32,
    #[arg(long, num_args = 0..)]
    select: Vec<String>,
    #[arg(long)]
    page: Option<usize>,
    #[arg(long)]
    script: Option<String>,
    #[arg(long)]
    tool: Option<String>,
    #[arg(long)]
    run: Option<String>,
    #[arg(long)]
    play: bool,
    #[arg(long, num_args = 0..)]
    action: Vec<String>,
}

fn find_assets(explicit: Option<PathBuf>) -> PathBuf {
    if let Some(p) = explicit {
        return p;
    }
    let mut candidates = vec![PathBuf::from("assets")];
    if let Ok(exe) = std::env::current_exe() {
        for a in exe.ancestors().skip(1).take(5) {
            candidates.push(a.join("assets"));
        }
    }
    candidates
        .into_iter()
        .find(|p| p.join("content").is_dir())
        .unwrap_or_else(|| PathBuf::from("assets"))
}

struct Gpu {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    egui: egui_wgpu::Renderer,
    viewport: Option<(wgpu::Texture, wgpu::TextureView, (u32, u32))>,
}

struct Host {
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    egui_ctx: egui::Context,
    egui_state: Option<egui_winit::State>,
    studio: Studio,
    screenshot: Option<(PathBuf, u32)>,
    frames: u32,
    last: rbx_base::time::Instant,
    behavior: u32,
    occluded: bool,
}

fn create_gpu(window: Arc<Window>, assets: &std::path::Path) -> Result<Gpu, String> {
    let size = window.inner_size();
    let instance = rbx_render::create_instance();
    let surface = instance
        .create_surface(window)
        .map_err(|e| format!("cannot create a surface: {e}"))?;
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: Some(&surface),
        apply_limit_buckets: false,
    }))
    .map_err(|e| format!("no graphics adapter: {e}"))?;
    let (device, queue) =
        pollster::block_on(adapter.request_device(&rbx_render::device_descriptor(&adapter)))
            .map_err(|e| format!("no graphics device: {e}"))?;
    let caps = surface.get_capabilities(&adapter);
    let format = caps
        .formats
        .iter()
        .copied()
        .find(|f| !f.is_srgb())
        .unwrap_or(caps.formats[0]);
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: size.width.max(1),
        height: size.height.max(1),
        present_mode: wgpu::PresentMode::AutoVsync,
        desired_maximum_frame_latency: 2,
        alpha_mode: caps.alpha_modes[0],
        view_formats: vec![],
        color_space: wgpu::SurfaceColorSpace::Auto,
    };
    surface.configure(&device, &config);
    let egui = egui_wgpu::Renderer::new(&device, format, egui_wgpu::RendererOptions::default());
    let renderer = Renderer::new(
        device,
        queue,
        format,
        &assets.join("content"),
        &assets.join("platform-content").join("pc"),
    );
    Ok(Gpu {
        surface,
        config,
        renderer,
        egui,
        viewport: None,
    })
}

impl Host {
    fn apply_mouse_behavior(&mut self) {
        let behavior = if self.studio.playing_game() {
            self.studio.doc.runtime.mouse_behavior()
        } else {
            0
        };
        if behavior == self.behavior {
            return;
        }
        self.behavior = behavior;
        let Some(w) = self.window.clone() else { return };
        if behavior == 0 {
            let _ = w.set_cursor_grab(winit::window::CursorGrabMode::None);
            w.set_cursor_visible(true);
            return;
        }
        if w.set_cursor_grab(winit::window::CursorGrabMode::Locked)
            .is_err()
        {
            let _ = w.set_cursor_grab(winit::window::CursorGrabMode::Confined);
        }
        w.set_cursor_visible(behavior != 1);
    }

    fn ensure_viewport(&mut self) {
        let Some(gpu) = &mut self.gpu else { return };
        let want = self.studio.view.pixels;
        if gpu.viewport.as_ref().is_some_and(|(_, _, s)| *s == want) {
            return;
        }
        let device = &gpu.renderer.device;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("studio viewport"),
            size: wgpu::Extent3d {
                width: want.0,
                height: want.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: gpu.renderer.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        match self.studio.view.texture {
            Some(id) => gpu.egui.update_egui_texture_from_wgpu_texture(
                device,
                &view,
                wgpu::FilterMode::Linear,
                id,
            ),
            None => {
                self.studio.view.texture = Some(gpu.egui.register_native_texture(
                    device,
                    &view,
                    wgpu::FilterMode::Linear,
                ));
            }
        }
        gpu.viewport = Some((texture, view, want));
    }

    fn frame(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let now = rbx_base::time::Instant::now();
        let dt = now.duration_since(self.last).as_secs_f64().min(0.1);
        self.last = now;
        let Some(state) = &mut self.egui_state else {
            return;
        };
        let raw = state.take_egui_input(&window);
        let studio = &mut self.studio;
        let output = self.egui_ctx.run_ui(raw, |ui| studio.ui(ui));
        if let Some(state) = &mut self.egui_state {
            state.handle_platform_output(&window, output.platform_output.clone());
        }
        self.ensure_viewport();
        self.apply_mouse_behavior();
        let ppp = output.pixels_per_point;
        let paint = self.egui_ctx.tessellate(output.shapes, ppp);
        let Some(gpu) = &mut self.gpu else { return };
        let screen = (
            self.studio.view.rect.width().max(1.0),
            self.studio.view.rect.height().max(1.0),
        );
        let step_started = rbx_base::time::Instant::now();
        if let Some(session) = &mut self.studio.play {
            session.frame(&self.studio.doc, dt, screen);
        } else {
            self.studio.doc.runtime.step(dt);
        }
        self.studio.views.step_ms = step_started.elapsed().as_secs_f64() * 1000.0;
        self.studio.views.frame_ms = dt * 1000.0;
        if !self.studio.playing_game() {
            let mut e = self.studio.doc.runtime.engine.lock();
            self.studio.view.camera.apply(&mut e.dm);
            e.dm.gui_view.screen = rbx_math::Vec2::new(screen.0, screen.1);
        }
        gpu.renderer.gui_scale = ppp;
        gpu.renderer.overlay = std::mem::take(&mut self.studio.overlay);
        gpu.renderer.frame_dt = dt as f32;
        if let Some((_, view, (w, h))) = &gpu.viewport {
            let render_started = rbx_base::time::Instant::now();
            let e = self.studio.doc.runtime.engine.lock();
            gpu.renderer.render(&e.dm, view, *w, *h);
            drop(e);
            self.studio.views.render_ms = render_started.elapsed().as_secs_f64() * 1000.0;
        }
        for (id, deltas) in &output.textures_delta.set {
            for delta in deltas {
                gpu.egui
                    .update_texture(&gpu.renderer.device, &gpu.renderer.queue, *id, delta);
            }
        }
        let frame = match gpu.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Occluded => {
                self.occluded = true;
                return;
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                window.request_redraw();
                return;
            }
            _ => {
                let size = window.inner_size();
                gpu.config.width = size.width.max(1);
                gpu.config.height = size.height.max(1);
                gpu.surface.configure(&gpu.renderer.device, &gpu.config);
                window.request_redraw();
                return;
            }
        };
        let target = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [gpu.config.width, gpu.config.height],
            pixels_per_point: ppp,
        };
        let capture = match &self.screenshot {
            Some((_, at)) if self.frames + 1 == *at => Some(capture_texture(gpu)),
            _ => None,
        };
        let mut encoder =
            gpu.renderer
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("egui"),
                });
        let extra = gpu.egui.update_buffers(
            &gpu.renderer.device,
            &gpu.renderer.queue,
            &mut encoder,
            &paint,
            &screen,
        );
        let mut targets = vec![&target];
        let capture_view = capture
            .as_ref()
            .map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()));
        if let Some(v) = &capture_view {
            targets.push(v);
        }
        for t in targets {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: t,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.93,
                            g: 0.93,
                            b: 0.93,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let mut pass = pass.forget_lifetime();
            gpu.egui.render(&mut pass, &paint, &screen);
        }
        gpu.renderer
            .queue
            .submit(extra.into_iter().chain(std::iter::once(encoder.finish())));
        gpu.renderer.queue.present(frame);
        for id in &output.textures_delta.free {
            gpu.egui.free_texture(id);
        }
        if std::mem::take(&mut self.studio.extras.pending_screenshot)
            && let Some((tex, _, _)) = &gpu.viewport
        {
            let path = Studio::screenshot_path();
            match save_texture(gpu, tex, &path) {
                Ok(()) => self.studio.status.push((
                    rbx_script::MessageType::Info,
                    format!("Screenshot saved to {}", path.display()),
                )),
                Err(e) => self.studio.status.push((rbx_script::MessageType::Error, e)),
            }
        }
        if std::mem::take(&mut self.studio.extras.pending_fullscreen) {
            let full = window.fullscreen().is_some();
            window.set_fullscreen(if full {
                None
            } else {
                Some(winit::window::Fullscreen::Borderless(None))
            });
        }
        if let (Some(tex), Some((path, _))) = (capture, &self.screenshot) {
            match save_texture(gpu, &tex, path) {
                Ok(()) => tracing::info!("saved {}", path.display()),
                Err(e) => tracing::error!("{e}"),
            }
        }
        let title = format!("{} - Roblox Studio", self.studio.place_name());
        if window.title() != title {
            window.set_title(&title);
        }
        self.frames += 1;
        window.request_redraw();
    }
}

fn capture_texture(gpu: &Gpu) -> wgpu::Texture {
    gpu.renderer
        .device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("studio capture"),
            size: wgpu::Extent3d {
                width: gpu.config.width,
                height: gpu.config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: gpu.config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
}

fn save_texture(gpu: &Gpu, tex: &wgpu::Texture, path: &std::path::Path) -> Result<(), String> {
    let (w, h) = (tex.width(), tex.height());
    let row = (w * 4).div_ceil(256) * 256;
    let device = &gpu.renderer.device;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("studio capture readback"),
        size: (row * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    encoder.copy_texture_to_buffer(
        tex.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    gpu.renderer.queue.submit(std::iter::once(encoder.finish()));
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let data = slice.get_mapped_range().map_err(|e| e.to_string())?;
    let bgra = matches!(
        gpu.config.format,
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
    );
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let line = &data[(y * row) as usize..(y * row + w * 4) as usize];
        for px in line.as_chunks::<4>().0 {
            if bgra {
                pixels.extend_from_slice(&[px[2], px[1], px[0], 255]);
            } else {
                pixels.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
        }
    }
    drop(data);
    buffer.unmap();
    let img = image::RgbaImage::from_raw(w, h, pixels).ok_or("bad capture size")?;
    img.save(path)
        .map_err(|e| format!("{}: {e}", path.display()))
}

impl ApplicationHandler for Host {
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.occluded {
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                std::time::Instant::now() + std::time::Duration::from_millis(100),
            ));
            self.studio.doc.runtime.step(0.1);
            return;
        }
        event_loop.set_control_flow(ControlFlow::Poll);
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Roblox Studio")
            .with_inner_size(winit::dpi::LogicalSize::new(1600.0, 940.0));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::error!("cannot open a window: {e}");
                event_loop.exit();
                return;
            }
        };
        match create_gpu(window.clone(), &self.studio.assets) {
            Ok(gpu) => {
                let max = gpu.renderer.device.limits().max_texture_dimension_2d as usize;
                self.egui_state = Some(egui_winit::State::new(
                    self.egui_ctx.clone(),
                    egui::ViewportId::ROOT,
                    &window,
                    Some(window.scale_factor() as f32),
                    None,
                    Some(max),
                ));
                self.gpu = Some(gpu);
            }
            Err(e) => {
                tracing::error!("{e}");
                event_loop.exit();
                return;
            }
        }
        window.request_redraw();
        self.window = Some(window);
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _id: winit::event::DeviceId,
        event: winit::event::DeviceEvent,
    ) {
        if self.behavior != 0
            && let winit::event::DeviceEvent::MouseMotion { delta } = event
            && let Some(session) = &mut self.studio.play
        {
            let cursor = session.cursor;
            session.mouse(
                &self.studio.doc,
                rbx_script::api::user_input::InputKind::Changed,
                rbx_script::api::user_input::MOUSE_MOVEMENT,
                cursor,
                (delta.0 as f32, delta.1 as f32),
            );
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let game_keys = self.studio.playing_game()
            && self.studio.viewport_focus
            && !self.egui_ctx.egui_wants_keyboard_input();
        if game_keys
            && let WindowEvent::KeyboardInput { event: key, .. } = &event
            && let winit::keyboard::PhysicalKey::Code(code) = key.physical_key
            && !key.repeat
        {
            let shift_f5 = code == winit::keyboard::KeyCode::F5;
            if !shift_f5 {
                let rc = rbx_platform::roblox_keycode(code);
                if rc != 0
                    && let Some(session) = &mut self.studio.play
                {
                    let text = key.text.as_ref().map(|t| t.to_string());
                    session.key(
                        &self.studio.doc,
                        rc,
                        key.state == winit::event::ElementState::Pressed,
                        text,
                    );
                }
                return;
            }
        }
        if let WindowEvent::Focused(false) = &event
            && let Some(session) = &mut self.studio.play
        {
            session.release_keys(&self.studio.doc);
        }
        if let (Some(state), Some(window)) = (&mut self.egui_state, &self.window) {
            let _ = state.on_window_event(window, &event);
        }
        match event {
            WindowEvent::CloseRequested => {
                self.studio.cleanup_servers();
                event_loop.exit();
            }
            WindowEvent::Occluded(hidden) => {
                self.occluded = hidden;
                if !hidden && let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::Focused(true) => {
                self.occluded = false;
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.config.width = size.width.max(1);
                    gpu.config.height = size.height.max(1);
                    gpu.surface.configure(&gpu.renderer.device, &gpu.config);
                }
            }
            WindowEvent::RedrawRequested => {
                self.frame();
                if let Some((_, at)) = &self.screenshot
                    && self.frames >= *at
                {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let args = Args::parse();
    let assets = find_assets(args.assets);
    let content = assets.join("content");
    let doc = match &args.place {
        Some(p) => Document::open(p, &content),
        None => Document::new_baseplate(&content),
    };
    let doc = match doc {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let mut studio = Studio::new(doc, assets);
    if let Some(src) = &args.run {
        let _ = studio
            .doc
            .runtime
            .execute(src, "CommandBar", rbx_script::Identity::CmdLine);
        studio.doc.runtime.pump();
        studio.view.camera = app::camera_for(&studio.doc);
    }
    studio.tool = match args.tool.as_deref() {
        Some("move") => viewport::Tool::Move,
        Some("scale") => viewport::Tool::Scale,
        Some("rotate") => viewport::Tool::Rotate,
        _ => viewport::Tool::Select,
    };
    if args.play {
        studio.start_play(play::Mode::Play);
    }
    if let Some(p) = args.page {
        studio.ribbon.page = p;
    }
    if let Some(name) = &args.script {
        let found = {
            let e = studio.doc.runtime.engine.lock();
            e.dm.descendants(e.dm.root())
                .into_iter()
                .find(|d| e.dm.name_of(*d) == name && e.dm.is_a(*d, "LuaSourceContainer"))
        };
        if let Some(id) = found {
            studio.open_script(id);
        }
    }
    if !args.select.is_empty() {
        let picked: Vec<_> = {
            let e = studio.doc.runtime.engine.lock();
            e.dm.find_service("Workspace")
                .map(|ws| {
                    e.dm.descendants(ws)
                        .into_iter()
                        .filter(|d| args.select.iter().any(|n| n == e.dm.name_of(*d)))
                        .collect()
                })
                .unwrap_or_default()
        };
        studio.doc.select(picked);
    }
    for a in &args.action {
        studio.run_action(a);
    }
    let event_loop = match EventLoop::new() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("cannot start the event loop: {e}");
            std::process::exit(1);
        }
    };
    event_loop.set_control_flow(ControlFlow::Poll);
    let egui_ctx = egui::Context::default();
    theme::install(&egui_ctx, &studio.assets.join("content").join("fonts"));
    let mut host = Host {
        window: None,
        gpu: None,
        egui_ctx,
        egui_state: None,
        studio,
        screenshot: args.screenshot.map(|p| (p, args.screenshot_frames.max(2))),
        frames: 0,
        last: rbx_base::time::Instant::now(),
        behavior: 0,
        occluded: false,
    };
    if let Err(e) = event_loop.run_app(&mut host) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
