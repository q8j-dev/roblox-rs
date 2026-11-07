use crate::gui::{GuiItem, collect_root, get_bool, get_i64, get_vec2};
use rbx_dom::gui::Placement;
use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{CFrame, Mat3, UDim2, Vec2, Vec3};

const SURFACE_LIFT: f32 = 0.01;

#[derive(Clone, Debug)]
pub struct WorldGui {
    pub items: Vec<GuiItem>,
    pub origin: Vec3,
    pub x_axis: Vec3,
    pub y_axis: Vec3,
    pub distance: f32,
    pub raster_scale: f32,
}

#[derive(Clone, Debug, Default)]
pub struct WorldGuis {
    pub on_top: Vec<GuiItem>,
    pub world: Vec<WorldGui>,
}

#[derive(Clone, Copy, Debug)]
pub struct GuiCamera {
    pub cframe: CFrame,
    pub fov_y: f32,
    pub screen: Vec2,
}

impl GuiCamera {
    pub fn from_dm(dm: &DataModel) -> Option<GuiCamera> {
        let ws = dm.find_service("Workspace")?;
        let cam = dm
            .get_ref(ws, "CurrentCamera")
            .or_else(|| dm.find_first_child_of_class(ws, "Camera"))?;
        let cframe = dm.get_raw(cam, "CFrame").and_then(|v| v.as_cframe())?;
        let fov = match dm.get(cam, "FieldOfView") {
            Ok(Variant::Float(v)) => v,
            Ok(Variant::Double(v)) => v as f32,
            _ => 70.0,
        };
        Some(GuiCamera {
            cframe,
            fov_y: fov.clamp(1.0, 120.0).to_radians(),
            screen: dm.gui_view.screen,
        })
    }

    pub fn project(&self, p: Vec3) -> Option<(Vec2, f32)> {
        let local = self.cframe.point_to_object_space(p);
        let d = -local.z;
        if d <= 0.0 {
            return None;
        }
        let (w, h) = (self.screen.x.max(1.0), self.screen.y.max(1.0));
        let f = 1.0 / (self.fov_y * 0.5).tan();
        let x = w * 0.5 + w * 0.5 * (f * h / w * local.x / d);
        let y = h * 0.5 - h * 0.5 * (f * local.y / d);
        let z = h * f * 0.5 * (5000.0 - d) / (4999.5 * d);
        Some((Vec2::new(x, y), z))
    }
}

pub(crate) fn vec3(dm: &DataModel, id: InstanceId, name: &str) -> Vec3 {
    match dm.get(id, name) {
        Ok(Variant::Vector3(v)) => v,
        _ => Vec3::ZERO,
    }
}

fn udim2(dm: &DataModel, id: InstanceId, name: &str) -> UDim2 {
    match dm.get(id, name) {
        Ok(Variant::UDim2(u)) => u,
        _ => UDim2::default(),
    }
}

fn part_box(dm: &DataModel, id: InstanceId) -> Option<(CFrame, Vec3)> {
    if dm.is_a(id, "BasePart") {
        let cf = dm.get_raw(id, "CFrame").and_then(|v| v.as_cframe())?;
        return Some((cf, vec3(dm, id, "Size")));
    }
    if !dm.is_a(id, "Model") {
        return None;
    }
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    for d in dm.descendants(id) {
        if !dm.is_a(d, "BasePart") {
            continue;
        }
        let Some(cf) = dm.get_raw(d, "CFrame").and_then(|v| v.as_cframe()) else {
            continue;
        };
        let half = vec3(dm, d, "Size") * 0.5;
        for a in 0..8 {
            let corner = Vec3::new(
                if a & 4 != 0 { half.x } else { -half.x },
                if a & 2 != 0 { half.y } else { -half.y },
                if a & 1 != 0 { half.z } else { -half.z },
            );
            let w = cf.point_to_world_space(corner);
            lo = lo.min(w);
            hi = hi.max(w);
        }
    }
    if !lo.is_finite() {
        return None;
    }
    Some((CFrame::from_translation((lo + hi) * 0.5), hi - lo))
}

fn adornee(dm: &DataModel, gui: InstanceId) -> Option<InstanceId> {
    dm.get_ref(gui, "Adornee").or_else(|| dm.parent_of(gui))
}

fn in_world(dm: &DataModel, id: InstanceId, ws: InstanceId) -> bool {
    dm.is_descendant_of(id, ws)
}

