use crate::explorer::{self, Explorer};
use crate::icons::Icons;
use crate::metadata::ClassMetadata;
use crate::plugins::Plugins;
use crate::properties::Properties;
use crate::ribbon::{self, Event, PluginItem, Ribbon};
use crate::theme;
use crate::viewport::{self, Drag, FlyCamera, Tool};
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};
use egui_stack::{DockArea, DockState, NodeIndex, TabStyle, TabViewer};
use rbx_dom::{InstanceId, Variant};
use rbx_script::api::user_input::{self, InputEvent, InputKind};
use rbx_script::{Identity, MessageType};
use rbx_studio_ui::{Document, panels};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub enum Tab {
    Viewport,
    Explorer,
    Properties,
    Output,
    Command,
    AdvancedObjects,
    Script(InstanceId),
}

pub struct ViewportState {
    pub camera: FlyCamera,
    pub rect: Rect,
    pub pixels: (u32, u32),
    pub texture: Option<egui::TextureId>,
    pub drag: Option<Drag>,
}

pub struct Studio {
    pub doc: Document,
    pub assets: PathBuf,
    pub dock: DockState<Tab>,
    pub tool: Tool,
    pub view: ViewportState,
    pub scripts: Vec<(InstanceId, String)>,
    pub command: String,
    pub status: Vec<(MessageType, String)>,
    pub plugins: Plugins,
    pub ribbon: Ribbon,
    pub icons: Icons,
    pub meta: ClassMetadata,
    pub explorer: Explorer,
    pub properties: Properties,
    pub collisions: bool,
    pub grid: f32,
    pub object_filter: String,
    pub context: Option<(InstanceId, egui::Pos2)>,
}

fn layout() -> DockState<Tab> {
    let mut dock = DockState::new(vec![Tab::Viewport]);
    let tree = dock.main_surface_mut();
    let [center, right] = tree.split_right(NodeIndex::root(), 0.80, vec![Tab::Explorer]);
    tree.split_below(right, 0.42, vec![Tab::Properties]);
    let [_, bottom] = tree.split_below(center, 0.80, vec![Tab::Output]);
    tree.split_below(bottom, 0.70, vec![Tab::Command]);
    dock
}

fn camera_for(doc: &Document) -> FlyCamera {
    let e = doc.runtime.engine.lock();
    let dm = &e.dm;
    let cam = dm.find_service("Workspace").and_then(|ws| {
        dm.get_ref(ws, "CurrentCamera")
            .or_else(|| dm.find_first_child_of_class(ws, "Camera"))
    });
    match cam.and_then(|c| dm.get(c, "CFrame").ok()?.as_cframe()) {
        Some(cf) => FlyCamera::from_cframe(cf, 70.0),
        None => FlyCamera::looking_at(
            rbx_math::Vec3::new(0.0, 25.0, 45.0),
            rbx_math::Vec3::new(0.0, 0.0, 0.0),
        ),
    }
}

struct RibbonState<'a> {
    studio: &'a Studio,
}

impl ribbon::State for RibbonState<'_> {
    fn checked(&self, action: &str) -> bool {
        let s = self.studio;
        match action {
            "advArrowToolAction" => s.tool == Tool::Select,
            "advTranslateAction" => s.tool == Tool::Move,
            "resizeAction" => s.tool == Tool::Scale,
            "advRotateAction" => s.tool == Tool::Rotate,
            "toggleCollisionCheckAction" => s.collisions,
            "gridToOneAction" => s.grid == 1.0,
            "gridToOneFifthAction" => s.grid == 0.2,
            "gridToOffAction" => s.grid == 0.0,
            "viewObjectExplorerAction" => s.dock.find_tab(&Tab::Explorer).is_some(),
            "viewPropertiesAction" => s.dock.find_tab(&Tab::Properties).is_some(),
            "viewOutputWindowAction" => s.dock.find_tab(&Tab::Output).is_some(),
            "viewCommandBarAction" => s.dock.find_tab(&Tab::Command).is_some(),
            "actionInsertAdvancedObject" => s.dock.find_tab(&Tab::AdvancedObjects).is_some(),
            _ => false,
        }
    }

    fn enabled(&self, action: &str) -> bool {
        let selected = !self.studio.doc.selection().is_empty();
        match action {
            "copyAction" | "cutAction" | "duplicateSelectionAction" | "groupSelectionAction"
            | "ungroupSelectionAction" | "anchorAction" | "actionMaterialSelector"
            | "actionColorSelector" | "actionSurface" => selected,
            "unionSelectionAction" | "negateSelectionAction" | "separateSelectionAction"
            | "publishToRobloxAction" | "publishToRobloxAsAction" | "publishSelectionToRobloxAction"
            | "pairRbxDevDeviceAction" | "startServerAndPlayersAction" | "simulationStopAction"
            | "toggleVideoRecordAction" | "filePublishedProjectsAction" | "downloadPlaceCopyAction"
            | "publishGameAction" | "viewTeamCreateAction" => false,
            _ => true,
        }
    }
}

