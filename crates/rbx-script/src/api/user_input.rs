use super::actions::{ActionInput, dispatch};
use super::camera::CameraView;
use super::{Registry, Ret, none, ret};
use crate::instance::push_instance;
use crate::runtime::{engine, fire_lua, pump};
use crate::types::{LCFrame, LRay};
use mlua::{Lua, MultiValue, Result, Value};
use rbx_dom::gui::{Placement, canvas, placement};
use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{CFrame, Ray, Vec2, Vec3};
use rustc_hash::FxHashMap as HashMap;

pub const MOUSE_BUTTON1: u32 = 0;
pub const MOUSE_BUTTON2: u32 = 1;
pub const MOUSE_BUTTON3: u32 = 2;
pub const MOUSE_WHEEL: u32 = 3;
pub const MOUSE_MOVEMENT: u32 = 4;
pub const TOUCH: u32 = 7;
pub const KEYBOARD: u32 = 8;
pub const FOCUS: u32 = 9;

const MOUSE_RAY_LENGTH: f32 = 1000.0;
const KEY_BACKSPACE: u32 = 8;
const KEY_RETURN: u32 = 13;
const KEY_ESCAPE: u32 = 27;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputKind {
    Began,
    Changed,
    Ended,
}

#[derive(Clone, Debug)]
pub struct InputEvent {
    pub kind: InputKind,
    pub input_type: u32,
    pub key_code: u32,
    pub position: Vec3,
    pub delta: Vec3,
    pub text: Option<String>,
}

#[derive(Default)]
pub struct PointerState {
    pub position: Vec2,
    pub objects: HashMap<(u32, u32), InstanceId>,
    pub hover: Vec<InstanceId>,
    pub pressed: HashMap<u32, InstanceId>,
    pub touch_enabled: bool,
    pub hover_target: Option<InstanceId>,
    pub frame_delta: Vec2,
    pub last_frame_delta: Vec2,
}

pub fn mouse_sensitivity(e: &crate::runtime::Engine) -> f32 {
    e.user_game_settings
        .and_then(|s| e.dm.get(s, "MouseSensitivity").ok())
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
        .unwrap_or(1.0)
}

fn state_value(kind: InputKind) -> u32 {
    match kind {
        InputKind::Began => 0,
        InputKind::Changed => 1,
        InputKind::Ended => 2,
    }
}

fn is_mouse(t: u32) -> bool {
    matches!(
        t,
        MOUSE_BUTTON1 | MOUSE_BUTTON2 | MOUSE_BUTTON3 | MOUSE_WHEEL | MOUSE_MOVEMENT
    )
}

#[derive(Clone, Copy)]
struct Rect {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

impl Rect {
    fn of(p: &Placement) -> Rect {
        Rect {
            x0: p.position.x,
            y0: p.position.y,
            x1: p.position.x + p.size.x,
            y1: p.position.y + p.size.y,
        }
    }

    fn contains(&self, p: Vec2) -> bool {
        p.x >= self.x0 && p.x < self.x1 && p.y >= self.y0 && p.y < self.y1
    }

    fn intersect(&self, o: &Rect) -> Rect {
        Rect {
            x0: self.x0.max(o.x0),
            y0: self.y0.max(o.y0),
            x1: self.x1.min(o.x1),
            y1: self.y1.min(o.y1),
        }
    }
}

struct Candidate {
    id: InstanceId,
    z: i64,
    order: usize,
}

fn effective_bool(dm: &DataModel, id: InstanceId, name: &str, default: bool) -> bool {
    match dm.effective(id, name) {
        Ok(Variant::Bool(b)) => b,
        _ => default,
    }
}

fn effective_z(dm: &DataModel, id: InstanceId) -> i64 {
    dm.effective(id, "ZIndex")
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0) as i64
}

struct Walker<'a> {
    dm: &'a DataModel,
    point: Vec2,
    sibling: bool,
    order: usize,
    out: Vec<Candidate>,
}

impl Walker<'_> {
    fn walk(&mut self, id: InstanceId, clip: Option<Rect>) {
        let dm = self.dm;
        let class = dm.class_of(id).unwrap_or("");
        if class == "Folder" {
            for c in self.children(id) {
                self.walk(c, clip);
            }
            return;
        }
        if !dm.is_a(id, "GuiObject") || !effective_bool(dm, id, "Visible", true) {
            return;
        }
        let p = placement(dm, id);
        let own = Rect::of(&p);
        self.order += 1;
        if own.contains(self.point)
            && clip.is_none_or(|c| c.contains(self.point))
            && effective_bool(dm, id, "Interactable", true)
        {
            self.out.push(Candidate {
                id,
                z: if self.sibling { 0 } else { effective_z(dm, id) },
                order: self.order,
            });
        }
        let clips = effective_bool(dm, id, "ClipsDescendants", false) || class == "ScrollingFrame";
        let child_clip = if clips {
            Some(clip.map_or(own, |c| c.intersect(&own)))
        } else {
            clip
        };
        for c in self.children(id) {
            self.walk(c, child_clip);
        }
    }

    fn children(&self, id: InstanceId) -> Vec<InstanceId> {
        let mut kids = self.dm.children_of(id).to_vec();
        if self.sibling {
            kids.sort_by_key(|k| effective_z(self.dm, *k));
        }
        kids
    }
}

