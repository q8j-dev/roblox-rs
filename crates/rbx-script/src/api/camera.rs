use super::{Registry, Ret, none, ret};
use crate::convert::arg_number;
use crate::runtime::{engine, fire_lua};
use crate::types::{LCFrame, LRay, LVector3, rt};
use mlua::{FromLua, Lua, MultiValue, Result, Value};
use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{CFrame, Ray, Vec2, Vec3};

const FAR_PLANE: f32 = 5000.0;
const NEAR_PLANE: f32 = 0.5;

pub struct CameraInterpolation {
    pub camera: InstanceId,
    pub from: CFrame,
    pub to: CFrame,
    pub from_focus: Vec3,
    pub to_focus: Vec3,
    pub from_up: Vec3,
    pub elapsed: f32,
    pub duration: f32,
}

#[derive(Default)]
pub struct CameraState {
    pub interpolation: Option<CameraInterpolation>,
    pub roll: rustc_hash::FxHashMap<InstanceId, f32>,
}

#[derive(Clone, Copy)]
pub struct CameraView {
    pub cframe: CFrame,
    pub fov: f32,
    pub viewport: Vec2,
    pub inset: Vec2,
}

impl CameraView {
    pub fn read(dm: &DataModel, camera: InstanceId) -> CameraView {
        let cframe = dm
            .get_raw(camera, "CFrame")
            .and_then(|v| v.as_cframe())
            .unwrap_or_default();
        let fov = dm
            .get(camera, "FieldOfView")
            .ok()
            .and_then(|v| v.as_f32())
            .unwrap_or(70.0)
            .clamp(1.0, 120.0)
            .to_radians();
        CameraView {
            cframe,
            fov,
            viewport: dm.gui_view.screen,
            inset: Vec2::new(dm.gui_view.inset[0], dm.gui_view.inset[1]),
        }
    }

    pub fn current(dm: &DataModel) -> Option<CameraView> {
        let ws = dm.find_service("Workspace")?;
        let cam = dm
            .get_ref(ws, "CurrentCamera")
            .or_else(|| dm.find_first_child_of_class(ws, "Camera"))?;
        Some(CameraView::read(dm, cam))
    }

    pub fn project(&self, point: Vec3) -> (Vec3, bool) {
        let local = self.cframe.point_to_object_space(point);
        let w = -local.z;
        let (sw, sh) = (self.viewport.x.max(1.0), self.viewport.y.max(1.0));
        let h = 1.0 / (self.fov * 0.5).tan();
        let aspect = sw / sh;
        if w <= 0.0 {
            let x = if local.x * h / aspect < 0.0 {
                f32::NEG_INFINITY
            } else {
                f32::INFINITY
            };
            let y = if local.y > 0.0 {
                f32::NEG_INFINITY
            } else {
                f32::INFINITY
            };
            return (Vec3::new(x, y, w), false);
        }
        let qx = local.x * h / aspect / w;
        let qy = local.y * h / w;
        let out = Vec3::new(sw / 2.0 + sw / 2.0 * qx, sh / 2.0 - sh / 2.0 * qy, w);
        let on = out.x >= 0.0 && out.x <= sw && out.y >= 0.0 && out.y <= sh;
        (out, on)
    }

    pub fn ray(&self, x: f32, y: f32, depth: f32) -> Ray {
        let (sw, sh) = (self.viewport.x.max(1.0), self.viewport.y.max(1.0));
        let t = (self.fov * 0.5).tan();
        let aspect = sw / sh;
        let local = Vec3::new(
            ((x / (sw / 2.0)) - 1.0) * t * aspect,
            (1.0 - y / (sh / 2.0)) * t,
            -1.0,
        );
        let dir = self
            .cframe
            .vector_to_world_space(local)
            .normalize_or(Vec3::NEG_Z);
        let look = self.cframe.look_vector();
        let cos = dir.dot(look).clamp(1e-4, 1.0);
        let image_plane = 1.0 / (2.0 * t);
        let origin = self.cframe.translation + dir * (image_plane / cos) + dir * depth;
        Ray::new(origin, dir)
    }
}

fn camera_view(lua: &Lua, id: InstanceId) -> CameraView {
    CameraView::read(&engine(lua).lock().dm, id)
}

fn vec3_arg(lua: &Lua, v: Option<&Value>) -> Result<Vec3> {
    match v {
        Some(v) => Ok(LVector3::from_lua(v.clone(), lua)?.0),
        None => Err(rt("Argument 1 missing or nil")),
    }
}

fn screen_point(lua: &Lua, id: InstanceId, args: &[Value], inset: bool) -> Result<Ret> {
    let p = vec3_arg(lua, args.first())?;
    let view = camera_view(lua, id);
    let (mut v, on) = view.project(p);
    if inset {
        v.x -= view.inset.x;
        v.y -= view.inset.y;
    }
    ret(lua, (LVector3(v), on))
}

