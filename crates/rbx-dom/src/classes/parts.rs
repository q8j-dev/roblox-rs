use crate::behavior::{Behavior, SetOutcome};
use crate::datamodel::DataModel;
use rbx_math::{BrickColor, CFrame, Mat3, Vec3};
use rbx_reflection::{InstanceId, Variant};
use std::collections::HashMap;

pub struct BasePartBehavior;

fn cframe(dm: &DataModel, id: InstanceId) -> CFrame {
    dm.get_raw(id, "CFrame")
        .and_then(|v| v.as_cframe())
        .unwrap_or_default()
}

fn sanitize(cf: CFrame) -> CFrame {
    if !cf.is_finite() {
        CFrame::from_translation(Vec3::new(0.0, -f32::MAX, 0.0))
    } else {
        cf.orthonormalized_if_needed()
    }
}

trait OrthoIfNeeded {
    fn orthonormalized_if_needed(self) -> Self;
}

impl OrthoIfNeeded for CFrame {
    fn orthonormalized_if_needed(self) -> CFrame {
        let m = self.rotation;
        let err = (m.x_axis.length_squared() - 1.0).abs()
            + (m.y_axis.length_squared() - 1.0).abs()
            + (m.z_axis.length_squared() - 1.0).abs()
            + m.x_axis.dot(m.y_axis).abs()
            + m.y_axis.dot(m.z_axis).abs()
            + m.x_axis.dot(m.z_axis).abs();
        if err > 1e-5 {
            self.orthonormalized()
        } else {
            self
        }
    }
}

