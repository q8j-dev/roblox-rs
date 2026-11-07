use crate::geometry::Surface;
use crate::lighting::{LightingInputs, SceneLighting};
use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{CFrame, Color3, Mat3, Vec3};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shape {
    Block,
    Ball,
    Cylinder,
    Wedge,
    CornerWedge,
    File,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shading {
    Plastic,
    SmoothPlastic,
    Neon,
    Textured(u32),
    Image(u32),
}

#[derive(Clone, Debug)]
pub struct PartDraw {
    pub id: InstanceId,
    pub shape: Shape,
    pub size: Vec3,
    pub cframe: CFrame,
    pub mesh_scale: Vec3,
    pub mesh_offset: Vec3,
    pub color: [f32; 4],
    pub surfaces: [Surface; 6],
    pub shading: Shading,
    pub specular: (f32, f32),
    pub reflectance: f32,
    pub file: Option<FileMeshRef>,
    pub texture: Option<String>,
    pub decals: Vec<DecalDraw>,
}

#[derive(Clone, Debug)]
pub struct DecalDraw {
    pub face: usize,
    pub texture: String,
    pub transparency: f32,
    pub tile: Option<(f32, f32)>,
}

#[derive(Clone, Debug)]
pub struct Camera {
    pub cframe: CFrame,
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Camera {
            cframe: CFrame::look_at(Vec3::new(0.0, 20.0, 20.0), Vec3::ZERO, Vec3::Y),
            fov_y: 70.0f32.to_radians(),
            near: 0.5,
            far: 10000.0,
        }
    }
}

pub struct Scene {
    pub parts: Vec<PartDraw>,
    pub camera: Camera,
    pub lighting: SceneLighting,
    pub sky_faces: Option<[String; 6]>,
}

pub const SURFACE_PROPS: [&str; 6] = [
    "RightSurface",
    "TopSurface",
    "BackSurface",
    "LeftSurface",
    "BottomSurface",
    "FrontSurface",
];

pub fn specular(material: u32) -> (f32, f32) {
    let (i, p) = match material {
        256 => (102, 9),
        272 | 288 => (191, 81),
        512 => (64, 32),
        528 => (71, 53),
        784 => (179, 54),
        800 => (36, 20),
        816 => (38, 22),
        832 => (48, 24),
        848 => (33, 44),
        864 => (18, 22),
        880 => (54, 22),
        1040 => (89, 103),
        1056 => (230, 160),
        1072 => (238, 240),
        1088 => (204, 120),
        1280 => (43, 18),
        1296 => (18, 6),
        1312 => (8, 16),
        1536 => (255, 190),
        _ => (0, 50),
    };
    (i as f32 / 255.0, p as f32)
}

pub fn tiling(material: u32) -> f32 {
    match material {
        256 => 1.3,
        272 | 288 => 1.0,
        512 | 528 => 0.2,
        784 | 800 | 832 | 864 | 1040 | 1072 | 1296 | 1536 => 0.1,
        816 | 1280 | 1312 => 0.15,
        848 => 0.125,
        1056 | 1088 | 880 => 0.2,
        _ => 1.0,
    }
}

pub fn material_folder(material: u32) -> Option<&'static str> {
    Some(match material {
        512 => "wood",
        528 => "woodplanks",
        784 => "marble",
        800 => "slate",
        816 => "concrete",
        832 => "granite",
        848 => "brick",
        864 => "pebble",
        880 => "cobblestone",
        1040 => "rust",
        1056 => "diamondplate",
        1072 => "aluminum",
        1088 => "metal",
        1280 => "grass",
        1296 => "sand",
        1312 => "fabric",
        1536 => "ice",
        _ => return None,
    })
}

fn get_f32(dm: &DataModel, id: InstanceId, name: &str, default: f32) -> f32 {
    dm.get(id, name)
        .ok()
        .and_then(|v| v.as_f32())
        .unwrap_or(default)
}

fn get_color3(dm: &DataModel, id: InstanceId, name: &str, default: Color3) -> Color3 {
    dm.get(id, name)
        .ok()
        .and_then(|v| v.as_color3())
        .unwrap_or(default)
}

fn part_color(dm: &DataModel, id: InstanceId) -> Color3 {
    match dm.get(id, "BrickColor") {
        Ok(Variant::BrickColor(b)) => b.color3(),
        Ok(Variant::Color3(c)) => c,
        _ => dm
            .get(id, "Color")
            .ok()
            .and_then(|v| v.as_color3())
            .unwrap_or(Color3::new(0.64, 0.64, 0.64)),
    }
}