impl Studio {
    pub fn new(doc: Document, assets: PathBuf) -> Studio {
        let camera = camera_for(&doc);
        let studio_dir = assets.join("studio");
        let mut studio = Studio {
            doc,
            dock: layout(),
            tool: Tool::Select,
            view: ViewportState {
                camera,
                rect: Rect::NOTHING,
                pixels: (1, 1),
                texture: None,
                drag: None,
            },
            scripts: Vec::new(),
            command: String::new(),
            status: Vec::new(),
            plugins: Plugins::default(),
            ribbon: Ribbon::load(&studio_dir),
            icons: Icons::new(&studio_dir),
            meta: ClassMetadata::load(&studio_dir.join("class_metadata.json")),
            explorer: Explorer::default(),
            properties: Properties::default(),
            collisions: false,
            grid: 1.0,
            object_filter: String::new(),
            context: None,
            assets,
        };
        studio.load_plugins();
        studio
    }

    pub fn load_plugins(&mut self) {
        let dir = self.assets.join("plugins").join("studio");
        self.plugins = Plugins::load(&self.doc, &dir);
        for e in self.plugins.errors.clone() {
            self.log(MessageType::Error, e);
        }
    }

    fn undo(&mut self) {
        match self.doc.undo() {
            Ok(true) => self.load_plugins(),
            Ok(false) => {}
            Err(e) => self.log(MessageType::Error, e),
        }
    }

    fn redo(&mut self) {
        match self.doc.redo() {
            Ok(true) => self.load_plugins(),
            Ok(false) => {}
            Err(e) => self.log(MessageType::Error, e),
        }
    }

    fn send_input(&self, kind: InputKind, input_type: u32, position: egui::Pos2, delta: egui::Vec2) {
        let local = position - self.view.rect.min;
        self.doc.runtime.input(InputEvent {
            kind,
            input_type,
            key_code: 0,
            position: rbx_math::Vec3::new(local.x, local.y, 0.0),
            delta: rbx_math::Vec3::new(delta.x, delta.y, 0.0),
            text: None,
        });
    }

    fn log(&mut self, kind: MessageType, text: impl Into<String>) {
        self.status.push((kind, text.into()));
    }

    pub fn place_name(&self) -> String {
        self.doc
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Place1".into())
    }

    pub fn open(&mut self, path: &Path) {
        match Document::open(path, &self.assets.join("content")) {
            Ok(doc) => {
                self.doc = doc;
                self.view.camera = camera_for(&self.doc);
                self.scripts.clear();
                self.explorer = Explorer::default();
                self.load_plugins();
                self.log(MessageType::Info, format!("opened {}", path.display()));
            }
            Err(e) => self.log(MessageType::Error, e),
        }
    }

    pub fn save_as(&mut self, path: &Path) {
        match self.doc.save_to(path) {
            Ok(()) => self.log(MessageType::Info, format!("saved {}", path.display())),
            Err(e) => self.log(MessageType::Error, e),
        }
    }

    fn save(&mut self) {
        if self.doc.path.is_some() {
            match self.doc.save() {
                Ok(()) => {
                    let name = self.place_name();
                    self.log(MessageType::Info, format!("saved {name}"));
                }
                Err(e) => self.log(MessageType::Error, e),
            }
        } else {
            self.save_dialog();
        }
    }

    fn save_dialog(&mut self) {
        let name = self.place_name();
        let name = if name.contains('.') { name } else { format!("{name}.rbxl") };
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Roblox Place Files", &["rbxl"])
            .add_filter("Roblox XML Place Files", &["rbxlx"])
            .set_file_name(name)
            .save_file()
        {
            self.save_as(&p);
        }
    }