fn world_to_screen(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    screen_point(lua, id, args, true)
}

fn world_to_viewport(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    screen_point(lua, id, args, false)
}

fn point_ray(lua: &Lua, id: InstanceId, args: &[Value], inset: bool) -> Result<Ret> {
    let x = arg_number(args.first()).ok_or_else(|| rt("Argument 1 missing or nil"))? as f32;
    let y = arg_number(args.get(1)).ok_or_else(|| rt("Argument 2 missing or nil"))? as f32;
    let depth = arg_number(args.get(2)).unwrap_or(0.0) as f32;
    let view = camera_view(lua, id);
    let (x, y) = if inset {
        (x + view.inset.x, y + view.inset.y)
    } else {
        (x, y)
    };
    ret(lua, LRay(view.ray(x, y, depth)))
}

fn screen_point_to_ray(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    point_ray(lua, id, args, true)
}

fn viewport_point_to_ray(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    point_ray(lua, id, args, false)
}

fn get_render_cframe(lua: &Lua, id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, LCFrame(camera_view(lua, id).cframe))
}

fn get_roll(lua: &Lua, id: InstanceId, _args: &[Value]) -> Result<Ret> {
    let r = engine(lua)
        .lock()
        .camera
        .roll
        .get(&id)
        .copied()
        .unwrap_or(0.0);
    ret(lua, r as f64)
}

fn set_roll(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let value = arg_number(args.first()).unwrap_or(0.0) as f32;
    let eng = engine(lua);
    let mut e = eng.lock();
    let scriptable = e.dm.get(id, "CameraType").ok().and_then(|v| v.as_enum()) == Some(6);
    if scriptable {
        e.camera.roll.insert(id, value);
    }
    none()
}

fn cframe_arg(lua: &Lua, v: Option<&Value>, n: usize) -> Result<CFrame> {
    match v {
        Some(v) => Ok(LCFrame::from_lua(v.clone(), lua)?.0),
        None => Err(rt(format!("Argument {n} missing or nil"))),
    }
}

fn interpolate(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let to = cframe_arg(lua, args.first(), 1)?;
    let focus = cframe_arg(lua, args.get(1), 2)?;
    let duration = arg_number(args.get(2)).ok_or_else(|| rt("Argument 3 missing or nil"))? as f32;
    if duration < 0.0 {
        return Err(rt("Interpolation time must be positive or 0."));
    }
    let eng = engine(lua);
    let mut e = eng.lock();
    if e.dm.get(id, "CameraType").ok().and_then(|v| v.as_enum()) != Some(6) {
        return Err(rt(
            "Attempted to use interpolation with a camera mode other than scriptable.",
        ));
    }
    let from =
        e.dm.get_raw(id, "CFrame")
            .and_then(|v| v.as_cframe())
            .unwrap_or_default();
    let from_focus =
        e.dm.get_raw(id, "Focus")
            .and_then(|v| v.as_cframe())
            .map(|c| c.translation)
            .unwrap_or(from.translation + from.look_vector() * 20.0);
    if duration == 0.0 {
        let _ = e.dm.set(id, "CFrame", Variant::CFrame(to));
        let _ = e.dm.set(id, "Focus", Variant::CFrame(focus));
        e.camera.interpolation = None;
        return none();
    }
    e.camera.interpolation = Some(CameraInterpolation {
        camera: id,
        from,
        to,
        from_focus,
        to_focus: focus.translation,
        from_up: from.up_vector(),
        elapsed: 0.0,
        duration,
    });
    none()
}

pub fn step(lua: &Lua, dt: f32) {
    let finished = {
        let eng = engine(lua);
        let mut e = eng.lock();
        let Some(mut interp) = e.camera.interpolation.take() else {
            return;
        };
        if !e.dm.contains(interp.camera)
            || e.dm
                .get(interp.camera, "CameraType")
                .ok()
                .and_then(|v| v.as_enum())
                != Some(6)
        {
            return;
        }
        interp.elapsed += dt;
        let t = (interp.elapsed / interp.duration).min(1.0);
        let pos = interp.from.translation.lerp(interp.to.translation, t);
        let focus = interp.from_focus.lerp(interp.to_focus, t);
        let up = interp
            .from_up
            .lerp(interp.to.up_vector(), t)
            .normalize_or(Vec3::Y);
        let cf = if (focus - pos).length() > 1e-5 {
            CFrame::look_at(pos, focus, up)
        } else {
            CFrame::new(interp.to.rotation, pos)
        };
        let look = cf.look_vector();
        let _ = e.dm.set(interp.camera, "CFrame", Variant::CFrame(cf));
        let _ = e.dm.set(
            interp.camera,
            "Focus",
            Variant::CFrame(CFrame::from_translation(pos + look * 20.0)),
        );
        let camera = interp.camera;
        if t >= 1.0 {
            Some(camera)
        } else {
            e.camera.interpolation = Some(interp);
            None
        }
    };
    if let Some(cam) = finished {
        fire_lua(lua, cam, "InterpolationFinished", MultiValue::new());
    }
}

