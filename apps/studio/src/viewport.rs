use egui::{Color32, Pos2, Rect, Stroke};
use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{CFrame, Mat3, Vec3};
use rbx_studio_ui::Document;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Move,
    Scale,
    Rotate,
}

pub struct FlyCamera {
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub fov_y: f32,
}

impl FlyCamera {
    pub fn looking_at(eye: Vec3, target: Vec3) -> FlyCamera {
        let d = (target - eye).normalize_or(Vec3::NEG_Z);
        FlyCamera {
            position: eye,
            yaw: (-d.x).atan2(-d.z),
            pitch: d.y.clamp(-1.0, 1.0).asin(),
            fov_y: 70.0,
        }
    }

    pub fn from_cframe(cf: CFrame, fov_y: f32) -> FlyCamera {
        let mut c = FlyCamera::looking_at(cf.translation, cf.translation + cf.look_vector());
        c.fov_y = fov_y;
        c
    }

    pub fn rotation(&self) -> Mat3 {
        Mat3::from_rotation_y(self.yaw) * Mat3::from_rotation_x(self.pitch)
    }

    pub fn cframe(&self) -> CFrame {
        CFrame::new(self.rotation(), self.position)
    }

    pub fn ray(&self, rect: Rect, pos: Pos2) -> (Vec3, Vec3) {
        let x = (pos.x - rect.left()) / rect.width().max(1.0) * 2.0 - 1.0;
        let y = 1.0 - (pos.y - rect.top()) / rect.height().max(1.0) * 2.0;
        let t = (self.fov_y.to_radians() * 0.5).tan();
        let aspect = rect.width().max(1.0) / rect.height().max(1.0);
        let dir = self.rotation() * Vec3::new(x * t * aspect, y * t, -1.0);
        (self.position, dir.normalize_or(Vec3::NEG_Z))
    }

    pub fn project(&self, rect: Rect, p: Vec3) -> Option<Pos2> {
        let local = self.rotation().transpose() * (p - self.position);
        if local.z > -0.05 {
            return None;
        }
        let t = (self.fov_y.to_radians() * 0.5).tan();
        let aspect = rect.width().max(1.0) / rect.height().max(1.0);
        let x = local.x / (-local.z * t * aspect);
        let y = local.y / (-local.z * t);
        Some(Pos2::new(
            rect.left() + (x + 1.0) * 0.5 * rect.width(),
            rect.top() + (1.0 - y) * 0.5 * rect.height(),
        ))
    }

    pub fn fly(&mut self, forward: f32, right: f32, up: f32) {
        let rot = self.rotation();
        self.position += rot * Vec3::new(right, 0.0, -forward) + Vec3::Y * up;
    }

    pub fn turn(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * 0.005;
        self.pitch = (self.pitch - dy * 0.005).clamp(-1.55, 1.55);
    }

    pub fn apply(&self, dm: &mut DataModel) {
        let Some(ws) = dm.find_service("Workspace") else {
            return;
        };
        let cam = match dm
            .get_ref(ws, "CurrentCamera")
            .or_else(|| dm.find_first_child_of_class(ws, "Camera"))
        {
            Some(c) => c,
            None => {
                let Ok(c) = dm.create("Camera") else {
                    return;
                };
                if dm.set_parent(c, Some(ws)).is_err() {
                    return;
                }
                let _ = dm.set(ws, "CurrentCamera", Variant::Ref(Some(c)));
                c
            }
        };
        dm.store_silent(cam, "CFrame", Variant::CFrame(self.cframe()));
        dm.store_silent(
            cam,
            "Focus",
            Variant::CFrame(CFrame::from_translation(
                self.position + self.cframe().look_vector() * 20.0,
            )),
        );
    }
}

pub struct Hit {
    pub part: InstanceId,
    pub distance: f32,
    pub point: Vec3,
    pub local_normal: Vec3,
}

fn ray_box(origin: Vec3, dir: Vec3, cf: CFrame, size: Vec3) -> Option<(f32, Vec3)> {
    let o = cf.point_to_object_space(origin);
    let d = cf.vector_to_object_space(dir);
    let half = size * 0.5;
    let mut t_min = f32::NEG_INFINITY;
    let mut t_max = f32::INFINITY;
    let mut normal = Vec3::ZERO;
    for axis in 0..3 {
        let (oa, da, ha) = (o[axis], d[axis], half[axis]);
        if da.abs() < 1e-8 {
            if oa.abs() > ha {
                return None;
            }
            continue;
        }
        let mut t1 = (-ha - oa) / da;
        let mut t2 = (ha - oa) / da;
        let mut sign = -1.0;
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
            sign = 1.0;
        }
        if t1 > t_min {
            t_min = t1;
            normal = Vec3::ZERO;
            normal[axis] = sign;
        }
        t_max = t_max.min(t2);
        if t_min > t_max {
            return None;
        }
    }
    if t_max < 0.0 {
        return None;
    }
    Some((t_min.max(0.0), normal))
}

pub fn part_box(dm: &DataModel, id: InstanceId) -> Option<(CFrame, Vec3)> {
    let cf = dm.get(id, "CFrame").ok()?.as_cframe()?;
    let size = dm.get(id, "Size").ok()?.as_vec3()?;
    Some((cf, size))
}

pub fn pick(dm: &DataModel, origin: Vec3, dir: Vec3, include_locked: bool) -> Option<Hit> {
    let ws = dm.find_service("Workspace")?;
    let mut best: Option<Hit> = None;
    for id in dm.descendants(ws) {
        if !dm.is_a(id, "BasePart") || dm.class_of(id) == Some("Terrain") {
            continue;
        }
        if !include_locked && dm.get_bool(id, "Locked") {
            continue;
        }
        let Some((cf, size)) = part_box(dm, id) else {
            continue;
        };
        if let Some((t, n)) = ray_box(origin, dir, cf, size)
            && best.as_ref().is_none_or(|b| t < b.distance)
        {
            best = Some(Hit {
                part: id,
                distance: t,
                point: origin + dir * t,
                local_normal: n,
            });
        }
    }
    best
}