    pub fn play_solo(&mut self) {
        let tmp = std::env::temp_dir().join(format!("studio-play-{}.rbxl", std::process::id()));
        let bytes = match self.doc.bytes() {
            Ok(b) => b,
            Err(e) => return self.log(MessageType::Error, e),
        };
        if let Err(e) = std::fs::write(&tmp, bytes) {
            return self.log(MessageType::Error, format!("{}: {e}", tmp.display()));
        }
        let exe = std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|d| d.join(format!("player{}", std::env::consts::EXE_SUFFIX))))
            .filter(|p| p.is_file());
        let Some(exe) = exe else {
            return self.log(MessageType::Error, "the player executable is not next to studio");
        };
        match std::process::Command::new(&exe)
            .arg(&tmp)
            .arg("--assets")
            .arg(&self.assets)
            .spawn()
        {
            Ok(_) => self.log(MessageType::Info, "started play solo"),
            Err(e) => self.log(MessageType::Error, format!("{}: {e}", exe.display())),
        }
    }

    fn insert_part(&mut self, class: &str, shape: Option<u32>) {
        let ws = self.doc.runtime.engine.lock().dm.find_service("Workspace");
        let Some(ws) = ws else { return };
        let cam = self.view.camera.cframe();
        match self.doc.insert(class, ws) {
            Ok(id) => {
                let at = cam.translation + cam.look_vector() * 20.0;
                let at = rbx_math::Vec3::new(
                    viewport::snap(at.x, 1.0),
                    viewport::snap(at.y.max(0.6), 1.0) + 0.6,
                    viewport::snap(at.z, 1.0),
                );
                let mut e = self.doc.runtime.engine.lock();
                let _ = e.dm.set(id, "Anchored", Variant::Bool(true));
                if let Some(s) = shape {
                    let _ = e.dm.set(id, "Shape", Variant::Enum(s));
                }
                let _ = e.dm.set(id, "CFrame", Variant::CFrame(rbx_math::CFrame::from_translation(at)));
                drop(e);
                self.doc.select(vec![id]);
            }
            Err(e) => self.log(MessageType::Error, e),
        }
    }

    fn insert_object(&mut self, class: &str) {
        let parent = {
            let sel = self.doc.selection();
            let e = self.doc.runtime.engine.lock();
            let preferred = self
                .meta
                .preferred_parent(class)
                .and_then(|p| e.dm.find_service(p));
            preferred
                .or_else(|| sel.first().copied())
                .or_else(|| e.dm.find_service("Workspace"))
        };
        if let Some(p) = parent {
            match self.doc.insert(class, p) {
                Ok(id) => self.doc.select(vec![id]),
                Err(e) => self.log(MessageType::Error, e),
            }
        }
    }

    fn selected_parts(&self) -> Vec<InstanceId> {
        let sel = self.doc.selection();
        let e = self.doc.runtime.engine.lock();
        let mut out = Vec::new();
        for id in sel {
            if e.dm.is_a(id, "BasePart") {
                out.push(id);
            }
            out.extend(e.dm.descendants(id).into_iter().filter(|d| e.dm.is_a(*d, "BasePart")));
        }
        out
    }

    fn set_on_parts(&mut self, prop: &str, value: Variant) {
        let parts = self.selected_parts();
        if parts.is_empty() {
            return;
        }
        self.doc.checkpoint();
        let mut e = self.doc.runtime.engine.lock();
        for p in parts {
            let _ = e.dm.set(p, prop, value.clone());
        }
    }

    fn group(&mut self) {
        let sel = self.doc.selection();
        if sel.is_empty() {
            return;
        }
        self.doc.checkpoint();
        let mut e = self.doc.runtime.engine.lock();
        let parent = e.dm.parent_of(sel[0]).or_else(|| e.dm.find_service("Workspace"));
        let Ok(model) = e.dm.create("Model") else { return };
        let _ = e.dm.set_parent(model, parent);
        for id in &sel {
            let _ = e.dm.set_parent(*id, Some(model));
        }
        drop(e);
        self.doc.select(vec![model]);
    }

    fn ungroup(&mut self) {
        let sel = self.doc.selection();
        self.doc.checkpoint();
        let mut released = Vec::new();
        let mut e = self.doc.runtime.engine.lock();
        for id in sel {
            if e.dm.class_of(id) != Some("Model") {
                continue;
            }
            let parent = e.dm.parent_of(id);
            for c in e.dm.children_of(id).to_vec() {
                let _ = e.dm.set_parent(c, parent);
                released.push(c);
            }
            let _ = e.dm.destroy(id);
        }
        drop(e);
        self.doc.select(released);
    }

    fn anchor(&mut self) {
        let parts = self.selected_parts();
        let all = {
            let e = self.doc.runtime.engine.lock();
            parts.iter().all(|p| e.dm.get_bool(*p, "Anchored"))
        };
        self.set_on_parts("Anchored", Variant::Bool(!all));
    }

    fn surface(&mut self, value: u32) {
        let parts = self.selected_parts();
        if parts.is_empty() {
            return;
        }
        self.doc.checkpoint();
        let mut e = self.doc.runtime.engine.lock();
        for p in parts {
            for s in ["TopSurface", "BottomSurface", "FrontSurface", "BackSurface", "LeftSurface", "RightSurface"] {
                let _ = e.dm.set(p, s, Variant::Enum(value));
            }
        }
    }

    fn toggle_tab(&mut self, tab: Tab) {
        if let Some(path) = self.dock.find_tab(&tab) {
            let _ = self.dock.remove_tab(path);
        } else {
            let surface = self.dock.main_surface_mut();
            match tab {
                Tab::Explorer | Tab::Properties | Tab::AdvancedObjects => {
                    surface.split_right(NodeIndex::root(), 0.80, vec![tab]);
                }
                _ => {
                    surface.split_below(NodeIndex::root(), 0.80, vec![tab]);
                }
            }
        }
    }

    fn handle(&mut self, ev: Event) {
        match ev {
            Event::Color(n) => self.set_on_parts("BrickColor", Variant::BrickColor(rbx_math::BrickColor::new(n as i64))),
            Event::Check | Event::Combo => {}
            Event::Action(name) => self.action(&name),
        }
    }

    fn action(&mut self, name: &str) {
        if let Some(id) = name.strip_prefix("#plugin:").and_then(|s| s.parse::<u64>().ok()) {
            if let Some(b) = InstanceId::from_raw(id) {
                self.plugins.click(&self.doc, b);
            }
            return;
        }
        if let Some(mat) = name.strip_prefix("ms_") {
            let value = rbx_reflection::database()
                .enum_desc("Material")
                .and_then(|d| d.items.iter().find(|i| i.name == mat).map(|i| i.value));
            if let Some(v) = value {
                self.set_on_parts("Material", Variant::Enum(v));
            }
            return;
        }
        match name {
            "fileNewAction" => match Document::new_baseplate(&self.assets.join("content")) {
                Ok(doc) => {
                    self.doc = doc;
                    self.view.camera = camera_for(&self.doc);
                    self.explorer = Explorer::default();
                    self.load_plugins();
                }
                Err(e) => self.log(MessageType::Error, e),
            },
            "fileOpenAction" => {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("Roblox Place Files", &["rbxl", "rbxlx"])
                    .pick_file()
                {
                    self.open(&p);
                }
            }
            "fileSaveAction" => self.save(),
            "fileSaveAsAction" => self.save_dialog(),
            "undoAction" => self.undo(),
            "redoAction" => self.redo(),
            "copyAction" => {
                self.doc.run_verb("Copy");
            }
            "cutAction" => {
                self.doc.run_verb("Cut");
            }
            "pasteAction" => {
                self.doc.run_verb("Paste");
            }
            "duplicateSelectionAction" => {
                self.doc.run_verb("Duplicate");
            }
            "advArrowToolAction" => self.tool = Tool::Select,
            "advTranslateAction" => self.tool = Tool::Move,
            "resizeAction" => self.tool = Tool::Scale,
            "advRotateAction" => self.tool = Tool::Rotate,
            "toggleCollisionCheckAction" => self.collisions = !self.collisions,
            "gridToOneAction" => self.grid = 1.0,
            "gridToOneFifthAction" => self.grid = 0.2,
            "gridToOffAction" => self.grid = 0.0,
            "placePartAction" | "PartLargeAction" => self.insert_part("Part", None),
            "SphereLargeAction" => self.insert_part("Part", Some(0)),
            "CylinderLargeAction" => self.insert_part("Part", Some(2)),
            "WedgeLargeAction" => self.insert_part("WedgePart", None),
            "startLocationAction" => self.insert_part("SpawnLocation", None),
            "ScriptLargeAction" => self.insert_object("Script"),
            "LocalScriptLargeAction" => self.insert_object("LocalScript"),
            "ModuleScriptLargeAction" => self.insert_object("ModuleScript"),
            "groupSelectionAction" => self.group(),
            "ungroupSelectionAction" => self.ungroup(),
            "anchorAction" => self.anchor(),
            "smoothSurfaceAction" => self.surface(0),
            "glueSurfaceAction" => self.surface(1),
            "weldSurfaceAction" => self.surface(2),
            "studsAction" => self.surface(3),
            "inletAction" => self.surface(4),
            "universalsAction" => self.surface(5),
            "hingeAction" => self.surface(6),
            "motorRightAction" => self.surface(7),
            "smoothNoOutlinesAction" => self.surface(10),
            "lockAction" => self.set_on_parts("Locked", Variant::Bool(true)),
            "unlockAllAction" => {
                let parts = {
                    let e = self.doc.runtime.engine.lock();
                    e.dm.find_service("Workspace")
                        .map(|ws| e.dm.descendants(ws).into_iter().filter(|d| e.dm.is_a(*d, "BasePart")).collect::<Vec<_>>())
                        .unwrap_or_default()
                };
                self.doc.checkpoint();
                let mut e = self.doc.runtime.engine.lock();
                for p in parts {
                    let _ = e.dm.set(p, "Locked", Variant::Bool(false));
                }
            }
            "simulationPlayAction" | "simulationRunAction" | "playGameActionGroup" => self.play_solo(),
            "viewObjectExplorerAction" => self.toggle_tab(Tab::Explorer),
            "viewPropertiesAction" => self.toggle_tab(Tab::Properties),
            "viewOutputWindowAction" => self.toggle_tab(Tab::Output),
            "viewCommandBarAction" => self.toggle_tab(Tab::Command),
            "actionInsertAdvancedObject" | "viewToolboxAction" => self.toggle_tab(Tab::AdvancedObjects),
            "insertServiceAction" => {
                let mut e = self.doc.runtime.engine.lock();
                for s in ["ReplicatedStorage", "ServerScriptService", "ServerStorage", "Teams"] {
                    let _ = e.dm.get_service(s);
                }
            }
            "openPluginsFolderAction" | "managePluginsAction" => {
                let dir = self.assets.join("plugins").join("studio");
                self.log(MessageType::Info, format!("plugins load from {}", dir.display()));
            }
            "unionSelectionAction" | "negateSelectionAction" | "separateSelectionAction" => {
                self.log(MessageType::Warning, "solid modeling is not available in this build")
            }
            other => {
                let label = other.split('.').next().unwrap_or(other).to_string();
                if let Some(b) = self.plugins.find(&label) {
                    self.plugins.click(&self.doc, b);
                }
            }
        }
    }

    fn run_command(&mut self) {
        let source = std::mem::take(&mut self.command);
        if source.trim().is_empty() {
            return;
        }
        self.doc.checkpoint();
        self.log(MessageType::Output, format!("> {source}"));
        if let Err(e) = self.doc.runtime.execute(&source, "CommandBar", Identity::CmdLine) {
            self.log(MessageType::Error, e.to_string());
        }
        self.doc.runtime.pump();
    }

    pub fn open_script(&mut self, id: InstanceId) {
        if !self.scripts.iter().any(|(s, _)| *s == id) {
            let src = self.doc.script_source(id).unwrap_or_default();
            self.scripts.push((id, src));
        }
        let tab = Tab::Script(id);
        if let Some(found) = self.dock.find_tab(&tab) {
            let _ = self.dock.set_active_tab(found);
        } else {
            if let Some(vp) = self.dock.find_tab(&Tab::Viewport) {
                self.dock.set_focused_node_and_surface(vp.node_path());
            }
            self.dock.main_surface_mut().push_to_focused_leaf(tab);
        }
    }

    pub fn shortcuts(&mut self, ctx: &egui::Context) {
        let wants_text = ctx.egui_wants_keyboard_input();
        let keys = ctx.input(|i| {
            let cmd = i.modifiers.command;
            [
                cmd && i.key_pressed(egui::Key::S),
                cmd && !i.modifiers.shift && i.key_pressed(egui::Key::Z),
                cmd && (i.key_pressed(egui::Key::Y) || (i.modifiers.shift && i.key_pressed(egui::Key::Z))),
                i.key_pressed(egui::Key::Delete),
                cmd && i.key_pressed(egui::Key::D),
                cmd && i.key_pressed(egui::Key::C),
                cmd && i.key_pressed(egui::Key::V),
                cmd && i.key_pressed(egui::Key::X),
                cmd && !i.modifiers.shift && i.key_pressed(egui::Key::G),
                cmd && i.modifiers.shift && i.key_pressed(egui::Key::G),
                !cmd && i.key_pressed(egui::Key::R),
                !cmd && i.key_pressed(egui::Key::T),
            ]
        });
        if keys[0] {
            self.save();
        }
        if wants_text {
            return;
        }
        let actions = [
            "",
            "undoAction",
            "redoAction",
            "#delete",
            "duplicateSelectionAction",
            "copyAction",
            "pasteAction",
            "cutAction",
            "groupSelectionAction",
            "ungroupSelectionAction",
        ];
        for (i, a) in actions.iter().enumerate().skip(1) {
            if keys[i] {
                if *a == "#delete" {
                    self.doc.delete_selection();
                } else {
                    self.action(a);
                }
            }
        }
        let sel = self.doc.selection();
        if (keys[10] || keys[11]) && !sel.is_empty() {
            self.doc.checkpoint();
            let axis = if keys[10] { rbx_math::Vec3::Y } else { rbx_math::Vec3::X };
            self.doc.rotate(&sel, axis, std::f32::consts::FRAC_PI_2);
        }
    }

    fn sync_plugin_groups(&mut self) {
        let buttons = self.doc.runtime.engine.lock().plugins.buttons.clone();
        let mut groups: Vec<(String, Vec<PluginItem>)> = Vec::new();
        for b in buttons {
            let item = PluginItem {
                id: b.id.raw(),
                text: b.text.clone(),
                tooltip: b.tooltip.clone(),
                icon: b.icon.clone(),
                active: b.active,
            };
            match groups.iter_mut().find(|(n, _)| *n == b.toolbar) {
                Some((_, items)) => items.push(item),
                None => groups.push((b.toolbar.clone(), vec![item])),
            }
        }
        self.ribbon.plugin_groups = groups;
        let opens: Vec<InstanceId> = std::mem::take(&mut self.doc.runtime.engine.lock().plugins.open_requests);
        for id in opens {
            self.open_script(id);
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.shortcuts(&ctx);
        self.sync_plugin_groups();
        let mut ribbon = std::mem::replace(&mut self.ribbon, Ribbon::empty());
        let mut icons = std::mem::replace(&mut self.icons, Icons::new(Path::new("")));
        let events = egui::Panel::top("ribbon")
            .exact_size(Ribbon::height())
            .frame(egui::Frame::new())
            .show(ui, |ui| ribbon.show(ui, &mut icons, &RibbonState { studio: self }))
            .inner;
        self.ribbon = ribbon;
        self.icons = icons;
        for ev in events {
            self.handle(ev);
        }
        let mut dock = std::mem::replace(&mut self.dock, DockState::new(Vec::new()));
        let style = dock_style(ui.style());
        let mut viewer = Viewer { studio: self, open: None, context: None };
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::WINDOW).inner_margin(2.0))
            .show(ui, |ui| {
                DockArea::new(&mut dock)
                    .style(style)
                    .show_leaf_close_all_buttons(false)
                    .show_leaf_collapse_buttons(false)
                    .show_inside(ui, &mut viewer);
            });
        let open = viewer.open;
        let context = viewer.context;
        self.dock = dock;
        if let Some(id) = open {
            self.open_script(id);
        }
        if context.is_some() {
            self.context = context;
        }
        let open_ids: HashSet<InstanceId> = self
            .scripts
            .iter()
            .filter(|(id, _)| self.dock.find_tab(&Tab::Script(*id)).is_some())
            .map(|(id, _)| *id)
            .collect();
        self.scripts.retain(|(id, _)| open_ids.contains(id));
        self.context_menu(&ctx);
    }

    fn context_menu(&mut self, ctx: &egui::Context) {
        let Some((id, at)) = self.context else { return };
        let mut close = false;
        let mut action: Option<&'static str> = None;
        let area = egui::Area::new(egui::Id::unique("explorer-context"))
            .order(egui::Order::Foreground)
            .fixed_pos(at)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .stroke(Stroke::new(1.0, theme::RIBBON_SEPARATOR))
                    .inner_margin(2.0)
                    .show(ui, |ui| {
                        ui.set_min_width(170.0);
                        for (label, act) in [
                            ("Cut", "cutAction"),
                            ("Copy", "copyAction"),
                            ("Paste Into", "#pasteinto"),
                            ("Duplicate", "duplicateSelectionAction"),
                            ("Delete", "#delete"),
                            ("Rename", "#rename"),
                            ("", ""),
                            ("Group", "groupSelectionAction"),
                            ("Ungroup", "ungroupSelectionAction"),
                            ("", ""),
                            ("Insert Part", "placePartAction"),
                            ("Insert Script", "ScriptLargeAction"),
                        ] {
                            if label.is_empty() {
                                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 5.0), Sense::hover());
                                ui.painter().line_segment(
                                    [pos2(r.left() + 4.0, r.center().y), pos2(r.right() - 4.0, r.center().y)],
                                    Stroke::new(1.0, theme::RIBBON_SEPARATOR),
                                );
                                continue;
                            }
                            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
                            if resp.hovered() {
                                ui.painter().rect_filled(r, 0.0, theme::HIGHLIGHT);
                                ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, theme::ACCENT_HILIGHT), egui::StrokeKind::Inside);
                            }
                            ui.painter().text(
                                pos2(r.left() + 26.0, r.center().y),
                                Align2::LEFT_CENTER,
                                label,
                                FontId::proportional(theme::FONT_SIZE),
                                theme::TEXT,
                            );
                            if resp.clicked() {
                                action = Some(act);
                                close = true;
                            }
                        }
                    });
            });
        if let Some(a) = action {
            match a {
                "#delete" => self.doc.delete_selection(),
                "#rename" => self.explorer.start_rename(&self.doc, id),
                "#pasteinto" => {
                    self.doc.run_verb("PasteInto");
                }
                other => self.action(other),
            }
        }
        if close || (ctx.input(|i| i.pointer.any_pressed()) && !area.response.contains_pointer()) {
            self.context = None;
        }
    }

    fn viewport_ui(&mut self, ui: &mut Ui) {
        let rect = ui.available_rect_before_wrap();
        let ppp = ui.ctx().pixels_per_point();
        self.view.rect = rect;
        self.view.pixels = (
            (rect.width() * ppp).round().max(1.0) as u32,
            (rect.height() * ppp).round().max(1.0) as u32,
        );
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        if let Some(tex) = self.view.texture {
            ui.painter().image(tex, rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        self.viewport_input(ui, &response, rect);
        viewport::draw_selection(&ui.painter_at(rect), rect, &self.view.camera, &self.doc);
    }

    fn viewport_input(&mut self, ui: &mut Ui, response: &egui::Response, rect: Rect) {
        let (dt, secondary, delta, scroll, keys) = ui.input(|i| {
            (
                i.stable_dt.min(0.1),
                i.pointer.secondary_down(),
                i.pointer.delta(),
                i.smooth_scroll_delta.y,
                [
                    i.key_down(egui::Key::W),
                    i.key_down(egui::Key::S),
                    i.key_down(egui::Key::A),
                    i.key_down(egui::Key::D),
                    i.key_down(egui::Key::E),
                    i.key_down(egui::Key::Q),
                    i.modifiers.shift,
                ],
            )
        });
        let hovered = response.hovered() || response.dragged();
        if hovered && secondary {
            self.view.camera.turn(delta.x, delta.y);
        }
        if hovered && response.dragged_by(egui::PointerButton::Middle) {
            self.view.camera.fly(0.0, -delta.x * 0.05, delta.y * 0.05);
        }
        if hovered && scroll != 0.0 {
            self.view.camera.fly(scroll * 0.05, 0.0, 0.0);
        }
        if !ui.ctx().egui_wants_keyboard_input() && (hovered || secondary) {
            let speed = if keys[6] { 20.0 } else { 60.0 } * dt;
            let f = (keys[0] as i32 - keys[1] as i32) as f32 * speed;
            let r = (keys[3] as i32 - keys[2] as i32) as f32 * speed;
            let u = (keys[4] as i32 - keys[5] as i32) as f32 * speed;
            if f != 0.0 || r != 0.0 || u != 0.0 {
                self.view.camera.fly(f, r, u);
            }
        }
        let pointer = response.interact_pointer_pos().or(response.hover_pos());
        let Some(pos) = pointer else { return };
        if delta != egui::Vec2::ZERO {
            self.send_input(InputKind::Changed, user_input::MOUSE_MOVEMENT, pos, delta);
        }
        let (pressed, released) = ui.input(|i| {
            (
                i.pointer.button_pressed(egui::PointerButton::Primary),
                i.pointer.button_released(egui::PointerButton::Primary),
            )
        });
        if pressed && response.hovered() {
            self.send_input(InputKind::Began, user_input::MOUSE_BUTTON1, pos, egui::Vec2::ZERO);
        }
        if released && (response.hovered() || response.dragged()) {
            self.send_input(InputKind::Ended, user_input::MOUSE_BUTTON1, pos, egui::Vec2::ZERO);
        }
        let (origin, dir) = self.view.camera.ray(rect, pos);
        if response.drag_started_by(egui::PointerButton::Primary) {
            let hit = {
                let e = self.doc.runtime.engine.lock();
                viewport::pick(&e.dm, origin, dir, false)
            };
            if let Some(hit) = hit {
                let target = {
                    let e = self.doc.runtime.engine.lock();
                    viewport::selectable(&e.dm, hit.part)
                };
                if !self.doc.selection().contains(&target) {
                    self.doc.select(vec![target]);
                }
                self.view.drag = viewport::begin_drag(self.tool, &mut self.doc, &hit, pos.x);
            }
        } else if response.dragged_by(egui::PointerButton::Primary) {
            if let Some(mut drag) = self.view.drag.take() {
                viewport::update_drag(&mut drag, &mut self.doc, origin, dir, pos.x, self.grid);
                self.view.drag = Some(drag);
            }
        } else if response.drag_stopped() {
            self.view.drag = None;
        } else if response.clicked() {
            let hit = {
                let e = self.doc.runtime.engine.lock();
                viewport::pick(&e.dm, origin, dir, false).map(|h| viewport::selectable(&e.dm, h.part))
            };
            let additive = ui.input(|i| i.modifiers.shift || i.modifiers.command);
            match hit {
                Some(id) if additive => {
                    let mut sel = self.doc.selection();
                    if let Some(i) = sel.iter().position(|x| *x == id) {
                        sel.remove(i);
                    } else {
                        sel.push(id);
                    }
                    self.doc.select(sel);
                }
                Some(id) => self.doc.select(vec![id]),
                None if !additive => self.doc.select(Vec::new()),
                None => {}
            }
        }
    }

    fn output_ui(&mut self, ui: &mut Ui) {
        let full = ui.available_rect_before_wrap();
        ui.painter().rect_filled(full, 0.0, Color32::WHITE);
        let engine_lines: Vec<(MessageType, String)> = {
            let e = self.doc.runtime.engine.lock();
            e.output.iter().map(|m| (m.kind, m.text.clone())).collect()
        };
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.add_space(2.0);
                for (kind, text) in engine_lines.iter().chain(self.status.iter()) {
                    let color = match kind {
                        MessageType::Error => Color32::from_rgb(255, 0, 0),
                        MessageType::Warning => Color32::from_rgb(255, 128, 0),
                        MessageType::Info => Color32::from_rgb(0, 0, 255),
                        MessageType::Output => Color32::BLACK,
                    };
                    ui.label(egui::RichText::new(format!(" {text}")).font(FontId::proportional(theme::FONT_SIZE)).color(color));
                }
            });
    }

    fn command_ui(&mut self, ui: &mut Ui) {
        let full = ui.available_rect_before_wrap();
        ui.painter().rect_filled(full, 0.0, Color32::WHITE);
        let r = ui.add_sized(
            vec2(full.width(), 22.0),
            egui::TextEdit::singleline(&mut self.command)
                .hint_text("Run a command")
                .font(FontId::monospace(12.0))
                .margin(vec2(4.0, 3.0)),
        );
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.run_command();
            r.request_focus();
        }
    }

    fn advanced_objects_ui(&mut self, ui: &mut Ui) {
        let full = ui.available_rect_before_wrap();
        ui.painter().rect_filled(full, 0.0, Color32::WHITE);
        ui.add_sized(
            vec2(full.width(), 22.0),
            egui::TextEdit::singleline(&mut self.object_filter).hint_text("Search object").margin(vec2(4.0, 3.0)),
        );
        let filter = self.object_filter.to_lowercase();
        let mut classes: Vec<String> = rbx_reflection::database()
            .classes()
            .filter(|c| c.creatable && self.meta.insertable(&c.name) && self.meta.order(&c.name) >= 0)
            .map(|c| c.name.clone())
            .filter(|n| filter.is_empty() || n.to_lowercase().contains(&filter))
            .collect();
        classes.sort();
        let mut chosen = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for c in &classes {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), explorer::ROW), Sense::click());
                if resp.hovered() {
                    theme::hovered_row(ui.painter(), r);
                }
                if let Some((tex, uv)) = self.icons.class(ui.ctx(), self.meta.image(c)) {
                    ui.painter().image(tex, Rect::from_min_size(pos2(r.left() + 4.0, r.center().y - 8.0), vec2(16.0, 16.0)), uv, Color32::WHITE);
                }
                ui.painter().text(pos2(r.left() + 24.0, r.center().y), Align2::LEFT_CENTER, c, FontId::proportional(theme::FONT_SIZE), Color32::BLACK);
                if resp.clicked() {
                    chosen = Some(c.clone());
                }
            }
        });
        if let Some(c) = chosen {
            self.insert_object(&c);
        }
    }
}