fn hits(dm: &DataModel, point: Vec2) -> Vec<Vec<InstanceId>> {
    let mut layers = Vec::new();
    for root in rbx_dom::gui::screen_gui_roots(dm).into_iter().rev() {
        let sibling = dm
            .effective(root, "ZIndexBehavior")
            .ok()
            .is_some_and(|v| matches!(v, Variant::Enum(1)));
        let mut w = Walker {
            dm,
            point,
            sibling,
            order: 0,
            out: Vec::new(),
        };
        for c in w.children(root) {
            w.walk(c, None);
        }
        let mut cands = w.out;
        cands.sort_by(|a, b| b.z.cmp(&a.z).then(b.order.cmp(&a.order)));
        layers.push(cands.into_iter().map(|c| c.id).collect());
    }
    layers
}

fn is_active(dm: &DataModel, id: InstanceId) -> bool {
    dm.get(id, "Active")
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        || dm.is_a(id, "GuiButton")
        || dm.class_of(id) == Some("TextBox")
}

fn input_object(lua: &Lua, ev: &InputEvent) -> Option<InstanceId> {
    let eng = engine(lua);
    let mut e = eng.lock();
    let key = (
        ev.input_type,
        if ev.input_type == KEYBOARD || ev.input_type == TOUCH {
            ev.key_code
        } else {
            0
        },
    );
    let existing = e
        .input
        .pointer
        .objects
        .get(&key)
        .copied()
        .filter(|&o| e.dm.contains(o));
    let obj = match existing {
        Some(o) => o,
        None => {
            let o = e.dm.create_internal("InputObject").ok()?;
            e.dm.retain(o);
            e.input.pointer.objects.insert(key, o);
            o
        }
    };
    let inset = Vec3::new(e.dm.gui_view.inset[0], e.dm.gui_view.inset[1], 0.0);
    let position = if is_mouse(ev.input_type) || ev.input_type == TOUCH {
        ev.position - inset
    } else {
        ev.position
    };
    if ev.input_type == TOUCH && ev.kind == InputKind::Ended {
        e.input.pointer.objects.remove(&key);
    }
    let key_code = if ev.input_type == TOUCH {
        0
    } else {
        ev.key_code
    };
    e.dm.set_unchecked(obj, "UserInputType", Variant::Enum(ev.input_type));
    e.dm.set_unchecked(obj, "UserInputState", Variant::Enum(state_value(ev.kind)));
    e.dm.set_unchecked(obj, "KeyCode", Variant::Enum(key_code));
    e.dm.set_unchecked(obj, "Position", Variant::Vector3(position));
    let delta = if ev.input_type == MOUSE_MOVEMENT {
        let k = mouse_sensitivity(&e);
        e.input.pointer.frame_delta += Vec2::new(ev.delta.x, ev.delta.y) * k;
        ev.delta * k
    } else {
        ev.delta
    };
    e.dm.set_unchecked(obj, "Delta", Variant::Vector3(delta));
    Some(obj)
}

fn fire_xy(lua: &Lua, id: InstanceId, event: &'static str, p: Vec2) {
    fire_lua(
        lua,
        id,
        event,
        MultiValue::from_vec(vec![
            Value::Integer(p.x as mlua::Integer),
            Value::Integer(p.y as mlua::Integer),
        ]),
    );
}

fn focus_text_box(lua: &Lua, id: InstanceId) {
    let previous = engine(lua).lock().input.focused_text_box;
    if previous == Some(id) {
        return;
    }
    if let Some(prev) = previous {
        release_focus(lua, prev, false);
    }
    let uis = {
        let eng = engine(lua);
        let mut e = eng.lock();
        e.input.focused_text_box = Some(id);
        if e.dm.get_bool(id, "ClearTextOnFocus") {
            let _ = e.dm.set(id, "Text", Variant::String(String::new()));
        }
        e.dm.find_service("UserInputService")
    };
    fire_lua(lua, id, "Focused", MultiValue::new());
    if let Some(uis) = uis
        && let Ok(v) = push_instance(lua, id)
    {
        fire_lua(lua, uis, "TextBoxFocused", MultiValue::from_vec(vec![v]));
    }
}

fn release_focus(lua: &Lua, id: InstanceId, enter: bool) {
    let uis = {
        let eng = engine(lua);
        let mut e = eng.lock();
        if e.input.focused_text_box != Some(id) {
            return;
        }
        e.input.focused_text_box = None;
        e.dm.find_service("UserInputService")
    };
    fire_lua(
        lua,
        id,
        "FocusLost",
        MultiValue::from_vec(vec![Value::Boolean(enter)]),
    );
    if let Some(uis) = uis
        && let Ok(v) = push_instance(lua, id)
    {
        fire_lua(
            lua,
            uis,
            "TextBoxFocusReleased",
            MultiValue::from_vec(vec![v]),
        );
    }
}

