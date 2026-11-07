use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::Vec3;

pub const COMPOSITE_WIDTH: u32 = 1024;
pub const COMPOSITE_HEIGHT: u32 = 512;
const CANVAS_EXTRA: f32 = 1024.0 * 256.0 / (1024.0 - 256.0);
pub const CANVAS_WIDTH: f32 = 1024.0 + CANVAS_EXTRA;
pub const CANVAS_HEIGHT: f32 = 512.0;
const BASE_WIDTH: f32 = 1024.0 / CANVAS_WIDTH;
const EXTRA_WIDTH: f32 = CANVAS_EXTRA / CANVAS_WIDTH;
const HEAD_SLOT: [f32; 4] = [BASE_WIDTH + 0.5 * EXTRA_WIDTH, 0.0, 0.5 * EXTRA_WIDTH, 0.25];
const BORDER_W: f32 = 8.0 / CANVAS_WIDTH;
const BORDER_H: f32 = 8.0 / CANVAS_HEIGHT;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerMode {
    Color,
    Blend,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompositeLayer {
    pub mesh: String,
    pub color: [f32; 3],
    pub texture: Option<String>,
    pub mode: LayerMode,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompositeDesc {
    pub url: String,
    pub layers: Vec<CompositeLayer>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyPart {
    Head,
    Torso,
    LeftArm,
    RightArm,
    LeftLeg,
    RightLeg,
}

#[derive(Clone, Debug, Default)]
struct CharacterMesh {
    mesh: i64,
    base: i64,
    overlay: i64,
}

#[derive(Clone, Debug, Default)]
pub struct Character {
    head: Option<InstanceId>,
    torso: Option<InstanceId>,
    left_arm: Option<InstanceId>,
    right_arm: Option<InstanceId>,
    left_leg: Option<InstanceId>,
    right_leg: Option<InstanceId>,
    shirt: Option<String>,
    pants: Option<String>,
    graphic: Option<String>,
    meshes: [Option<CharacterMesh>; 6],
}

fn content(dm: &DataModel, id: InstanceId, name: &str) -> Option<String> {
    match dm.get(id, name) {
        Ok(Variant::Content(s)) | Ok(Variant::String(s)) if !s.trim().is_empty() => Some(s),
        _ => None,
    }
}

fn int(dm: &DataModel, id: InstanceId, name: &str) -> i64 {
    match dm.get(id, name) {
        Ok(Variant::Int(v)) => v as i64,
        Ok(Variant::Int64(v)) => v,
        Ok(Variant::Enum(v)) => v as i64,
        _ => 0,
    }
}

fn color(dm: &DataModel, id: InstanceId) -> [f32; 3] {
    match dm.get(id, "BrickColor") {
        Ok(Variant::BrickColor(b)) => {
            let c = b.color3();
            [c.r, c.g, c.b]
        }
        _ => match dm.get(id, "Color") {
            Ok(Variant::Color3(c)) => [c.r, c.g, c.b],
            _ => [0.64, 0.64, 0.64],
        },
    }
}

fn asset_url(id: i64) -> Option<String> {
    (id > 0).then(|| format!("rbxassetid://{id}"))
}

pub fn identify(dm: &DataModel, model: InstanceId) -> Option<Character> {
    dm.find_first_child_of_class(model, "Humanoid")?;
    let mut ch = Character::default();
    for &c in dm.children_of(model) {
        let class = dm.class_of(c).unwrap_or("");
        if dm.is_a(c, "BasePart") {
            match dm.name_of(c) {
                "Head" => ch.head = Some(c),
                "Torso" => ch.torso = Some(c),
                "Left Arm" => ch.left_arm = Some(c),
                "Right Arm" => ch.right_arm = Some(c),
                "Left Leg" => ch.left_leg = Some(c),
                "Right Leg" => ch.right_leg = Some(c),
                _ => {}
            }
        } else if dm.is_a(c, "Clothing") {
            let (pants, shirt) = match class {
                "Pants" => (content(dm, c, "PantsTemplate"), None),
                "Shirt" => (None, content(dm, c, "ShirtTemplate")),
                _ => (content(dm, c, "Outfit1"), content(dm, c, "Outfit2")),
            };
            if ch.pants.is_none() {
                ch.pants = pants;
            }
            if ch.shirt.is_none() {
                ch.shirt = shirt;
            }
        } else if class == "ShirtGraphic" {
            if ch.graphic.is_none() {
                ch.graphic = content(dm, c, "Graphic");
            }
        } else if class == "CharacterMesh" {
            let part = int(dm, c, "BodyPart");
            if (1..=5).contains(&part) {
                ch.meshes[part as usize] = Some(CharacterMesh {
                    mesh: int(dm, c, "MeshId"),
                    base: int(dm, c, "BaseTextureId"),
                    overlay: int(dm, c, "OverlayTextureId"),
                });
            }
        }
    }
    Some(ch)
}

impl Character {
    pub fn body_part(&self, part: InstanceId) -> Option<BodyPart> {
        let p = Some(part);
        if p == self.head {
            Some(BodyPart::Head)
        } else if p == self.torso {
            Some(BodyPart::Torso)
        } else if p == self.left_arm {
            Some(BodyPart::LeftArm)
        } else if p == self.right_arm {
            Some(BodyPart::RightArm)
        } else if p == self.left_leg {
            Some(BodyPart::LeftLeg)
        } else if p == self.right_leg {
            Some(BodyPart::RightLeg)
        } else {
            None
        }
    }

    fn mesh_of(&self, part: BodyPart) -> Option<&CharacterMesh> {
        let i = match part {
            BodyPart::Head => return None,
            BodyPart::Torso => 1,
            BodyPart::LeftArm => 2,
            BodyPart::RightArm => 3,
            BodyPart::LeftLeg => 4,
            BodyPart::RightLeg => 5,
        };
        self.meshes[i].as_ref()
    }

    pub fn has_graphic(&self) -> bool {
        self.graphic.is_some()
    }

    pub fn is_head(&self, dm: &DataModel, part: InstanceId) -> bool {
        if Some(part) != self.head {
            return false;
        }
        let has_face = dm.children_of(part).iter().any(|c| dm.is_a(*c, "Decal"));
        for &c in dm.children_of(part) {
            match dm.class_of(c) {
                Some("SpecialMesh") => {
                    return match int(dm, c, "MeshType") {
                        0 | 5 => true,
                        3 => has_face,
                        _ => false,
                    };
                }
                Some("FileMesh") => return true,
                Some(_) if dm.is_a(c, "DataModelMesh") => return has_face,
                _ => {}
            }
        }
        false
    }

    pub fn is_composited(
        &self,
        dm: &DataModel,
        part: InstanceId,
        block: bool,
        has_mesh: bool,
    ) -> bool {
        let Some(kind) = self.body_part(part) else {
            return false;
        };
        let reflectance = match dm.get(part, "Reflectance") {
            Ok(Variant::Float(r)) => r,
            _ => 0.0,
        };
        let transparency = match dm.get(part, "Transparency") {
            Ok(Variant::Float(t)) => t,
            _ => 0.0,
        };
        let no_reflectance = reflectance <= 0.015;
        if kind == BodyPart::Head {
            return transparency <= 0.0 && self.is_head(dm, part) && no_reflectance;
        }
        if has_mesh || !block {
            return false;
        }
        if self.mesh_of(kind).is_some() {
            return true;
        }
        match kind {
            BodyPart::LeftArm | BodyPart::RightArm if self.shirt.is_some() => return true,
            BodyPart::LeftLeg | BodyPart::RightLeg if self.pants.is_some() => return true,
            BodyPart::Torso
                if self.pants.is_some() || self.shirt.is_some() || self.graphic.is_some() =>
            {
                return true;
            }
            _ => {}
        }
        let material = int(dm, part, "Material");
        (material == 256 || material == 272) && no_reflectance
    }

    pub fn substitute(&self, kind: BodyPart, size: Vec3) -> Option<(String, Vec3)> {
        if let Some(m) = self.mesh_of(kind)
            && let Some(url) = asset_url(m.mesh)
        {
            let standard = standard_size(kind);
            return Some((url, size / standard));
        }
        let url = match kind {
            BodyPart::Torso => "rbxasset://fonts/torso.mesh",
            BodyPart::LeftArm => "rbxasset://fonts/leftarm.mesh",
            BodyPart::RightArm => "rbxasset://fonts/rightarm.mesh",
            BodyPart::LeftLeg => "rbxasset://fonts/leftleg.mesh",
            BodyPart::RightLeg => "rbxasset://fonts/rightleg.mesh",
            BodyPart::Head => return None,
        };
        Some((url.to_string(), size / standard_size(kind)))
    }

    pub fn uv_rect(kind: BodyPart) -> [f32; 4] {
        match kind {
            BodyPart::Head => [
                HEAD_SLOT[0] + BORDER_W,
                HEAD_SLOT[1] + BORDER_H,
                HEAD_SLOT[2] - 2.0 * BORDER_W,
                HEAD_SLOT[3] - 2.0 * BORDER_H,
            ],
            _ => [0.0, 0.0, BASE_WIDTH, 1.0],
        }
    }

    pub fn composite(&self, dm: &DataModel) -> CompositeDesc {
        let mut layers = Vec::new();
        let solid = |mesh: &str, part: Option<InstanceId>, layers: &mut Vec<CompositeLayer>| {
            if let Some(p) = part {
                layers.push(CompositeLayer {
                    mesh: mesh.into(),
                    color: color(dm, p),
                    texture: None,
                    mode: LayerMode::Color,
                });
            }
        };
        let textured = |mesh: &str, texture: Option<String>, layers: &mut Vec<CompositeLayer>| {
            if let Some(t) = texture {
                layers.push(CompositeLayer {
                    mesh: mesh.into(),
                    color: [1.0, 1.0, 1.0],
                    texture: Some(t),
                    mode: LayerMode::Blend,
                });
            }
        };
        solid(
            "rbxasset://fonts/CompositTorsoBase.mesh",
            self.torso,
            &mut layers,
        );
        solid(
            "rbxasset://fonts/CompositLeftArmBase.mesh",
            self.left_arm,
            &mut layers,
        );
        solid(
            "rbxasset://fonts/CompositRightArmBase.mesh",
            self.right_arm,
            &mut layers,
        );
        solid(
            "rbxasset://fonts/CompositLeftLegBase.mesh",
            self.left_leg,
            &mut layers,
        );
        solid(
            "rbxasset://fonts/CompositRightLegBase.mesh",
            self.right_leg,
            &mut layers,
        );
        if let Some(head) = self.head {
            let slot = "rbxasset://fonts/CompositExtraSlot4.mesh";
            solid(slot, Some(head), &mut layers);
            let mesh_texture = dm
                .children_of(head)
                .iter()
                .find(|c| matches!(dm.class_of(**c), Some("SpecialMesh" | "FileMesh")))
                .and_then(|m| content(dm, *m, "TextureId"));
            textured(slot, mesh_texture, &mut layers);
            for &c in dm.children_of(head).iter().rev() {
                if dm.class_of(c) == Some("Decal")
                    && int(dm, c, "Face") == 5
                    && let Some(t) = content(dm, c, "Texture")
                {
                    textured(slot, Some(t), &mut layers);
                }
            }
        }
        let torso_mesh = self.meshes[1].as_ref();
        textured(
            "rbxasset://fonts/CompositFullAtlasBaseTexture.mesh",
            torso_mesh.and_then(|m| asset_url(m.base)),
            &mut layers,
        );
        textured(
            "rbxasset://fonts/CompositPantsTemplate.mesh",
            self.pants.clone(),
            &mut layers,
        );
        textured(
            "rbxasset://fonts/CompositShirtTemplate.mesh",
            self.shirt.clone(),
            &mut layers,
        );
        textured(
            "rbxasset://fonts/CompositTShirt.mesh",
            self.graphic.clone(),
            &mut layers,
        );
        textured(
            "rbxasset://fonts/CompositFullAtlasOverlayTexture.mesh",
            torso_mesh.and_then(|m| asset_url(m.overlay)),
            &mut layers,
        );
        let mut key = String::from("composite://");
        for l in &layers {
            key.push_str(&format!(
                "{}|{:.3},{:.3},{:.3}|{};",
                l.mesh,
                l.color[0],
                l.color[1],
                l.color[2],
                l.texture.as_deref().unwrap_or("")
            ));
        }
        CompositeDesc { url: key, layers }
    }
}

fn standard_size(kind: BodyPart) -> Vec3 {
    match kind {
        BodyPart::Head => Vec3::ONE,
        BodyPart::Torso => Vec3::new(2.0, 2.0, 1.0),
        _ => Vec3::new(1.0, 2.0, 1.0),
    }
}