fn dock_style(base: &egui::Style) -> egui_stack::Style {
    let mut s = egui_stack::Style::from_egui(base);
    s.dock_area_padding = None;
    s.main_surface_border_stroke = Stroke::NONE;
    s.separator.width = 4.0;
    s.separator.color_idle = theme::WINDOW;
    s.separator.color_hovered = theme::RIBBON_SEPARATOR;
    s.separator.color_dragged = theme::ACCENT_HILIGHT;
    s.tab_bar.bg_fill = theme::DOCK_TITLE;
    s.tab_bar.height = 20.0;
    s.tab_bar.hline_color = theme::DOCK_TITLE;
    s.tab_bar.corner_radius = egui::CornerRadius::ZERO;
    for t in [&mut s.tab.active, &mut s.tab.inactive, &mut s.tab.focused, &mut s.tab.hovered] {
        t.bg_fill = theme::DOCK_TITLE;
        t.outline_color = theme::DOCK_TITLE;
        t.text_color = Color32::BLACK;
        t.corner_radius = egui::CornerRadius::ZERO;
    }
    s.tab.hline_below_active_tab_name = false;
    s.tab.tab_body.bg_fill = Color32::WHITE;
    s.tab.tab_body.stroke = Stroke::new(1.0, theme::VIEW_BORDER);
    s.tab.tab_body.corner_radius = egui::CornerRadius::ZERO;
    s.tab.tab_body.inner_margin = egui::Margin::ZERO;
    s.buttons.close_tab_color = Color32::from_gray(90);
    s.buttons.close_tab_active_color = Color32::BLACK;
    s.buttons.close_tab_bg_fill = theme::HIGHLIGHT;
    s
}

