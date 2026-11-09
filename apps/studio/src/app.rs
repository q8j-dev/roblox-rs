use crate::explorer::{self, Explorer};
use crate::extras::Extras;
use crate::icons::Icons;
use crate::metadata::ClassMetadata;
use crate::play::{self, Mode, PlaySession};
use crate::plugins::Plugins;
use crate::properties::Properties;
use crate::ribbon::{self, Event, PluginItem, Ribbon};
use crate::scripteditor::EditorState;
use crate::theme;
use crate::viewport::{self, FlyCamera, Tool, Tools};
use crate::views::Views;
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};
use egui_stack::{DockArea, DockState, NodeIndex, TabStyle, TabViewer};
use rbx_dom::{InstanceId, Variant};
use rbx_script::api::user_input::{self, InputEvent, InputKind};
use rbx_script::{Identity, MessageType};
use rbx_studio_ui::Document;
use std::collections::HashMap;
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
    ScriptAnalysis,
    Diagnostics,
    TaskScheduler,
    ScriptPerformance,
    FindResults,
    ObjectBrowser,
    Script(InstanceId),
}

pub struct ViewportState {
    pub camera: FlyCamera,
    pub rect: Rect,
    pub pixels: (u32, u32),
    pub texture: Option<egui::TextureId>,
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
    pub tools: Tools,
    pub overlay: rbx_render::adorn::Adorns,
    pub play: Option<PlaySession>,
    pub viewport_focus: bool,
    pub extras: Extras,
    pub views: Views,
    pub editors: HashMap<InstanceId, EditorState>,
    pub active_doc: Option<InstanceId>,
    pub ctx: Option<egui::Context>,
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

fn is_document(tab: &Tab) -> bool {
    matches!(tab, Tab::Viewport | Tab::Script(_) | Tab::ObjectBrowser)
}

fn hide_dock_tab_bars(dock: &mut DockState<Tab>) {
    for (_, leaf) in dock.iter_leaves_mut() {
        leaf.tab_bar_hidden = leaf.tabs.len() == 1 && !leaf.tabs.iter().any(is_document);
    }
}

fn dock_title(ui: &mut Ui, title: &str) -> bool {
    let full = ui.available_rect_before_wrap();
    let bar = Rect::from_min_size(full.min, vec2(full.width(), 20.0));
    ui.allocate_rect(bar, Sense::hover());
    let p = ui.painter();
    p.rect_filled(bar, 0.0, theme::DOCK_TITLE);
    p.text(
        bar.center(),
        Align2::CENTER_CENTER,
        title,
        FontId::proportional(theme::FONT_SIZE),
        Color32::BLACK,
    );
    let close = Rect::from_center_size(
        egui::pos2(bar.right() - 10.0, bar.center().y),
        vec2(14.0, 14.0),
    );
    let float = Rect::from_center_size(
        egui::pos2(bar.right() - 26.0, bar.center().y),
        vec2(14.0, 14.0),
    );
    let cr = ui.interact(
        close,
        ui.scope_id().with(("dock-close", title)),
        Sense::click(),
    );
    let fr = ui.interact(
        float,
        ui.scope_id().with(("dock-float", title)),
        Sense::click(),
    );
    for (r, hovered) in [(close, cr.hovered()), (float, fr.hovered())] {
        if hovered {
            ui.painter().rect_filled(r, 0.0, theme::HIGHLIGHT);
            ui.painter().rect_stroke(
                r,
                0.0,
                Stroke::new(1.0, theme::ACCENT_HILIGHT),
                egui::StrokeKind::Inside,
            );
        }
    }
    let ink = Stroke::new(1.0, Color32::from_gray(40));
    let c = close.shrink(4.0);
    ui.painter()
        .line_segment([c.left_top(), c.right_bottom()], ink);
    ui.painter()
        .line_segment([c.right_top(), c.left_bottom()], ink);
    let f = float.shrink(4.0);
    ui.painter().rect_stroke(
        Rect::from_min_max(f.min + vec2(2.0, 0.0), f.max - vec2(0.0, 2.0)),
        0.0,
        ink,
        egui::StrokeKind::Inside,
    );
    ui.painter().rect_filled(
        Rect::from_min_max(f.min + vec2(0.0, 2.0), f.max - vec2(2.0, 0.0)),
        0.0,
        theme::DOCK_TITLE,
    );
    ui.painter().rect_stroke(
        Rect::from_min_max(f.min + vec2(0.0, 2.0), f.max - vec2(2.0, 0.0)),
        0.0,
        ink,
        egui::StrokeKind::Inside,
    );
    cr.clicked()
}

pub fn camera_for(doc: &Document) -> FlyCamera {
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
        if let Some(c) = s.extra_checked(action) {
            return c;
        }
        match action {
            "advArrowToolAction" => s.tool == Tool::Select,
            "advTranslateAction" => s.tool == Tool::Move,
            "resizeAction" => s.tool == Tool::Scale,
            "lockAction" => s.tool == Tool::Lock,
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
            "viewScriptAnalysisAction" => s.dock.find_tab(&Tab::ScriptAnalysis).is_some(),
            "viewDiagnosticsAction" => s.dock.find_tab(&Tab::Diagnostics).is_some(),
            "viewTaskSchedulerAction" => s.dock.find_tab(&Tab::TaskScheduler).is_some(),
            "viewScriptPerformanceAction" => s.dock.find_tab(&Tab::ScriptPerformance).is_some(),
            "viewFindResultsWindowAction" => s.dock.find_tab(&Tab::FindResults).is_some(),
            "objectBrowserAction" => s.dock.find_tab(&Tab::ObjectBrowser).is_some(),
            other => s.plugins.proxy(other).is_some_and(|b| b.active),
        }
    }

