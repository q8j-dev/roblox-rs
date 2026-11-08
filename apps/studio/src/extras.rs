use crate::app::Studio;
use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke};
use rbx_dom::InstanceId;
use rbx_math::Vec3;
use rbx_render::adorn::{AdornLine, Adorns};
use rbx_script::MessageType;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Child;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const GRID_SIZE_BASE: f32 = 400.0;
const GRID_FACTOR: i32 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinMode {
    Always,
    Surface,
    Never,
}

pub struct Extras {
    pub show_grid: bool,
    pub grid_studs: i32,
    pub axis_widget: bool,
    pub muted: bool,
    pub analyze_physics: bool,
    pub join_mode: JoinMode,
    pub material_as_tool: bool,
    pub color_as_tool: bool,
    pub start_server: bool,
    pub players: usize,
    pub stats: [bool; 6],
    pub children: Vec<Child>,
    pub pending_fullscreen: bool,
    pub pending_screenshot: bool,
    pub launch: Option<mpsc::Receiver<Result<String, String>>>,
    pub show_about: bool,
    pub show_shortcuts: bool,
    pub show_settings: bool,
    pub settings_selected: Option<InstanceId>,
    pub settings_props: crate::properties::Properties,
    pub paint_color: Option<u16>,
    pub paint_material: Option<u32>,
}

impl Default for Extras {
    fn default() -> Self {
        Extras {
            show_grid: false,
            grid_studs: 4,
            axis_widget: false,
            muted: false,
            analyze_physics: false,
            join_mode: JoinMode::Always,
            material_as_tool: false,
            color_as_tool: false,
            start_server: true,
            players: 1,
            stats: [false; 6],
            children: Vec::new(),
            pending_fullscreen: false,
            pending_screenshot: false,
            launch: None,
            show_about: false,
            show_shortcuts: false,
            show_settings: false,
            settings_selected: None,
            settings_props: crate::properties::Properties::default(),
            paint_color: None,
            paint_material: None,
        }
    }
}

pub fn zero_plane_grid(out: &mut Adorns, camera: Vec3, studs: i32) {
    let studs = studs.max(1);
    let center = Vec3::new(camera.x, 0.0, camera.z);
    let snap = |v: f32| ((v + studs as f32 / 2.0) / studs as f32).round() as i32 * studs;
    let on_grid = Vec3::new(snap(center.x) as f32, 0.0, snap(center.z) as f32);
    let dist = (camera - center).length();
    let small = [0.3, 0.3, 0.3];
    let large = [0.4, 0.4, 0.4];
    let large_step = studs * GRID_FACTOR;
    let base = GRID_SIZE_BASE as i32;
    if dist < GRID_SIZE_BASE {
        let z_start = center.z - GRID_SIZE_BASE;
        let z_end = center.z + GRID_SIZE_BASE;
        let x_start = center.x - GRID_SIZE_BASE;
        let x_end = center.x + GRID_SIZE_BASE;
        let cx = (on_grid.x / studs as f32).round() as i32 * studs;
        let cz = (on_grid.z / studs as f32).round() as i32 * studs;
        let mut i = -base + cx;
        while i < base + cx {
            let mut last = z_start;
            let mut j = z_start as i32 + large_step;
            while j as f32 <= z_end + large_step as f32 {
                let mid = Vec3::new(i as f32, 0.0, j as f32 + (j as f32 - last) / 2.0);
                let alpha = (1.0 - (mid - camera).length() / GRID_SIZE_BASE).max(0.0);
                if alpha > 0.0 {
                    out.lines.push(AdornLine {
                        a: Vec3::new(i as f32, 0.0, last),
                        b: Vec3::new(i as f32, 0.0, j as f32),
                        color: [small[0], small[1], small[2], alpha],
                    });
                }
                last = j as f32;
                j += large_step;
            }
            i += studs;
        }
        let mut i = -base + cz;
        while i < base + cz {
            let mut last = x_start;
            let mut j = x_start as i32 + large_step;
            while j as f32 <= x_end + large_step as f32 {
                let mid = Vec3::new(j as f32 + (j as f32 - last) / 2.0, 0.0, i as f32);
                let alpha = (1.0 - (mid - camera).length() / GRID_SIZE_BASE).max(0.0);
                if alpha > 0.0 {
                    out.lines.push(AdornLine {
                        a: Vec3::new(last, 0.0, i as f32),
                        b: Vec3::new(j as f32, 0.0, i as f32),
                        color: [small[0], small[1], small[2], alpha],
                    });
                }
                last = j as f32;
                j += large_step;
            }
            i += studs;
        }
    }
    let lx = (on_grid.x / large_step as f32).round() as i32 * large_step;
    let lz = (on_grid.z / large_step as f32).round() as i32 * large_step;
    let mut i = -base + lx;
    while i <= base + lx {
        let a = Vec3::new(i as f32, 0.0, (-base + lz) as f32);
        let b = Vec3::new(i as f32, 0.0, (base + lz) as f32);
        let mid = b + (b - a) * 0.5;
        let alpha = (1.0 - ((camera - mid).length() * 1.5) / GRID_SIZE_BASE).max(0.25);
        out.lines.push(AdornLine { a, b, color: [large[0], large[1], large[2], alpha] });
        i += large_step;
    }
    let mut i = -base + lz;
    while i <= base + lz {
        let a = Vec3::new(i as f32, 0.0, (-base + lz) as f32);
        let b = Vec3::new(i as f32, 0.0, (base + lz) as f32);
        let mid = b + (b - a) * 0.5;
        let alpha = (1.0 - ((camera - mid).length() * 1.5) / GRID_SIZE_BASE).max(0.25);
        out.lines.push(AdornLine {
            a: Vec3::new((-base + lx) as f32, 0.0, i as f32),
            b: Vec3::new((base + lx) as f32, 0.0, i as f32),
            color: [large[0], large[1], large[2], alpha],
        });
        i += large_step;
    }
    for (axis, color) in [(Vec3::X, [1.0, 0.0, 0.0, 1.0]), (Vec3::Y, [0.0, 1.0, 0.0, 1.0]), (Vec3::Z, [0.0, 0.0, 1.0, 1.0])] {
        rbx_render::adorn::overlay_ray(out, camera, Vec3::ZERO, axis * 4.0, color);
    }
}

