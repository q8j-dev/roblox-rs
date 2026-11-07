use serde_json::Value;
use std::sync::LazyLock;

const MATERIALS_JSON: &str =
    include_str!("../../../assets/platform-content/pc/terrain/materials.json");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MaterialType {
    #[default]
    Soft,
    Hard,
    HardSoft,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Deformation {
    #[default]
    None,
    Shift,
    Cubify,
    Quantize,
    Barrel,
    Water,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mapping {
    #[default]
    Default,
    Cube,
}

#[derive(Clone, Debug, Default)]
pub struct Material {
    pub name: String,
    pub top_layer: i32,
    pub side_layer: i32,
    pub bottom_layer: i32,
    pub kind: MaterialType,
    pub mapping: Mapping,
    pub deformation: Deformation,
    pub parameter: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Layer {
    pub tiling: f32,
    pub detiling: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Atlas {
    pub width: i32,
    pub height: i32,
    pub tile_size: i32,
    pub tile_count: i32,
    pub border_size: i32,
}

#[derive(Clone, Debug)]
pub struct MaterialTable {
    pub atlas: Atlas,
    pub materials: Vec<Material>,
    pub layers: Vec<Layer>,
}

fn num(v: &Value, def: f64) -> f64 {
    v.as_f64().unwrap_or(def)
}

impl MaterialTable {
    pub fn parse(text: &str, material_count: usize) -> MaterialTable {
        let mut table = MaterialTable {
            atlas: Atlas {
                width: 1,
                height: 1,
                tile_size: 0,
                tile_count: 1,
                border_size: 0,
            },
            materials: Vec::new(),
            layers: Vec::new(),
        };
        if let Ok(root) = serde_json::from_str::<Value>(text) {
            let platform = root["platform"].as_str().unwrap_or("");
            let a = &root["atlas"][platform];
            if a.is_object() {
                table.atlas = Atlas {
                    width: num(&a["width"], 1.0) as i32,
                    height: num(&a["height"], 1.0) as i32,
                    tile_size: num(&a["tileSize"], 0.0) as i32,
                    tile_count: num(&a["tileCount"], 1.0) as i32,
                    border_size: num(&a["borderSize"], 0.0) as i32,
                };
            }
            if let Some(list) = root["materials"].as_array() {
                for m in list {
                    let layer = |node: &Value, layers: &mut Vec<Layer>| -> i32 {
                        layers.push(Layer {
                            tiling: num(&node["tiling"], 1.0) as f32,
                            detiling: num(&node["detiling"], 0.0) as f32,
                        });
                        layers.len() as i32 - 1
                    };
                    let (top, side, bottom) =
                        if m["texture_top"].is_object() && m["texture_side"].is_object() {
                            let t = layer(&m["texture_top"], &mut table.layers);
                            let s = layer(&m["texture_side"], &mut table.layers);
                            let b = if m["texture_bottom"].is_object() {
                                layer(&m["texture_bottom"], &mut table.layers)
                            } else {
                                t
                            };
                            (t, s, b)
                        } else if m["texture"].is_object() {
                            let t = layer(&m["texture"], &mut table.layers);
                            (t, t, t)
                        } else {
                            (-1, -1, -1)
                        };
                    let kind = match m["type"].as_str().unwrap_or("") {
                        "hard" => MaterialType::Hard,
                        "hardsoft" => MaterialType::HardSoft,
                        _ => MaterialType::Soft,
                    };
                    let mapping = if m["mapping"].as_str() == Some("cube") {
                        Mapping::Cube
                    } else {
                        Mapping::Default
                    };
                    let mut desc = Material {
                        name: m["name"].as_str().unwrap_or("").to_string(),
                        top_layer: top,
                        side_layer: side,
                        bottom_layer: bottom,
                        kind,
                        mapping,
                        deformation: Deformation::None,
                        parameter: 0.0,
                    };
                    for (key, d) in [
                        ("shift", Deformation::Shift),
                        ("cubify", Deformation::Cubify),
                        ("quantize", Deformation::Quantize),
                        ("barrel", Deformation::Barrel),
                        ("water", Deformation::Water),
                    ] {
                        if let Some(p) = m[key].as_f64() {
                            desc.deformation = d;
                            desc.parameter = p as f32;
                        }
                    }
                    table.materials.push(desc);
                }
            }
        }
        if table.layers.is_empty() {
            table.layers.push(Layer {
                tiling: 1.0,
                detiling: 0.0,
            });
        }
        if table.materials.len() < material_count {
            table.materials.resize(material_count, Material::default());
        }
        table
    }

    pub fn material(&self, index: u8) -> &Material {
        &self.materials[index as usize]
    }
}

static TABLE: LazyLock<MaterialTable> = LazyLock::new(|| MaterialTable::parse(MATERIALS_JSON, 64));

pub fn material_table() -> &'static MaterialTable {
    &TABLE
}