fn document_tab_style(global: &TabStyle) -> TabStyle {
    let mut t = global.clone();
    t.active.bg_fill = Color32::WHITE;
    t.active.outline_color = theme::VIEW_BORDER;
    t.focused.bg_fill = Color32::WHITE;
    t.focused.outline_color = theme::VIEW_BORDER;
    t.inactive.bg_fill = theme::DOC_TAB;
    t.inactive.outline_color = theme::VIEW_BORDER;
    t.hovered.bg_fill = Color32::from_rgb(245, 245, 245);
    t.hovered.outline_color = theme::VIEW_BORDER;
    t
}

struct Viewer<'a> {
    studio: &'a mut Studio,
    open: Option<InstanceId>,
    context: Option<(InstanceId, egui::Pos2)>,
}

impl TabViewer for Viewer<'_> {
    type Tab = Tab;

    fn id(&mut self, tab: &mut Tab) -> egui::Id {
        egui::Id::unique(format!("{tab:?}"))
    }

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        match tab {
            Tab::Script(id) => {
                let e = self.studio.doc.runtime.engine.lock();
                e.dm.name_of(*id).to_string().into()
            }
            Tab::Viewport => self.studio.place_name().into(),
            Tab::AdvancedObjects => "Advanced Objects".into(),
            other => format!("{other:?}").into(),
        }
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Tab) {
        match tab {
            Tab::Viewport => self.studio.viewport_ui(ui),
            Tab::Explorer => {
                let s = &mut *self.studio;
                match s.explorer.show(ui, &mut s.doc, &s.meta, &mut s.icons) {
                    Some(explorer::Request::OpenScript(id)) => self.open = Some(id),
                    Some(explorer::Request::Context(id)) => {
                        let at = ui.ctx().pointer_latest_pos().unwrap_or_default();
                        self.context = Some((id, at));
                    }
                    None => {}
                }
            }
            Tab::Properties => {
                let s = &mut *self.studio;
                s.properties.show(ui, &mut s.doc);
            }
            Tab::Output => self.studio.output_ui(ui),
            Tab::Command => self.studio.command_ui(ui),
            Tab::AdvancedObjects => self.studio.advanced_objects_ui(ui),
            Tab::Script(id) => {
                let id = *id;
                let s = &mut *self.studio;
                if let Some(i) = s.scripts.iter().position(|(sid, _)| *sid == id) {
                    let mut buffer = std::mem::take(&mut s.scripts[i].1);
                    panels::script_editor(ui, &mut s.doc, id, &mut buffer);
                    s.scripts[i].1 = buffer;
                }
            }
        }
    }

    fn is_closeable(&self, tab: &Tab) -> bool {
        !matches!(tab, Tab::Viewport)
    }

    fn tab_style_override(&self, tab: &Tab, global: &TabStyle) -> Option<TabStyle> {
        matches!(tab, Tab::Viewport | Tab::Script(_)).then(|| document_tab_style(global))
    }
}