fn pan_units(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let units = arg_number(args.first()).unwrap_or(0.0) as i32;
    let eng = engine(lua);
    let mut e = eng.lock();
    let cf =
        e.dm.get_raw(id, "CFrame")
            .and_then(|v| v.as_cframe())
            .unwrap_or_default();
    let focus =
        e.dm.get_raw(id, "Focus")
            .and_then(|v| v.as_cframe())
            .map(|c| c.translation)
            .unwrap_or(cf.translation + cf.look_vector() * 20.0);
    let look = cf.look_vector();
    let angle = (-look.z).atan2(-look.x);
    let q = std::f32::consts::PI / 4.0;
    let new_angle = ((angle * 4.0 / std::f32::consts::PI).round() + units as f32) * q;
    let rot = rbx_math::Mat3::from_rotation_y(angle - new_angle);
    let offset = rot * (cf.translation - focus);
    let pos = focus + offset;
    let _ = e.dm.set(
        id,
        "CFrame",
        Variant::CFrame(CFrame::look_at(pos, focus, Vec3::Y)),
    );
    none()
}

fn tilt_units(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let up = arg_number(args.first()).unwrap_or(0.0) as i32;
    let eng = engine(lua);
    let mut e = eng.lock();
    let cf =
        e.dm.get_raw(id, "CFrame")
            .and_then(|v| v.as_cframe())
            .unwrap_or_default();
    let focus =
        e.dm.get_raw(id, "Focus")
            .and_then(|v| v.as_cframe())
            .map(|c| c.translation)
            .unwrap_or(cf.translation + cf.look_vector() * 20.0);
    let look = cf.look_vector();
    let angle = look.y.clamp(-1.0, 1.0).asin();
    let slot = (angle.to_degrees() * 0.1).round() as i32;
    let target = (10.0 * (slot + up) as f32).to_radians();
    let new_angle = target.clamp(-1.4, 1.4);
    if (new_angle - angle).abs() < 1e-5 {
        drop(e);
        return ret(lua, false);
    }
    let dist = (cf.translation - focus).length();
    let flat = Vec3::new(look.x, 0.0, look.z).normalize_or(Vec3::NEG_Z);
    let dir = flat * new_angle.cos() + Vec3::Y * new_angle.sin();
    let pos = focus - dir * dist;
    let _ = e.dm.set(
        id,
        "CFrame",
        Variant::CFrame(CFrame::look_at(pos, focus, Vec3::Y)),
    );
    drop(e);
    ret(lua, true)
}

fn zoom(lua: &Lua, id: InstanceId, args: &[Value]) -> Result<Ret> {
    let distance = arg_number(args.first()).unwrap_or(0.0) as f32;
    let eng = engine(lua);
    let mut e = eng.lock();
    let cf =
        e.dm.get_raw(id, "CFrame")
            .and_then(|v| v.as_cframe())
            .unwrap_or_default();
    let focus =
        e.dm.get_raw(id, "Focus")
            .and_then(|v| v.as_cframe())
            .map(|c| c.translation)
            .unwrap_or(cf.translation + cf.look_vector() * 20.0);
    let current = (cf.translation - focus).length();
    let wanted = (current - distance).clamp(NEAR_PLANE, FAR_PLANE * 0.2);
    let pos = focus - cf.look_vector() * wanted;
    let _ =
        e.dm.set(id, "CFrame", Variant::CFrame(CFrame::new(cf.rotation, pos)));
    drop(e);
    ret(lua, true)
}

fn speed(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, 0.0)
}

fn set_pan_mode(_lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    none()
}

pub fn register(r: &mut Registry) {
    r.add("Camera", &["WorldToScreenPoint"], world_to_screen);
    r.add("Camera", &["WorldToViewportPoint"], world_to_viewport);
    r.add("Camera", &["ScreenPointToRay"], screen_point_to_ray);
    r.add("Camera", &["ViewportPointToRay"], viewport_point_to_ray);
    r.add("Camera", &["GetRenderCFrame"], get_render_cframe);
    r.add("Camera", &["GetRoll"], get_roll);
    r.add("Camera", &["SetRoll"], set_roll);
    r.add("Camera", &["Interpolate"], interpolate);
    r.add("Camera", &["PanUnits"], pan_units);
    r.add("Camera", &["TiltUnits"], tilt_units);
    r.add("Camera", &["Zoom"], zoom);
    r.add("Camera", &["GetPanSpeed", "GetTiltSpeed"], speed);
    r.add("Camera", &["SetCameraPanMode"], set_pan_mode);
}
