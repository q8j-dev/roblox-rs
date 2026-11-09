#[cfg(not(target_arch = "wasm32"))]
use clap::Parser;
use parking_lot::Mutex;
use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::Vec3;
use rbx_platform::winit::dpi::PhysicalPosition;
use rbx_platform::winit::window::{CursorGrabMode, Window};
use rbx_platform::{App, Input};
use rbx_render::{Renderer, wgpu};
use rbx_script::api::user_input::{self, InputEvent, InputKind};
use rbx_script::net::NetClient;
use rbx_script::{Engine, ScriptRuntime};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const STEP: f64 = 1.0 / 60.0;

use rbx_render::video;

#[cfg(not(target_arch = "wasm32"))]
#[derive(Parser, Debug)]
#[command(
    name = "player",
    version,
    about = "Plays a place or joins a game server"
)]
struct Args {
    place: Option<PathBuf>,
    #[arg(long)]
    join: Option<std::net::SocketAddr>,
    #[arg(long)]
    cert: Option<String>,
    #[arg(long, default_value = "Player1")]
    name: String,
    #[arg(long)]
    assets: Option<PathBuf>,
    #[arg(long)]
    screenshot: Option<PathBuf>,
    #[arg(long, default_value_t = 120)]
    screenshot_frames: u32,
    #[arg(long = "input")]
    inputs: Vec<String>,
    #[arg(long)]
    dump_gui: Option<PathBuf>,
    #[arg(long, default_value = "info")]
    log: String,
}

struct Gpu {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    copy_src: bool,
}

struct Player {
    runtime: ScriptRuntime,
    net: Option<NetClient>,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    pending_gpu: Arc<Mutex<Option<Gpu>>>,
    surface_size: Option<(u32, u32)>,
    assets: PathBuf,
    accumulator: f64,
    keys: HashSet<u32>,
    cursor: (f32, f32),
    behavior: u32,
    lock_point: Option<(f32, f32)>,
    #[cfg(not(target_arch = "wasm32"))]
    screenshot: Option<(PathBuf, u32)>,
    #[cfg(not(target_arch = "wasm32"))]
    scripted: Vec<(u32, ScriptedInput)>,
    #[cfg(not(target_arch = "wasm32"))]
    dump_gui: Option<PathBuf>,
    #[cfg(not(target_arch = "wasm32"))]
    recorder: Option<video::VideoRecorder>,
    frames: u32,
    done: bool,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug)]