impl Behavior for BasePartBehavior {
    fn computed(&self) -> &'static [&'static str] {
        &[
            "Position",
            "Rotation",
            "Color",
            "CoordinateFrame",
            "SpecificGravity",
        ]
    }

    fn get(&self, dm: &DataModel, id: InstanceId, prop: &str) -> Option<Variant> {
        match prop {
            "Position" => Some(Variant::Vector3(cframe(dm, id).translation)),
            "CoordinateFrame" => Some(Variant::CFrame(cframe(dm, id))),
            "Rotation" => {
                let (x, y, z) = cframe(dm, id).to_euler_xyz();
                Some(Variant::Vector3(Vec3::new(
                    x.to_degrees(),
                    y.to_degrees(),
                    z.to_degrees(),
                )))
            }
            "Color" => Some(Variant::Color3(
                dm.get_raw(id, "BrickColor")
                    .and_then(|v| v.as_brick_color())
                    .unwrap_or_default()
                    .color3(),
            )),
            "SpecificGravity" => Some(Variant::Float(specific_gravity(
                dm.get_raw(id, "Material")
                    .and_then(|v| v.as_enum())
                    .unwrap_or(256),
            ))),
            _ => None,
        }
    }

    fn set(&self, dm: &mut DataModel, id: InstanceId, prop: &str, value: &Variant) -> SetOutcome {
        match prop {
            "CFrame" | "CoordinateFrame" => {
                let Some(cf) = value.as_cframe() else {
                    return SetOutcome::Unhandled;
                };
                let target = dm
                    .with_world_hooks(|h, dm| h.mechanism_root_for_script(dm, id))
                    .unwrap_or(id);
                let cf = sanitize(cf);
                if cframe(dm, target) != cf {
                    dm.set_unchecked(target, "CFrame", Variant::CFrame(cf));
                    dm.notify_changed(target, "Position");
                    dm.notify_changed(target, "Rotation");
                    dm.with_world_hooks(|h, dm| h.pv_changed_from_reflection(dm, target));
                }
                SetOutcome::Handled
            }
            "Position" => {
                let Some(p) = value.as_vec3() else {
                    return SetOutcome::Unhandled;
                };
                let mut cf = cframe(dm, id);
                if cf.translation != p {
                    cf.translation = p;
                    self.move_with_joints(dm, id, cf);
                }
                SetOutcome::Handled
            }
            "Rotation" => {
                let Some(r) = value.as_vec3() else {
                    return SetOutcome::Unhandled;
                };
                let mut cf = cframe(dm, id);
                let rot = Mat3::from_rotation_x(r.x.to_radians())
                    * Mat3::from_rotation_y(r.y.to_radians())
                    * Mat3::from_rotation_z(r.z.to_radians());
                if rot != cf.rotation {
                    cf.rotation = rot;
                    self.move_with_joints(dm, id, cf);
                }
                SetOutcome::Handled
            }
            "Color" => {
                let Some(c) = value.as_color3() else {
                    return SetOutcome::Unhandled;
                };
                let b = BrickColor::closest(c);
                dm.set_unchecked(id, "BrickColor", Variant::BrickColor(b));
                dm.notify_changed(id, "Color");
                SetOutcome::Handled
            }
            "Size" => {
                let Some(s) = value.as_vec3() else {
                    return SetOutcome::Unhandled;
                };
                dm.with_world_hooks(|h, dm| h.destroy_joints(dm, id));
                let clamped = clamp_size(dm, id, s);
                let current = dm.get_raw(id, "size").and_then(|v| v.as_vec3());
                if Some(clamped) != current {
                    set_size_xml(dm, id, clamped);
                } else if clamped != s {
                    dm.notify_changed(id, "Size");
                }
                dm.with_world_hooks(|h, dm| h.safe_move(dm, id));
                dm.with_world_hooks(|h, dm| h.join(dm, id));
                SetOutcome::Handled
            }
            "FormFactor" | "formFactor" if dm.is_a(id, "FormFactorPart") => {
                let Some(mut ff) = value.as_enum() else {
                    return SetOutcome::Unhandled;
                };
                if !has_three_dimensional_size(dm, id) {
                    ff = 0;
                }
                dm.standard_out(
                    crate::datamodel::MessageLevel::Warning,
                    "FormFactor is deprecated. You should no longer use this property.",
                );
                if form_factor(dm, id) != ff {
                    dm.with_world_hooks(|h, dm| h.destroy_joints(dm, id));
                    set_form_factor_xml(dm, id, ff);
                    if ff != 3 {
                        let mut ui = size_xml(dm, id);
                        ui.y = ui.y.round().max(1.0);
                        let rbx = ui_to_xml(dm, id, ui);
                        set_size_xml(dm, id, rbx);
                        dm.with_world_hooks(|h, dm| h.safe_move(dm, id));
                    }
                    dm.with_world_hooks(|h, dm| h.join(dm, id));
                }
                SetOutcome::Handled
            }
            "formFactorRaw" if dm.is_a(id, "FormFactorPart") => {
                let Some(ff) = value.as_enum() else {
                    return SetOutcome::Unhandled;
                };
                set_form_factor_xml(dm, id, ff);
                SetOutcome::Handled
            }
            "Shape" if dm.class_of(id) == Some("Part") => {
                let Some(shape) = value.as_enum() else {
                    return SetOutcome::Unhandled;
                };
                let current = dm
                    .get_raw(id, "shape")
                    .and_then(|v| v.as_enum())
                    .unwrap_or(1);
                if current != shape {
                    dm.with_world_hooks(|h, dm| h.destroy_joints(dm, id));
                    set_shape_xml(dm, id, shape);
                    if !has_three_dimensional_size(dm, id) {
                        let ui = size_xml(dm, id);
                        let rbx = ui_to_xml(dm, id, ui);
                        set_size_xml(dm, id, rbx);
                        dm.with_world_hooks(|h, dm| h.safe_move(dm, id));
                    }
                    dm.with_world_hooks(|h, dm| h.join(dm, id));
                }
                SetOutcome::Handled
            }
            "shape" if dm.class_of(id) == Some("Part") => {
                let Some(shape) = value.as_enum() else {
                    return SetOutcome::Unhandled;
                };
                set_shape_xml(dm, id, shape);
                SetOutcome::Handled
            }
            "size" | "siz" => {
                let Some(s) = value.as_vec3() else {
                    return SetOutcome::Unhandled;
                };
                set_size_xml(dm, id, s);
                SetOutcome::Handled
            }
            "Velocity" | "RotVelocity" => {
                let target = dm
                    .with_world_hooks(|h, dm| h.mechanism_root_for_script(dm, id))
                    .unwrap_or(id);
                if target != id {
                    dm.set_unchecked(target, prop, value.clone());
                    dm.with_world_hooks(|h, dm| h.pv_changed_from_reflection(dm, target));
                    return SetOutcome::Handled;
                }
                SetOutcome::Unhandled
            }
            _ => SetOutcome::Unhandled,
        }
    }

    fn changed(&self, dm: &mut DataModel, id: InstanceId, prop: &'static str) {
        match prop {
            "BrickColor" => dm.notify_changed(id, "Color"),
            "size" => dm.notify_changed(id, "Size"),
            "Velocity" | "RotVelocity" => {
                dm.with_world_hooks(|h, dm| h.pv_changed_from_reflection(dm, id));
            }
            _ => {}
        }
    }
}