fn text_input(lua: &Lua, ev: &InputEvent) -> bool {
    let Some(tb) = engine(lua).lock().input.focused_text_box else {
        return false;
    };
    if ev.input_type != KEYBOARD {
        return false;
    }
    if ev.kind != InputKind::Began {
        return true;
    }
    match ev.key_code {
        KEY_RETURN => release_focus(lua, tb, true),
        KEY_ESCAPE => release_focus(lua, tb, false),
        KEY_BACKSPACE => {
            let eng = engine(lua);
            let mut e = eng.lock();
            let mut text =
                e.dm.get(tb, "Text")
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default();
            text.pop();
            let _ = e.dm.set(tb, "Text", Variant::String(text));
        }
        _ => {
            if let Some(t) = ev
                .text
                .as_ref()
                .filter(|t| t.chars().all(|c| !c.is_control()))
            {
                let eng = engine(lua);
                let mut e = eng.lock();
                let mut text =
                    e.dm.get(tb, "Text")
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_string))
                        .unwrap_or_default();
                text.push_str(t);
                let _ = e.dm.set(tb, "Text", Variant::String(text));
            }
        }
    }
    true
}

fn set_gui_state(lua: &Lua, id: InstanceId, state: u32) {
    let eng = engine(lua);
    let mut e = eng.lock();
    if e.dm.contains(id)
        && e.dm.property_descriptor(id, "GuiState").is_some()
        && !matches!(e.dm.get(id, "GuiState"), Ok(Variant::Enum(v)) if v == state)
    {
        let _ = e.dm.set(id, "GuiState", Variant::Enum(state));
    }
}

fn fire_input(lua: &Lua, chain: &[InstanceId], event: &'static str, obj: InstanceId) {
    for &id in chain {
        if let Ok(v) = push_instance(lua, obj) {
            fire_lua(lua, id, event, MultiValue::from_vec(vec![v]));
        }
    }
}

fn input_chain(dm: &DataModel, under: &[InstanceId]) -> Vec<InstanceId> {
    let mut chain = Vec::new();
    for &id in under {
        chain.push(id);
        if is_active(dm, id) {
            break;
        }
    }
    chain
}

fn gui_mouse(lua: &Lua, ev: &InputEvent, obj: InstanceId) -> bool {
    let point = Vec2::new(ev.position.x, ev.position.y);
    let (layers, inset) = {
        let eng = engine(lua);
        let e = eng.lock();
        (
            hits(&e.dm, point),
            Vec2::new(e.dm.gui_view.inset[0], e.dm.gui_view.inset[1]),
        )
    };
    let local = point - inset;
    let under: Vec<InstanceId> = layers.iter().flatten().copied().collect();
    if ev.input_type == MOUSE_MOVEMENT {
        let previous = std::mem::take(&mut engine(lua).lock().input.pointer.hover);
        for &id in previous.iter().filter(|id| !under.contains(id)) {
            fire_xy(lua, id, "MouseLeave", local);
        }
        for &id in under.iter().filter(|id| !previous.contains(id)) {
            fire_xy(lua, id, "MouseEnter", local);
        }
        for &id in &under {
            fire_xy(lua, id, "MouseMoved", local);
        }
        let (chain, top, old, pressed) = {
            let eng = engine(lua);
            let mut e = eng.lock();
            let chain = input_chain(&e.dm, &under);
            let top = chain.last().copied().filter(|&t| is_active(&e.dm, t));
            let pressed: Vec<InstanceId> = e.input.pointer.pressed.values().copied().collect();
            e.input.pointer.hover = under;
            let old = std::mem::replace(&mut e.input.pointer.hover_target, top);
            (chain, top, old, pressed)
        };
        if old != top {
            if let Some(o) = old
                && !pressed.contains(&o)
            {
                set_gui_state(lua, o, 0);
            }
            if let Some(t) = top
                && !pressed.contains(&t)
            {
                set_gui_state(lua, t, 1);
            }
        }
        fire_input(lua, &chain, "InputChanged", obj);
        return false;
    }
    let target = {
        let eng = engine(lua);
        let e = eng.lock();
        layers
            .iter()
            .find_map(|l| l.iter().copied().find(|&id| is_active(&e.dm, id)))
    };
    if ev.input_type == MOUSE_WHEEL {
        let scroller = {
            let eng = engine(lua);
            let e = eng.lock();
            under.iter().copied().find(|&id| {
                e.dm.class_of(id) == Some("ScrollingFrame") && e.dm.get_bool(id, "ScrollingEnabled")
            })
        };
        if let Some(sf) = scroller {
            let eng = engine(lua);
            let mut e = eng.lock();
            let own = placement(&e.dm, sf);
            let cv = canvas(&e.dm, sf, own);
            let current = match e.dm.get(sf, "CanvasPosition") {
                Ok(Variant::Vector2(v)) => v,
                _ => Vec2::ZERO,
            };
            let max = (cv.size.y - own.size.y).max(0.0);
            let y = (current.y - ev.position.z * 40.0).clamp(0.0, max);
            let _ = e.dm.set(
                sf,
                "CanvasPosition",
                Variant::Vector2(Vec2::new(current.x, y)),
            );
        }
        if let Some(t) = target {
            let event = if ev.position.z > 0.0 {
                "MouseWheelForward"
            } else {
                "MouseWheelBackward"
            };
            fire_xy(lua, t, event, local);
        }
        return target.is_some() || scroller.is_some();
    }
    let button = match ev.input_type {
        MOUSE_BUTTON1 => 1,
        MOUSE_BUTTON2 => 2,
        _ => return target.is_some(),
    };
    let focused = engine(lua).lock().input.focused_text_box;
    if ev.kind == InputKind::Began {
        if let Some(f) = focused
            && target != Some(f)
        {
            release_focus(lua, f, false);
        }
        let chain = {
            let eng = engine(lua);
            let e = eng.lock();
            input_chain(&e.dm, &under)
        };
        fire_input(lua, &chain, "InputBegan", obj);
        let Some(t) = target else {
            return false;
        };
        if button == 1 {
            set_gui_state(lua, t, 2);
        }
        let (is_button, is_box) = {
            let eng = engine(lua);
            let e = eng.lock();
            (
                e.dm.is_a(t, "GuiButton"),
                e.dm.class_of(t) == Some("TextBox"),
            )
        };
        if is_box && button == 1 {
            focus_text_box(lua, t);
        }
        if is_button {
            engine(lua).lock().input.pointer.pressed.insert(button, t);
            fire_xy(
                lua,
                t,
                if button == 1 {
                    "MouseButton1Down"
                } else {
                    "MouseButton2Down"
                },
                local,
            );
        }
        return true;
    }
    let pressed = engine(lua).lock().input.pointer.pressed.remove(&button);
    let chain = {
        let eng = engine(lua);
        let e = eng.lock();
        input_chain(&e.dm, &under)
    };
    fire_input(lua, &chain, "InputEnded", obj);
    if button == 1 {
        if let Some(p) = pressed
            && Some(p) != target
        {
            set_gui_state(lua, p, 0);
        }
        if let Some(t) = target {
            set_gui_state(lua, t, 1);
        }
    }
    if let Some(t) = target {
        let is_button = engine(lua).lock().dm.is_a(t, "GuiButton");
        if is_button {
            fire_xy(
                lua,
                t,
                if button == 1 {
                    "MouseButton1Up"
                } else {
                    "MouseButton2Up"
                },
                local,
            );
            if pressed == Some(t) {
                if button == 1 {
                    let eng = engine(lua);
                    let mut e = eng.lock();
                    if let Some(verb) = e.gui.button_verbs.get(&t).cloned() {
                        e.gui.requested_verbs.push(verb);
                    }
                }
                fire_lua(
                    lua,
                    t,
                    if button == 1 {
                        "MouseButton1Click"
                    } else {
                        "MouseButton2Click"
                    },
                    MultiValue::new(),
                );
                if button == 1
                    && let Ok(v) = push_instance(lua, obj)
                {
                    fire_lua(
                        lua,
                        t,
                        "Activated",
                        MultiValue::from_vec(vec![v, Value::Integer(1)]),
                    );
                }
            }
        }
    }
    target.is_some() || pressed.is_some()
}