enum ScriptedInput {
    Key(u32),
    Click(f32, f32),
    Verb(String),
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_input(spec: &str) -> Option<(u32, ScriptedInput)> {
    let (what, frame) = spec.rsplit_once('@')?;
    let frame: u32 = frame.parse().ok()?;
    let (kind, value) = what.split_once(':')?;
    let input = match kind {
        "key" => ScriptedInput::Key(value.parse().ok()?),
        "verb" => ScriptedInput::Verb(value.to_string()),
        "click" => {
            let (x, y) = value.split_once(',')?;
            ScriptedInput::Click(x.parse().ok()?, y.parse().ok()?)
        }
        _ => return None,
    };
    Some((frame, input))
}

#[cfg(not(target_arch = "wasm32"))]
fn dump_gui(dm: &DataModel) -> String {
    fn walk(dm: &DataModel, id: InstanceId, depth: usize, out: &mut String) {
        let class = dm.class_of(id).unwrap_or("");
        let mut line = format!("{}{} ({class})", "  ".repeat(depth), dm.name_of(id));
        if dm.is_a(id, "GuiBase2d") {
            let p = rbx_dom::gui::placement(dm, id);
            line.push_str(&format!(
                " pos=({:.0},{:.0}) size=({:.0},{:.0})",
                p.position.x, p.position.y, p.size.x, p.size.y
            ));
            for prop in [
                "Visible",
                "ZIndex",
                "BackgroundTransparency",
                "Position",
                "Size",
                "Text",
                "Image",
                "ClipsDescendants",
            ] {
                if let Ok(v) = dm.get(id, prop) {
                    line.push_str(&format!(" {prop}={v:?}"));
                }
            }
        }
        out.push_str(&line);
        out.push('\n');
        for &c in dm.children_of(id) {
            walk(dm, c, depth + 1, out);
        }
    }
    let mut out = String::new();
    if let Some(core) = dm.find_service("CoreGui") {
        walk(dm, core, 0, &mut out);
    }
    out
}

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(not(target_arch = "wasm32"))]
fn decode_hash(hex: &str) -> Option<[u8; 32]> {
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

fn make_runtime(dm: DataModel, assets: &Path) -> Result<ScriptRuntime, String> {
    let mut engine = Engine::new(dm);
    engine.content_root = Some(assets.join("content"));
    engine.animate_fallback = true;
    engine.audio = rbx_audio::AudioEngine::open();
    engine.process_after_load();
    engine.running = true;
    ScriptRuntime::new(Arc::new(Mutex::new(engine))).map_err(|e| e.to_string())
}

fn spawn_local_player(
    runtime: &ScriptRuntime,
    name: &str,
    character: bool,
) -> Result<InstanceId, String> {
    let players = runtime
        .engine
        .lock()
        .dm
        .get_service("Players")
        .map_err(|e| e.to_string())?;
    let player =
        rbx_script::api::players::make_local_player(&runtime.lua, players, 1, false, Some(name))
            .map_err(|e| e.to_string())?;
    if character {
        rbx_script::api::players::load_character(&runtime.lua, player, true, "")
            .map_err(|e| e.to_string())?;
    }
    Ok(player)
}

fn platform_content() -> &'static str {
    if cfg!(target_os = "ios") {
        "ios"
    } else if cfg!(target_os = "android") {
        "android"
    } else {
        "pc"
    }
}

async fn create_gpu(window: Arc<Window>, assets: PathBuf) -> Result<Gpu, String> {
    let size = window.inner_size();
    let instance = rbx_render::create_instance();
    let surface = instance
        .create_surface(window.clone())
        .map_err(|e| format!("cannot create a surface: {e}"))?;
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            apply_limit_buckets: false,
        })
        .await
        .map_err(|e| format!("no graphics adapter: {e}"))?;
    let (device, queue) = adapter
        .request_device(&rbx_render::device_descriptor(&adapter))
        .await
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
    let mut renderer = Renderer::new(
        device,
        queue,
        format,
        &assets.join("content"),
        &assets.join("platform-content").join(platform_content()),
    );
    renderer.async_loads = true;
    Ok(Gpu {
        surface,
        config,
        renderer,
        copy_src: caps.usages.contains(wgpu::TextureUsages::COPY_SRC),
    })
}

impl Player {
    fn gui_scale(&self) -> f32 {
        self.window
            .as_ref()
            .map(|w| w.scale_factor() as f32)
            .unwrap_or(1.0)
            .max(0.5)
    }

    fn update_gui_view(&mut self) {
        let Some(gpu) = &mut self.gpu else {
            return;
        };
        let scale = self
            .window
            .as_ref()
            .map(|w| w.scale_factor() as f32)
            .unwrap_or(1.0)
            .max(0.5);
        gpu.renderer.gui_scale = scale;
        let screen = rbx_math::Vec2::new(
            gpu.config.width as f32 / scale,
            gpu.config.height as f32 / scale,
        );
        self.runtime.engine.lock().dm.gui_view.screen = screen;
    }

    fn step(&mut self) {
        if let Some(net) = &mut self.net {
            net.tick(&self.runtime);
        }
        self.runtime.step(STEP);
        if let Some(net) = &mut self.net {
            net.tick(&self.runtime);
        }
    }