impl BasePartBehavior {
    fn move_with_joints(&self, dm: &mut DataModel, id: InstanceId, cf: CFrame) {
        dm.with_world_hooks(|h, dm| h.destroy_joints(dm, id));
        let cf = sanitize(cf);
        dm.set_unchecked(id, "CFrame", Variant::CFrame(cf));
        dm.notify_changed(id, "Position");
        dm.notify_changed(id, "Rotation");
        dm.with_world_hooks(|h, dm| h.pv_changed_from_reflection(dm, id));
        dm.with_world_hooks(|h, dm| h.safe_move(dm, id));
        dm.with_world_hooks(|h, dm| h.join(dm, id));
    }
}

fn has_three_dimensional_size(dm: &DataModel, id: InstanceId) -> bool {
    if dm.class_of(id) == Some("Part") {
        let shape = dm
            .get_raw(id, "shape")
            .and_then(|v| v.as_enum())
            .unwrap_or(1);
        return shape != 0;
    }
    true
}

fn set_size_xml(dm: &mut DataModel, id: InstanceId, size: Vec3) {
    let size = if has_three_dimensional_size(dm, id) {
        size
    } else {
        Vec3::splat(size.x)
    };
    let current = dm.get_raw(id, "size").and_then(|v| v.as_vec3());
    if current != Some(size) {
        dm.set_unchecked(id, "size", Variant::Vector3(size));
    }
}

fn form_factor(dm: &DataModel, id: InstanceId) -> u32 {
    dm.get_raw(id, "formFactorRaw")
        .and_then(|v| v.as_enum())
        .unwrap_or(1)
}

fn size_xml(dm: &DataModel, id: InstanceId) -> Vec3 {
    dm.get_raw(id, "size")
        .and_then(|v| v.as_vec3())
        .unwrap_or(Vec3::new(4.0, 1.2, 2.0))
}

fn set_form_factor_xml(dm: &mut DataModel, id: InstanceId, ff: u32) {
    if form_factor(dm, id) != ff {
        dm.set_unchecked(id, "formFactorRaw", Variant::Enum(ff));
        dm.notify_changed(id, "FormFactor");
    }
}

fn set_shape_xml(dm: &mut DataModel, id: InstanceId, shape: u32) {
    let current = dm
        .get_raw(id, "shape")
        .and_then(|v| v.as_enum())
        .unwrap_or(1);
    if current == shape {
        return;
    }
    dm.set_unchecked(id, "shape", Variant::Enum(shape));
    dm.notify_changed(id, "Shape");
    if !has_three_dimensional_size(dm, id) {
        set_form_factor_xml(dm, id, 0);
    }
}

fn ui_to_xml(dm: &DataModel, id: InstanceId, ui: Vec3) -> Vec3 {
    if !dm.is_a(id, "FormFactorPart") {
        return ui.max(min_size(dm, id));
    }
    let round = |v: Vec3| Vec3::new(v.x.round(), v.y.round(), v.z.round());
    match form_factor(dm, id) {
        1 => {
            let g = round(ui.max(Vec3::ONE));
            Vec3::new(g.x, g.y * 1.2, g.z)
        }
        2 => {
            let g = round(ui.max(Vec3::ONE));
            Vec3::new(g.x, g.y * 0.4, g.z)
        }
        0 => round(ui.max(Vec3::ONE)),
        _ => ui.max(min_size(dm, id)),
    }
}