pub fn axis_widget(painter: &egui::Painter, rect: Rect, cam: &crate::viewport::FlyCamera) {
    let cf = cam.cframe();
    let origin = cf.translation + cf.look_vector() * 2.0;
    let offset = egui::vec2(rect.width() / 2.0 - 50.0, -(rect.height() / 2.0) + 50.0);
    let Some(o) = cam.project(rect, origin) else { return };
    let o = o - offset;
    for (axis, color, label, nudge) in [
        (Vec3::X, Color32::RED, "X", 0.0),
        (Vec3::Y, Color32::GREEN, "Y", 3.0),
        (Vec3::Z, Color32::BLUE, "Z", 0.0),
    ] {
        let Some(p) = cam.project(rect, origin + axis * 0.1) else { continue };
        let p = p - offset;
        painter.line_segment([o, p], Stroke::new(1.0, color));
        painter.text(p + egui::vec2(nudge, 0.0), Align2::LEFT_TOP, label, FontId::proportional(12.0), color);
    }
}

pub fn stats_overlay(painter: &egui::Painter, rect: Rect, studio: &Studio, fps: f32) {
    let names = ["Stats", "Render", "Physics", "Network", "Summary", "Custom"];
    let e = studio.doc.runtime.engine.lock();
    let parts = e
        .dm
        .find_service("Workspace")
        .map(|ws| e.dm.descendants(ws).into_iter().filter(|d| e.dm.is_a(*d, "BasePart")).count())
        .unwrap_or(0);
    let instances = e.dm.descendants(e.dm.root()).len();
    let mut y = rect.top() + 8.0;
    for (i, on) in studio.extras.stats.iter().enumerate() {
        if !on {
            continue;
        }
        let lines: Vec<String> = match i {
            0 => vec![format!("Instances: {instances}"), format!("Game time: {:.1}", e.game_time)],
            1 => vec![format!("FPS: {fps:.1}"), format!("Frame: {:.2} ms", 1000.0 / fps.max(1.0))],
            2 => vec![format!("Parts: {parts}"), format!("Physics FPS: {:.1}", e.fps)],
            3 => vec!["Network: not connected".to_string()],
            4 => vec![format!("FPS {fps:.1}"), format!("Parts {parts}"), format!("Instances {instances}")],
            _ => vec!["No custom stats".to_string()],
        };
        let h = 18.0 + lines.len() as f32 * 15.0;
        let r = Rect::from_min_size(Pos2::new(rect.left() + 8.0, y), egui::vec2(200.0, h));
        painter.rect_filled(r, 0.0, Color32::from_black_alpha(140));
        painter.text(r.left_top() + egui::vec2(6.0, 3.0), Align2::LEFT_TOP, names[i], FontId::proportional(12.0), Color32::WHITE);
        for (n, l) in lines.iter().enumerate() {
            painter.text(
                r.left_top() + egui::vec2(10.0, 18.0 + n as f32 * 15.0),
                Align2::LEFT_TOP,
                l,
                FontId::monospace(11.0),
                Color32::from_gray(220),
            );
        }
        y = r.bottom() + 6.0;
    }
}