fn local_tool(dm: &DataModel) -> Option<InstanceId> {
    let player = dm.local_player?;
    let character = dm.get_ref(player, "Character")?;
    dm.children_of(character)
        .iter()
        .copied()
        .find(|&c| dm.class_of(c) == Some("Tool"))
}

fn arrow_tool_click(lua: &Lua) {
    let modifiers = {
        let eng = engine(lua);
        let e = eng.lock();
        let down = |codes: [u32; 2]| codes.iter().any(|c| e.input.keys_down.contains(c));
        (down([303, 304]), down([305, 306]), down([307, 308]))
    };
    let ray = {
        let eng = engine(lua);
        let e = eng.lock();
        mouse_ray(&e.dm, e.input.pointer.position)
    };
    let Some(ray) = ray else {
        return;
    };
    let hit = super::physics::with_world(lua, |w, dm| {
        w.raycast(ray.origin, ray.direction * 2048.0, |id| {
            !dm.get_bool(id, "Locked") && !dm.is_a(id, "Terrain")
        })
        .map(|h| h.part)
    });
    let (shift, ctrl, alt) = modifiers;
    let Some(part) = hit else {
        if !shift && !ctrl {
            super::selection::set(lua, Vec::new());
        }
        return;
    };
    let target = {
        let eng = engine(lua);
        let e = eng.lock();
        if alt {
            Some(part)
        } else {
            super::selection::top_selectable_3d(&e.dm, part)
        }
    };
    let Some(target) = target else {
        return;
    };
    let mut current = super::selection::current(lua);
    if shift || ctrl {
        if let Some(i) = current.iter().position(|&s| s == target) {
            current.remove(i);
        } else {
            current.push(target);
        }
        super::selection::set(lua, current);
    } else if !current.contains(&target) {
        super::selection::set(lua, vec![target]);
    }
}

fn workspace_input(lua: &Lua, ev: &InputEvent) {
    if ev.input_type != MOUSE_BUTTON1 || ev.kind == InputKind::Changed {
        return;
    }
    let (studio_arrow, host_selection) = {
        let eng = engine(lua);
        let e = eng.lock();
        (e.dm.local_player.is_none(), e.host_selection)
    };
    if studio_arrow {
        if host_selection {
            return;
        }
        if ev.kind == InputKind::Began {
            arrow_tool_click(lua);
        }
        return;
    }
    let eng = engine(lua);
    let mut e = eng.lock();
    let Some(tool) = local_tool(&e.dm) else {
        return;
    };
    if ev.kind == InputKind::Began {
        if !e.dm.get_bool(tool, "ManualActivationOnly")
            && e.dm
                .get(tool, "Enabled")
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(true)
        {
            e.dm.tool_activate(tool);
        }
    } else {
        e.dm.tool_deactivate(tool);
    }
}

