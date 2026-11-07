use crate::DataModel;
use rbx_math::{UDim2, Vec2};
use rbx_reflection::{InstanceId, Variant};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuiView {
    pub screen: Vec2,
    pub inset: [f32; 4],
}

impl Default for GuiView {
    fn default() -> Self {
        GuiView {
            screen: Vec2::new(800.0, 600.0),
            inset: [0.0; 4],
        }
    }
}

impl GuiView {
    pub fn user_rect(&self) -> Placement {
        Placement {
            position: Vec2::new(self.inset[0], self.inset[1]),
            size: Vec2::new(
                (self.screen.x - self.inset[0] - self.inset[2]).max(0.0),
                (self.screen.y - self.inset[1] - self.inset[3]).max(0.0),
            ),
            rotation: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Placement {
    pub position: Vec2,
    pub size: Vec2,
    pub rotation: f32,
}

impl Placement {
    pub fn rounded_position(&self) -> Vec2 {
        Vec2::new(self.position.x.round(), self.position.y.round())
    }

    pub fn rounded_size(&self) -> Vec2 {
        Vec2::new(self.size.x.round(), self.size.y.round())
    }
}

fn udim2(dm: &DataModel, id: InstanceId, name: &str) -> UDim2 {
    match dm.get(id, name) {
        Ok(Variant::UDim2(u)) => u,
        _ => UDim2::default(),
    }
}

fn vec2(dm: &DataModel, id: InstanceId, name: &str) -> Vec2 {
    match dm.get(id, name) {
        Ok(Variant::Vector2(v)) => v,
        _ => Vec2::ZERO,
    }
}

fn number(dm: &DataModel, id: InstanceId, name: &str) -> f32 {
    match dm.get(id, name) {
        Ok(Variant::Float(v)) => v,
        Ok(Variant::Double(v)) => v as f32,
        Ok(Variant::Int(v)) => v as f32,
        _ => 0.0,
    }
}

fn enum_value(dm: &DataModel, id: InstanceId, name: &str) -> u32 {
    match dm.get(id, name) {
        Ok(Variant::Enum(v)) => v,
        _ => 0,
    }
}

fn resolve(u: UDim2, size: Vec2) -> Vec2 {
    Vec2::new(
        u.x.scale * size.x + u.x.offset as f32,
        u.y.scale * size.y + u.y.offset as f32,
    )
}

fn rotate(p: Vec2, center: Vec2, degrees: f32) -> Vec2 {
    if degrees == 0.0 {
        return p;
    }
    let (s, c) = degrees.to_radians().sin_cos();
    let d = p - center;
    center + Vec2::new(d.x * c - d.y * s, d.x * s + d.y * c)
}

pub fn gui_parent(dm: &DataModel, id: InstanceId) -> Option<InstanceId> {
    let mut p = dm.parent_of(id)?;
    loop {
        if dm.is_a(p, "GuiBase2d") {
            return Some(p);
        }
        if dm.class_of(p) != Some("Folder") {
            return None;
        }
        p = dm.parent_of(p)?;
    }
}

pub fn canvas(dm: &DataModel, id: InstanceId, own: Placement) -> Placement {
    if dm.class_of(id) == Some("ScrollingFrame") {
        let basis = gui_parent(dm, id)
            .map(|p| placement(dm, p).size)
            .unwrap_or(own.size);
        let canvas_size = resolve(udim2(dm, id, "CanvasSize"), basis).max(own.size);
        let offset = vec2(dm, id, "CanvasPosition");
        return Placement {
            position: own.position - offset,
            size: canvas_size,
            rotation: own.rotation,
        };
    }
    own
}

pub fn placement(dm: &DataModel, id: InstanceId) -> Placement {
    let view = dm.gui_view;
    let class = dm.class_of(id).unwrap_or("");
    match class {
        "ScreenGui" | "GuiMain" => return view.user_rect(),
        "BillboardGui" => {
            let size = udim2(dm, id, "Size");
            return Placement {
                position: Vec2::ZERO,
                size: Vec2::new(size.x.offset as f32, size.y.offset as f32),
                rotation: 0.0,
            };
        }
        "SurfaceGui" => {
            return Placement {
                position: Vec2::ZERO,
                size: vec2(dm, id, "CanvasSize"),
                rotation: 0.0,
            };
        }
        _ => {}
    }
    if !dm.is_a(id, "GuiObject") {
        return Placement::default();
    }
    let Some(parent) = gui_parent(dm, id) else {
        return Placement::default();
    };
    let parent_placement = placement(dm, parent);
    let parent_canvas = canvas(dm, parent, parent_placement);
    place_child(dm, id, parent_canvas)
}

pub fn place_child(dm: &DataModel, id: InstanceId, parent: Placement) -> Placement {
    let constrained = match enum_value(dm, id, "SizeConstraint") {
        1 => Vec2::new(parent.size.x, parent.size.x),
        2 => Vec2::new(parent.size.y, parent.size.y),
        _ => parent.size,
    };
    let size = resolve(udim2(dm, id, "Size"), constrained);
    let local = parent.position + resolve(udim2(dm, id, "Position"), parent.size);
    let parent_center = parent.position + parent.size * 0.5;
    let center = rotate(local + size * 0.5, parent_center, parent.rotation);
    Placement {
        position: center - size * 0.5,
        size,
        rotation: parent.rotation + number(dm, id, "Rotation"),
    }
}

pub fn absolute_position(dm: &DataModel, id: InstanceId) -> Vec2 {
    let p = placement(dm, id).rounded_position();
    if dm.is_a(id, "GuiObject") || dm.class_of(id) == Some("ScreenGui") {
        p - Vec2::new(dm.gui_view.inset[0], dm.gui_view.inset[1])
    } else {
        p
    }
}

pub fn absolute_size(dm: &DataModel, id: InstanceId) -> Vec2 {
    placement(dm, id).rounded_size()
}

pub fn absolute_window_size(dm: &DataModel, id: InstanceId) -> Vec2 {
    let own = placement(dm, id);
    let canvas = canvas(dm, id, own);
    let thickness = number(dm, id, "ScrollBarThickness");
    let mut size = own.rounded_size();
    if canvas.size.y > own.size.y {
        size.x -= thickness;
    }
    if canvas.size.x > own.size.x {
        size.y -= thickness;
    }
    size.max(Vec2::ZERO)
}