fn exe(name: &str) -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(|d| d.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)))
        .filter(|p| p.is_file())
}

impl Studio {
    pub fn insert_into_selection(&mut self, class: &str) {
        let parent = {
            let sel = self.doc.selection();
            let e = self.doc.runtime.engine.lock();
            sel.first().copied().or_else(|| e.dm.find_service("Workspace"))
        };
        if let Some(p) = parent {
            match self.doc.insert(class, p) {
                Ok(id) => self.doc.select(vec![id]),
                Err(e) => self.status.push((MessageType::Error, e)),
            }
        }
    }

    pub fn import_model(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Roblox Model Files", &["rbxm", "rbxmx"])
            .pick_file()
        else {
            return;
        };
        let data = match std::fs::read(&path) {
            Ok(d) => d,
            Err(e) => return self.status.push((MessageType::Error, format!("{}: {e}", path.display()))),
        };
        self.doc.checkpoint();
        let roots: Result<Vec<InstanceId>, String> = {
            let mut e = self.doc.runtime.engine.lock();
            let ws = e.dm.find_service("Workspace");
            e.dm.load_model(&data).map_err(|err| err.to_string()).map(|(roots, _)| {
                for r in &roots {
                    let _ = e.dm.set_parent(*r, ws);
                }
                roots
            })
        };
        match roots {
            Ok(r) => self.doc.select(r),
            Err(e) => self.status.push((MessageType::Error, e)),
        }
    }

    pub fn execute_script_file(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("Lua", &["lua", "txt"]).pick_file() else {
            return;
        };
        match std::fs::read_to_string(&path) {
            Ok(src) => {
                self.doc.checkpoint();
                if let Err(e) = self.doc.runtime.execute(&src, &path.display().to_string(), rbx_script::Identity::CmdLine) {
                    self.status.push((MessageType::Error, e.to_string()));
                }
            }
            Err(e) => self.status.push((MessageType::Error, format!("{}: {e}", path.display()))),
        }
    }

    pub fn toggle_mute(&mut self) {
        self.extras.muted = !self.extras.muted;
        let volume = if self.extras.muted { 0.0 } else { 1.0 };
        self.doc.runtime.engine.lock().audio.set_master_volume(volume);
    }