fn mouse_events(lua: &Lua, ev: &InputEvent) {
    let mouse = {
        let eng = engine(lua);
        let e = eng.lock();
        e.dm.local_player
            .and_then(|p| e.mice.get(&p).copied())
            .filter(|m| e.dm.contains(*m))
    };
    let plugin_mouse = engine(lua).lock().plugins.active_mouse();
    for m in mouse.into_iter().chain(plugin_mouse) {
        fire_mouse(lua, m, ev);
    }
}

fn fire_mouse(lua: &Lua, mouse: InstanceId, ev: &InputEvent) {
    match (ev.input_type, ev.kind) {
        (MOUSE_MOVEMENT, _) => fire_lua(lua, mouse, "Move", MultiValue::new()),
        (MOUSE_BUTTON1, InputKind::Began) => fire_lua(lua, mouse, "Button1Down", MultiValue::new()),
        (MOUSE_BUTTON1, InputKind::Ended) => fire_lua(lua, mouse, "Button1Up", MultiValue::new()),
        (MOUSE_BUTTON2, InputKind::Began) => fire_lua(lua, mouse, "Button2Down", MultiValue::new()),
        (MOUSE_BUTTON2, InputKind::Ended) => fire_lua(lua, mouse, "Button2Up", MultiValue::new()),
        (MOUSE_WHEEL, _) => {
            let event = if ev.position.z > 0.0 {
                "WheelForward"
            } else {
                "WheelBackward"
            };
            fire_lua(lua, mouse, event, MultiValue::new());
        }
        (KEYBOARD, InputKind::Began | InputKind::Ended) => {
            let ch = char::from((ev.key_code & 0xff) as u8);
            let Ok(s) = lua.create_string(ch.to_string()) else {
                return;
            };
            if ev.kind == InputKind::Began {
                fire_lua(
                    lua,
                    mouse,
                    "KeyDown",
                    MultiValue::from_vec(vec![Value::String(s.clone())]),
                );
                fire_lua(
                    lua,
                    mouse,
                    "keyDown",
                    MultiValue::from_vec(vec![Value::String(s)]),
                );
            } else {
                fire_lua(
                    lua,
                    mouse,
                    "KeyUp",
                    MultiValue::from_vec(vec![Value::String(s)]),
                );
            }
        }
        _ => {}
    }
}

fn gui_service_keys(lua: &Lua, ev: &InputEvent) -> bool {
    if ev.input_type != KEYBOARD || ev.kind != InputKind::Began {
        return false;
    }
    let (gui, special, character) = {
        let eng = engine(lua);
        let e = eng.lock();
        let held = |codes: &[u32]| codes.iter().any(|c| e.input.keys_down.contains(c));
        if held(&[305, 306, 307, 308]) {
            return false;
        }
        let Some(gui) = e.dm.find_service("GuiService") else {
            return false;
        };
        let special = match ev.key_code {
            47 => Some(5),
            277 => Some(0),
            278 => Some(1),
            279 => Some(2),
            280 => Some(3),
            281 => Some(4),
            _ => None,
        }
        .filter(|k| e.gui.special_keys.contains(k));
        let character = if special.is_none() && ev.key_code != 47 {
            ev.text
                .as_ref()
                .and_then(|t| t.chars().next())
                .map(|c| c.to_ascii_lowercase().to_string())
                .filter(|c| e.gui.keys.contains(c))
        } else {
            None
        };
        (gui, special, character)
    };
    if let Some(k) = special {
        let Ok(v) = crate::convert::variant_to_lua(
            lua,
            &Variant::Enum(k),
            Some(&rbx_reflection::ValueType::Enum("SpecialKey".into())),
        ) else {
            return false;
        };
        let Ok(m) = lua.create_string("") else {
            return false;
        };
        fire_lua(
            lua,
            gui,
            "SpecialKeyPressed",
            MultiValue::from_vec(vec![v, Value::String(m)]),
        );
        return engine(lua).lock().input.focused_text_box.is_some();
    }
    if let Some(c) = character {
        let (Ok(k), Ok(m)) = (lua.create_string(c), lua.create_string("")) else {
            return false;
        };
        fire_lua(
            lua,
            gui,
            "KeyPressed",
            MultiValue::from_vec(vec![Value::String(k), Value::String(m)]),
        );
        return true;
    }
    false
}

fn update_state(lua: &Lua, ev: &InputEvent) {
    let changed = {
        let eng = engine(lua);
        let mut e = eng.lock();
        if ev.input_type == KEYBOARD {
            match ev.kind {
                InputKind::Began => {
                    e.input.keys_down.insert(ev.key_code);
                }
                InputKind::Ended => {
                    e.input.keys_down.remove(&ev.key_code);
                }
                InputKind::Changed => {}
            }
        }
        if is_mouse(ev.input_type) && ev.input_type != MOUSE_WHEEL {
            e.input.pointer.position = Vec2::new(ev.position.x, ev.position.y);
        }
        let last = if is_mouse(ev.input_type) {
            Some(ev.input_type)
        } else if ev.input_type == KEYBOARD {
            Some(KEYBOARD)
        } else {
            None
        };
        match last {
            Some(t) if e.input.last_input_type != Some(t) => {
                e.input.last_input_type = Some(t);
                e.dm.find_service("UserInputService").map(|u| (u, t))
            }
            _ => None,
        }
    };
    if let Some((uis, t)) = changed
        && let Ok(v) = crate::convert::variant_to_lua(
            lua,
            &Variant::Enum(t),
            Some(&rbx_reflection::ValueType::Enum("UserInputType".into())),
        )
    {
        fire_lua(
            lua,
            uis,
            "LastInputTypeChanged",
            MultiValue::from_vec(vec![v]),
        );
    }
}

