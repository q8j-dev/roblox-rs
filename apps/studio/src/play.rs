use parking_lot::Mutex;
use rbx_dom::{DataModel, Variant};
use rbx_math::Vec3;
use rbx_script::api::user_input::{self, InputEvent, InputKind};
use rbx_script::{Engine, ScriptRuntime};
use rbx_studio_ui::Document;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

const STEP: f64 = 1.0 / 60.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Play,
    Run,
}

pub struct PlaySession {
    pub mode: Mode,
    pub edit: Document,
    pub paused: bool,
    accumulator: f64,
    keys: HashSet<u32>,
    pub cursor: (f32, f32),
}

pub fn start(edit: &Document, assets: &Path, mode: Mode) -> Result<Document, String> {
    let bytes = edit.bytes()?;
    let mut dm = DataModel::new();
    dm.load_place(&bytes).map_err(|e| e.to_string())?;
    dm.ensure_terrain();
    let mut engine = Engine::new(dm);
    engine.content_root = Some(assets.join("content"));
    engine.animate_fallback = true;
    engine.audio = rbx_audio::AudioEngine::open();
    engine.process_after_load();
    engine.running = true;
    let runtime = ScriptRuntime::new(Arc::new(Mutex::new(engine))).map_err(|e| e.to_string())?;
    if mode == Mode::Play {
        let players = runtime.engine.lock().dm.get_service("Players");
        let result = players.map_err(|e| e.to_string()).and_then(|players| {
            let player = rbx_script::api::players::make_local_player(
                &runtime.lua,
                players,
                1,
                false,
                Some("Player1"),
            )
            .map_err(|e| e.to_string())?;
            rbx_script::api::players::load_character(&runtime.lua, player, true, "")
                .map_err(|e| e.to_string())
        });
        result?;
        runtime
            .start_core_scripts("StarterScript")
            .map_err(|e| e.to_string())?;
    }
    Ok(Document::from_runtime(runtime, &assets.join("content")))
}

impl PlaySession {
    pub fn new(edit: Document, mode: Mode) -> PlaySession {
        PlaySession {
            mode,
            edit,
            paused: false,
            accumulator: 0.0,
            keys: HashSet::new(),
            cursor: (0.0, 0.0),
        }
    }

    pub fn frame(&mut self, live: &Document, dt: f64, screen: (f32, f32)) {
        live.runtime.engine.lock().dm.gui_view.screen = rbx_math::Vec2::new(screen.0, screen.1);
        if self.paused {
            return;
        }
        self.accumulator += dt.min(0.25);
        let mut steps = 0;
        while self.accumulator >= STEP && steps < 4 {
            live.runtime.step(STEP);
            self.accumulator -= STEP;
            steps += 1;
        }
        let mut e = live.runtime.engine.lock();
        if let Some(ws) = e.dm.find_service("Workspace")
            && e.dm.get_ref(ws, "CurrentCamera").is_none()
            && let Ok(c) = e.dm.create("Camera")
        {
            let _ = e.dm.set_parent(c, Some(ws));
            let _ = e.dm.set(ws, "CurrentCamera", Variant::Ref(Some(c)));
        }
        drop(e);
        live.runtime.render_step(dt);
    }

    pub fn key(&mut self, live: &Document, code: u32, pressed: bool, text: Option<String>) {
        live.runtime.engine.lock().audio.resume_output();
        let kind = if pressed {
            self.keys.insert(code);
            InputKind::Began
        } else {
            self.keys.remove(&code);
            InputKind::Ended
        };
        live.runtime.input(InputEvent {
            kind,
            input_type: user_input::KEYBOARD,
            key_code: code,
            position: Vec3::ZERO,
            delta: Vec3::ZERO,
            text: if pressed { text } else { None },
        });
    }

    pub fn release_keys(&mut self, live: &Document) {
        for k in self.keys.drain().collect::<Vec<_>>() {
            live.runtime.input(InputEvent {
                kind: InputKind::Ended,
                input_type: user_input::KEYBOARD,
                key_code: k,
                position: Vec3::ZERO,
                delta: Vec3::ZERO,
                text: None,
            });
        }
    }

    pub fn mouse(
        &mut self,
        live: &Document,
        kind: InputKind,
        input_type: u32,
        pos: (f32, f32),
        delta: (f32, f32),
    ) {
        if input_type != user_input::MOUSE_MOVEMENT {
            live.runtime.engine.lock().audio.resume_output();
        }
        self.cursor = pos;
        live.runtime.input(InputEvent {
            kind,
            input_type,
            key_code: 0,
            position: Vec3::new(pos.0, pos.1, 0.0),
            delta: Vec3::new(delta.0, delta.1, 0.0),
            text: None,
        });
    }

    pub fn wheel(&mut self, live: &Document, pos: (f32, f32), amount: f32) {
        live.runtime.input(InputEvent {
            kind: InputKind::Changed,
            input_type: user_input::MOUSE_WHEEL,
            key_code: 0,
            position: Vec3::new(pos.0, pos.1, amount),
            delta: Vec3::ZERO,
            text: None,
        });
    }
}
