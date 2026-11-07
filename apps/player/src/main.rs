use clap::Parser;
use parking_lot::Mutex;
use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{CFrame, Vec3};
use rbx_platform::winit::window::Window;
use rbx_platform::{App, Input};
use rbx_render::{Renderer, wgpu};
use rbx_script::net::NetClient;
use rbx_script::{Engine, ScriptRuntime};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const STEP: f64 = 1.0 / 60.0;

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
    #[arg(long, default_value = "info")]
    log: String,
}

struct OrbitCamera {
    yaw: f32,
    pitch: f32,
    distance: f32,
    focus: Vec3,
}

impl OrbitCamera {
    fn cframe(&self) -> CFrame {
        let dir = Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        );
        let eye = self.focus + dir * self.distance;
        CFrame::look_at(eye, self.focus, Vec3::Y)
    }
}

struct Gpu {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
}

struct Player {
    runtime: ScriptRuntime,
    net: Option<NetClient>,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    assets: PathBuf,
    accumulator: f64,
    camera: OrbitCamera,
    keys: HashSet<u32>,
    rotating: bool,
    screenshot: Option<(PathBuf, u32)>,
    frames: u32,
    done: bool,
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

impl Player {
    fn character_head(&self) -> Option<Vec3> {
        let e = self.runtime.engine.lock();
        let player = e.dm.local_player?;
        let ch = e.dm.get_ref(player, "Character")?;
        let head =
            e.dm.find_first_child(ch, "Head", false)
                .or_else(|| e.dm.find_first_child(ch, "Torso", false))?;
        e.dm.get_raw(head, "CFrame")
            .and_then(|v| v.as_cframe())
            .map(|c| c.translation)
    }

    fn humanoid(&self) -> Option<InstanceId> {
        let e = self.runtime.engine.lock();
        let player = e.dm.local_player?;
        let ch = e.dm.get_ref(player, "Character")?;
        e.dm.find_first_child_of_class(ch, "Humanoid")
    }

    fn drive_character(&mut self) {
        let Some(h) = self.humanoid() else {
            return;
        };
        let mut forward = 0.0f32;
        let mut right = 0.0f32;
        for (code, f, r) in [
            (119, 1.0, 0.0),
            (115, -1.0, 0.0),
            (97, 0.0, -1.0),
            (100, 0.0, 1.0),
            (273, 1.0, 0.0),
            (274, -1.0, 0.0),
        ] {
            if self.keys.contains(&code) {
                forward += f;
                right += r;
            }
        }
        let look = Vec3::new(-self.camera.yaw.sin(), 0.0, -self.camera.yaw.cos());
        let side = Vec3::new(-look.z, 0.0, look.x);
        let dir = (look * forward + side * right).normalize_or_zero();
        let mut e = self.runtime.engine.lock();
        let _ = e.dm.set(h, "WalkDirection", Variant::Vector3(dir));
        if self.keys.contains(&32) {
            let _ = e.dm.set(h, "Jump", Variant::Bool(true));
        }
    }

    fn step(&mut self) {
        self.drive_character();
        if let Some(net) = &mut self.net {
            net.tick(&self.runtime);
        }
        self.runtime.step(STEP);
        if let Some(net) = &mut self.net {
            net.tick(&self.runtime);
        }
    }

    fn update_camera(&mut self) {
        if let Some(head) = self.character_head() {
            self.camera.focus = head;
        }
        let cf = self.camera.cframe();
        let mut e = self.runtime.engine.lock();
        let Some(ws) = e.dm.find_service("Workspace") else {
            return;
        };
        let cam =
            e.dm.get_ref(ws, "CurrentCamera")
                .or_else(|| e.dm.find_first_child_of_class(ws, "Camera"));
        let cam = match cam {
            Some(c) => c,
            None => match e.dm.create("Camera") {
                Ok(c) => {
                    let _ = e.dm.set_parent(c, Some(ws));
                    let _ = e.dm.set(ws, "CurrentCamera", Variant::Ref(Some(c)));
                    c
                }
                Err(_) => return,
            },
        };
        e.dm.store_silent(cam, "CFrame", Variant::CFrame(cf));
        e.dm.store_silent(
            cam,
            "Focus",
            Variant::CFrame(CFrame::from_translation(self.camera.focus)),
        );
    }

    fn render(&mut self) {
        let Some(gpu) = &mut self.gpu else {
            return;
        };
        let frame = match gpu.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
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
        gpu.renderer.queue.present(frame);
    }

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
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = match instance.create_surface(window.clone()) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("cannot create a surface: {e}");
                self.done = true;
                return;
            }
        };
        let adapter =
            match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
                apply_limit_buckets: false,
            })) {
                Ok(a) => a,
                Err(e) => {
                    tracing::error!("no graphics adapter: {e}");
                    self.done = true;
                    return;
                }
            };
        let (device, queue) =
            match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())) {
                Ok(d) => d,
                Err(e) => {
                    tracing::error!("no graphics device: {e}");
                    self.done = true;
                    return;
                }
            };
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
        let renderer = Renderer::new(
            device,
            queue,
            format,
            &self.assets.join("content"),
            &self.assets.join("platform-content").join("pc"),
        );
        self.gpu = Some(Gpu {
            surface,
            config,
            renderer,
        });
        self.window = Some(window);
    }

    fn input(&mut self, input: Input) {
        match input {
            Input::Key { code, pressed, .. } => {
                if pressed {
                    self.keys.insert(code);
                } else {
                    self.keys.remove(&code);
                }
            }
            Input::MouseButton {
                button: 2, pressed, ..
            } => self.rotating = pressed,
            Input::MouseDelta { dx, dy } if self.rotating => {
                self.camera.yaw -= dx * 0.008;
                self.camera.pitch = (self.camera.pitch + dy * 0.008).clamp(-1.4, 1.4);
            }
            Input::Wheel { delta } => {
                self.camera.distance =
                    (self.camera.distance * (1.0 - delta * 0.1)).clamp(0.5, 400.0);
            }
            Input::Resized { width, height } => {
                if let Some(gpu) = &mut self.gpu
                    && width > 0
                    && height > 0
                {
                    gpu.config.width = width;
                    gpu.config.height = height;
                    gpu.surface.configure(&gpu.renderer.device, &gpu.config);
                }
            }
            _ => {}
        }
    }

    fn frame(&mut self, dt: f64) {
        self.accumulator += dt.min(0.25);
        let mut steps = 0;
        while self.accumulator >= STEP && steps < 4 {
            self.step();
            self.accumulator -= STEP;
            steps += 1;
        }
        self.update_camera();
        self.render();
        self.frames += 1;
        if let Some((path, at)) = self.screenshot.clone()
            && self.frames >= at
        {
            self.take_screenshot(&path);
            self.screenshot = None;
            self.done = true;
        }
    }

    fn should_exit(&self) -> bool {
        self.done
    }
}

fn main() {
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
        (runtime, None)
    };
    let player = Player {
        runtime,
        net,
        window: None,
        gpu: None,
        assets,
        accumulator: 0.0,
        camera: OrbitCamera {
            yaw: 0.6,
            pitch: 0.35,
            distance: 16.0,
            focus: Vec3::ZERO,
        },
        keys: HashSet::new(),
        rotating: false,
        screenshot: args.screenshot.map(|p| (p, args.screenshot_frames)),
        frames: 0,
        done: false,
    };
    if let Err(e) = rbx_platform::run("Roblox", (1280, 720), player) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