fn touch_as_mouse(ev: &InputEvent) -> InputEvent {
    InputEvent {
        kind: ev.kind,
        input_type: if ev.kind == InputKind::Changed {
            MOUSE_MOVEMENT
        } else {
            MOUSE_BUTTON1
        },
        key_code: 0,
        position: ev.position,
        delta: ev.delta,
        text: None,
    }
}

pub fn process(lua: &Lua, ev: InputEvent) {
    if ev.input_type == TOUCH {
        process_touch(lua, ev);
        return;
    }
    update_state(lua, &ev);
    let Some(obj) = input_object(lua, &ev) else {
        return;
    };
    let menu_open = engine(lua).lock().gui.menu_open;
    let mut sunk = text_input(lua, &ev);
    if !sunk {
        sunk = gui_service_keys(lua, &ev);
    }
    if !sunk && is_mouse(ev.input_type) {
        sunk = gui_mouse(lua, &ev, obj);
    }
    let state = state_value(ev.kind);
    let action = if ev.input_type == KEYBOARD {
        ActionInput::KeyCode(ev.key_code)
    } else {
        ActionInput::InputType(ev.input_type)
    };
    if !sunk {
        sunk = dispatch(lua, &action, ev.input_type, state, obj, true, false);
    }
    if !sunk {
        sunk = dispatch(lua, &action, ev.input_type, state, obj, false, menu_open);
    }
    let uis = engine(lua).lock().dm.find_service("UserInputService");
    if let Some(uis) = uis
        && let Ok(v) = push_instance(lua, obj)
    {
        let event = match ev.kind {
            InputKind::Began => "InputBegan",
            InputKind::Changed => "InputChanged",
            InputKind::Ended => "InputEnded",
        };
        fire_lua(
            lua,
            uis,
            event,
            MultiValue::from_vec(vec![v, Value::Boolean(sunk)]),
        );
    }
    if !sunk && !menu_open {
        workspace_input(lua, &ev);
    }
    if !sunk || ev.input_type == MOUSE_MOVEMENT {
        mouse_events(lua, &ev);
    }
    pump(lua);
}

fn process_touch(lua: &Lua, ev: InputEvent) {
    let primary = {
        let eng = engine(lua);
        let mut e = eng.lock();
        let first = !e
            .input
            .pointer
            .objects
            .keys()
            .any(|(t, id)| *t == TOUCH && *id != ev.key_code);
        if first {
            e.input.pointer.position = Vec2::new(ev.position.x, ev.position.y);
        }
        if e.input.last_input_type != Some(TOUCH) {
            e.input.last_input_type = Some(TOUCH);
        }
        first
    };
    let Some(obj) = input_object(lua, &ev) else {
        return;
    };
    let menu_open = engine(lua).lock().gui.menu_open;
    let mouse = touch_as_mouse(&ev);
    let mut sunk = false;
    if primary {
        if ev.kind != InputKind::Changed {
            let moved = InputEvent {
                kind: InputKind::Changed,
                input_type: MOUSE_MOVEMENT,
                ..touch_as_mouse(&ev)
            };
            gui_mouse(lua, &moved, obj);
        }
        sunk = gui_mouse(lua, &mouse, obj);
    }
    let state = state_value(ev.kind);
    let action = ActionInput::InputType(TOUCH);
    if !sunk {
        sunk = dispatch(lua, &action, TOUCH, state, obj, true, false);
    }
    if !sunk {
        sunk = dispatch(lua, &action, TOUCH, state, obj, false, menu_open);
    }
    let uis = engine(lua).lock().dm.find_service("UserInputService");
    if let Some(uis) = uis {
        let (input_event, touch_event) = match ev.kind {
            InputKind::Began => ("InputBegan", "TouchStarted"),
            InputKind::Changed => ("InputChanged", "TouchMoved"),
            InputKind::Ended => ("InputEnded", "TouchEnded"),
        };
        for event in [input_event, touch_event] {
            if let Ok(v) = push_instance(lua, obj) {
                fire_lua(
                    lua,
                    uis,
                    event,
                    MultiValue::from_vec(vec![v, Value::Boolean(sunk)]),
                );
            }
        }
    }
    if primary {
        if !sunk && !menu_open {
            workspace_input(lua, &mouse);
        }
        if !sunk || mouse.input_type == MOUSE_MOVEMENT {
            mouse_events(lua, &mouse);
        }
    }
    pump(lua);
}

pub fn mouse_ray(dm: &DataModel, position: Vec2) -> Option<Ray> {
    let view = CameraView::current(dm)?;
    let r = view.ray(position.x, position.y, 0.0);
    Some(Ray::new(view.cframe.translation, r.direction))
}