    fn ensure_camera(&mut self) {
        let mut e = self.runtime.engine.lock();
        let Some(ws) = e.dm.find_service("Workspace") else {
            return;
        };
        let cam =
            e.dm.get_ref(ws, "CurrentCamera")
                .or_else(|| e.dm.find_first_child_of_class(ws, "Camera"));
        if cam.is_none()
            && let Ok(c) = e.dm.create("Camera")
        {
            let _ = e.dm.set_parent(c, Some(ws));
            let _ = e.dm.set(ws, "CurrentCamera", Variant::Ref(Some(c)));
        }
    }

    fn logical(&self, x: f32, y: f32) -> Vec3 {
        let s = self.gui_scale();
        Vec3::new(x / s, y / s, 0.0)
    }

    fn send(&self, kind: InputKind, input_type: u32, key_code: u32, position: Vec3, delta: Vec3) {
        self.runtime.input(InputEvent {
            kind,
            input_type,
            key_code,
            position,
            delta,
            text: None,
        });
    }

    fn pointer(&self) -> Vec3 {
        let (x, y) = self.lock_point.unwrap_or(self.cursor);
        self.logical(x, y)
    }

    fn apply_mouse_behavior(&mut self) {
        let behavior = self.runtime.mouse_behavior();
        if behavior == self.behavior {
            return;
        }
        self.behavior = behavior;
        let Some(w) = self.window.clone() else {
            return;
        };
        if behavior == 0 {
            let _ = w.set_cursor_grab(CursorGrabMode::None);
            w.set_cursor_visible(true);
            self.lock_point = None;
            return;
        }
        if w.set_cursor_grab(CursorGrabMode::Locked).is_err() {
            let _ = w.set_cursor_grab(CursorGrabMode::Confined);
        }
        w.set_cursor_visible(behavior != 1);
        let point = if behavior == 1 {
            let size = w.inner_size();
            (size.width as f32 / 2.0, size.height as f32 / 2.0)
        } else {
            self.cursor
        };
        self.lock_point = Some(point);
        if behavior == 1 {
            let _ = w.set_cursor_position(PhysicalPosition::new(point.0 as f64, point.1 as f64));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn play_scripted(&mut self, input: ScriptedInput) {
        match input {
            ScriptedInput::Key(code) => {
                for pressed in [true, false] {
                    self.input(Input::Key {
                        code,
                        pressed,
                        text: None,
                    });
                }
            }
            ScriptedInput::Verb(verb) => self.do_verb(&verb),
            ScriptedInput::Click(x, y) => {
                let p = Vec3::new(x, y, 0.0);
                self.send(
                    InputKind::Changed,
                    user_input::MOUSE_MOVEMENT,
                    0,
                    p,
                    Vec3::ZERO,
                );
                for kind in [InputKind::Began, InputKind::Ended] {
                    self.send(kind, user_input::MOUSE_BUTTON1, 0, p, Vec3::ZERO);
                }
            }
        }
    }

    fn release_keys(&mut self) {
        let keys: Vec<u32> = self.keys.drain().collect();
        for k in keys {
            self.send(
                InputKind::Ended,
                user_input::KEYBOARD,
                k,
                Vec3::ZERO,
                Vec3::ZERO,
            );
        }
    }

    fn render(&mut self) {
        let Some(gpu) = &mut self.gpu else {
            return;
        };
        let frame = match gpu.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => return,
            _ => {
                gpu.surface.configure(&gpu.renderer.device, &gpu.config);
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        {
            let e = self.runtime.engine.lock();
            gpu.renderer
                .render(&e.dm, &view, gpu.config.width, gpu.config.height);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(rec) = &mut self.recorder
            && rec.wants_frame()
            && rec.width == gpu.config.width
            && rec.height == gpu.config.height
            && let Some(pixels) =
                video::read_texture(&gpu.renderer, &frame.texture, rec.width, rec.height)
        {
            rec.push(pixels);
        }
        gpu.renderer.queue.present(frame);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn run_verbs(&mut self) {
        for verb in self.runtime.take_verbs() {
            self.do_verb(&verb);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn do_verb(&mut self, verb: &str) {
        let home = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let stamp = rbx_base::time::unix_seconds() as u64;
        match verb {
            "Screenshot" => {
                let dir = home.join("Pictures").join("Roblox");
                let _ = std::fs::create_dir_all(&dir);
                let path = dir.join(format!("RobloxScreenShot{stamp}.png"));
                self.take_screenshot(&path);
                self.runtime
                    .show_on_screen_message(0, "Screenshot saved", 2.0);
            }
            "RecordToggle" => {
                if let Some(rec) = self.recorder.take() {
                    self.set_recording_surface(false);
                    match rec.finish() {
                        Ok((path, frames)) => self.runtime.engine.lock().print(
                            rbx_script::MessageType::Info,
                            format!(
                                "Video recording stopped: {frames} frames saved to {}",
                                path.display()
                            ),
                        ),
                        Err(e) => self.runtime.engine.lock().print(
                            rbx_script::MessageType::Error,
                            format!("Video recording failed: {e}"),
                        ),
                    }
                    self.runtime.fire_video_recording_changed(false);
                    return;
                }
                let Some((width, height, bgra, supported)) = self.gpu.as_ref().map(|g| {
                    (
                        g.config.width,
                        g.config.height,
                        matches!(
                            g.config.format,
                            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
                        ),
                        g.copy_src,
                    )
                }) else {
                    return;
                };
                if !supported {
                    self.runtime.engine.lock().print(
                        rbx_script::MessageType::Error,
                        "Video recording is not supported by this graphics backend",
                    );
                    return;
                }
                let quality = self.video_quality();
                match video::VideoRecorder::start(
                    video::video_path(&home, &stamp.to_string()),
                    width,
                    height,
                    30,
                    quality,
                    bgra,
                ) {
                    Ok(rec) => {
                        self.recorder = Some(rec);
                        self.set_recording_surface(true);
                        self.runtime
                            .engine
                            .lock()
                            .print(rbx_script::MessageType::Info, "Video recording started");
                        self.runtime.fire_video_recording_changed(true);
                    }
                    Err(e) => self.runtime.engine.lock().print(
                        rbx_script::MessageType::Error,
                        format!("Video recording failed: {e}"),
                    ),
                }
            }
            _ => {}
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn video_quality(&self) -> u8 {
        let e = self.runtime.engine.lock();
        let level =
            e.dm.descendants(e.dm.root())
                .into_iter()
                .find(|&d| e.dm.class_of(d) == Some("GameSettings"))
                .and_then(|g| e.dm.get(g, "VideoQuality").ok())
                .and_then(|v| v.as_enum())
                .unwrap_or(1);
        match level {
            0 => 60,
            2 => 92,
            _ => 80,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn set_recording_surface(&mut self, recording: bool) {
        if let Some(gpu) = &mut self.gpu {
            if recording {
                gpu.config.usage |= wgpu::TextureUsages::COPY_SRC;
            } else {
                gpu.config.usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
            }
            gpu.surface.configure(&gpu.renderer.device, &gpu.config);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn take_screenshot(&mut self, path: &Path) {
        let (w, h) = match &self.gpu {
            Some(g) => (g.config.width, g.config.height),
            None => (1280, 720),
        };
        let assets = self.assets.clone();
        match rbx_render::Offscreen::new(
            w,
            h,
            &assets.join("content"),
            &assets.join("platform-content").join("pc"),
        ) {
            Ok(mut off) => {
                off.renderer.gui_scale = self.gui_scale();
                let e = self.runtime.engine.lock();
                if let Err(err) = off.save_png(&e.dm, path) {
                    tracing::error!("screenshot failed: {err}");
                } else {
                    tracing::info!("saved {}", path.display());
                }
            }
            Err(err) => tracing::error!("screenshot failed: {err}"),
        }
    }
}

impl App for Player {
    fn init(&mut self, window: Arc<Window>) {
        self.window = Some(window.clone());
        let assets = self.assets.clone();
        let slot = self.pending_gpu.clone();
        let task = async move {
            match create_gpu(window, assets).await {
                Ok(gpu) => *slot.lock() = Some(gpu),
                Err(e) => tracing::error!("{e}"),
            }
        };
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(task);
        #[cfg(not(target_arch = "wasm32"))]
        {
            pollster::block_on(task);
            if self.pending_gpu.lock().is_none() {
                self.done = true;
            }
        }
    }

    fn input(&mut self, input: Input) {
        if matches!(
            input,
            Input::Key { .. } | Input::MouseButton { .. } | Input::Touch { .. }
        ) {
            self.runtime.engine.lock().audio.resume_output();
        }
        match input {
            Input::Key {
                code,
                pressed,
                text,
            } => {
                #[cfg(not(target_arch = "wasm32"))]
                if pressed && code == 316 {
                    self.do_verb("Screenshot");
                }
                let kind = if pressed {
                    self.keys.insert(code);
                    InputKind::Began
                } else {
                    self.keys.remove(&code);
                    InputKind::Ended
                };
                self.runtime.input(InputEvent {
                    kind,
                    input_type: user_input::KEYBOARD,
                    key_code: code,
                    position: Vec3::ZERO,
                    delta: Vec3::ZERO,
                    text: if pressed { text } else { None },
                });
            }
            Input::MouseButton {
                button, pressed, ..
            } => {
                let input_type = match button {
                    1 => user_input::MOUSE_BUTTON1,
                    2 => user_input::MOUSE_BUTTON2,
                    3 => user_input::MOUSE_BUTTON3,
                    _ => return,
                };
                let kind = if pressed {
                    InputKind::Began
                } else {
                    InputKind::Ended
                };
                self.send(kind, input_type, 0, self.pointer(), Vec3::ZERO);
            }
            Input::MouseMove { x, y } => {
                if let Some(point) = self.lock_point {
                    if let Some(w) = &self.window
                        && (x, y) != point
                    {
                        let _ = w.set_cursor_position(PhysicalPosition::new(
                            point.0 as f64,
                            point.1 as f64,
                        ));
                    }
                    return;
                }
                let before = self.logical(self.cursor.0, self.cursor.1);
                self.cursor = (x, y);
                let now = self.logical(x, y);
                self.send(
                    InputKind::Changed,
                    user_input::MOUSE_MOVEMENT,
                    0,
                    now,
                    now - before,
                );
            }
            Input::MouseDelta { dx, dy } => {
                if self.lock_point.is_some() {
                    self.send(
                        InputKind::Changed,
                        user_input::MOUSE_MOVEMENT,
                        0,
                        self.pointer(),
                        Vec3::new(dx, dy, 0.0),
                    );
                }
            }
            Input::Wheel { delta } => {
                let mut p = self.pointer();
                p.z = delta;
                self.send(
                    InputKind::Changed,
                    user_input::MOUSE_WHEEL,
                    0,
                    p,
                    Vec3::ZERO,
                );
            }
            Input::Focus(focused) => {
                if !focused {
                    self.release_keys();
                }
                self.runtime.window_focus(focused);
            }
            Input::Touch { id, phase, x, y } => {
                let kind = match phase {
                    rbx_platform::TouchStage::Began => InputKind::Began,
                    rbx_platform::TouchStage::Moved => InputKind::Changed,
                    rbx_platform::TouchStage::Ended => InputKind::Ended,
                };
                let position = self.logical(x, y);
                self.send(kind, user_input::TOUCH, id as u32, position, Vec3::ZERO);
            }
            Input::Resized { width, height } => {
                if width > 0 && height > 0 {
                    self.surface_size = Some((width, height));
                }
                if let Some(gpu) = &mut self.gpu
                    && width > 0
                    && height > 0
                {
                    gpu.config.width = width;
                    gpu.config.height = height;
                    gpu.surface.configure(&gpu.renderer.device, &gpu.config);
                }
            }
        }
    }

    fn frame(&mut self, dt: f64) {
        #[cfg(target_arch = "wasm32")]
        if let Some((width, height)) = self.window.as_ref().and_then(|w| web::canvas_size(w))
            && self.surface_size != Some((width, height))
        {
            self.input(Input::Resized { width, height });
        }
        let ready = if self.gpu.is_none() {
            self.pending_gpu.lock().take()
        } else {
            None
        };
        if let Some(gpu) = ready {
            self.gpu = Some(gpu);
            let size = self.surface_size.or_else(|| {
                self.window
                    .as_ref()
                    .map(|w| w.inner_size())
                    .map(|s| (s.width, s.height))
            });
            if let Some((width, height)) = size {
                self.input(Input::Resized { width, height });
            }
        }
        self.update_gui_view();
        if let Some(gpu) = &mut self.gpu {
            gpu.renderer.frame_dt = dt as f32;
        }
        self.accumulator += dt.min(0.25);
        let mut steps = 0;
        while self.accumulator >= STEP && steps < 4 {
            self.step();
            self.accumulator -= STEP;
            steps += 1;
        }
        self.apply_mouse_behavior();
        self.ensure_camera();
        self.runtime.render_step(dt);
        #[cfg(not(target_arch = "wasm32"))]
        self.run_verbs();
        self.render();
        self.frames += 1;
        #[cfg(target_arch = "wasm32")]
        if self.frames % 30 == 0 {
            web::report_frames(self.frames);
        }
        #[cfg(not(target_arch = "wasm32"))]
        let due: Vec<ScriptedInput> = self
            .scripted
            .iter()
            .filter(|(f, _)| *f == self.frames)
            .map(|(_, i)| i.clone())
            .collect();
        #[cfg(not(target_arch = "wasm32"))]
        for input in due {
            self.play_scripted(input);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some((path, at)) = self.screenshot.clone()
            && self.frames >= at
        {
            self.take_screenshot(&path);
            if let Some(dump) = self.dump_gui.clone() {
                let mut text = dump_gui(&self.runtime.engine.lock().dm);
                for item in rbx_render::gui::extract(&self.runtime.engine.lock().dm) {
                    text.push_str(&format!("ITEM {:?} clip={:?}\n", item.cmd, item.clip));
                }
                if let Err(e) = std::fs::write(&dump, text) {
                    tracing::error!("{}: {e}", dump.display());
                }
            }
            self.screenshot = None;
            self.done = true;
        }
    }

    fn should_exit(&self) -> bool {
        self.done
    }
}

#[cfg(not(any(target_arch = "wasm32", target_os = "ios", target_os = "android")))]
pub fn desktop_main() {
    let args = Args::parse();
    rbx_base::log::init(&args.log);
    let assets = find_assets(args.assets.clone());
    let (runtime, net) = if let Some(addr) = args.join {
        let hash = match args.cert.as_deref().and_then(decode_hash) {
            Some(h) => h,
            None => {
                eprintln!("--join needs --cert with the server certificate hash");
                std::process::exit(2);
            }
        };
        let conn = match rbx_net::quic::connect(addr, hash) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("could not connect to {addr}: {e}");
                std::process::exit(1);
            }
        };
        let mut dm = DataModel::new();
        for s in [
            "Workspace",
            "Players",
            "Lighting",
            "ReplicatedStorage",
            "StarterGui",
            "StarterPack",
        ] {
            let _ = dm.get_service(s);
        }
        let runtime = match make_runtime(dm, &assets) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        };
        let net = NetClient::connect(&runtime, Box::new(conn));
        if let Err(e) = spawn_local_player(&runtime, &args.name, false) {
            eprintln!("{e}");
        }
        if let Err(e) = runtime.start_core_scripts("StarterScript") {
            eprintln!("{e}");
        }
        (runtime, Some(net))
    } else {
        let mut dm = DataModel::new();
        if let Some(place) = &args.place {
            let data = match std::fs::read(place) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("{}: {e}", place.display());
                    std::process::exit(1);
                }
            };
            if let Err(e) = dm.load_place(&data) {
                eprintln!("{}: {e}", place.display());
                std::process::exit(1);
            }
        }
        dm.ensure_terrain();
        let runtime = match make_runtime(dm, &assets) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        };
        if let Err(e) = spawn_local_player(&runtime, &args.name, true) {
            eprintln!("{e}");
        }
        runtime.scan_scripts();
        if let Err(e) = runtime.start_server_core_scripts() {
            eprintln!("{e}");
        }
        if let Err(e) = runtime.start_core_scripts("StarterScript") {
            eprintln!("{e}");
        }
        (runtime, None)
    };
    let player = Player {
        runtime,
        net,
        window: None,
        gpu: None,
        pending_gpu: Arc::new(Mutex::new(None)),
        surface_size: None,
        assets,
        accumulator: 0.0,
        keys: HashSet::new(),
        cursor: (0.0, 0.0),
        behavior: 0,
        lock_point: None,
        screenshot: args.screenshot.map(|p| (p, args.screenshot_frames)),
        scripted: args.inputs.iter().filter_map(|s| parse_input(s)).collect(),
        dump_gui: args.dump_gui.clone(),
        recorder: None,
        frames: 0,
        done: false,
    };
    if let Err(e) = rbx_platform::run("Roblox", (1280, 720), player) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use super::*;
    use std::io::Write;

    const DEFAULT_PLACE: &str = "tests/places/RobloxStudio/content/FirstPlaceBasePlate.rbxl";

    struct ConsoleWriter(Vec<u8>);

    impl Write for ConsoleWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Drop for ConsoleWriter {
        fn drop(&mut self) {
            let text = String::from_utf8_lossy(&self.0);
            let text = text.trim_end();
            if !text.is_empty() {
                web_sys::console::log_1(&text.into());
            }
        }
    }

    fn init_logging() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new("info"))
            .with_writer(|| ConsoleWriter(Vec::new()))
            .with_ansi(false)
            .without_time()
            .with_target(false)
            .try_init();
    }

    fn query(name: &str) -> Option<String> {
        let search = web_sys::window()?.location().search().ok()?;
        web_sys::UrlSearchParams::new_with_str(&search)
            .ok()?
            .get(name)
    }

    pub fn canvas_size(window: &Window) -> Option<(u32, u32)> {
        use rbx_platform::winit::platform::web::WindowExtWebSys;
        let canvas = window.canvas()?;
        let ratio = web_sys::window()?.device_pixel_ratio();
        let width = (canvas.client_width() as f64 * ratio).round() as u32;
        let height = (canvas.client_height() as f64 * ratio).round() as u32;
        if width == 0 || height == 0 {
            return None;
        }
        if canvas.width() != width || canvas.height() != height {
            canvas.set_width(width);
            canvas.set_height(height);
        }
        Some((width, height))
    }

    pub fn report_frames(frames: u32) {
        if let Some(root) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.document_element())
        {
            let _ = root.set_attribute("data-frames", &frames.to_string());
        }
    }

    fn touch_capable() -> bool {
        web_sys::window().is_some_and(|w| w.navigator().max_touch_points() > 0)
    }

    #[cfg(target_feature = "atomics")]
    pub use wasm_bindgen_rayon::init_thread_pool;

    #[wasm_bindgen::prelude::wasm_bindgen]
    pub fn threaded() -> bool {
        rbx_base::jobs::threaded()
    }

    #[wasm_bindgen::prelude::wasm_bindgen]
    pub fn run() {
        rbx_script::init_c_runtime();
        console_error_panic_hook::set_once();
        init_logging();
        let assets = PathBuf::from("assets");
        let place = query("place").unwrap_or_else(|| DEFAULT_PLACE.to_string());
        let name = query("name").unwrap_or_else(|| "Player1".to_string());
        let mut dm = DataModel::new();
        match rbx_base::vfs::read(&place) {
            Ok(data) => {
                if let Err(e) = dm.load_place(&data) {
                    tracing::error!("{place}: {e}");
                }
            }
            Err(e) => tracing::error!("{place}: {e}"),
        }
        dm.ensure_terrain();
        let runtime = match make_runtime(dm, &assets) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("{e}");
                return;
            }
        };
        runtime.set_touch_enabled(touch_capable());
        if let Err(e) = spawn_local_player(&runtime, &name, true) {
            tracing::error!("{e}");
        }
        runtime.scan_scripts();
        if let Err(e) = runtime.start_server_core_scripts() {
            tracing::error!("{e}");
        }
        if let Err(e) = runtime.start_core_scripts("StarterScript") {
            tracing::error!("{e}");
        }
        tracing::info!("loaded {place}");
        let player = Player {
            runtime,
            net: None,
            window: None,
            gpu: None,
            pending_gpu: Arc::new(Mutex::new(None)),
            surface_size: None,
            assets,
            accumulator: 0.0,
            keys: HashSet::new(),
            cursor: (0.0, 0.0),
            behavior: 0,
            lock_point: None,
            frames: 0,
            done: false,
        };
        if let Err(e) = rbx_platform::run("Roblox", (1280, 720), player) {
            tracing::error!("{e}");
        }
    }
}

#[cfg(any(target_os = "ios", target_os = "android"))]
pub mod mobile {
    use super::*;

    fn startup_place(assets: &Path) -> DataModel {
        let mut dm = DataModel::new();
        let place = assets.join("startup.rbxl");
        match rbx_base::vfs::read(&place) {
            Ok(data) => {
                if let Err(e) = dm.load_place(&data) {
                    tracing::error!("{}: {e}", place.display());
                }
            }
            Err(_) => {
                if let Ok(ws) = dm.get_service("Workspace")
                    && let Ok(base) = dm.create("Part")
                {
                    dm.set_name(base, "BasePlate");
                    let _ = dm.set(base, "Anchored", Variant::Bool(true));
                    let _ = dm.set(
                        base,
                        "Size",
                        Variant::Vector3(Vec3::new(512.0, 20.0, 512.0)),
                    );
                    let _ = dm.set(
                        base,
                        "CFrame",
                        Variant::CFrame(rbx_math::CFrame::from_translation(Vec3::new(
                            0.0, -10.0, 0.0,
                        ))),
                    );
                    let _ = dm.set_parent(base, Some(ws));
                }
            }
        }
        dm.ensure_terrain();
        dm
    }

    pub fn start(assets: PathBuf) {
        rbx_base::log::init("info");
        let runtime = match make_runtime(startup_place(&assets), &assets) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("{e}");
                return;
            }
        };
        runtime.set_touch_enabled(true);
        if let Err(e) = spawn_local_player(&runtime, "Player1", true) {
            tracing::error!("{e}");
        }
        runtime.scan_scripts();
        if let Err(e) = runtime.start_server_core_scripts() {
            tracing::error!("{e}");
        }
        if let Err(e) = runtime.start_core_scripts("StarterScript") {
            tracing::error!("{e}");
        }
        let player = Player {
            runtime,
            net: None,
            window: None,
            gpu: None,
            pending_gpu: Arc::new(Mutex::new(None)),
            surface_size: None,
            assets,
            accumulator: 0.0,
            keys: HashSet::new(),
            cursor: (0.0, 0.0),
            behavior: 0,
            lock_point: None,
            screenshot: None,
            scripted: Vec::new(),
            dump_gui: None,
            recorder: None,
            frames: 0,
            done: false,
        };
        if let Err(e) = rbx_platform::run("Roblox", (1280, 720), player) {
            tracing::error!("{e}");
        }
    }

    #[cfg(target_os = "ios")]
    pub fn ios_main() {
        let assets = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("assets")))
            .unwrap_or_else(|| PathBuf::from("assets"));
        start(assets);
    }
}

#[cfg(target_os = "android")]
fn android_start(app: rbx_platform::AndroidApp) {
    let assets = app
        .internal_data_path()
        .map(|p| p.join("assets"))
        .unwrap_or_else(|| PathBuf::from("assets"));
    mobile::start(assets);
}

rbx_platform::android_entry!(android_start);