    fn enabled(&self, action: &str) -> bool {
        let selected = !self.studio.doc.selection().is_empty();
        let playing = self.studio.play.is_some();
        match action {
            "simulationPlayAction"
            | "simulationRunAction"
            | "playGameActionGroup"
            | "fileSaveAction"
            | "fileSaveAsAction"
            | "fileOpenAction"
            | "fileNewAction"
            | "undoAction"
            | "redoAction" => !playing,
            "simulationResetAction" | "simulationStopAction" => playing,
            "copyAction"
            | "cutAction"
            | "duplicateSelectionAction"
            | "groupSelectionAction"
            | "ungroupSelectionAction"
            | "anchorAction"
            | "actionMaterialSelector"
            | "actionColorSelector"
            | "actionSurface" => selected,
            "unionSelectionAction"
            | "negateSelectionAction"
            | "separateSelectionAction"
            | "publishToRobloxAction"
            | "publishToRobloxAsAction"
            | "publishSelectionToRobloxAction"
            | "pairRbxDevDeviceAction"
            | "toggleVideoRecordAction"
            | "filePublishedProjectsAction"
            | "downloadPlaceCopyAction"
            | "publishGameAction"
            | "viewTeamCreateAction"
            | "breakpointsDockWidgetAction"
            | "callStackDockWidgetAction"
            | "watchDockWidgetAction"
            | "actionStartPage"
            | "gameExplorerAction"
            | "viewContextualHelpAction"
            | "viewTutorialsAction"
            | "emulateDeviceAction"
            | "manageEmulationDeviceAction"
            | "onlineHelpAction"
            | "fastLogDumpAction"
            | "fileOpenRecentSavesAction"
            | "stepIntoAction"
            | "stepOverAction"
            | "stepOutAction"
            | "addWatchAction"
            | "neverBreakOnScriptErrorsAction"
            | "breakOnAllScriptErrorsAction"
            | "breakOnUnhandledScriptErrorsAction" => false,
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
            tools: Tools::default(),
            overlay: rbx_render::adorn::Adorns::default(),
            play: None,
            viewport_focus: false,
            extras: Extras::default(),
            views: Views::default(),
            editors: HashMap::new(),
            active_doc: None,
            ctx: None,
            assets,
        };
        studio
            .ribbon
            .set_combo("playersMode", studio.extras.players);
        studio.load_plugins();
        studio
    }