fn mouse_hit(lua: &Lua, mouse: InstanceId) -> (Ray, Option<(InstanceId, Vec3, Vec3)>) {
    let (ray, filter, character) = {
        let eng = engine(lua);
        let e = eng.lock();
        let ray = mouse_ray(&e.dm, e.input.pointer.position)
            .unwrap_or_else(|| Ray::new(Vec3::ZERO, Vec3::NEG_Z));
        let filter = e.dm.get_ref(mouse, "TargetFilter");
        let character = e.dm.local_player.and_then(|p| e.dm.get_ref(p, "Character"));
        (ray, filter, character)
    };
    let dir = ray.direction * MOUSE_RAY_LENGTH;
    let hit = super::physics::with_world(lua, |w, dm| {
        let mut ignore = rustc_hash::FxHashSet::default();
        for root in [filter, character].into_iter().flatten() {
            if dm.contains(root) {
                ignore.insert(root);
                ignore.extend(dm.descendants(root));
            }
        }
        w.raycast(ray.origin, dir, |id| {
            !ignore.contains(&id)
                && !(dm.get_f32(id, "Transparency") >= 1.0 && !dm.get_bool(id, "CanCollide"))
        })
        .map(|h| (h.part, h.position, h.normal))
    });
    (ray, hit)
}

fn normal_id(cf: &CFrame, normal: Vec3) -> u32 {
    let local = cf.vector_to_object_space(normal);
    let a = local.abs();
    if a.x >= a.y && a.x >= a.z {
        if local.x > 0.0 { 0 } else { 3 }
    } else if a.y >= a.z {
        if local.y > 0.0 { 1 } else { 4 }
    } else if local.z > 0.0 {
        2
    } else {
        5
    }
}

pub fn get_mouse_property(lua: &Lua, id: InstanceId, name: &str) -> Result<Option<Value>> {
    match name {
        "X" | "Y" | "ViewSizeX" | "ViewSizeY" => {
            let eng = engine(lua);
            let e = eng.lock();
            let p = e.input.pointer.position;
            let inset = Vec2::new(e.dm.gui_view.inset[0], e.dm.gui_view.inset[1]);
            let screen = e.dm.gui_view.screen;
            let v = match name {
                "X" => p.x - inset.x,
                "Y" => p.y - inset.y,
                "ViewSizeX" => screen.x - inset.x,
                _ => screen.y - inset.y,
            };
            Ok(Some(Value::Integer(v as mlua::Integer)))
        }
        "UnitRay" | "Origin" => {
            let ray = {
                let eng = engine(lua);
                let e = eng.lock();
                mouse_ray(&e.dm, e.input.pointer.position)
                    .unwrap_or_else(|| Ray::new(Vec3::ZERO, Vec3::NEG_Z))
            };
            if name == "UnitRay" {
                Ok(Some(mlua::IntoLua::into_lua(LRay(ray), lua)?))
            } else {
                let cf = CFrame::look_at(ray.origin, ray.origin + ray.direction, Vec3::Y);
                Ok(Some(mlua::IntoLua::into_lua(LCFrame(cf), lua)?))
            }
        }
        "Hit" | "hit" => {
            let (ray, hit) = mouse_hit(lua, id);
            let p = hit
                .map(|h| h.1)
                .unwrap_or(ray.origin + ray.direction * MOUSE_RAY_LENGTH);
            let cf = CFrame::look_at(p, p + ray.direction, Vec3::Y);
            let cf = CFrame::new(cf.rotation, p);
            Ok(Some(mlua::IntoLua::into_lua(LCFrame(cf), lua)?))
        }
        "Target" | "target" => {
            let (_, hit) = mouse_hit(lua, id);
            match hit {
                Some((part, _, _)) => Ok(Some(push_instance(lua, part)?)),
                None => Ok(Some(Value::Nil)),
            }
        }
        "TargetSurface" => {
            let (_, hit) = mouse_hit(lua, id);
            let surface = hit
                .map(|(part, _, normal)| {
                    let eng = engine(lua);
                    let e = eng.lock();
                    let cf =
                        e.dm.get_raw(part, "CFrame")
                            .and_then(|v| v.as_cframe())
                            .unwrap_or_default();
                    normal_id(&cf, normal)
                })
                .unwrap_or(5);
            Ok(Some(crate::convert::variant_to_lua(
                lua,
                &Variant::Enum(surface),
                Some(&rbx_reflection::ValueType::Enum("NormalId".into())),
            )?))
        }
        _ => Ok(None),
    }
}

pub fn render_step(lua: &Lua, dt: f64) {
    super::camera::step(lua, dt as f32);
    {
        let eng = engine(lua);
        let mut e = eng.lock();
        e.input.pointer.last_frame_delta = e.input.pointer.frame_delta;
        e.input.pointer.frame_delta = Vec2::ZERO;
    }
    let bindings: Vec<(mlua::Function, crate::security::Identity)> = {
        let eng = engine(lua);
        let e = eng.lock();
        e.render_bindings
            .iter()
            .filter_map(|b| {
                lua.registry_value::<mlua::Function>(&b.2)
                    .ok()
                    .map(|f| (f, b.3))
            })
            .collect()
    };
    for (f, identity) in bindings {
        crate::runtime::spawn_function(
            lua,
            f,
            MultiValue::from_vec(vec![Value::Number(dt)]),
            crate::runtime::ThreadInfo {
                script: None,
                identity,
            },
        );
    }
    let rs = engine(lua).lock().dm.find_service("RunService");
    if let Some(rs) = rs {
        fire_lua(
            lua,
            rs,
            "RenderStepped",
            MultiValue::from_vec(vec![Value::Number(dt)]),
        );
    }
    pump(lua);
}

fn capture_focus(lua: &Lua, id: InstanceId, _args: &[Value]) -> Result<Ret> {
    focus_text_box(lua, id);
    none()
}

