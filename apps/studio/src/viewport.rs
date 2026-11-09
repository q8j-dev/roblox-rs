use egui::{Color32, Pos2, Rect};
use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{CFrame, Mat3, Vec3};
use rbx_render::adorn::{self, Adorns, OverlayHandle};
use rbx_studio_ui::Document;

pub const SELECT_COLOR: [f32; 4] = [0.1, 0.6, 1.0, 1.0];
pub const HOVER_COLOR: [f32; 4] = [0.7, 0.9, 1.0, 1.0];
pub const RESIZE_COLOR: [f32; 4] = [0.1, 0.6, 1.0, 1.0];
pub const HANDLE_HIGHLIGHT: [f32; 4] = [0.3, 0.7, 1.0, 1.0];
const CORNFLOWER: [f32; 4] = [100.0 / 255.0, 149.0 / 255.0, 237.0 / 255.0, 1.0];
const AXIS_COLORS: [Color32; 3] = [Color32::RED, Color32::GREEN, Color32::BLUE];
const MINIMUM_EXTENTS: f32 = 0.4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Move,
    Scale,
    Rotate,
    Lock,
    Paint,
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

pub fn pick(
    dm: &DataModel,
    origin: Vec3,
    dir: Vec3,
    include_locked: bool,
    ignore: &[InstanceId],
) -> Option<Hit> {
    let ws = dm.find_service("Workspace")?;
    let mut best: Option<Hit> = None;
    for id in dm.descendants(ws) {
        if !dm.is_a(id, "BasePart") || dm.class_of(id) == Some("Terrain") || ignore.contains(&id) {
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

fn parts_in(dm: &DataModel, ids: &[InstanceId]) -> Vec<InstanceId> {
    let mut out = Vec::new();
    for &id in ids {
        if !dm.contains(id) {
            continue;
        }
        if dm.is_a(id, "BasePart") {
            out.push(id);
        }
        out.extend(
            dm.descendants(id)
                .into_iter()
                .filter(|d| dm.is_a(*d, "BasePart")),
        );
    }
    out
}

fn extents(dm: &DataModel, parts: &[InstanceId]) -> Option<(Vec3, Vec3)> {
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    let mut any = false;
    for &p in parts {
        let Some((cf, size)) = part_box(dm, p) else {
            continue;
        };
        let h = size * 0.5;
        for i in 0..8 {
            let c = Vec3::new(
                if i & 1 == 0 { -h.x } else { h.x },
                if i & 2 == 0 { -h.y } else { h.y },
                if i & 4 == 0 { -h.z } else { h.z },
            );
            let w = cf.point_to_world_space(c);
            lo = lo.min(w);
            hi = hi.max(w);
            any = true;
        }
    }
    any.then_some((lo, hi))
}

fn instance_box(dm: &DataModel, id: InstanceId) -> Option<(CFrame, Vec3, f32)> {
    if dm.is_a(id, "BasePart") {
        let (cf, size) = part_box(dm, id)?;
        return Some((cf, size, 0.02));
    }
    if dm.is_a(id, "Model") {
        let parts = parts_in(dm, &[id]);
        let (lo, hi) = extents(dm, &parts)?;
        return Some((CFrame::from_translation((lo + hi) * 0.5), hi - lo, 0.05));
    }
    None
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

fn ray_sphere(origin: Vec3, dir: Vec3, center: Vec3, radius: f32) -> bool {
    let oc = origin - center;
    let b = oc.dot(dir);
    let c = oc.dot(oc) - radius * radius;
    let disc = b * b - c;
    disc >= 0.0 && (-b + disc.sqrt()) >= 0.0
}

fn ray_segment_distance(origin: Vec3, dir: Vec3, a: Vec3, b: Vec3) -> f32 {
    let u = b - a;
    let w0 = origin - a;
    let aa = dir.dot(dir);
    let bb = dir.dot(u);
    let cc = u.dot(u);
    let dd = dir.dot(w0);
    let ee = u.dot(w0);
    let denom = aa * cc - bb * bb;
    let (mut sc, mut tc) = if denom.abs() < 1e-6 {
        (0.0, ee / cc.max(1e-6))
    } else {
        ((bb * ee - cc * dd) / denom, (aa * ee - bb * dd) / denom)
    };
    tc = tc.clamp(0.0, 1.0);
    sc = sc.max(0.0);
    let p = origin + dir * sc;
    let q = a + u * tc;
    (p - q).length()
}

pub fn snap(v: f32, step: f32) -> f32 {
    if step <= 0.0 {
        return v;
    }
    (v / step).round() * step
}

pub enum Drag {
    Run {
        parts: Vec<(InstanceId, CFrame)>,
        grab_offset: Vec3,
        center: Vec3,
        half: Vec3,
    },
    Move {
        axis: Vec3,
        start: Vec3,
        applied: f32,
    },
    Scale {
        part: InstanceId,
        normal_id: usize,
        anchor: Vec3,
        axis_world: Vec3,
        orig_cf: CFrame,
        orig_size: Vec3,
    },
    Rotate {
        normal_id: usize,
        center: Vec3,
        start_vec: Vec3,
        applied: f32,
    },
}

pub struct Tools {
    pub hover: Option<InstanceId>,
    pub over_handle: Option<(Option<InstanceId>, usize)>,
    pub drag: Option<Drag>,
    pub rotation_frame: Mat3,
    pub saved_transparency: Vec<(InstanceId, f32)>,
    pub press: Option<(Vec3, Vec3, Option<(Option<InstanceId>, usize)>)>,
}

impl Default for Tools {
    fn default() -> Self {
        Tools {
            hover: None,
            over_handle: None,
            drag: None,
            rotation_frame: Mat3::IDENTITY,
            saved_transparency: Vec::new(),
            press: None,
        }
    }
}

struct HandleSet {
    owner: Option<InstanceId>,
    kind: OverlayHandle,
    frame: CFrame,
    size: Vec3,
    mask: u8,
}

impl Tools {
    fn handle_sets(&self, dm: &DataModel, tool: Tool, selection: &[InstanceId]) -> Vec<HandleSet> {
        let parts = parts_in(dm, selection);
        if parts.is_empty() {
            return Vec::new();
        }
        match tool {
            Tool::Select | Tool::Lock | Tool::Paint => Vec::new(),
            Tool::Scale => selection
                .iter()
                .filter(|id| dm.is_a(**id, "BasePart"))
                .filter_map(|&id| {
                    let (cf, size) = part_box(dm, id)?;
                    Some(HandleSet {
                        owner: Some(id),
                        kind: OverlayHandle::Resize,
                        frame: cf,
                        size,
                        mask: 0x3f,
                    })
                })
                .collect(),
            Tool::Move | Tool::Rotate => {
                let Some((lo, hi)) = extents(dm, &parts) else {
                    return Vec::new();
                };
                let mut size = hi - lo;
                if size.max_element() < MINIMUM_EXTENTS {
                    size = Vec3::splat(MINIMUM_EXTENTS);
                }
                let kind = if tool == Tool::Move {
                    OverlayHandle::Move
                } else {
                    OverlayHandle::Rotate
                };
                let mask = match (&self.drag, tool) {
                    (Some(Drag::Rotate { normal_id, .. }), Tool::Rotate) => 1u8 << normal_id,
                    _ => 0x3f,
                };
                vec![HandleSet {
                    owner: None,
                    kind,
                    frame: CFrame::new(
                        if tool == Tool::Rotate {
                            Mat3::IDENTITY
                        } else {
                            self.rotation_frame
                        },
                        (lo + hi) * 0.5,
                    ),
                    size,
                    mask,
                }]
            }
        }
    }

    pub fn overlay(
        &self,
        dm: &DataModel,
        tool: Tool,
        selection: &[InstanceId],
        camera: Vec3,
    ) -> Adorns {
        let mut out = Adorns::default();
        if let Some(h) = self.hover
            && self.drag.is_none()
            && let Some((cf, size, thick)) = instance_box(dm, h)
        {
            adorn::overlay_selection_box(&mut out, camera, cf, size, HOVER_COLOR, thick * 2.0);
        }
        for &id in selection {
            if let Some((cf, size, thick)) = instance_box(dm, id) {
                adorn::overlay_selection_box(&mut out, camera, cf, size, SELECT_COLOR, thick);
            }
        }
        if tool == Tool::Rotate
            && let Some((lo, hi)) = extents(dm, &parts_in(dm, selection))
        {
            adorn::overlay_star(&mut out, camera, (lo + hi) * 0.5, 3.0, [1.0, 1.0, 0.0, 1.0]);
            adorn::overlay_outline_box(&mut out, camera, lo, hi, CORNFLOWER);
        }
        for set in self.handle_sets(dm, tool, selection) {
            let highlight = match self.over_handle {
                Some((owner, id)) if owner == set.owner => Some(id),
                _ => match (&self.drag, set.kind) {
                    (Some(Drag::Move { axis, .. }), OverlayHandle::Move) => {
                        axis_id(*axis, set.frame.rotation)
                    }
                    (
                        Some(Drag::Scale {
                            part, normal_id, ..
                        }),
                        OverlayHandle::Resize,
                    ) if Some(*part) == set.owner => Some(*normal_id),
                    (Some(Drag::Rotate { normal_id, .. }), OverlayHandle::Rotate) => {
                        Some(*normal_id)
                    }
                    _ => None,
                },
            };
            let color = if set.kind == OverlayHandle::Rotate {
                [0.0, 1.0, 0.0, 1.0]
            } else {
                RESIZE_COLOR
            };
            let highlight_color = if set.kind == OverlayHandle::Rotate {
                let c = AXIS_COLORS[highlight.unwrap_or(0) % 3];
                [
                    c.r() as f32 / 255.0,
                    c.g() as f32 / 255.0,
                    c.b() as f32 / 255.0,
                    1.0,
                ]
            } else {
                HANDLE_HIGHLIGHT
            };
            adorn::overlay_handles(
                &mut out,
                camera,
                set.size,
                set.frame,
                set.kind,
                color,
                true,
                set.mask,
                highlight,
                highlight_color,
            );
        }
        out
    }

    pub fn draw_2d(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        cam: &FlyCamera,
        dm: &DataModel,
        tool: Tool,
        selection: &[InstanceId],
    ) {
        if tool == Tool::Rotate && self.drag.is_some() {
            return;
        }
        for set in self.handle_sets(dm, tool, selection) {
            for id in 0..6 {
                if set.mask & (1 << id) == 0 {
                    continue;
                }
                let local = adorn::handle_position(cam.position, set.size, set.kind, id);
                let mut world = set.frame.point_to_world_space(local);
                if set.kind == OverlayHandle::Move {
                    let r = adorn::handle_tip_radius(cam.position, world);
                    world = CFrame::new(set.frame.rotation, world)
                        .point_to_world_space(adorn::normal_vector_of(id) * r);
                }
                if let Some(p) = cam.project(rect, world) {
                    painter.rect_filled(
                        Rect::from_center_size(p, egui::vec2(6.0, 6.0)),
                        0.0,
                        AXIS_COLORS[id % 3],
                    );
                }
            }
        }
    }

    pub fn update_hover(
        &mut self,
        dm: &DataModel,
        tool: Tool,
        selection: &[InstanceId],
        cam: &FlyCamera,
        origin: Vec3,
        dir: Vec3,
    ) {
        if self.drag.is_some() {
            return;
        }
        self.over_handle = None;
        for set in self.handle_sets(dm, tool, selection) {
            for id in 0..6 {
                if set.mask & (1 << id) == 0 {
                    continue;
                }
                let local = adorn::handle_position(cam.position, set.size, set.kind, id);
                let world = set.frame.point_to_world_space(local);
                let radius = adorn::handle_radius(cam.position, set.kind, world);
                let hit = if set.kind == OverlayHandle::Move {
                    let arrow = set
                        .frame
                        .vector_to_world_space(adorn::normal_vector_of(id) * radius);
                    ray_segment_distance(origin, dir, world, world + arrow) <= 0.05 * radius
                } else {
                    ray_sphere(origin, dir, world, radius)
                };
                if hit {
                    self.over_handle = Some((set.owner, id));
                    return;
                }
            }
        }
        self.hover = pick(dm, origin, dir, false, &[]).map(|h| selectable(dm, h.part));
    }

    fn set_transparent(&mut self, doc: &mut Document, parts: &[InstanceId]) {
        let mut e = doc.runtime.engine.lock();
        for &p in parts {
            let t =
                e.dm.get(p, "Transparency")
                    .ok()
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0) as f32;
            self.saved_transparency.push((p, t));
            let _ = e.dm.set(p, "Transparency", Variant::Float(0.5));
        }
    }

    fn restore_transparency(&mut self, doc: &mut Document) {
        let mut e = doc.runtime.engine.lock();
        for (p, t) in self.saved_transparency.drain(..) {
            if e.dm.contains(p) {
                let _ = e.dm.set(p, "Transparency", Variant::Float(t));
            }
        }
    }

    pub fn begin(
        &mut self,
        doc: &mut Document,
        tool: Tool,
        collisions: bool,
        cam: &FlyCamera,
        origin: Vec3,
        dir: Vec3,
    ) -> bool {
        let selection = doc.selection();
        if let Some((owner, id)) = self.over_handle {
            let (sets, parts) = {
                let e = doc.runtime.engine.lock();
                (
                    self.handle_sets(&e.dm, tool, &selection),
                    parts_in(&e.dm, &selection),
                )
            };
            let Some(set) = sets.into_iter().find(|s| s.owner == owner) else {
                return false;
            };
            doc.checkpoint();
            let local = adorn::handle_position(cam.position, set.size, set.kind, id);
            let world = set.frame.point_to_world_space(local);
            self.drag = Some(match set.kind {
                OverlayHandle::Move => {
                    let axis = set.frame.vector_to_world_space(adorn::normal_vector_of(id));
                    let start = world + axis * closest_on_axis(origin, dir, world, axis);
                    if !collisions {
                        self.set_transparent(doc, &parts);
                    }
                    Drag::Move {
                        axis,
                        start,
                        applied: 0.0,
                    }
                }
                OverlayHandle::Resize => {
                    let part = owner.unwrap_or(set.owner.unwrap_or(selection[0]));
                    let axis_world = set.frame.vector_to_world_space(adorn::normal_vector_of(id));
                    if !collisions {
                        self.set_transparent(doc, &[part]);
                    }
                    Drag::Scale {
                        part,
                        normal_id: id,
                        anchor: world,
                        axis_world,
                        orig_cf: set.frame,
                        orig_size: set.size,
                    }
                }
                OverlayHandle::Rotate => {
                    let normal = adorn::normal_vector_of(id % 3);
                    let center = set.frame.translation;
                    let start = plane_point(origin, dir, center, normal).unwrap_or(world) - center;
                    Drag::Rotate {
                        normal_id: id,
                        center,
                        start_vec: start,
                        applied: 0.0,
                    }
                }
            });
            return true;
        }
        let hit = {
            let e = doc.runtime.engine.lock();
            pick(&e.dm, origin, dir, false, &[]).map(|h| (selectable(&e.dm, h.part), h))
        };
        let Some((target, hit)) = hit else {
            return false;
        };
        if !selection.contains(&target) {
            doc.select(vec![target]);
        }
        let selection = doc.selection();
        let parts = {
            let e = doc.runtime.engine.lock();
            parts_in(&e.dm, &selection)
                .into_iter()
                .filter_map(|p| part_box(&e.dm, p).map(|(cf, _)| (p, cf)))
                .collect::<Vec<_>>()
        };
        let Some((lo, hi)) = ({
            let e = doc.runtime.engine.lock();
            extents(&e.dm, &parts.iter().map(|p| p.0).collect::<Vec<_>>())
        }) else {
            return false;
        };
        doc.checkpoint();
        if !collisions {
            self.set_transparent(doc, &parts.iter().map(|p| p.0).collect::<Vec<_>>());
        }
        let center = (lo + hi) * 0.5;
        self.drag = Some(Drag::Run {
            parts,
            grab_offset: hit.point - center,
            center,
            half: (hi - lo) * 0.5,
        });
        true
    }

    pub fn update(&mut self, doc: &mut Document, origin: Vec3, dir: Vec3, grid: f32) {
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        match drag {
            Drag::Run {
                parts,
                grab_offset,
                center,
                half,
            } => {
                let ignore: Vec<InstanceId> = parts.iter().map(|p| p.0).collect();
                let hit = {
                    let e = doc.runtime.engine.lock();
                    pick(&e.dm, origin, dir, true, &ignore)
                };
                let target = match hit {
                    Some(h) => {
                        let (cf, _) = {
                            let e = doc.runtime.engine.lock();
                            part_box(&e.dm, h.part).unwrap_or((CFrame::IDENTITY, Vec3::ONE))
                        };
                        let n = cf
                            .vector_to_world_space(h.local_normal)
                            .normalize_or(Vec3::Y);
                        let support = half.x * n.x.abs() + half.y * n.y.abs() + half.z * n.z.abs();
                        let tangent = *grab_offset - n * grab_offset.dot(n);
                        h.point + n * support - tangent
                    }
                    None => match plane_point(origin, dir, *center, Vec3::Y) {
                        Some(p) => p - Vec3::new(grab_offset.x, 0.0, grab_offset.z),
                        None => return,
                    },
                };
                let snapped = Vec3::new(
                    snap(target.x - half.x, grid) + half.x,
                    target.y,
                    snap(target.z - half.z, grid) + half.z,
                );
                let delta = snapped - *center;
                let mut e = doc.runtime.engine.lock();
                for (p, orig) in parts.iter() {
                    let _ = e.dm.set(
                        *p,
                        "CFrame",
                        Variant::CFrame(CFrame::new(orig.rotation, orig.translation + delta)),
                    );
                }
                doc.dirty = true;
            }
            Drag::Move {
                axis,
                start,
                applied,
            } => {
                let along = closest_on_axis(origin, dir, *start, *axis);
                let target = snap(along, grid);
                let delta = target - *applied;
                if delta != 0.0 {
                    let sel = doc.selection();
                    doc.translate(&sel, *axis * delta);
                    *applied = target;
                }
            }
            Drag::Scale {
                part,
                normal_id,
                anchor,
                axis_world,
                orig_cf,
                orig_size,
                ..
            } => {
                let along = closest_on_axis(origin, dir, *anchor, *axis_world);
                let step = if grid > 0.0 { grid } else { 0.01 };
                let amount = (along / step).round() * step;
                let axis = *normal_id % 3;
                let sign = if *normal_id < 3 { 1.0 } else { -1.0 };
                let mut size = *orig_size;
                size[axis] = (orig_size[axis] + amount).max(if grid > 0.0 {
                    grid.clamp(0.2, 1.0)
                } else {
                    0.2
                });
                let grown = size[axis] - orig_size[axis];
                let mut local = Vec3::ZERO;
                local[axis] = grown * 0.5 * sign;
                let cf = CFrame::new(
                    orig_cf.rotation,
                    orig_cf.translation + orig_cf.vector_to_world_space(local),
                );
                let mut e = doc.runtime.engine.lock();
                let _ = e.dm.set(*part, "Size", Variant::Vector3(size));
                let _ = e.dm.set(*part, "CFrame", Variant::CFrame(cf));
                doc.dirty = true;
            }
            Drag::Rotate {
                normal_id,
                center,
                start_vec,
                applied,
            } => {
                let normal = adorn::normal_vector_of(*normal_id % 3);
                let Some(p) = plane_point(origin, dir, *center, normal) else {
                    return;
                };
                let cur = p - *center;
                let a = start_vec.normalize_or(Vec3::X);
                let b = cur.normalize_or(Vec3::X);
                let angle = a.cross(b).dot(normal).atan2(a.dot(b));
                let step = if grid >= 1.0 {
                    std::f32::consts::FRAC_PI_4
                } else if grid > 0.0 {
                    std::f32::consts::PI / 12.0
                } else {
                    0.0
                };
                let target = if step > 0.0 {
                    (angle / step).trunc() * step
                } else {
                    angle
                };
                let delta = target - *applied;
                if delta.abs() > 0.0001 {
                    let sel = doc.selection();
                    doc.rotate(&sel, normal, delta);
                    *applied = target;
                }
            }
        }
    }

    pub fn end(&mut self, doc: &mut Document) {
        if self.drag.take().is_some() {
            self.restore_transparency(doc);
        }
    }
}

fn plane_point(origin: Vec3, dir: Vec3, center: Vec3, normal: Vec3) -> Option<Vec3> {
    let denom = dir.dot(normal);
    if denom.abs() < 1e-5 {
        return None;
    }
    let t = (center - origin).dot(normal) / denom;
    (t > 0.0).then(|| origin + dir * t)
}

fn axis_id(axis: Vec3, rotation: Mat3) -> Option<usize> {
    (0..6).find(|&id| (rotation * adorn::normal_vector_of(id)).dot(axis) > 0.99)
}