    pub fn start_servers(&mut self) {
        self.cleanup_servers();
        let place = std::env::temp_dir().join(format!("studio-server-{}", std::process::id()));
        if let Err(e) = std::fs::create_dir_all(&place) {
            return self.status.push((MessageType::Error, e.to_string()));
        }
        let bytes = match self.doc.bytes() {
            Ok(b) => b,
            Err(e) => return self.status.push((MessageType::Error, e)),
        };
        if let Err(e) = std::fs::write(place.join("server.rbxl"), bytes) {
            return self.status.push((MessageType::Error, e.to_string()));
        }
        let (Some(server), Some(player)) = (exe("server"), exe("player")) else {
            return self.status.push((MessageType::Error, "the server and player executables are not next to studio".into()));
        };
        let port = 64000 + (std::process::id() % 900) as u16;
        let http = format!("127.0.0.1:{port}");
        let child = std::process::Command::new(&server)
            .args(["--http", &http, "--game-bind", "127.0.0.1"])
            .arg("--places")
            .arg(&place)
            .arg("--content")
            .arg(self.assets.join("content"))
            .arg("--assets")
            .arg(&self.assets)
            .spawn();
        match child {
            Ok(c) => self.extras.children.push(c),
            Err(e) => return self.status.push((MessageType::Error, format!("{}: {e}", server.display()))),
        }
        let players = self.extras.players;
        let assets = self.assets.clone();
        let (tx, rx) = mpsc::channel();
        self.extras.launch = Some(rx);
        std::thread::spawn(move || {
            let base = format!("http://{http}");
            let start = Instant::now();
            let opened = loop {
                let r = ureq::post(format!("{base}/v1/jobs")).send_json(json!({
                    "job": { "id": "studio", "place": "server.rbxl", "expirationInSeconds": 86400 },
                    "script": { "source": "game:GetService('NetworkServer'):Start(0)" }
                }));
                if r.is_ok() {
                    break true;
                }
                if start.elapsed() > Duration::from_secs(15) {
                    break false;
                }
                std::thread::sleep(Duration::from_millis(200));
            };
            if !opened {
                let _ = tx.send(Err("the server did not start".into()));
                return;
            }
            let info = loop {
                let info: Option<Value> = ureq::get(format!("{base}/v1/jobs/studio"))
                    .call()
                    .ok()
                    .and_then(|mut r| r.body_mut().read_json::<Value>().ok());
                if let Some(i) = info.filter(|i| i["port"].as_u64().is_some() && i["certHash"].as_str().is_some()) {
                    break Some(i);
                }
                if start.elapsed() > Duration::from_secs(30) {
                    break None;
                }
                std::thread::sleep(Duration::from_millis(100));
            };
            let Some(info) = info else {
                let _ = tx.send(Err("the game server never opened a port".into()));
                return;
            };
            let port = info["port"].as_u64().unwrap_or(0);
            let hash = info["certHash"].as_str().unwrap_or("").to_string();
            let mut spawned = 0;
            for n in 1..=players {
                let r = std::process::Command::new(&player)
                    .arg("--join")
                    .arg(format!("127.0.0.1:{port}"))
                    .arg("--cert")
                    .arg(&hash)
                    .arg("--name")
                    .arg(format!("Player{n}"))
                    .arg("--assets")
                    .arg(&assets)
                    .spawn();
                if r.is_ok() {
                    spawned += 1;
                }
            }
            let _ = tx.send(Ok(format!("started a server on port {port} with {spawned} players")));
        });
    }