fn gather(
    dm: &DataModel,
    id: InstanceId,
    out: &mut Vec<InstanceId>,
    mut humanoids: Option<&mut Vec<InstanceId>>,
) {
    for &c in dm.children_of(id) {
        match dm.class_of(c) {
            Some("BillboardGui" | "SurfaceGui") => out.push(c),
            Some("ScreenGui" | "GuiMain") => {}
            Some("Humanoid") => {
                if let Some(h) = humanoids.as_deref_mut() {
                    h.push(c);
                }
            }
            _ => gather(dm, c, out, humanoids.as_deref_mut()),
        }
    }
}

fn roots(dm: &DataModel, humanoids: &mut Vec<InstanceId>) -> Vec<InstanceId> {
    let mut out = Vec::new();
    if let Some(ws) = dm.find_service("Workspace") {
        gather(dm, ws, &mut out, Some(humanoids));
    }
    if let Some(pg) = dm
        .local_player
        .and_then(|p| dm.find_first_child_of_class(p, "PlayerGui"))
    {
        gather(dm, pg, &mut out, None);
    }
    if let Some(core) = dm.find_service("CoreGui") {
        gather(dm, core, &mut out, None);
    }
    out
}

fn quantize(scale: f32) -> f32 {
    let s = scale.clamp(0.125, 8.0);
    2f32.powf((s.log2() * 4.0).round() / 4.0)
}

fn billboard(
    dm: &DataModel,
    cam: &GuiCamera,
    gui: InstanceId,
    part: InstanceId,
    out: &mut WorldGuis,
) {
    if let (Some(local), Some(hide)) = (dm.local_player, dm.get_ref(gui, "PlayerToHideFrom"))
        && local == hide
    {
        return;
    }
    let Some((cf, size)) = part_box(dm, part) else {
        return;
    };
    let half = size * 0.5;
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    for a in 0..8 {
        let corner = Vec3::new(
            if a & 4 != 0 { half.x } else { -half.x },
            if a & 2 != 0 { half.y } else { -half.y },
            if a & 1 != 0 { half.z } else { -half.z },
        );
        let p = cam
            .cframe
            .point_to_object_space(cf.point_to_world_space(corner));
        lo = lo.min(p);
        hi = hi.max(p);
    }
    let extents = vec3(dm, gui, "ExtentsOffset");
    let relative = (extents + Vec3::ONE) * 0.5 * (hi - lo) + lo;
    let mut center = relative + vec3(dm, gui, "StudsOffset");
    let Some((_, pps)) = cam.project(cam.cframe.point_to_world_space(center)) else {
        return;
    };
    if pps <= 0.0 || pps >= 1000.0 {
        return;
    }
    let s = udim2(dm, gui, "Size");
    let viewport = Vec2::new(
        s.x.scale * pps + s.x.offset as f32,
        s.y.scale * pps + s.y.offset as f32,
    );
    if viewport.x <= 0.0 || viewport.y <= 0.0 {
        return;
    }
    let studs = viewport / pps;
    let size_offset = get_vec2(dm, gui, "SizeOffset");
    center += Vec3::new(size_offset.x * studs.x, size_offset.y * studs.y, 0.0);
    let top_left = center - Vec3::new(studs.x * 0.5, -studs.y * 0.5, 0.0);
    let origin = cam.cframe.point_to_world_space(top_left);
    let distance = (origin - cam.cframe.translation).length();
    let clip = get_bool(dm, gui, "ClipsDescendants", false);
    if get_bool(dm, gui, "AlwaysOnTop", false) {
        let Some((screen, _)) = cam.project(origin) else {
            return;
        };
        let position = Vec2::new(screen.x.round(), screen.y.round());
        let base = Placement {
            position,
            size: viewport,
            rotation: 0.0,
        };
        let clip_rect =
            clip.then(|| crate::gui::Rect::xywh(position.x, position.y, viewport.x, viewport.y));
        let mut items = Vec::new();
        collect_root(dm, gui, base, clip_rect, &mut items);
        out.on_top.extend(items);
        return;
    }
    let base = Placement {
        position: Vec2::ZERO,
        size: viewport,
        rotation: 0.0,
    };
    let clip_rect = clip.then(|| crate::gui::Rect::xywh(0.0, 0.0, viewport.x, viewport.y));
    let mut items = Vec::new();
    collect_root(dm, gui, base, clip_rect, &mut items);
    if items.is_empty() {
        return;
    }
    let rot = cam.cframe.rotation;
    out.world.push(WorldGui {
        items,
        origin,
        x_axis: rot.x_axis / pps,
        y_axis: -rot.y_axis / pps,
        distance,
        raster_scale: 1.0,
    });
}