    pub fn load_plugins(&mut self) {
        let builtin = self.assets.join("plugins").join("builtin");
        let user = self.assets.join("plugins").join("studio");
        self.plugins = Plugins::load(&self.doc, &builtin, &user);
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

    fn send_input(
        &self,
        kind: InputKind,
        input_type: u32,
        position: egui::Pos2,
        delta: egui::Vec2,
    ) {
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
        let doc = self.play.as_ref().map(|p| &p.edit).unwrap_or(&self.doc);
        doc.path
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
        let name = if name.contains('.') {
            name
        } else {
            format!("{name}.rbxl")
        };
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Roblox Place Files", &["rbxl"])
            .add_filter("Roblox XML Place Files", &["rbxlx"])
            .set_file_name(name)
            .save_file()
        {
            self.save_as(&p);
        }
    }

    pub fn start_play(&mut self, mode: Mode) {
        if self.play.is_some() {
            return;
        }
        self.tools.end(&mut self.doc);
        match play::start(&self.doc, &self.assets, mode) {
            Ok(live) => {
                let edit = std::mem::replace(&mut self.doc, live);
                self.play = Some(PlaySession::new(edit, mode));
                self.viewport_focus = mode == Mode::Play;
                self.explorer = Explorer::default();
            }
            Err(e) => self.log(MessageType::Error, e),
        }
    }

    pub fn stop_play(&mut self) {
        let Some(mut session) = self.play.take() else {
            return;
        };
        session.release_keys(&self.doc);
        self.doc = session.edit;
        self.explorer = Explorer::default();
        self.scripts
            .retain(|(id, _)| self.doc.runtime.engine.lock().dm.contains(*id));
    }

    pub fn playing_game(&self) -> bool {
        matches!(&self.play, Some(p) if p.mode == Mode::Play)
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
                let _ = e.dm.set(
                    id,
                    "CFrame",
                    Variant::CFrame(rbx_math::CFrame::from_translation(at)),
                );
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

    pub fn selected_parts(&self) -> Vec<InstanceId> {
        let sel = self.doc.selection();
        let e = self.doc.runtime.engine.lock();
        let mut out = Vec::new();
        for id in sel {
            if e.dm.is_a(id, "BasePart") {
                out.push(id);
            }
            out.extend(
                e.dm.descendants(id)
                    .into_iter()
                    .filter(|d| e.dm.is_a(*d, "BasePart")),
            );
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
        let parent =
            e.dm.parent_of(sel[0])
                .or_else(|| e.dm.find_service("Workspace"));
        let Ok(model) = e.dm.create("Model") else {
            return;
        };
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
            for s in [
                "TopSurface",
                "BottomSurface",
                "FrontSurface",
                "BackSurface",
                "LeftSurface",
                "RightSurface",
            ] {
                let _ = e.dm.set(p, s, Variant::Enum(value));
            }
        }
    }

    fn toggle_tab(&mut self, tab: Tab) {
        if let Some(path) = self.dock.find_tab(&tab) {
            let _ = self.dock.remove_tab(path);
        } else {
            let side = matches!(tab, Tab::Explorer | Tab::Properties | Tab::AdvancedObjects);
            let neighbours: &[Tab] = if side {
                &[Tab::Properties, Tab::Explorer, Tab::AdvancedObjects]
            } else {
                &[
                    Tab::Output,
                    Tab::ScriptAnalysis,
                    Tab::FindResults,
                    Tab::Diagnostics,
                    Tab::TaskScheduler,
                    Tab::ScriptPerformance,
                    Tab::Command,
                ]
            };
            if let Some(path) = neighbours.iter().find_map(|t| self.dock.find_tab(t)) {
                self.dock.set_focused_node_and_surface(path.node_path());
                self.dock.main_surface_mut().push_to_focused_leaf(tab);
            } else {
                let surface = self.dock.main_surface_mut();
                if side {
                    surface.split_right(NodeIndex::root(), 0.80, vec![tab]);
                } else {
                    surface.split_below(NodeIndex::root(), 0.80, vec![tab]);
                }
            }
        }
    }

    fn handle(&mut self, ev: Event) {
        match ev {
            Event::Color(n) => {
                if self.extras.color_as_tool {
                    self.extras.paint_color = Some(n);
                    self.extras.paint_material = None;
                    self.tool = Tool::Paint;
                } else {
                    self.set_on_parts(
                        "BrickColor",
                        Variant::BrickColor(rbx_math::BrickColor::new(n as i64)),
                    );
                }
            }
            Event::Check(name, on) => {
                if name == "startServerCB" {
                    self.extras.start_server = on;
                }
            }
            Event::Combo(name, idx) => {
                if name == "playersMode" {
                    self.extras.players = idx;
                }
            }
            Event::Action(name) => self.action(&name),
        }
    }

    pub fn run_action(&mut self, name: &str) {
        self.action(name);
    }

    fn action(&mut self, name: &str) {
        if self.extra_action(name) {
            return;
        }
        if let Some(ctx) = self.ctx.clone()
            && self.script_command(&ctx, name)
        {
            return;
        }
        if let Some(id) = name
            .strip_prefix("#plugin:")
            .and_then(|s| s.parse::<u64>().ok())
        {
            if let Some(b) = InstanceId::from_raw(id) {
                self.plugins.click(&self.doc, b);
            }
            return;
        }
        if let Some(target) = name.strip_prefix("#focus:") {
            let tab = if target == "viewport" {
                Some(Tab::Viewport)
            } else {
                target
                    .parse::<u64>()
                    .ok()
                    .and_then(InstanceId::from_raw)
                    .map(Tab::Script)
            };
            if let Some(path) = tab.and_then(|t| self.dock.find_tab(&t)) {
                let _ = self.dock.set_active_tab(path);
            }
            return;
        }
        if let Some(mat) = name.strip_prefix("ms_") {
            let value = rbx_reflection::database()
                .enum_desc("Material")
                .and_then(|d| d.items.iter().find(|i| i.name == mat).map(|i| i.value));
            if let Some(v) = value {
                if self.extras.material_as_tool {
                    self.extras.paint_material = Some(v);
                    self.extras.paint_color = None;
                    self.tool = Tool::Paint;
                } else {
                    self.set_on_parts("Material", Variant::Enum(v));
                }
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
            "fileCloseAction" => self.run_action("fileNewAction"),
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
            "deleteSelectedAction" => self.doc.delete_selection(),
            "pasteIntoAction" => {
                self.doc.run_verb("PasteInto");
            }
            "zoomExtentsAction" => self.zoom_to_selection(),
            "selectionSaveToFileAction" => self.save_selection_to_file(),
            "exportSelectionAction" => self.export_selection_obj(),
            "insertIntoFileAction" => self.import_model(),
            "selectChildrenAction" => {
                let sel = self.doc.selection();
                let kids: Vec<InstanceId> = {
                    let e = self.doc.runtime.engine.lock();
                    sel.iter()
                        .flat_map(|s| e.dm.children_of(*s).to_vec())
                        .collect()
                };
                self.doc.select(kids);
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
            "lockAction" => {
                self.tool = if self.tool == Tool::Lock {
                    Tool::Select
                } else {
                    Tool::Lock
                }
            }
            "unlockAllAction" => {
                let parts = {
                    let e = self.doc.runtime.engine.lock();
                    e.dm.find_service("Workspace")
                        .map(|ws| {
                            e.dm.descendants(ws)
                                .into_iter()
                                .filter(|d| e.dm.is_a(*d, "BasePart"))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                };
                self.doc.checkpoint();
                let mut e = self.doc.runtime.engine.lock();
                for p in parts {
                    let _ = e.dm.set(p, "Locked", Variant::Bool(false));
                }
            }
            "simulationPlayAction" | "playGameActionGroup" => self.start_play(Mode::Play),
            "simulationRunAction" => self.start_play(Mode::Run),
            "simulationResetAction" => self.stop_play(),
            "simulationStopAction" => {
                if let Some(p) = &mut self.play {
                    p.paused = !p.paused;
                }
            }
            "viewObjectExplorerAction" => self.toggle_tab(Tab::Explorer),
            "viewPropertiesAction" => self.toggle_tab(Tab::Properties),
            "viewOutputWindowAction" => self.toggle_tab(Tab::Output),
            "viewCommandBarAction" => self.toggle_tab(Tab::Command),
            "viewScriptAnalysisAction" => self.toggle_tab(Tab::ScriptAnalysis),
            "viewDiagnosticsAction" => self.toggle_tab(Tab::Diagnostics),
            "viewTaskSchedulerAction" => self.toggle_tab(Tab::TaskScheduler),
            "viewScriptPerformanceAction" => self.toggle_tab(Tab::ScriptPerformance),
            "viewFindResultsWindowAction" => self.toggle_tab(Tab::FindResults),
            "objectBrowserAction" => {
                if let Some(t) = self.dock.find_tab(&Tab::ObjectBrowser) {
                    let _ = self.dock.remove_tab(t);
                } else {
                    if let Some(vp) = self.dock.find_tab(&Tab::Viewport) {
                        self.dock.set_focused_node_and_surface(vp.node_path());
                    }
                    self.dock
                        .main_surface_mut()
                        .push_to_focused_leaf(Tab::ObjectBrowser);
                }
            }
            "actionInsertAdvancedObject" | "viewToolboxAction" => {
                self.toggle_tab(Tab::AdvancedObjects)
            }
            "insertServiceAction" => {
                let mut e = self.doc.runtime.engine.lock();
                for s in [
                    "ReplicatedStorage",
                    "ServerScriptService",
                    "ServerStorage",
                    "Teams",
                ] {
                    let _ = e.dm.get_service(s);
                }
            }
            "openPluginsFolderAction" | "managePluginsAction" => {
                let dir = self.assets.join("plugins").join("studio");
                self.log(
                    MessageType::Info,
                    format!("plugins load from {}", dir.display()),
                );
            }
            "unionSelectionAction" | "negateSelectionAction" | "separateSelectionAction" => self
                .log(
                    MessageType::Warning,
                    "solid modeling is not available in this build",
                ),
            other => {
                if let Some(b) = self.plugins.find(other) {
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
        if let Err(e) = self
            .doc
            .runtime
            .execute(&source, "CommandBar", Identity::CmdLine)
        {
            self.log(MessageType::Error, e.to_string());
        }
        self.doc.runtime.pump();
    }

    pub fn open_script(&mut self, id: InstanceId) {
        self.ribbon.select_script_menu();
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
        if self.active_doc.is_some() {
            let keys = ctx.input(|i| {
                let cmd = i.modifiers.command;
                let shift = i.modifiers.shift;
                [
                    (cmd && !shift && i.key_pressed(egui::Key::F), "findAction"),
                    (
                        cmd && shift && i.key_pressed(egui::Key::F),
                        "findInScriptsAction",
                    ),
                    (cmd && i.key_pressed(egui::Key::H), "replaceAction"),
                    (cmd && i.key_pressed(egui::Key::G), "goToLineAction"),
                    (!shift && i.key_pressed(egui::Key::F3), "findNextAction"),
                    (shift && i.key_pressed(egui::Key::F3), "findPreviousAction"),
                ]
            });
            for (pressed, action) in keys {
                if pressed {
                    self.script_command(ctx, action);
                }
            }
        }
        if self.play.is_some() {
            if ctx.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::F5)) {
                self.stop_play();
            }
            if self.playing_game() {
                return;
            }
        } else if ctx.input(|i| i.key_pressed(egui::Key::F5) && !i.modifiers.shift) {
            self.start_play(Mode::Play);
            return;
        } else if ctx.input(|i| i.key_pressed(egui::Key::F8)) {
            self.start_play(Mode::Run);
            return;
        }
        let keys = ctx.input(|i| {
            let cmd = i.modifiers.command;
            [
                cmd && i.key_pressed(egui::Key::S),
                cmd && !i.modifiers.shift && i.key_pressed(egui::Key::Z),
                cmd && (i.key_pressed(egui::Key::Y)
                    || (i.modifiers.shift && i.key_pressed(egui::Key::Z))),
                i.key_pressed(egui::Key::Delete),
                cmd && i.key_pressed(egui::Key::D),
                cmd && i.key_pressed(egui::Key::C),
                cmd && i.key_pressed(egui::Key::V),
                cmd && i.key_pressed(egui::Key::X),
                cmd && !i.modifiers.shift && i.key_pressed(egui::Key::G),
                cmd && i.key_pressed(egui::Key::U),
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
        if ctx.input(|i| i.key_pressed(egui::Key::F) && !i.modifiers.command) {
            self.zoom_to_selection();
        }
        if ctx.input(|i| i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::V)) {
            self.doc.run_verb("PasteInto");
        }
        let sel = self.doc.selection();
        if (keys[10] || keys[11]) && !sel.is_empty() {
            self.doc.checkpoint();
            let axis = if keys[10] {
                rbx_math::Vec3::Y
            } else {
                rbx_math::Vec3::X
            };
            self.doc.rotate(&sel, axis, std::f32::consts::FRAC_PI_2);
        }
    }

    fn sync_plugin_groups(&mut self) {
        let buttons = self.doc.runtime.engine.lock().plugins.buttons.clone();
        let mut groups: Vec<(String, Vec<PluginItem>)> = Vec::new();
        for b in buttons.into_iter().filter(|b| !b.builtin) {
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
        let opens: Vec<InstanceId> =
            std::mem::take(&mut self.doc.runtime.engine.lock().plugins.open_requests);
        for id in opens {
            self.open_script(id);
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.ctx = Some(ctx.clone());
        self.poll_launch();
        self.shortcuts(&ctx);
        self.sync_plugin_groups();
        let windows: Vec<(String, String)> = {
            let mut out = vec![(self.place_name(), "#focus:viewport".to_string())];
            let e = self.doc.runtime.engine.lock();
            for (id, _) in &self.scripts {
                if e.dm.contains(*id) {
                    out.push((
                        e.dm.name_of(*id).to_string(),
                        format!("#focus:{}", id.raw()),
                    ));
                }
            }
            out
        };
        self.ribbon
            .dynamic
            .insert("switchWindowsMenu".into(), windows);
        let mut ribbon = std::mem::replace(&mut self.ribbon, Ribbon::empty());
        let mut icons = std::mem::replace(&mut self.icons, Icons::new(Path::new("")));
        let events = egui::Panel::top("ribbon")
            .exact_size(self.ribbon.height())
            .frame(egui::Frame::new())
            .show(ui, |ui| {
                ribbon.show(ui, &mut icons, &RibbonState { studio: self })
            })
            .inner;
        self.ribbon = ribbon;
        self.icons = icons;
        for ev in events {
            self.handle(ev);
        }
        let mut dock = std::mem::replace(&mut self.dock, DockState::new(Vec::new()));
        hide_dock_tab_bars(&mut dock);
        let style = dock_style(ui.style());
        let mut viewer = Viewer {
            studio: self,
            open: None,
            context: None,
            close: None,
        };
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::WINDOW).inner_margin(2.0))
            .show(ui, |ui| {
                DockArea::new(&mut dock)
                    .style(style)
                    .show_leaf_close_all_buttons(false)
                    .show_leaf_collapse_buttons(false)
                    .hidable_tab_bars(true)
                    .show_inside(ui, &mut viewer);
            });
        let open = viewer.open;
        let context = viewer.context;
        let close = viewer.close;
        self.dock = dock;
        let script_menu = self.active_doc.is_some();
        if script_menu != self.ribbon.script_menu {
            self.ribbon.set_script_menu(script_menu);
        }
        if let Some(tab) = close {
            self.toggle_tab(tab);
        }
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
        self.dialogs(&ctx);
    }

    fn context_menu(&mut self, ctx: &egui::Context) {
        let Some((id, at)) = self.context else { return };
        use crate::menus::{Entry, item};
        let selected = !self.doc.selection().is_empty();
        let classes: Vec<Entry> = {
            let mut names: Vec<String> = rbx_reflection::database()
                .classes()
                .filter(|c| {
                    c.creatable && self.meta.insertable(&c.name) && self.meta.order(&c.name) >= 0
                })
                .map(|c| c.name.clone())
                .collect();
            names.sort();
            names
                .into_iter()
                .map(|n| Entry::Item {
                    id: format!("#insert:{n}"),
                    class_icon: Some(self.meta.image(&n)),
                    text: n,
                    shortcut: String::new(),
                    icon: None,
                    enabled: true,
                    checked: false,
                })
                .collect()
        };
        let entries = vec![
            item(
                "cutAction",
                "Cut",
                "Ctrl+X",
                Some("images/Studio 2.0 icons/16x16/cut_clipboard_16_h.png"),
                selected,
            ),
            item(
                "copyAction",
                "Copy",
                "Ctrl+C",
                Some("images/Studio 2.0 icons/16x16/copy_clipboard_16_h.png"),
                selected,
            ),
            item("pasteIntoAction", "Paste Into", "Ctrl+Shift+V", None, true),
            item(
                "duplicateSelectionAction",
                "Duplicate",
                "Ctrl+D",
                Some("images/Studio 2.0 icons/16x16/copy_clipboard_16_h.png"),
                selected,
            ),
            item(
                "deleteSelectedAction",
                "Delete",
                "Del",
                Some("images/Studio 2.0 icons/16x16/delete_x_16_h.png"),
                selected,
            ),
            item("renameObjectAction", "Rename", "", None, selected),
            Entry::Separator,
            item(
                "groupSelectionAction",
                "Group",
                "Ctrl+G",
                Some("images/Studio 2.0 icons/16x16/group_16.png"),
                selected,
            ),
            item(
                "ungroupSelectionAction",
                "Ungroup",
                "Ctrl+U",
                Some("images/Studio 2.0 icons/16x16/ungroup_16.png"),
                selected,
            ),
            item(
                "selectChildrenAction",
                "Select Children",
                "",
                None,
                selected,
            ),
            item(
                "zoomExtentsAction",
                "Zoom to",
                "F",
                Some("images/c_zoomExtent_old.png"),
                selected,
            ),
            Entry::Separator,
            Entry::Item {
                id: "#insertpart".into(),
                text: "Insert Part".into(),
                shortcut: String::new(),
                icon: None,
                class_icon: Some(1),
                enabled: true,
                checked: false,
            },
            Entry::Sub {
                text: "Insert Object".into(),
                items: classes,
            },
            item(
                "insertIntoFileAction",
                "Insert from File...",
                "",
                None,
                true,
            ),
            Entry::Separator,
            item(
                "selectionSaveToFileAction",
                "Save to File...",
                "",
                None,
                selected,
            ),
            item("saveToRobloxAction", "Save to Roblox...", "", None, false),
            item(
                "createNewLinkedSourceAction",
                "Create new LinkedSource...",
                "",
                None,
                false,
            ),
            item(
                "publishAsPluginAction",
                "Publish as Plugin...",
                "",
                None,
                false,
            ),
            item(
                "exportSelectionAction",
                "Export Selection...",
                "",
                None,
                selected,
            ),
            Entry::Separator,
            item(
                "launchHelpForSelectionAction",
                "Help",
                "",
                Some("images/Studio Ribbon Icons/help.png"),
                false,
            ),
        ];
        let mut icons = std::mem::replace(&mut self.icons, Icons::new(Path::new("")));
        let (chosen, outside) = crate::menus::show(
            ctx,
            egui::Id::unique("explorer-context"),
            at,
            &entries,
            &mut icons,
        );
        self.icons = icons;
        if let Some(a) = chosen {
            self.context = None;
            match a.as_str() {
                "renameObjectAction" => self.explorer.start_rename(&self.doc, id),
                "#insertpart" => {
                    let parent = self.doc.selection().first().copied().unwrap_or(id);
                    if let Ok(p) = self.doc.insert("Part", parent) {
                        self.doc.select(vec![p]);
                    }
                }
                other => {
                    if let Some(class) = other.strip_prefix("#insert:") {
                        self.insert_into_selection(class);
                    } else {
                        self.action(other);
                    }
                }
            }
        } else if outside {
            self.context = None;
        }
    }

    pub fn zoom_to_selection(&mut self) {
        let sel = self.doc.selection();
        let e = self.doc.runtime.engine.lock();
        let parts: Vec<InstanceId> = sel
            .iter()
            .flat_map(|s| std::iter::once(*s).chain(e.dm.descendants(*s)))
            .filter(|d| e.dm.is_a(*d, "BasePart"))
            .collect();
        let mut lo = rbx_math::Vec3::splat(f32::INFINITY);
        let mut hi = rbx_math::Vec3::splat(f32::NEG_INFINITY);
        for p in parts {
            if let Some((cf, size)) = viewport::part_box(&e.dm, p) {
                let h = size * 0.5;
                for i in 0..8 {
                    let c = rbx_math::Vec3::new(
                        if i & 1 == 0 { -h.x } else { h.x },
                        if i & 2 == 0 { -h.y } else { h.y },
                        if i & 4 == 0 { -h.z } else { h.z },
                    );
                    let w = cf.point_to_world_space(c);
                    lo = lo.min(w);
                    hi = hi.max(w);
                }
            }
        }
        drop(e);
        if !lo.is_finite() {
            return;
        }
        let center = (lo + hi) * 0.5;
        let radius = ((hi - lo).length() * 0.5).max(1.0);
        let distance = radius / (self.view.camera.fov_y.to_radians() * 0.5).tan() * 1.1;
        let dir = self.view.camera.cframe().look_vector();
        let fov = self.view.camera.fov_y;
        self.view.camera = FlyCamera::looking_at(center - dir * distance, center);
        self.view.camera.fov_y = fov;
    }

    pub fn save_selection_to_file(&mut self) {
        let sel = self.doc.selection();
        if sel.is_empty() {
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Roblox Model Files", &["rbxm"])
            .add_filter("Roblox XML Model Files", &["rbxmx"])
            .set_file_name("Model.rbxm")
            .save_file()
        else {
            return;
        };
        let tree = self
            .doc
            .runtime
            .engine
            .lock()
            .dm
            .save_tree(&sel, rbx_dom::SaveOptions::default());
        let data = if path.extension().is_some_and(|e| e == "rbxmx") {
            Ok(rbx_format::write_xml(&tree).into_bytes())
        } else {
            rbx_format::write_binary(&tree, rbx_format::BinaryWriteOptions::default())
                .map_err(|e| e.to_string())
        };
        match data.and_then(|d| std::fs::write(&path, d).map_err(|e| e.to_string())) {
            Ok(()) => self.log(MessageType::Info, format!("saved {}", path.display())),
            Err(e) => self.log(MessageType::Error, e),
        }
    }

    pub fn export_selection_obj(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Wavefront OBJ", &["obj"])
            .set_file_name("Selection.obj")
            .save_file()
        else {
            return;
        };
        let parts = self.selected_parts();
        let e = self.doc.runtime.engine.lock();
        let mut out = String::from("# exported by Roblox Studio\n");
        let mut base = 1;
        for p in parts {
            let Some((cf, size)) = viewport::part_box(&e.dm, p) else {
                continue;
            };
            out.push_str(&format!("o {}\n", e.dm.name_of(p)));
            let h = size * 0.5;
            for i in 0..8 {
                let c = rbx_math::Vec3::new(
                    if i & 1 == 0 { -h.x } else { h.x },
                    if i & 2 == 0 { -h.y } else { h.y },
                    if i & 4 == 0 { -h.z } else { h.z },
                );
                let w = cf.point_to_world_space(c);
                out.push_str(&format!("v {} {} {}\n", w.x, w.y, w.z));
            }
            for f in [
                [0, 2, 3, 1],
                [4, 5, 7, 6],
                [0, 1, 5, 4],
                [2, 6, 7, 3],
                [0, 4, 6, 2],
                [1, 3, 7, 5],
            ] {
                out.push_str(&format!(
                    "f {} {} {} {}\n",
                    base + f[0],
                    base + f[1],
                    base + f[2],
                    base + f[3]
                ));
            }
            base += 8;
        }
        drop(e);
        match std::fs::write(&path, out) {
            Ok(()) => self.log(MessageType::Info, format!("exported {}", path.display())),
            Err(err) => self.log(MessageType::Error, err.to_string()),
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
            ui.painter().image(
                tex,
                rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        self.viewport_input(ui, &response, rect);
        if self.playing_game() {
            self.overlay = rbx_render::adorn::Adorns::default();
            let fps = 1.0 / ui.input(|i| i.stable_dt).max(1e-4);
            crate::extras::stats_overlay(&ui.painter_at(rect), rect, self, fps);
            return;
        }
        let sel = self.doc.selection();
        let fps = 1.0 / ui.input(|i| i.stable_dt).max(1e-4);
        {
            let e = self.doc.runtime.engine.lock();
            self.overlay = self
                .tools
                .overlay(&e.dm, self.tool, &sel, self.view.camera.position);
            if self.extras.show_grid {
                crate::extras::zero_plane_grid(
                    &mut self.overlay,
                    self.view.camera.position,
                    self.extras.grid_studs,
                );
            }
            self.tools.draw_2d(
                &ui.painter_at(rect),
                rect,
                &self.view.camera,
                &e.dm,
                self.tool,
                &sel,
            );
        }
        if self.extras.axis_widget {
            crate::extras::axis_widget(&ui.painter_at(rect), rect, &self.view.camera);
        }
        crate::extras::stats_overlay(&ui.painter_at(rect), rect, self, fps);
    }

    fn game_input(&mut self, ui: &mut Ui, response: &egui::Response, rect: Rect) {
        let Some(session) = self.play.as_mut() else {
            return;
        };
        let (delta, scroll, events) = ui.input(|i| {
            (
                i.pointer.delta(),
                i.smooth_scroll_delta.y,
                i.events
                    .iter()
                    .filter_map(|e| match e {
                        egui::Event::PointerButton {
                            pos,
                            button,
                            pressed,
                            ..
                        } => Some((*pos, *button, *pressed)),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
            )
        });
        let local = |p: egui::Pos2| (p.x - rect.left(), p.y - rect.top());
        if let Some(pos) = response.hover_pos().or(response.interact_pointer_pos()) {
            if delta != egui::Vec2::ZERO {
                session.mouse(
                    &self.doc,
                    InputKind::Changed,
                    user_input::MOUSE_MOVEMENT,
                    local(pos),
                    (delta.x, delta.y),
                );
            }
            if scroll != 0.0 && response.hovered() {
                session.wheel(&self.doc, local(pos), scroll.signum());
            }
        }
        for (pos, button, pressed) in events {
            if !rect.contains(pos) && pressed {
                continue;
            }
            let input_type = match button {
                egui::PointerButton::Primary => user_input::MOUSE_BUTTON1,
                egui::PointerButton::Secondary => user_input::MOUSE_BUTTON2,
                egui::PointerButton::Middle => user_input::MOUSE_BUTTON3,
                _ => continue,
            };
            let kind = if pressed {
                InputKind::Began
            } else {
                InputKind::Ended
            };
            session.mouse(&self.doc, kind, input_type, local(pos), (0.0, 0.0));
        }
    }

    fn viewport_input(&mut self, ui: &mut Ui, response: &egui::Response, rect: Rect) {
        if ui.input(|i| i.pointer.any_pressed()) {
            self.viewport_focus = response.hovered();
        }
        if self.playing_game() {
            self.game_input(ui, response, rect);
            return;
        }
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
            self.send_input(
                InputKind::Began,
                user_input::MOUSE_BUTTON1,
                pos,
                egui::Vec2::ZERO,
            );
        }
        if released && (response.hovered() || response.dragged()) {
            self.send_input(
                InputKind::Ended,
                user_input::MOUSE_BUTTON1,
                pos,
                egui::Vec2::ZERO,
            );
        }
        let (origin, dir) = self.view.camera.ray(rect, pos);
        {
            let sel = self.doc.selection();
            let e = self.doc.runtime.engine.lock();
            self.tools
                .update_hover(&e.dm, self.tool, &sel, &self.view.camera, origin, dir);
        }
        if pressed && response.hovered() {
            self.tools.press = Some((origin, dir, self.tools.over_handle));
        }
        if response.drag_started_by(egui::PointerButton::Primary)
            && !matches!(self.tool, Tool::Lock | Tool::Paint)
        {
            let (o, d, handle) =
                self.tools
                    .press
                    .take()
                    .unwrap_or((origin, dir, self.tools.over_handle));
            self.tools.over_handle = handle;
            if self.tools.begin(
                &mut self.doc,
                self.tool,
                self.collisions,
                &self.view.camera,
                o,
                d,
            ) {
                self.tools.update(&mut self.doc, origin, dir, self.grid);
            }
        } else if response.dragged_by(egui::PointerButton::Primary) {
            self.tools.update(&mut self.doc, origin, dir, self.grid);
        } else if response.drag_stopped() {
            self.tools.end(&mut self.doc);
        } else if response.clicked() && matches!(self.tool, Tool::Lock | Tool::Paint) {
            let hit = {
                let e = self.doc.runtime.engine.lock();
                viewport::pick(&e.dm, origin, dir, self.tool == Tool::Lock, &[]).map(|h| h.part)
            };
            if let Some(part) = hit {
                self.doc.checkpoint();
                let mut e = self.doc.runtime.engine.lock();
                match self.tool {
                    Tool::Lock => {
                        let locked = e.dm.get_bool(part, "Locked");
                        let _ = e.dm.set(part, "Locked", Variant::Bool(!locked));
                    }
                    _ => {
                        if let Some(c) = self.extras.paint_color {
                            let _ = e.dm.set(
                                part,
                                "BrickColor",
                                Variant::BrickColor(rbx_math::BrickColor::new(c as i64)),
                            );
                        }
                        if let Some(m) = self.extras.paint_material {
                            let _ = e.dm.set(part, "Material", Variant::Enum(m));
                        }
                    }
                }
            }
        } else if response.clicked() {
            if self.tools.over_handle.is_some() {
                return;
            }
            let hit = {
                let e = self.doc.runtime.engine.lock();
                viewport::pick(&e.dm, origin, dir, false, &[])
                    .map(|h| viewport::selectable(&e.dm, h.part))
            };
            let additive =
                ui.input(|i| i.modifiers.shift || i.modifiers.command || i.modifiers.ctrl);
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
                    ui.label(
                        egui::RichText::new(format!(" {text}"))
                            .font(FontId::proportional(theme::FONT_SIZE))
                            .color(color),
                    );
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
            egui::TextEdit::singleline(&mut self.object_filter)
                .hint_text("Search object (Ctrl+I)")
                .margin(vec2(4.0, 3.0)),
        );
        let filter = self.object_filter.to_lowercase();
        let mut classes: Vec<String> = rbx_reflection::database()
            .classes()
            .filter(|c| {
                c.creatable && self.meta.insertable(&c.name) && self.meta.order(&c.name) >= 0
            })
            .map(|c| c.name.clone())
            .filter(|n| filter.is_empty() || n.to_lowercase().contains(&filter))
            .collect();
        classes.sort();
        let mut chosen = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for c in &classes {
                    let (r, resp) = ui.allocate_exact_size(
                        vec2(ui.available_width(), explorer::ROW),
                        Sense::click(),
                    );
                    if resp.hovered() {
                        theme::hovered_row(ui.painter(), r);
                    }
                    if let Some((tex, uv)) = self.icons.class(ui.ctx(), self.meta.image(c)) {
                        ui.painter().image(
                            tex,
                            Rect::from_min_size(
                                pos2(r.left() + 4.0, r.center().y - 8.0),
                                vec2(16.0, 16.0),
                            ),
                            uv,
                            Color32::WHITE,
                        );
                    }
                    ui.painter().text(
                        pos2(r.left() + 24.0, r.center().y),
                        Align2::LEFT_CENTER,
                        c,
                        FontId::proportional(theme::FONT_SIZE),
                        Color32::BLACK,
                    );
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
    for t in [
        &mut s.tab.active,
        &mut s.tab.inactive,
        &mut s.tab.focused,
        &mut s.tab.hovered,
    ] {
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
    s.tab.tab_body.hidden_tab_bar_drag_height = Some(0.0);
    s.buttons.show_tab_bar_size = 0.0;
    s.buttons.show_tab_bar_color = Color32::TRANSPARENT;
    s.buttons.show_tab_bar_active_color = Color32::TRANSPARENT;
    s.tab_bar.bg_fill = theme::DOC_TAB_BAR;
    s.tab_bar.hline_color = theme::VIEW_BORDER;
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
    close: Option<Tab>,
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
            Tab::ScriptAnalysis => "Script Analysis".into(),
            Tab::TaskScheduler => "Task Scheduler".into(),
            Tab::ScriptPerformance => "Script Performance".into(),
            Tab::FindResults => "Find Results".into(),
            Tab::ObjectBrowser => "Object Browser".into(),
            other => format!("{other:?}").into(),
        }
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Tab) {
        match tab {
            Tab::Script(id) => self.studio.active_doc = Some(*id),
            Tab::Viewport | Tab::ObjectBrowser => self.studio.active_doc = None,
            _ => {}
        }
        if !is_document(tab) {
            let title = self.title(tab).text().to_string();
            if dock_title(ui, &title) {
                self.close = Some(tab.clone());
            }
            ui.add_space(1.0);
        }
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
            Tab::ScriptAnalysis => self.studio.script_analysis_ui(ui),
            Tab::Diagnostics => self.studio.diagnostics_ui(ui),
            Tab::TaskScheduler => self.studio.task_scheduler_ui(ui),
            Tab::ScriptPerformance => self.studio.script_performance_ui(ui),
            Tab::FindResults => self.studio.find_results_ui(ui),
            Tab::ObjectBrowser => self.studio.object_browser_ui(ui),
            Tab::Script(id) => {
                let id = *id;
                self.studio.script_editor_ui(ui, id);
            }
        }
    }

    fn is_closeable(&self, tab: &Tab) -> bool {
        !matches!(tab, Tab::Viewport)
    }

    fn tab_style_override(&self, tab: &Tab, global: &TabStyle) -> Option<TabStyle> {
        is_document(tab).then(|| document_tab_style(global))
    }
}