    pub fn cleanup_servers(&mut self) {
        for mut c in self.extras.children.drain(..) {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    pub fn poll_launch(&mut self) {
        let Some(rx) = &self.extras.launch else { return };
        if let Ok(r) = rx.try_recv() {
            match r {
                Ok(m) => self.status.push((MessageType::Info, m)),
                Err(e) => self.status.push((MessageType::Error, e)),
            }
            self.extras.launch = None;
        }
    }

    pub fn extra_checked(&self, action: &str) -> Option<bool> {
        let x = &self.extras;
        Some(match action {
            "toggle3DGridAction" => x.show_grid,
            "toggleAxisWidgetAction" => x.axis_widget,
            "gridSizeToTwoAction" => x.grid_studs == 2,
            "gridSizeToFourAction" => x.grid_studs == 4,
            "gridSizeToSixteenAction" => x.grid_studs == 16,
            "audioToggleAction" => x.muted,
            "analyzePhysicsToggleAction" => x.analyze_physics,
            "Always" => x.join_mode == JoinMode::Always,
            "Surface only" => x.join_mode == JoinMode::Surface,
            "Never" => x.join_mode == JoinMode::Never,
            "materialActionAsTool" => x.material_as_tool,
            "colorFillActionAsTool" => x.color_as_tool,
            "startServerCB" => x.start_server,
            "testStatsAction" => x.stats[0],
            "testRenderStatsAction" => x.stats[1],
            "testPhysicsStatsAction" => x.stats[2],
            "testNetworkStatsAction" => x.stats[3],
            "testSummaryStatsAction" => x.stats[4],
            "testCustomStatsAction" => x.stats[5],
            _ => return None,
        })
    }

    pub fn extra_action(&mut self, action: &str) -> bool {
        match action {
            "toggle3DGridAction" => self.extras.show_grid = !self.extras.show_grid,
            "toggleAxisWidgetAction" => self.extras.axis_widget = !self.extras.axis_widget,
            "gridSizeToTwoAction" => self.extras.grid_studs = 2,
            "gridSizeToFourAction" => self.extras.grid_studs = 4,
            "gridSizeToSixteenAction" => self.extras.grid_studs = 16,
            "audioToggleAction" => self.toggle_mute(),
            "analyzePhysicsToggleAction" => self.extras.analyze_physics = !self.extras.analyze_physics,
            "Always" => self.extras.join_mode = JoinMode::Always,
            "Surface only" => self.extras.join_mode = JoinMode::Surface,
            "Never" => self.extras.join_mode = JoinMode::Never,
            "materialActionAsTool" => self.extras.material_as_tool = !self.extras.material_as_tool,
            "colorFillActionAsTool" => self.extras.color_as_tool = !self.extras.color_as_tool,
            "actionFullScreen" => self.extras.pending_fullscreen = true,
            "screenShotAction" => self.extras.pending_screenshot = true,
            "insertModelAction" => self.import_model(),
            "executeScriptAction" => self.execute_script_file(),
            "startServerAndPlayersAction" => self.start_servers(),
            "cleanupServersAndPlayersAction" => self.cleanup_servers(),
            "Explosion" | "Fire" | "PointLight" | "Smoke" | "Sparkles" | "SpotLight" | "SurfaceLight" | "ParticleEmitter" => {
                self.insert_into_selection(action)
            }
            "testStatsAction" => self.extras.stats[0] = !self.extras.stats[0],
            "testRenderStatsAction" => self.extras.stats[1] = !self.extras.stats[1],
            "testPhysicsStatsAction" => self.extras.stats[2] = !self.extras.stats[2],
            "testNetworkStatsAction" => self.extras.stats[3] = !self.extras.stats[3],
            "testSummaryStatsAction" => self.extras.stats[4] = !self.extras.stats[4],
            "testCustomStatsAction" => self.extras.stats[5] = !self.extras.stats[5],
            "testClearStatsAction" => self.extras.stats = [false; 6],
            "aboutRobloxAction" => self.extras.show_about = true,
            "shortcutHelpAction" => self.extras.show_shortcuts = true,
            "settingsAction" => self.extras.show_settings = true,
            "exportPlaceAction" => {
                let all: Vec<InstanceId> = {
                    let e = self.doc.runtime.engine.lock();
                    e.dm.find_service("Workspace").map(|w| vec![w]).unwrap_or_default()
                };
                let prev = self.doc.selection();
                self.doc.select(all);
                self.export_selection_obj();
                self.doc.select(prev);
            }
            _ => return false,
        }
        true
    }

    pub fn screenshot_path() -> PathBuf {
        let home = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let dir = home.join("Pictures").join("Roblox");
        let _ = std::fs::create_dir_all(&dir);
        let stamp = rbx_base::time::unix_seconds() as u64;
        dir.join(format!("RobloxScreenShot{stamp}.png"))
    }

    pub fn dialogs(&mut self, ctx: &egui::Context) {
        if self.extras.show_about {
            let mut open = true;
            egui::Window::new("Welcome to ROBLOX!")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.set_width(380.0);
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new(format!("ROBLOX Studio Version {}", env!("CARGO_PKG_VERSION"))).font(crate::theme::bold(crate::theme::FONT_SIZE)));
                        ui.add_space(8.0);
                        ui.label("ROBLOX, \"Online Building Toy\", characters, logos, names, and all related indicia are trademarks of ROBLOX Corporation \u{a9}2014. US Patents 7,874,921. Patents pending.");
                        ui.add_space(8.0);
                        ui.label("Contact us at info@roblox.com.");
                        ui.add_space(8.0);
                        if ui.button("   OK   ").clicked() {
                            self.extras.show_about = false;
                        }
                    });
                });
            if !open {
                self.extras.show_about = false;
            }
        }
        if self.extras.show_shortcuts {
            let mut open = true;
            let mut rows: Vec<(String, String)> = Vec::new();
            if let Ok(text) = std::fs::read_to_string(self.assets.join("studio").join("actions.json"))
                && let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&text)
            {
                for (_, v) in map {
                    if let (Some(t), Some(k)) = (v.get("text").and_then(Value::as_str), v.get("shortcut").and_then(Value::as_str)) {
                        rows.push((t.to_string(), k.to_string()));
                    }
                }
            }
            rows.sort();
            egui::Window::new("Customize Shortcuts")
                .collapsible(false)
                .open(&mut open)
                .default_size(egui::vec2(420.0, 480.0))
                .show(ctx, |ui| {
                    let xs = crate::views::header(ui, &[("Action", 2.0), ("Shortcut", 1.0)]);
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        for (t, k) in &rows {
                            crate::views::row(ui, &xs, &[t.clone(), k.clone()], Color32::BLACK);
                        }
                    });
                });
            if !open {
                self.extras.show_shortcuts = false;
            }
        }
        if self.extras.show_settings {
            let mut open = true;
            let root = rbx_script::api::services::settings_root(&self.doc.runtime.lua, "GlobalSettings").ok();
            let categories: Vec<(InstanceId, String)> = {
                let e = self.doc.runtime.engine.lock();
                root.map(|r| e.dm.children_of(r).iter().map(|c| (*c, e.dm.name_of(*c).to_string())).collect())
                    .unwrap_or_default()
            };
            egui::Window::new("Settings")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .fixed_size(egui::vec2(640.0, 420.0))
                .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    let area = ui.available_rect_before_wrap();
                    let body = Rect::from_min_max(area.min, egui::pos2(area.right(), area.bottom() - 32.0));
                    let left = Rect::from_min_max(body.min, egui::pos2(body.left() + 200.0, body.bottom()));
                    let right = Rect::from_min_max(egui::pos2(left.right() + 6.0, body.top()), body.max);
                    let buttons = Rect::from_min_max(egui::pos2(area.left(), body.bottom() + 6.0), area.max);
                    ui.painter().rect_filled(left, 0.0, Color32::WHITE);
                    ui.painter().rect_stroke(left, 0.0, Stroke::new(1.0, crate::theme::VIEW_BORDER), egui::StrokeKind::Inside);
                    ui.scope_builder(egui::UiBuilder::new().max_rect(left.shrink(1.0)), |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                        crate::views::header(ui, &[("Names", 1.0)]);
                        for (i, (id, name)) in categories.iter().enumerate() {
                            let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 19.0), egui::Sense::click());
                            let selected = self.extras.settings_selected == Some(*id);
                            if selected {
                                crate::theme::selected_row(ui.painter(), r);
                            } else if i % 2 == 1 {
                                ui.painter().rect_filled(r, 0.0, Color32::from_rgb(249, 249, 249));
                            }
                            ui.painter().text(
                                r.left_center() + egui::vec2(6.0, 0.0),
                                Align2::LEFT_CENTER,
                                name,
                                FontId::proportional(crate::theme::FONT_SIZE),
                                if selected { Color32::WHITE } else { Color32::BLACK },
                            );
                            if resp.clicked() {
                                self.extras.settings_selected = Some(*id);
                            }
                        }
                    });
                    ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| {
                        if let Some(sel) = self.extras.settings_selected {
                            let mut props = std::mem::take(&mut self.extras.settings_props);
                            props.grid(ui, &mut self.doc, &[sel]);
                            self.extras.settings_props = props;
                        } else {
                            ui.painter().rect_filled(right, 0.0, Color32::WHITE);
                            ui.painter().rect_stroke(right, 0.0, Stroke::new(1.0, crate::theme::VIEW_BORDER), egui::StrokeKind::Inside);
                        }
                    });
                    ui.scope_builder(egui::UiBuilder::new().max_rect(buttons), |ui| {
                        ui.horizontal(|ui| {
                            let _ = ui.button("Reset All Settings");
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("Close").clicked() {
                                    self.extras.show_settings = false;
                                }
                            });
                        });
                    });
                    ui.allocate_rect(area, egui::Sense::hover());
                });
            if !open {
                self.extras.show_settings = false;
            }
        }
    }
}
