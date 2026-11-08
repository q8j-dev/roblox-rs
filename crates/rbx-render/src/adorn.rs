use crate::gui::{get_bool, get_f32, get_i64, get_string, get_vec2};
use crate::world_gui::{part_box, vec3};
use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{CFrame, Mat3, Vec3};

const SELECTION_LINE_THRESHOLD: f32 = 1500.0;
const HANDLE_OFFSET: f32 = 0.5;
const TORUS_THICKNESS: f32 = 0.025;
const TORUS_MIN_ANGLE: f32 = 2.5 * std::f32::consts::PI / 180.0;
const SPHERE_RADIUS: f32 = 0.5;
const SPHERE_MIN_ANGLE: f32 = 2.0 * std::f32::consts::PI / 180.0;
const ARROW_LENGTH: f32 = 3.0;
const ARROW_MIN_ANGLE: f32 = 3.5 * std::f32::consts::PI / 180.0;
const HANDLE_TRANSPARENCY: f32 = 0.65;
const AXIS_TRANSPARENCY: f32 = 0.28;
const AXIS_COLORS: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shape {
    Box,
    Cylinder,
    Sphere,
    Cone,
    Line,
    Quad,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Material {
    NoLighting = 0,
    Lit = 1,
    SelfLit = 2,
    SelfLitHighlight = 3,
    Outline = 4,
    AaLine = 5,
}

#[derive(Clone, Debug)]
pub struct AdornMesh {
    pub shape: Shape,
    pub material: Material,
    pub rotation: Mat3,
    pub translation: Vec3,
    pub color: [f32; 4],
    pub thickness: f32,
    pub z_index: i32,
    pub always_on_top: bool,
    pub texture: Option<String>,
}

impl AdornMesh {
    pub fn rows(&self) -> [[f32; 4]; 3] {
        let m = self.rotation;
        let t = self.translation;
        [
            [m.x_axis.x, m.y_axis.x, m.z_axis.x, t.x],
            [m.x_axis.y, m.y_axis.y, m.z_axis.y, t.y],
            [m.x_axis.z, m.y_axis.z, m.z_axis.z, t.z],
        ]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AdornLine {
    pub a: Vec3,
    pub b: Vec3,
    pub color: [f32; 4],
}

#[derive(Clone, Debug, Default)]
pub struct Adorns {
    pub meshes: Vec<AdornMesh>,
    pub lines: Vec<AdornLine>,
}

impl Adorns {
    pub fn is_empty(&self) -> bool {
        self.meshes.is_empty() && self.lines.is_empty()
    }
}

struct Ctx<'a> {
    out: &'a mut Adorns,
    camera: Vec3,
    cframe: CFrame,
    material: Material,
}

fn scaled(rotation: Mat3, scale: Vec3) -> Mat3 {
    Mat3::from_cols(
        rotation.x_axis * scale.x,
        rotation.y_axis * scale.y,
        rotation.z_axis * scale.z,
    )
}

fn frame_from_x(x: Vec3) -> Mat3 {
    let axis_x = x.normalize_or(Vec3::X);
    let helper = if axis_x.y.abs() < 0.7 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let axis_y = helper.cross(axis_x).normalize_or(Vec3::Y);
    let axis_z = axis_x.cross(axis_y).normalize_or(Vec3::Z);
    Mat3::from_cols(axis_x, axis_y, axis_z)
}

impl Ctx<'_> {
    #[allow(clippy::too_many_arguments)]
    fn submit(
        &mut self,
        shape: Shape,
        material: Material,
        translation: Vec3,
        rotation: Mat3,
        scale: Vec3,
        color: [f32; 4],
        thickness: f32,
        z_index: i32,
        always_on_top: bool,
    ) {
        self.out.meshes.push(AdornMesh {
            shape,
            material,
            rotation: scaled(rotation, scale),
            translation,
            color,
            thickness,
            z_index,
            always_on_top,
            texture: None,
        });
    }

    fn aabox(&mut self, lo: Vec3, hi: Vec3, color: [f32; 4]) {
        let center = self.cframe.point_to_world_space((lo + hi) * 0.5);
        let rot = self.cframe.rotation;
        self.submit(
            Shape::Box,
            Material::NoLighting,
            center,
            rot,
            (hi - lo) * 0.5,
            color,
            0.0,
            -1,
            false,
        );
    }

    fn sphere(&mut self, center: Vec3, radius: f32, color: [f32; 4]) {
        let c = self.cframe.point_to_world_space(center);
        let rot = self.cframe.rotation;
        let material = self.material;
        self.submit(
            Shape::Sphere,
            material,
            c,
            rot,
            Vec3::splat(radius),
            color,
            0.0,
            -1,
            false,
        );
    }

    fn cylinder_along_x(&mut self, radius: f32, length: f32, color: [f32; 4]) {
        let c = self.cframe.translation;
        let rot = self.cframe.rotation;
        let material = self.material;
        self.submit(
            Shape::Cylinder,
            material,
            c,
            rot,
            Vec3::new(length / 2.0, radius, radius),
            color,
            0.0,
            -1,
            false,
        );
    }

    fn ray(&mut self, origin: Vec3, direction: Vec3, color: [f32; 4]) {
        let length = direction.length();
        if length <= 0.0 {
            return;
        }
        let stalk = 1.0f32;
        let head = 0.3f32;
        let total = stalk + head;
        let head_radius = 0.075f32;
        let stalk_radius = head_radius / 3.0;
        let scale = length / total;
        let stalk_center = self
            .cframe
            .point_to_world_space(origin + direction / 2.0 * (stalk / total));
        let cone_origin = self
            .cframe
            .point_to_world_space(origin + direction * (stalk / total));
        let rotation = frame_from_x(self.cframe.vector_to_world_space(direction));
        let material = self.material;
        self.submit(
            Shape::Cylinder,
            material,
            stalk_center,
            rotation,
            Vec3::new(
                stalk / 2.0 * scale,
                stalk_radius * scale,
                stalk_radius * scale,
            ),
            color,
            0.0,
            -1,
            false,
        );
        self.submit(
            Shape::Cone,
            material,
            cone_origin,
            rotation,
            Vec3::new(head * scale, head_radius * scale, head_radius * scale),
            color,
            0.0,
            -1,
            false,
        );
    }

    fn line(&mut self, a: Vec3, b: Vec3, color: [f32; 4]) {
        let a = self.cframe.point_to_world_space(a);
        let b = self.cframe.point_to_world_space(b);
        self.out.lines.push(AdornLine { a, b, color });
    }

    fn outline_box(&mut self, lo: Vec3, hi: Vec3, color: [f32; 4]) {
        let (x0, y0, z0, x1, y1, z1) = (lo.x, lo.y, lo.z, hi.x, hi.y, hi.z);
        let v = Vec3::new;
        for (a, b) in [
            (v(x0, y0, z0), v(x1, y0, z0)),
            (v(x0, y1, z0), v(x1, y1, z0)),
            (v(x0, y0, z1), v(x1, y0, z1)),
            (v(x0, y1, z1), v(x1, y1, z1)),
            (v(x0, y0, z0), v(x0, y1, z0)),
            (v(x1, y0, z0), v(x1, y1, z0)),
            (v(x0, y0, z1), v(x0, y1, z1)),
            (v(x1, y0, z1), v(x1, y1, z1)),
            (v(x0, y0, z0), v(x0, y0, z1)),
            (v(x1, y0, z0), v(x1, y0, z1)),
            (v(x0, y1, z0), v(x0, y1, z1)),
            (v(x1, y1, z0), v(x1, y1, z1)),
        ] {
            self.line(a, b, color);
        }
    }

    fn extrusion(&mut self, points: &[Vec3], radius: f32, color: [f32; 4]) {
        let n = points.len();
        for i in 0..n {
            let from = self.cframe.point_to_world_space(points[i]);
            let to = self.cframe.point_to_world_space(points[(i + 1) % n]);
            let length = (to - from).length();
            let rotation = frame_from_x(to - from);
            let material = self.material;
            self.submit(
                Shape::Cylinder,
                material,
                (from + to) / 2.0,
                rotation,
                Vec3::new(length / 2.0, radius, radius),
                color,
                0.0,
                -1,
                false,
            );
        }
    }
}

fn alpha(c: [f32; 3], a: f32) -> [f32; 4] {
    [c[0], c[1], c[2], a]
}

fn color3(dm: &DataModel, id: InstanceId, name: &str) -> [f32; 3] {
    match dm.get(id, name) {
        Ok(Variant::Color3(c)) => [c.r, c.g, c.b],
        Ok(Variant::BrickColor(b)) => {
            let c = b.color3();
            [c.r, c.g, c.b]
        }
        _ => [1.0, 1.0, 1.0],
    }
}

fn selection_box(ctx: &mut Ctx, cf: CFrame, size: Vec3, color: [f32; 4], thickness: f32) {
    let delta_sq = (cf.translation - ctx.camera).length_squared();
    let grid = size.x.max(size.y).max(size.z);
    let grid_sq = grid * grid;
    ctx.cframe = cf;
    if delta_sq > grid_sq {
        let threshold = SELECTION_LINE_THRESHOLD * thickness;
        if delta_sq - grid_sq > threshold * threshold {
            let half = size / 2.0;
            ctx.outline_box(-half, half, color);
            return;
        }
    }
    let half = size * 0.5;
    let h = thickness;
    for c1 in 0..3 {
        let c2 = (c1 + 1) % 3;
        let c3 = (c1 + 2) % 3;
        for d2 in [-1.0f32, 1.0] {
            for d3 in [-1.0f32, 1.0] {
                let mut p0 = Vec3::ZERO;
                let mut p1 = Vec3::ZERO;
                let shorten = if c1 == 0 { h } else { -h };
                p0[c1] = -half[c1] - shorten;
                p0[c2] = d2 * half[c2] - h;
                p0[c3] = d3 * half[c3] - h;
                p1[c1] = half[c1] + shorten;
                p1[c2] = d2 * half[c2] + h;
                p1[c3] = d3 * half[c3] + h;
                if p0.x <= p1.x && p0.y <= p1.y && p0.z <= p1.z {
                    ctx.aabox(p0, p1, color);
                }
            }
        }
    }
}

fn surface_border(ctx: &mut Ctx, half: Vec3, highlight: f32, surface: usize, color: [f32; 4]) {
    let c1 = surface % 3;
    let cz = half[c1] * if surface < 3 { 1.0 } else { -1.0 };
    let mut p0 = Vec3::ZERO;
    let mut p1 = Vec3::ZERO;
    p0[c1] = cz - highlight;
    p1[c1] = cz + highlight;
    let c2 = (c1 + 1) % 3;
    let c3 = (c1 + 2) % 3;
    for direction in 0..2 {
        let (cy, czz) = if direction == 1 { (c2, c3) } else { (c3, c2) };
        for polarity in [-1.0f32, 1.0] {
            p0[cy] = polarity * half[cy] - highlight;
            p1[cy] = polarity * half[cy] + highlight;
            p0[czz] = -half[czz] - highlight;
            p1[czz] = half[czz] + highlight;
            ctx.aabox(p0, p1, color);
        }
    }
}

fn normal_vector(id: usize) -> Vec3 {
    match id {
        0 => Vec3::X,
        1 => Vec3::Y,
        2 => Vec3::Z,
        3 => Vec3::NEG_X,
        4 => Vec3::NEG_Y,
        _ => Vec3::NEG_Z,
    }
}

fn normal_to_u(id: usize) -> usize {
    match id {
        0 => 2,
        1 => 0,
        2 => 1,
        3 => 5,
        4 => 3,
        _ => 4,
    }
}

fn uvw_to_object(v: Vec3, axis: usize) -> Vec3 {
    match axis {
        0 => Vec3::new(v.z, v.y, -v.x),
        1 => Vec3::new(-v.x, v.z, v.y),
        2 => v,
        3 => Vec3::new(-v.z, v.y, v.x),
        4 => Vec3::new(v.x, -v.z, v.y),
        _ => Vec3::new(-v.x, v.y, -v.z),
    }
}

fn scale_to_camera(camera: Vec3, pos: Vec3, min_angle: f32, expected: f32) -> f32 {
    let distance = (camera - pos).length();
    let scale = if distance > 1.0 / min_angle {
        distance * min_angle
    } else {
        1.0
    };
    scale * expected
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HandleType {
    Resize,
    Move,
    Rotate,
}

#[allow(clippy::too_many_arguments)]
fn handles(
    ctx: &mut Ctx,
    size: Vec3,
    position: CFrame,
    kind: HandleType,
    color: [f32; 4],
    mask: u8,
    use_axis_color: bool,
    highlight: Option<usize>,
    highlight_color: [f32; 4],
) {
    let half = size / 2.0;
    let mut used_axes = 0u8;
    for pos_neg in 0..2 {
        for xyz in 0..3 {
            let id = pos_neg * 3 + xyz;
            if mask & (1 << id) == 0 {
                continue;
            }
            let handle_pos = match kind {
                HandleType::Rotate => {
                    let radius = half.length() + HANDLE_OFFSET * 4.0;
                    normal_vector(normal_to_u(id)) * radius
                }
                HandleType::Resize => normal_vector(id) * (half + Vec3::splat(HANDLE_OFFSET * 4.0)),
                HandleType::Move => normal_vector(id) * (half + Vec3::splat(HANDLE_OFFSET)),
            };
            let world = position.point_to_world_space(handle_pos);
            let (min_angle, expected) = match kind {
                HandleType::Move => (ARROW_MIN_ANGLE, ARROW_LENGTH),
                _ => (SPHERE_MIN_ANGLE, SPHERE_RADIUS),
            };
            let radius = scale_to_camera(ctx.camera, world, min_angle, expected);
            ctx.cframe = CFrame::new(position.rotation, world);
            let lit = highlight == Some(id);
            ctx.material = if lit { Material::SelfLitHighlight } else { Material::SelfLit };
            let mut handle_color = if use_axis_color || kind == HandleType::Rotate {
                alpha(AXIS_COLORS[id % 3], color[3])
            } else {
                color
            };
            if lit {
                handle_color = highlight_color;
            } else {
                handle_color[3] *= HANDLE_TRANSPARENCY;
            }
            match kind {
                HandleType::Resize => {
                    ctx.sphere(Vec3::ZERO, if lit { radius + 0.08 } else { radius }, handle_color)
                }
                HandleType::Move => ctx.ray(Vec3::ZERO, normal_vector(id) * radius, handle_color),
                HandleType::Rotate => {
                    let flag = 1u8 << (id % 3);
                    if used_axes & flag == 0 {
                        let torus_radius = (position.translation - world).length();
                        let thickness = scale_to_camera(
                            ctx.camera,
                            position.translation,
                            TORUS_MIN_ANGLE,
                            TORUS_THICKNESS,
                        );
                        let mut axis_color = handle_color;
                        if !lit {
                            axis_color[3] = AXIS_TRANSPARENCY;
                        }
                        let x = uvw_to_object(Vec3::X, id);
                        let y = uvw_to_object(Vec3::Y, id);
                        let points: Vec<Vec3> = (0..32)
                            .map(|i| {
                                let theta = i as f32 / 32.0 * std::f32::consts::TAU;
                                x * (theta.cos() * torus_radius) + y * (theta.sin() * torus_radius)
                            })
                            .collect();
                        ctx.cframe = position;
                        ctx.extrusion(&points, thickness, axis_color);
                        used_axes |= flag;
                    }
                    ctx.cframe = CFrame::from_translation(world);
                    ctx.sphere(
                        Vec3::ZERO,
                        radius,
                        [
                            handle_color[0] - 0.02,
                            handle_color[1] - 0.02,
                            handle_color[2] - 0.02,
                            1.0,
                        ],
                    );
                }
            }
        }
    }
    ctx.material = Material::Lit;
}

fn handle_frame(dm: &DataModel, id: InstanceId, adornee: InstanceId) -> Option<CFrame> {
    let (pv, size) = part_box(dm, adornee)?;
    let local = dm
        .get_raw(id, "CFrame")
        .and_then(|v| v.as_cframe())
        .unwrap_or(CFrame::IDENTITY);
    let world = pv.to_world_space(&local);
    let offset =
        pv.point_to_world_space(vec3(dm, id, "SizeRelativeOffset") * size * 0.5) - pv.translation;
    Some(CFrame::new(world.rotation, world.translation + offset))
}

fn torso_position(dm: &DataModel, humanoid: InstanceId) -> Option<Vec3> {
    let model = dm.parent_of(humanoid)?;
    let torso = dm
        .find_first_child(model, "Torso", false)
        .or_else(|| dm.find_first_child(model, "UpperTorso", false))?;
    dm.get_raw(torso, "CFrame")
        .and_then(|v| v.as_cframe())
        .map(|c| c.translation)
}

fn adornment(dm: &DataModel, id: InstanceId, ctx: &mut Ctx) {
    let class = dm.class_of(id).unwrap_or("");
    if !get_bool(dm, id, "Visible", true) {
        return;
    }
    let color = color3(dm, id, "Color3");
    let transparency = get_f32(dm, id, "Transparency");
    let rgba = alpha(color, 1.0 - transparency);
    ctx.material = Material::Lit;
    match class {
        "SelectionPartLasso" | "SelectionPointLasso" => {
            let Some(a) = dm
                .get_ref(id, "Humanoid")
                .and_then(|h| torso_position(dm, h))
            else {
                return;
            };
            let b = if class == "SelectionPartLasso" {
                match dm
                    .get_ref(id, "Part")
                    .and_then(|p| dm.get_raw(p, "CFrame").and_then(|v| v.as_cframe()))
                {
                    Some(cf) => cf.translation,
                    None => return,
                }
            } else {
                vec3(dm, id, "Point")
            };
            let distance = (b - a).length();
            if distance == 0.0 {
                return;
            }
            ctx.cframe = CFrame::new(frame_from_x(b - a), (a + b) * 0.5);
            ctx.cylinder_along_x(0.125, distance, alpha(color, 1.0));
            return;
        }
        _ => {}
    }
    let Some(adornee) = dm.get_ref(id, "Adornee") else {
        return;
    };
    match class {
        "SelectionBox" => {
            let Some((cf, size)) = part_box(dm, adornee) else {
                return;
            };
            if transparency < 1.0 {
                selection_box(ctx, cf, size, rgba, get_f32(dm, id, "LineThickness"));
            }
            let surface_t = get_f32(dm, id, "SurfaceTransparency");
            if surface_t < 1.0 {
                ctx.cframe = cf;
                ctx.aabox(
                    -size * 0.5,
                    size * 0.5,
                    alpha(color3(dm, id, "SurfaceColor3"), 1.0 - surface_t),
                );
            }
        }
        "SelectionSphere" => {
            let Some((cf, size)) = part_box(dm, adornee) else {
                return;
            };
            let radius = size.x.min(size.y).min(size.z) * 0.5;
            ctx.cframe = cf;
            if transparency < 1.0 {
                ctx.material = Material::Outline;
                ctx.sphere(Vec3::ZERO, radius + 0.2, rgba);
            }
            let surface_t = get_f32(dm, id, "SurfaceTransparency");
            if surface_t < 1.0 {
                ctx.material = Material::NoLighting;
                ctx.sphere(
                    Vec3::ZERO,
                    radius,
                    alpha(color3(dm, id, "SurfaceColor3"), 1.0 - surface_t),
                );
            }
        }
        "SurfaceSelection" => {
            if !dm.is_a(adornee, "BasePart") {
                return;
            }
            let Some((cf, size)) = part_box(dm, adornee) else {
                return;
            };
            ctx.cframe = cf;
            let surface = get_i64(dm, id, "TargetSurface").clamp(0, 5) as usize;
            surface_border(ctx, size * 0.5, 0.2, surface, rgba);
        }
        "Handles" | "ArcHandles" => {
            if !dm.is_a(adornee, "BasePart") {
                return;
            }
            let Some((cf, size)) = part_box(dm, adornee) else {
                return;
            };
            let (kind, mask) = if class == "ArcHandles" {
                let axes = match dm.get(id, "Axes") {
                    Ok(Variant::Axes(a)) => a.0,
                    _ => 7,
                };
                (HandleType::Rotate, (axes & 7) | ((axes & 7) << 3))
            } else {
                let faces = match dm.get(id, "Faces") {
                    Ok(Variant::Faces(f)) => f.0,
                    _ => 63,
                };
                let kind = if get_i64(dm, id, "Style") == 1 {
                    HandleType::Move
                } else {
                    HandleType::Resize
                };
                (kind, faces)
            };
            handles(ctx, size, cf, kind, rgba, mask, false, None, rgba);
        }
        "BoxHandleAdornment"
        | "ConeHandleAdornment"
        | "CylinderHandleAdornment"
        | "SphereHandleAdornment"
        | "LineHandleAdornment"
        | "ImageHandleAdornment" => {
            let Some(frame) = handle_frame(dm, id, adornee) else {
                return;
            };
            let z_index = get_i64(dm, id, "ZIndex").clamp(-1, 10) as i32;
            let on_top = get_bool(dm, id, "AlwaysOnTop", false);
            let (shape, material, translation, rotation, scale, thickness) = match class {
                "BoxHandleAdornment" => (
                    Shape::Box,
                    Material::NoLighting,
                    frame.translation,
                    frame.rotation,
                    vec3(dm, id, "Size") / 2.0,
                    0.0,
                ),
                "ConeHandleAdornment" => {
                    let r = get_f32(dm, id, "Radius");
                    (
                        Shape::Cone,
                        Material::NoLighting,
                        frame.translation,
                        frame.rotation,
                        Vec3::new(get_f32(dm, id, "Height"), r, r),
                        0.0,
                    )
                }
                "CylinderHandleAdornment" => {
                    let r = get_f32(dm, id, "Radius");
                    (
                        Shape::Cylinder,
                        Material::NoLighting,
                        frame.translation,
                        frame.rotation,
                        Vec3::new(get_f32(dm, id, "Height") / 2.0, r, r),
                        0.0,
                    )
                }
                "SphereHandleAdornment" => (
                    Shape::Sphere,
                    Material::NoLighting,
                    frame.translation,
                    frame.rotation,
                    Vec3::splat(get_f32(dm, id, "Radius")),
                    0.0,
                ),
                "LineHandleAdornment" => {
                    let start = frame.translation;
                    let end = start + frame.look_vector() * get_f32(dm, id, "Length");
                    if (end - start).length() <= 0.0 {
                        return;
                    }
                    (
                        Shape::Line,
                        Material::AaLine,
                        (start + end) * 0.5,
                        frame_from_x(start - end),
                        Vec3::new((end - start).length() * 0.5, 1.0, 1.0),
                        get_f32(dm, id, "Thickness"),
                    )
                }
                _ => {
                    let size = get_vec2(dm, id, "Size");
                    let image = get_string(dm, id, "Image");
                    if image.trim().is_empty() {
                        return;
                    }
                    ctx.out.meshes.push(AdornMesh {
                        shape: Shape::Quad,
                        material: Material::NoLighting,
                        rotation: scaled(
                            frame.rotation,
                            Vec3::new(size.x / 2.0, size.y / 2.0, 1.0),
                        ),
                        translation: frame.translation,
                        color: rgba,
                        thickness: 0.0,
                        z_index,
                        always_on_top: on_top && z_index >= 0,
                        texture: Some(image),
                    });
                    return;
                }
            };
            ctx.submit(
                shape,
                material,
                translation,
                rotation,
                scale,
                rgba,
                thickness,
                z_index,
                on_top,
            );
        }
        _ => {}
    }
}

fn gather(dm: &DataModel, id: InstanceId, out: &mut Vec<InstanceId>) {
    for &c in dm.children_of(id) {
        if dm.is_a(c, "GuiBase3d") {
            out.push(c);
        }
        gather(dm, c, out);
    }
}

pub fn extract(dm: &DataModel, camera: Vec3) -> Adorns {
    let mut ids = Vec::new();
    if let Some(ws) = dm.find_service("Workspace") {
        gather(dm, ws, &mut ids);
    }
    if let Some(pg) = dm
        .local_player
        .and_then(|p| dm.find_first_child_of_class(p, "PlayerGui"))
    {
        gather(dm, pg, &mut ids);
    }
    if let Some(core) = dm.find_service("CoreGui") {
        gather(dm, core, &mut ids);
    }
    let mut out = Adorns::default();
    let mut ctx = Ctx {
        out: &mut out,
        camera,
        cframe: CFrame::IDENTITY,
        material: Material::Lit,
    };
    for id in ids {
        adornment(dm, id, &mut ctx);
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayHandle {
    Resize,
    Move,
    Rotate,
}

impl OverlayHandle {
    fn kind(self) -> HandleType {
        match self {
            OverlayHandle::Resize => HandleType::Resize,
            OverlayHandle::Move => HandleType::Move,
            OverlayHandle::Rotate => HandleType::Rotate,
        }
    }
}

pub fn overlay_selection_box(out: &mut Adorns, camera: Vec3, cf: CFrame, size: Vec3, color: [f32; 4], thickness: f32) {
    let mut ctx = Ctx {
        out,
        camera,
        cframe: cf,
        material: Material::Lit,
    };
    selection_box(&mut ctx, cf, size, color, thickness);
}

pub fn overlay_outline_box(out: &mut Adorns, camera: Vec3, lo: Vec3, hi: Vec3, color: [f32; 4]) {
    let mut ctx = Ctx {
        out,
        camera,
        cframe: CFrame::IDENTITY,
        material: Material::Lit,
    };
    ctx.outline_box(lo, hi, color);
}

#[allow(clippy::too_many_arguments)]
pub fn overlay_handles(
    out: &mut Adorns,
    camera: Vec3,
    size: Vec3,
    position: CFrame,
    kind: OverlayHandle,
    color: [f32; 4],
    use_axis_color: bool,
    mask: u8,
    highlight: Option<usize>,
    highlight_color: [f32; 4],
) {
    let mut ctx = Ctx {
        out,
        camera,
        cframe: position,
        material: Material::SelfLit,
    };
    handles(&mut ctx, size, position, kind.kind(), color, mask, use_axis_color, highlight, highlight_color);
}

pub fn overlay_star(out: &mut Adorns, camera: Vec3, center: Vec3, radius: f32, color: [f32; 4]) {
    let mut ctx = Ctx {
        out,
        camera,
        cframe: CFrame::from_translation(center),
        material: Material::SelfLit,
    };
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        ctx.ray(-axis * radius, axis * radius * 2.0, color);
    }
}

pub fn handle_position(camera: Vec3, size: Vec3, kind: OverlayHandle, id: usize) -> Vec3 {
    let half = size / 2.0;
    let _ = camera;
    match kind {
        OverlayHandle::Rotate => normal_vector(normal_to_u(id)) * (half.length() + HANDLE_OFFSET * 4.0),
        OverlayHandle::Resize => normal_vector(id) * (half + Vec3::splat(HANDLE_OFFSET * 4.0)),
        OverlayHandle::Move => normal_vector(id) * (half + Vec3::splat(HANDLE_OFFSET)),
    }
}

pub fn handle_radius(camera: Vec3, kind: OverlayHandle, world: Vec3) -> f32 {
    match kind {
        OverlayHandle::Move => scale_to_camera(camera, world, ARROW_MIN_ANGLE, ARROW_LENGTH),
        _ => scale_to_camera(camera, world, SPHERE_MIN_ANGLE, SPHERE_RADIUS),
    }
}

pub fn handle_tip_radius(camera: Vec3, world: Vec3) -> f32 {
    scale_to_camera(camera, world, ARROW_MIN_ANGLE, ARROW_LENGTH - 0.5)
}

pub fn normal_vector_of(id: usize) -> Vec3 {
    normal_vector(id)
}
