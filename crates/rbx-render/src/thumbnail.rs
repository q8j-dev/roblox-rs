use crate::offscreen::Offscreen;
use rbx_dom::{DataModel, Variant};
use rbx_math::{CFrame, Vec2, Vec3};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{LazyLock, Mutex};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Avatar,
    AvatarBust,
    AvatarHeadShot,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub kind: Kind,
    pub id: i64,
    pub width: u32,
    pub height: u32,
}

pub fn parse(url: &str) -> Option<Request> {
    let rest = url.trim().strip_prefix("rbxthumb://")?;
    let mut kind = None;
    let mut id = None;
    let mut width = 150;
    let mut height = 150;
    for pair in rest.split('&') {
        let (k, v) = pair.split_once('=')?;
        match k.to_ascii_lowercase().as_str() {
            "type" => {
                kind = Some(match v {
                    "Avatar" => Kind::Avatar,
                    "AvatarBust" => Kind::AvatarBust,
                    "AvatarHeadShot" => Kind::AvatarHeadShot,
                    _ => return None,
                })
            }
            "id" => id = v.parse().ok(),
            "w" => width = v.parse().unwrap_or(150),
            "h" => height = v.parse().unwrap_or(150),
            _ => {}
        }
    }
    Some(Request {
        kind: kind?,
        id: id?,
        width,
        height,
    })
}

static CACHE: LazyLock<Mutex<HashMap<String, Option<Vec<u8>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn fetch(url: &str, content: &Path, platform: &Path) -> Option<Vec<u8>> {
    let request = parse(url)?;
    let key = format!(
        "{:?}/{}/{}x{}",
        request.kind, request.id, request.width, request.height
    );
    if let Some(found) = CACHE.lock().ok()?.get(&key) {
        return found.clone();
    }
    let png = render(&request, content, platform);
    if let Ok(mut cache) = CACHE.lock() {
        cache.insert(key, png.clone());
    }
    png
}

fn bounds(dm: &DataModel, model: rbx_dom::InstanceId) -> Option<(Vec3, Vec3)> {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    let mut any = false;
    for d in dm.descendants(model) {
        if !dm.is_a(d, "BasePart") {
            continue;
        }
        let (Some(cf), Some(size)) = (
            dm.get_raw(d, "CFrame").and_then(|v| v.as_cframe()),
            dm.get_raw(d, "size")
                .or_else(|| dm.get_raw(d, "Size"))
                .and_then(|v| match v {
                    Variant::Vector3(s) => Some(*s),
                    _ => None,
                }),
        ) else {
            continue;
        };
        let half = size * 0.5;
        for sx in [-1.0, 1.0] {
            for sy in [-1.0, 1.0] {
                for sz in [-1.0, 1.0] {
                    let p =
                        cf.point_to_world_space(Vec3::new(half.x * sx, half.y * sy, half.z * sz));
                    min = min.min(p);
                    max = max.max(p);
                    any = true;
                }
            }
        }
    }
    any.then_some((min, max))
}

fn render(request: &Request, content: &Path, platform: &Path) -> Option<Vec<u8>> {
    let size = (request.width.max(request.height) * 2).clamp(128, 840);
    let (width, height) = (size, size);
    let mut dm = DataModel::new();
    let ws = dm.get_service("Workspace").ok()?;
    let data = rbx_base::vfs::read(content.join("fonts").join("character3.rbxm")).ok()?;
    let (roots, _) = dm.load_model(&data).ok()?;
    let model = *roots.first()?;
    for d in dm.descendants(model) {
        if dm.is_a(d, "BasePart") {
            let _ = dm.set(d, "Anchored", Variant::Bool(true));
        }
    }
    dm.set_parent(model, Some(ws)).ok()?;
    let (min, max) = bounds(&dm, model)?;
    let head = dm
        .find_first_child(model, "Head", false)
        .and_then(|h| dm.get_raw(h, "CFrame").and_then(|v| v.as_cframe()))
        .map(|cf| cf.translation)
        .unwrap_or((min + max) * 0.5);
    let (target, extent) = match request.kind {
        Kind::Avatar => ((min + max) * 0.5, (max - min).y * 0.5 * 1.22),
        Kind::AvatarBust => (head - Vec3::new(0.0, 0.6, 0.0), 1.6),
        Kind::AvatarHeadShot => (head, 0.9),
    };
    let fov: f32 = 30.0;
    let distance = extent / (fov.to_radians() * 0.5).tan();
    let yaw = (-20.0f32).to_radians();
    let dir = Vec3::new(yaw.sin(), 0.12, -yaw.cos()).normalize();
    let eye = target + dir * distance;
    let cam = dm.create("Camera").ok()?;
    dm.set(
        cam,
        "CFrame",
        Variant::CFrame(CFrame::look_at(eye, target, Vec3::Y)),
    )
    .ok()?;
    let _ = dm.set(cam, "FieldOfView", Variant::Float(fov));
    let _ = dm.set(
        cam,
        "Focus",
        Variant::CFrame(CFrame::from_translation(target)),
    );
    dm.set_parent(cam, Some(ws)).ok()?;
    dm.set(ws, "CurrentCamera", Variant::Ref(Some(cam))).ok()?;
    dm.gui_view.screen = Vec2::new(width as f32, height as f32);
    let mut off = Offscreen::new(width, height, content, platform).ok()?;
    off.renderer.hide_sky = true;
    let pixels = off.render(&dm).ok()?;
    let mut out = Vec::new();
    let img = image::RgbaImage::from_raw(width, height, pixels)?;
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .ok()?;
    Some(out)
}