fn release_focus_native(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let enter = crate::convert::arg_bool(args.first());
    release_focus(lua, id, enter);
    none()
}

fn is_focused(lua: &Lua, id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let f = engine(lua).lock().input.focused_text_box == Some(id);
    ret(lua, f)
}

const VIRTUAL_WIDTH: f32 = 800.0;
const VIRTUAL_HEIGHT: f32 = 600.0;

fn virtual_mouse(lua: &Lua, args: &[Value], input_type: u32, kinds: &[InputKind]) -> Result<Ret> {
    let pos = match args.first() {
        Some(v) => <crate::types::LVector2 as mlua::FromLua>::from_lua(v.clone(), lua)?.0,
        None => Vec2::ZERO,
    };
    let camera = match args.get(1) {
        Some(v) => Some(<LCFrame as mlua::FromLua>::from_lua(v.clone(), lua)?.0),
        None => None,
    };
    let pos = pos.clamp(Vec2::splat(-1.0), Vec2::ONE);
    let x = (VIRTUAL_WIDTH / 2.0 * (1.0 + pos.x)).trunc();
    let y = (VIRTUAL_HEIGHT / 2.0 * (1.0 - pos.y)).trunc();
    {
        let eng = engine(lua);
        let mut e = eng.lock();
        e.dm.gui_view.screen = Vec2::new(VIRTUAL_WIDTH, VIRTUAL_HEIGHT);
        if let Some(cf) = camera.filter(|cf| *cf != CFrame::IDENTITY)
            && let Some(ws) = e.dm.find_service("Workspace")
            && let Some(cam) = e.dm.get_ref(ws, "CurrentCamera")
        {
            e.dm.store_silent(cam, "CFrame", Variant::CFrame(cf));
        }
    }
    for kind in kinds {
        process(
            lua,
            InputEvent {
                kind: *kind,
                input_type,
                key_code: 0,
                position: Vec3::new(x, y, 0.0),
                delta: Vec3::ZERO,
                text: None,
            },
        );
    }
    none()
}

fn virtual_key(lua: &Lua, args: &[Value], kinds: &[InputKind]) -> Result<Ret> {
    let key = crate::convert::arg_string(args.first()).unwrap_or_default();
    let code = key
        .chars()
        .next()
        .map(|c| c.to_ascii_lowercase() as u32)
        .unwrap_or(0);
    for kind in kinds {
        process(
            lua,
            InputEvent {
                kind: *kind,
                input_type: KEYBOARD,
                key_code: code,
                position: Vec3::ZERO,
                delta: Vec3::ZERO,
                text: Some(key.clone()),
            },
        );
    }
    none()
}

fn vu_button1_down(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    virtual_mouse(lua, args, MOUSE_BUTTON1, &[InputKind::Began])
}

fn vu_button1_up(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    virtual_mouse(lua, args, MOUSE_BUTTON1, &[InputKind::Ended])
}

fn vu_button2_down(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    virtual_mouse(lua, args, MOUSE_BUTTON2, &[InputKind::Began])
}

fn vu_button2_up(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    virtual_mouse(lua, args, MOUSE_BUTTON2, &[InputKind::Ended])
}

fn vu_click1(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    virtual_mouse(
        lua,
        args,
        MOUSE_BUTTON1,
        &[InputKind::Began, InputKind::Ended],
    )
}

fn vu_click2(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    virtual_mouse(
        lua,
        args,
        MOUSE_BUTTON2,
        &[InputKind::Began, InputKind::Ended],
    )
}

fn vu_move(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    virtual_mouse(lua, args, MOUSE_MOVEMENT, &[InputKind::Changed])
}

fn vu_key_down(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    virtual_key(lua, args, &[InputKind::Began])
}

fn vu_key_up(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    virtual_key(lua, args, &[InputKind::Ended])
}

fn vu_type_key(lua: &Lua, _id: InstanceId, args: &[Value]) -> Result<Ret> {
    virtual_key(lua, args, &[InputKind::Began, InputKind::Ended])
}

fn vu_noop(_lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    none()
}

fn vu_stop_recording(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, "")
}

pub fn register(r: &mut Registry) {
    r.add("VirtualUser", &["Button1Down"], vu_button1_down);
    r.add("VirtualUser", &["Button1Up"], vu_button1_up);
    r.add("VirtualUser", &["Button2Down"], vu_button2_down);
    r.add("VirtualUser", &["Button2Up"], vu_button2_up);
    r.add("VirtualUser", &["ClickButton1"], vu_click1);
    r.add("VirtualUser", &["ClickButton2"], vu_click2);
    r.add("VirtualUser", &["MoveMouse"], vu_move);
    r.add("VirtualUser", &["SetKeyDown"], vu_key_down);
    r.add("VirtualUser", &["SetKeyUp"], vu_key_up);
    r.add("VirtualUser", &["TypeKey"], vu_type_key);
    r.add(
        "VirtualUser",
        &["CaptureController", "StartRecording"],
        vu_noop,
    );
    r.add("VirtualUser", &["StopRecording"], vu_stop_recording);
    r.add("TextBox", &["CaptureFocus"], capture_focus);
    r.add("TextBox", &["ReleaseFocus"], release_focus_native);
    r.add("TextBox", &["IsFocused"], is_focused);
}