pub fn selectable(dm: &DataModel, part: InstanceId) -> InstanceId {
    let ws = dm.find_service("Workspace");
    let mut top = part;
    let mut cur = dm.parent_of(part);
    while let Some(p) = cur {
        if Some(p) == ws {
            break;
        }
        if dm.is_a(p, "Model") {
            top = p;
        }
        cur = dm.parent_of(p);
    }
    top
}

pub fn draw_selection(painter: &egui::Painter, rect: Rect, cam: &FlyCamera, doc: &Document) {
    let selection = doc.selection();
    let e = doc.runtime.engine.lock();
    let stroke = Stroke::new(2.0, Color32::from_rgb(25, 153, 255));
    for id in selection {
        let mut parts = Vec::new();
        if e.dm.is_a(id, "BasePart") {
            parts.push(id);
        }
        for d in e.dm.descendants(id) {
            if e.dm.is_a(d, "BasePart") {
                parts.push(d);
            }
        }
        for p in parts {
            let Some((cf, size)) = part_box(&e.dm, p) else {
                continue;
            };
            let h = size * 0.5;
            let corners: Vec<Vec3> = (0..8)
                .map(|i| {
                    let s = Vec3::new(
                        if i & 1 == 0 { -1.0 } else { 1.0 },
                        if i & 2 == 0 { -1.0 } else { 1.0 },
                        if i & 4 == 0 { -1.0 } else { 1.0 },
                    );
                    cf.point_to_world_space(s * h)
                })
                .collect();
            for (a, b) in [
                (0, 1),
                (2, 3),
                (4, 5),
                (6, 7),
                (0, 2),
                (1, 3),
                (4, 6),
                (5, 7),
                (0, 4),
                (1, 5),
                (2, 6),
                (3, 7),
            ] {
                if let (Some(pa), Some(pb)) =
                    (cam.project(rect, corners[a]), cam.project(rect, corners[b]))
                {
                    painter.line_segment([pa, pb], stroke);
                }
            }
        }
    }
}

pub enum Drag {
    Move {
        plane_y: f32,
        start: Vec3,
        applied: Vec3,
    },
    Scale {
        part: InstanceId,
        axis_world: Vec3,
        local_normal: Vec3,
        anchor: Vec3,
        applied: f32,
    },
    Rotate {
        start_x: f32,
        applied: f32,
    },
}

fn plane_hit(origin: Vec3, dir: Vec3, y: f32) -> Option<Vec3> {
    if dir.y.abs() < 1e-5 {
        return None;
    }
    let t = (y - origin.y) / dir.y;
    (t > 0.0).then(|| origin + dir * t)
}

fn closest_on_axis(origin: Vec3, dir: Vec3, anchor: Vec3, axis: Vec3) -> f32 {
    let w = anchor - origin;
    let b = axis.dot(dir);
    let d = axis.dot(w);
    let e = dir.dot(w);
    let denom = 1.0 - b * b;
    if denom.abs() < 1e-5 {
        return 0.0;
    }
    (b * e - d) / denom
}

pub fn snap(v: f32, step: f32) -> f32 {
    (v / step).round() * step
}

pub fn begin_drag(tool: Tool, doc: &mut Document, hit: &Hit, mouse_x: f32) -> Option<Drag> {
    match tool {
        Tool::Select => None,
        Tool::Move => {
            doc.checkpoint();
            Some(Drag::Move {
                plane_y: hit.point.y,
                start: hit.point,
                applied: Vec3::ZERO,
            })
        }
        Tool::Scale => {
            let cf = {
                let e = doc.runtime.engine.lock();
                part_box(&e.dm, hit.part)?.0
            };
            doc.checkpoint();
            Some(Drag::Scale {
                part: hit.part,
                axis_world: cf.vector_to_world_space(hit.local_normal),
                local_normal: hit.local_normal,
                anchor: hit.point,
                applied: 0.0,
            })
        }
        Tool::Rotate => {
            doc.checkpoint();
            Some(Drag::Rotate {
                start_x: mouse_x,
                applied: 0.0,
            })
        }
    }
}

pub fn update_drag(drag: &mut Drag, doc: &mut Document, origin: Vec3, dir: Vec3, mouse_x: f32, grid: f32) {
    let step = |v: f32| if grid > 0.0 { snap(v, grid) } else { v };
    let selection = doc.selection();
    match drag {
        Drag::Move {
            plane_y,
            start,
            applied,
        } => {
            let Some(p) = plane_hit(origin, dir, *plane_y) else {
                return;
            };
            let raw = p - *start;
            let target = Vec3::new(step(raw.x), 0.0, step(raw.z));
            let delta = target - *applied;
            if delta != Vec3::ZERO {
                doc.translate(&selection, delta);
                *applied = target;
            }
        }
        Drag::Scale {
            part,
            axis_world,
            local_normal,
            anchor,
            applied,
        } => {
            let along = closest_on_axis(origin, dir, *anchor, *axis_world);
            let target = step(along);
            let delta = target - *applied;
            if delta != 0.0 {
                doc.resize(*part, *local_normal, delta);
                *applied = target;
            }
        }
        Drag::Rotate { start_x, applied } => {
            let raw = (mouse_x - *start_x) * 0.01;
            let step = 15f32.to_radians();
            let target = snap(raw, step);
            let delta = target - *applied;
            if delta != 0.0 {
                doc.rotate(&selection, Vec3::Y, delta);
                *applied = target;
            }
        }
    }
}