fn face_rotation(face: i64) -> Mat3 {
    const YAW: [f32; 6] = [90.0, 90.0, 0.0, -90.0, 90.0, 180.0];
    const PITCH: [f32; 6] = [0.0, 90.0, 0.0, 0.0, -90.0, 0.0];
    let i = face.clamp(0, 5) as usize;
    Mat3::from_rotation_z(PITCH[i].to_radians()) * Mat3::from_rotation_y(YAW[i].to_radians())
}

fn surface(
    dm: &DataModel,
    cam: &GuiCamera,
    gui: InstanceId,
    part: InstanceId,
    out: &mut WorldGuis,
) {
    if !dm.is_a(part, "BasePart") {
        return;
    }
    let Some((cf, size)) = part_box(dm, part) else {
        return;
    };
    let canvas = get_vec2(dm, gui, "CanvasSize");
    if canvas.x <= 0.0 || canvas.y <= 0.0 {
        return;
    }
    let rot = face_rotation(get_i64(dm, gui, "Face"));
    let local = |v: Vec3| (rot * v) * size;
    let normal = cf.vector_to_world_space(rot * Vec3::Z);
    let face_center = cf.point_to_world_space(local(Vec3::new(0.0, 0.0, 0.5)));
    if (cam.cframe.translation - face_center).dot(normal) <= 0.0 {
        return;
    }
    let origin = cf.point_to_world_space(local(Vec3::new(-0.5, 0.5, 0.5))) + normal * SURFACE_LIFT;
    let x_axis = cf.vector_to_world_space(local(Vec3::new(1.0 / canvas.x, 0.0, 0.0)));
    let y_axis = cf.vector_to_world_space(local(Vec3::new(0.0, -1.0 / canvas.y, 0.0)));
    let Some((_, pps)) = cam.project(face_center) else {
        return;
    };
    let base = Placement {
        position: Vec2::ZERO,
        size: canvas,
        rotation: 0.0,
    };
    let clip = crate::gui::Rect::xywh(0.0, 0.0, canvas.x, canvas.y);
    let mut items = Vec::new();
    collect_root(dm, gui, base, Some(clip), &mut items);
    if items.is_empty() {
        return;
    }
    out.world.push(WorldGui {
        items,
        origin,
        x_axis,
        y_axis,
        distance: (face_center - cam.cframe.translation).length(),
        raster_scale: quantize(pps * x_axis.length().max(y_axis.length())),
    });
}

pub fn extract(dm: &DataModel) -> WorldGuis {
    let mut out = WorldGuis::default();
    let Some(cam) = GuiCamera::from_dm(dm) else {
        return out;
    };
    let Some(ws) = dm.find_service("Workspace") else {
        return out;
    };
    let mut on_top: Vec<(f32, Vec<GuiItem>)> = Vec::new();
    let mut humanoids = Vec::new();
    for gui in roots(dm, &mut humanoids) {
        if !get_bool(dm, gui, "Enabled", true) {
            continue;
        }
        let Some(part) = adornee(dm, gui) else {
            continue;
        };
        if !in_world(dm, part, ws) {
            continue;
        }
        match dm.class_of(gui) {
            Some("BillboardGui") => {
                let mut one = WorldGuis::default();
                billboard(dm, &cam, gui, part, &mut one);
                if !one.on_top.is_empty() {
                    let d = part_box(dm, part)
                        .map(|(cf, _)| (cf.translation - cam.cframe.translation).length())
                        .unwrap_or(0.0);
                    on_top.push((d, one.on_top));
                }
                out.world.extend(one.world);
            }
            Some("SurfaceGui") => surface(dm, &cam, gui, part, &mut out),
            _ => {}
        }
    }
    crate::name_tags::extract(dm, &cam, &humanoids, &mut on_top);
    on_top.sort_by(|a, b| b.0.total_cmp(&a.0));
    out.on_top = on_top.into_iter().flat_map(|(_, items)| items).collect();
    out.world.sort_by(|a, b| b.distance.total_cmp(&a.distance));
    out
}