pub struct MeshOverride {
    pub shape: Shape,
    pub scale: Vec3,
    pub offset: Vec3,
    pub file: Option<FileMeshRef>,
    pub texture: Option<String>,
    pub vertex_color: Vec3,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileMeshRef {
    pub url: String,
    pub scale: Vec3,
}

fn content_prop(dm: &DataModel, id: InstanceId, name: &str) -> Option<String> {
    dm.get(id, name)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .filter(|s| !s.trim().is_empty())
}

fn mesh_override(
    dm: &DataModel,
    part: InstanceId,
    part_size: Vec3,
    shape: Shape,
) -> Option<MeshOverride> {
    for &c in dm.children_of(part) {
        let class = dm.class_of(c)?;
        if !dm.is_a(c, "DataModelMesh") {
            continue;
        }
        let scale = dm
            .get(c, "Scale")
            .ok()
            .and_then(|v| v.as_vec3())
            .unwrap_or(Vec3::ONE);
        let offset = dm
            .get(c, "Offset")
            .ok()
            .and_then(|v| v.as_vec3())
            .unwrap_or(Vec3::ZERO);
        let vertex_color = dm
            .get(c, "VertexColor")
            .ok()
            .and_then(|v| v.as_vec3())
            .unwrap_or(Vec3::ONE);
        let mut file = None;
        let mut texture = None;
        let s = match class {
            "BlockMesh" => Shape::Block,
            "CylinderMesh" => Shape::Cylinder,
            "FileMesh" => {
                file = content_prop(dm, c, "MeshId").map(|url| FileMeshRef { url, scale });
                texture = content_prop(dm, c, "TextureId");
                Shape::File
            }
            "SpecialMesh" => match dm
                .get(c, "MeshType")
                .ok()
                .and_then(|v| v.as_enum())
                .unwrap_or(6)
            {
                0 => {
                    let size = part_size * scale;
                    let xz = size.x.min(size.z);
                    file = Some(FileMeshRef {
                        url: "rbxasset://fonts/head.mesh".into(),
                        scale: Vec3::new(xz, size.y, xz) / 1.25,
                    });
                    Shape::File
                }
                2 => Shape::Wedge,
                3 => Shape::Ball,
                4 => Shape::Cylinder,
                5 => {
                    file = content_prop(dm, c, "MeshId").map(|url| FileMeshRef { url, scale });
                    texture = content_prop(dm, c, "TextureId");
                    Shape::File
                }
                _ => Shape::Block,
            },
            _ => shape,
        };
        if s == Shape::File && file.is_none() {
            return Some(MeshOverride {
                shape: Shape::Block,
                scale,
                offset,
                file: None,
                texture: None,
                vertex_color,
            });
        }
        return Some(MeshOverride {
            shape: s,
            scale,
            offset,
            file,
            texture,
            vertex_color,
        });
    }
    None
}

fn collect_decals(dm: &DataModel, part: InstanceId, out: &mut Vec<DecalDraw>) {
    for &c in dm.children_of(part) {
        if !dm.is_a(c, "Decal") {
            continue;
        }
        let Some(texture) = content_prop(dm, c, "Texture") else {
            continue;
        };
        let transparency = get_f32(dm, c, "Transparency", 0.0).clamp(0.0, 1.0);
        if transparency >= 1.0 {
            continue;
        }
        let face = dm
            .get(c, "Face")
            .ok()
            .and_then(|v| v.as_enum())
            .unwrap_or(5) as usize;
        let tile = dm.is_a(c, "Texture").then(|| {
            (
                get_f32(dm, c, "StudsPerTileU", 2.0).max(0.01),
                get_f32(dm, c, "StudsPerTileV", 2.0).max(0.01),
            )
        });
        out.push(DecalDraw {
            face: face.min(5),
            texture,
            transparency,
            tile,
        });
    }
}

fn collect_parts(dm: &DataModel, root: InstanceId, out: &mut Vec<PartDraw>) {
    for id in dm.descendants(root) {
        let Some(class) = dm.class_of(id) else {
            continue;
        };
        if !dm.is_a(id, "BasePart") || class == "Terrain" {
            continue;
        }
        let transparency = get_f32(dm, id, "Transparency", 0.0).clamp(0.0, 1.0);
        if transparency >= 1.0 {
            continue;
        }
        let Some(cframe) = dm.get_raw(id, "CFrame").and_then(|v| v.as_cframe()) else {
            continue;
        };
        let size = dm
            .get(id, "Size")
            .ok()
            .and_then(|v| v.as_vec3())
            .unwrap_or(Vec3::ONE);
        let mut shape = match class {
            "WedgePart" => Shape::Wedge,
            "CornerWedgePart" => Shape::CornerWedge,
            _ => match dm.get(id, "Shape").ok().and_then(|v| v.as_enum()) {
                Some(0) => Shape::Ball,
                Some(2) => Shape::Cylinder,
                _ => Shape::Block,
            },
        };
        let mut mesh_scale = Vec3::ONE;
        let mut mesh_offset = Vec3::ZERO;
        let mut file = None;
        let mut texture = None;
        let mut vertex_color = Vec3::ONE;
        if let Some(m) = mesh_override(dm, id, size, shape) {
            shape = m.shape;
            mesh_scale = m.scale;
            mesh_offset = m.offset;
            file = m.file;
            texture = m.texture;
            vertex_color = m.vertex_color;
        }
        let material = dm
            .get(id, "Material")
            .ok()
            .and_then(|v| v.as_enum())
            .unwrap_or(256);
        let shading = match material {
            256 => Shading::Plastic,
            272 => Shading::SmoothPlastic,
            288 => Shading::Neon,
            m if material_folder(m).is_some() => Shading::Textured(m),
            _ => Shading::Plastic,
        };
        let mut surfaces = [Surface::Smooth; 6];
        for (i, name) in SURFACE_PROPS.iter().enumerate() {
            surfaces[i] =
                Surface::from_enum(dm.get(id, name).ok().and_then(|v| v.as_enum()).unwrap_or(0));
        }
        let mut c = part_color(dm, id);
        if texture.is_some() {
            c = Color3::new(vertex_color.x, vertex_color.y, vertex_color.z);
        }
        let mut decals = Vec::new();
        collect_decals(dm, id, &mut decals);
        out.push(PartDraw {
            id,
            shape,
            size,
            cframe,
            mesh_scale,
            mesh_offset,
            color: [c.r, c.g, c.b, 1.0 - transparency],
            surfaces,
            shading,
            specular: specular(material),
            reflectance: get_f32(dm, id, "Reflectance", 0.0).clamp(0.0, 1.0),
            file,
            texture,
            decals,
        });
    }
}

pub fn extract(dm: &DataModel, aspect: f32) -> Scene {
    let _ = aspect;
    let mut parts = Vec::new();
    let ws = dm.find_service("Workspace");
    if let Some(ws) = ws {
        collect_parts(dm, ws, &mut parts);
    }
    let mut camera = Camera::default();
    if let Some(ws) = ws {
        let cam = dm
            .get_ref(ws, "CurrentCamera")
            .or_else(|| dm.find_first_child_of_class(ws, "Camera"));
        if let Some(cam) = cam {
            if let Some(cf) = dm.get_raw(cam, "CFrame").and_then(|v| v.as_cframe()) {
                camera.cframe = cf;
            }
            camera.fov_y = get_f32(dm, cam, "FieldOfView", 70.0)
                .clamp(1.0, 120.0)
                .to_radians();
        }
    }
    let lighting = match dm.find_service("Lighting") {
        Some(l) => SceneLighting::compute(&LightingInputs {
            game_time: dm.lighting_seconds(l) as f64,
            latitude: get_f32(dm, l, "GeographicLatitude", 41.7333),
            ambient: get_color3(dm, l, "Ambient", Color3::new(0.5, 0.5, 0.5)),
            brightness: get_f32(dm, l, "Brightness", 1.0),
            color_shift_top: get_color3(dm, l, "ColorShift_Top", Color3::new(0.0, 0.0, 0.0)),
            color_shift_bottom: get_color3(dm, l, "ColorShift_Bottom", Color3::new(0.0, 0.0, 0.0)),
            fog_color: get_color3(dm, l, "FogColor", Color3::new(0.75, 0.75, 0.75)),
            fog_start: get_f32(dm, l, "FogStart", 0.0),
            fog_end: get_f32(dm, l, "FogEnd", 100000.0),
            outlines: dm
                .get(l, "Outlines")
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
        }),
        None => SceneLighting::compute(&LightingInputs {
            game_time: 14.0 * 3600.0,
            latitude: 41.7333,
            ambient: Color3::new(0.5, 0.5, 0.5),
            brightness: 1.0,
            color_shift_top: Color3::new(0.0, 0.0, 0.0),
            color_shift_bottom: Color3::new(0.0, 0.0, 0.0),
            fog_color: Color3::new(0.75, 0.75, 0.75),
            fog_start: 0.0,
            fog_end: 100000.0,
            outlines: true,
        }),
    };
    let sky_faces = dm.find_service("Lighting").and_then(|l| {
        let sky = dm.find_first_child_of_class(l, "Sky")?;
        let names = [
            "SkyboxRt", "SkyboxLf", "SkyboxBk", "SkyboxFt", "SkyboxUp", "SkyboxDn",
        ];
        let mut out: [String; 6] = Default::default();
        for (i, n) in names.iter().enumerate() {
            out[i] = dm
                .get(sky, n)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default();
        }
        Some(out)
    });
    Scene {
        parts,
        camera,
        lighting,
        sky_faces,
    }
}

pub fn instance_rows(cframe: &CFrame, scale: Vec3, offset: Vec3) -> [[f32; 4]; 3] {
    let r: Mat3 = cframe.rotation;
    let t = cframe.translation + r * offset;
    let c0 = r.x_axis * scale.x;
    let c1 = r.y_axis * scale.y;
    let c2 = r.z_axis * scale.z;
    [
        [c0.x, c1.x, c2.x, t.x],
        [c0.y, c1.y, c2.y, t.y],
        [c0.z, c1.z, c2.z, t.z],
    ]
}