pub fn min_size(dm: &DataModel, id: InstanceId) -> Vec3 {
    let class = dm.class_of(id).unwrap_or("");
    if dm.is_a(id, "FormFactorPart") {
        let ff = dm
            .get_raw(id, "formFactorRaw")
            .and_then(|v| v.as_enum())
            .unwrap_or(1);
        return match ff {
            0 => Vec3::ONE,
            1 => Vec3::new(1.0, 1.2, 1.0),
            2 => Vec3::new(1.0, 0.4, 1.0),
            _ => Vec3::splat(0.2),
        };
    }
    if class == "TrussPart" {
        return Vec3::splat(2.0);
    }
    Vec3::splat(0.2)
}

pub fn clamp_size(dm: &DataModel, id: InstanceId, ui: Vec3) -> Vec3 {
    let min = min_size(dm, id);
    let mut s = ui.max(min);
    if dm.is_a(id, "FormFactorPart") {
        let ff = dm
            .get_raw(id, "formFactorRaw")
            .and_then(|v| v.as_enum())
            .unwrap_or(1);
        if let 0..=2 = ff {
            let y_grid = if ff == 2 {
                0.4
            } else if ff == 1 {
                1.2
            } else {
                1.0
            };
            s.x = s.x.round().max(1.0);
            s.z = s.z.round().max(1.0);
            s.y = ((s.y / y_grid).round() * y_grid).max(y_grid.min(min.y));
            if ff == 0 {
                s.y = s.y.round().max(1.0);
            }
        }
    }
    if dm.class_of(id) == Some("TrussPart") {
        s = Vec3::new(2.0, (s.y / 2.0).round().max(1.0) * 2.0, 2.0);
    }
    if !has_three_dimensional_size(dm, id) {
        s = Vec3::splat(s.x);
    }
    s
}

impl DataModel {
    pub fn convert_to_new_physical_properties(&mut self) {
        let root = self.root();
        let parts: Vec<InstanceId> = self
            .descendants(root)
            .into_iter()
            .filter(|&d| self.is_a(d, "BasePart") && !self.is_a(d, "Terrain"))
            .collect();
        for id in parts {
            let custom = matches!(
                self.get(id, "CustomPhysicalProperties"),
                Ok(Variant::PhysicalProperties(p)) if p.custom.is_some()
            );
            if custom {
                continue;
            }
            let f = |dm: &DataModel, n: &str, d: f32| {
                dm.get(id, n).ok().and_then(|v| v.as_f32()).unwrap_or(d)
            };
            let friction = f(self, "Friction", 0.3);
            let elasticity = f(self, "Elasticity", 0.5);
            if (friction - 0.3).abs() <= f32::EPSILON && (elasticity - 0.5).abs() <= f32::EPSILON {
                continue;
            }
            let material = self
                .get(id, "Material")
                .ok()
                .and_then(|v| v.as_enum())
                .unwrap_or(256);
            let density = rbx_math::materials::material_physics(material).density;
            let props =
                rbx_math::PhysicalProperties::custom(density, friction, elasticity, 1.0, 1.0);
            self.set_unchecked(
                id,
                "CustomPhysicalProperties",
                Variant::PhysicalProperties(props),
            );
            self.notify_changed(id, "CustomPhysicalProperties");
        }
    }
}

pub struct PartBehavior;

impl Behavior for PartBehavior {
    fn aliases(&self) -> &'static [(&'static str, &'static str)] {
        &[("shap", "shape")]
    }
}

pub fn register(map: &mut HashMap<&'static str, &'static dyn Behavior>) {
    map.insert("BasePart", &BasePartBehavior);
    map.insert("Part", &PartBehavior);
}

pub fn specific_gravity(material: u32) -> f32 {
    match material {
        0xFFFF | 0x0100 | 0x0110 | 0x0120 => 0.7,
        0x0200 | 0x0210 => 0.35,
        0x0310 => 2.563,
        0x0320 | 0x0340 | 0x0370 | 0x0380 | 0x0390 | 0x0314 | 0x0324 => 2.691,
        0x0330 | 0x0360 => 2.403,
        0x0350 => 1.922,
        0x0410 | 0x0420 | 0x0440 => 7.85,
        0x0430 => 7.7,
        0x0500 | 0x0530 | 0x0540 | 0x0550 => 0.9,
        0x0510 => 1.602,
        0x0520 => 0.7,
        0x0600 | 0x0610 => 0.919,
        0x0700 => 0.0,
        0x0800 => 1.0,
        _ => 1.0,
    }
}
