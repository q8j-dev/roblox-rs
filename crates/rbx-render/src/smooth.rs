use bytemuck::{Pod, Zeroable};
use rbx_math::{IVec3, Vec3};
use rbx_terrain::materials::{Mapping, MaterialTable, MaterialType};
use rbx_terrain::mesher::{self, BasicMesh, Options};
use rbx_terrain::smooth::Region;
use rbx_terrain::{CELL_SIZE, SmoothGrid};

pub const CHUNK_SIZE_LOG2: IVec3 = IVec3::new(5, 4, 5);
pub const CHUNK_BORDER: i32 = 2;

const BASIS_U: [[f32; 3]; 18] = [
    [0.0, 0.0, -1.0],
    [0.0, 0.0, 1.0],
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.7, 0.0, -0.7],
    [-0.7, 0.0, -0.7],
    [0.7, 0.0, 0.7],
    [-0.7, 0.0, 0.7],
    [1.0, 0.0, 0.0],
    [0.7, -0.7, 0.0],
    [0.7, 0.7, 0.0],
    [1.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [-0.7, -0.7, 0.0],
    [-0.7, 0.7, 0.0],
    [-1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
];

const BASIS_V: [[f32; 3]; 18] = [
    [0.0, -1.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.0, 0.0, 1.0],
    [0.0, 0.0, 1.0],
    [0.0, -0.7, 0.7],
    [0.0, 0.7, 0.7],
    [0.0, 0.0, -1.0],
    [0.0, 0.0, -1.0],
    [0.0, 0.0, -1.0],
    [0.0, -0.7, -0.7],
    [0.0, 0.7, -0.7],
];

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct SmoothVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub weights: [f32; 3],
    pub uv0: [f32; 4],
    pub uv1: [f32; 4],
    pub uv2: [f32; 4],
}

impl SmoothVertex {
    pub const ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x3,
        2 => Float32x3,
        3 => Float32x4,
        4 => Float32x4,
        5 => Float32x4,
    ];
}

#[derive(Default)]
pub struct SmoothMesh {
    pub vertices: Vec<SmoothVertex>,
    pub solid: Vec<u32>,
    pub water: Vec<u32>,
}

pub fn layer_scale(table: &MaterialTable) -> [f32; 2] {
    let a = &table.atlas;
    [
        a.tile_size as f32 / a.width.max(1) as f32,
        a.tile_size as f32 / a.height.max(1) as f32,
    ]
}

fn segment_2d_4(x: f32, z: f32) -> usize {
    if x.abs() > z.abs() {
        (x < 0.0) as usize
    } else {
        2 + (z < 0.0) as usize
    }
}

fn segment_2d_8(x: f32, z: f32) -> usize {
    let ax = x.abs();
    let az = z.abs();
    if ax > az * 2.0 {
        (x < 0.0) as usize
    } else if az > ax * 2.0 {
        2 + (z < 0.0) as usize
    } else {
        4 + 2 * (x < 0.0) as usize + (z < 0.0) as usize
    }
}

fn segment_default(n: Vec3) -> usize {
    if n.y > 0.9 {
        8
    } else if n.y > 0.4 {
        9 + segment_2d_4(n.x, n.z)
    } else if n.y < -0.8 {
        13
    } else if n.y < -0.6 {
        14 + segment_2d_4(n.x, n.z)
    } else {
        segment_2d_8(n.x, n.z)
    }
}

fn segment_cube(n: Vec3) -> usize {
    let ax = n.x.abs();
    let az = n.z.abs();
    if n.y > ax && n.y > az {
        8
    } else if n.y < -ax && n.y < -az {
        13
    } else if ax > az {
        (n.x < 0.0) as usize
    } else {
        2 + (n.z < 0.0) as usize
    }
}

fn compute_normals(table: &MaterialTable, material: u8, hard: Vec3, soft: Vec3) -> (Vec3, Vec3) {
    match table.material(material).kind {
        MaterialType::Hard => (hard, hard),
        MaterialType::HardSoft => {
            if hard.dot(soft) > 0.75 {
                (hard, soft)
            } else {
                (hard, hard)
            }
        }
        MaterialType::Soft => (soft, soft),
    }
}

struct Corner {
    layer: usize,
    segment: usize,
    seed: f32,
}

fn corner(table: &MaterialTable, material: u8, normal: Vec3, seed: u16) -> Corner {
    let desc = table.material(material);
    let segment = match desc.mapping {
        Mapping::Cube => segment_cube(normal),
        Mapping::Default => segment_default(normal),
    };
    let layer = if segment >= 13 {
        desc.bottom_layer
    } else if segment >= 8 {
        desc.top_layer
    } else {
        desc.side_layer
    };
    Corner {
        layer: layer.max(0) as usize,
        segment,
        seed: (seed & 0xff) as f32,
    }
}

fn corner_uv(table: &MaterialTable, c: &Corner, p: Vec3) -> [f32; 4] {
    let a = &table.atlas;
    let full = a.tile_size + a.border_size * 2;
    let i = c.layer.min(table.layers.len().saturating_sub(1));
    let layer = table.layers[i];
    let tiles = a.tile_count.max(1) as usize;
    let u = Vec3::from_array(BASIS_U[c.segment]);
    let v = Vec3::from_array(BASIS_V[c.segment]);
    let tiling = layer.tiling / CELL_SIZE;
    let offset = (
        ((i % tiles) as i32 * full + a.border_size) as f32 / a.width.max(1) as f32,
        ((i / tiles) as i32 * full + a.border_size) as f32 / a.height.max(1) as f32,
    );
    [
        p.dot(u) * tiling + layer.detiling * c.seed,
        p.dot(v) * tiling + layer.detiling * (c.seed * 2.6651441).floor(),
        offset.0,
        offset.1,
    ]
}

pub fn build(mesh: &BasicMesh, table: &MaterialTable) -> SmoothMesh {
    let mut out = SmoothMesh::default();
    let tris = mesh.indices.len() / 3;
    if tris == 0 {
        return out;
    }
    let pos: Vec<Vec3> = mesh
        .vertices
        .iter()
        .map(|v| v.position * CELL_SIZE)
        .collect();
    let mut soft = vec![Vec3::ZERO; mesh.vertices.len()];
    let mut hard = Vec::with_capacity(tris);
    let mut water = Vec::with_capacity(tris);
    for t in 0..tris {
        let i = [
            mesh.indices[3 * t] as usize,
            mesh.indices[3 * t + 1] as usize,
            mesh.indices[3 * t + 2] as usize,
        ];
        let n = (pos[i[1]] - pos[i[0]]).cross(pos[i[2]] - pos[i[0]]);
        for k in i {
            soft[k] += n;
        }
        hard.push(n.normalize_or_zero());
        water.push(BasicMesh::is_water(
            &mesh.vertices[i[0]],
            &mesh.vertices[i[1]],
            &mesh.vertices[i[2]],
        ));
    }
    for n in &mut soft {
        *n = n.normalize_or_zero();
    }
    for t in 0..tris {
        let i = [
            mesh.indices[3 * t] as usize,
            mesh.indices[3 * t + 1] as usize,
            mesh.indices[3 * t + 2] as usize,
        ];
        let v = [
            &mesh.vertices[i[0]],
            &mesh.vertices[i[1]],
            &mesh.vertices[i[2]],
        ];
        if v.iter().any(|v| v.border) {
            continue;
        }
        let normals: Vec<(Vec3, Vec3)> = (0..3)
            .map(|k| compute_normals(table, v[k].material, hard[t], soft[i[k]]))
            .collect();
        let corners: Vec<Corner> = (0..3)
            .map(|k| corner(table, v[k].material, normals[k].1, v[k].seed))
            .collect();
        let base = out.vertices.len() as u32;
        for k in 0..3 {
            let p = pos[i[k]];
            let mut weights = [0.0; 3];
            weights[k] = 1.0;
            out.vertices.push(SmoothVertex {
                pos: p.to_array(),
                normal: normals[k].0.to_array(),
                weights,
                uv0: corner_uv(table, &corners[0], p),
                uv1: corner_uv(table, &corners[1], p),
                uv2: corner_uv(table, &corners[2], p),
            });
        }
        if water[t] {
            out.water.extend_from_slice(&[base, base + 1, base + 2]);
        } else {
            out.solid.extend_from_slice(&[base, base + 1, base + 2]);
            if water.get(t ^ 1).copied().unwrap_or(false) {
                out.solid.extend_from_slice(&[base, base + 2, base + 1]);
            }
        }
    }
    out
}

pub fn chunk_region(id: IVec3) -> Region {
    let begin = IVec3::new(
        id.x << CHUNK_SIZE_LOG2.x,
        id.y << CHUNK_SIZE_LOG2.y,
        id.z << CHUNK_SIZE_LOG2.z,
    );
    let size = IVec3::new(
        1 << CHUNK_SIZE_LOG2.x,
        1 << CHUNK_SIZE_LOG2.y,
        1 << CHUNK_SIZE_LOG2.z,
    );
    Region::new(begin, begin + size)
}

pub fn chunks_touching(begin: IVec3, end: IVec3, out: &mut Vec<IVec3>) {
    let b = begin - IVec3::splat(CHUNK_BORDER);
    let e = end + IVec3::splat(CHUNK_BORDER);
    let size = IVec3::new(
        1 << CHUNK_SIZE_LOG2.x,
        1 << CHUNK_SIZE_LOG2.y,
        1 << CHUNK_SIZE_LOG2.z,
    );
    let lo = IVec3::new(
        b.x >> CHUNK_SIZE_LOG2.x,
        b.y >> CHUNK_SIZE_LOG2.y,
        b.z >> CHUNK_SIZE_LOG2.z,
    );
    let last = e + size - IVec3::ONE;
    let hi = IVec3::new(
        last.x >> CHUNK_SIZE_LOG2.x,
        last.y >> CHUNK_SIZE_LOG2.y,
        last.z >> CHUNK_SIZE_LOG2.z,
    );
    for y in lo.y..hi.y {
        for z in lo.z..hi.z {
            for x in lo.x..hi.x {
                out.push(IVec3::new(x, y, z));
            }
        }
    }
}

pub fn chunk(grid: &SmoothGrid, table: &MaterialTable, id: IVec3) -> SmoothMesh {
    let region = chunk_region(id);
    let region = Region::new(
        region.begin - IVec3::splat(CHUNK_BORDER),
        region.end + IVec3::splat(CHUNK_BORDER),
    );
    let cells = grid.read(&region, 0);
    if cells.is_empty() {
        return SmoothMesh::default();
    }
    let options = Options {
        materials: table,
        generate_water: true,
    };
    let basic = mesher::generate_geometry(&cells, region.begin, 0, &options);
    build(&basic, table)
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct TerrainParams {
    layer_scale: [f32; 4],
    water_color: [f32; 4],
}

struct GpuChunk {
    vertices: wgpu::Buffer,
    solid: Option<(wgpu::Buffer, u32)>,
    water: Option<(wgpu::Buffer, u32)>,
}

fn upload(device: &wgpu::Device, mesh: &SmoothMesh) -> Option<GpuChunk> {
    use wgpu::util::DeviceExt;
    if mesh.solid.is_empty() && mesh.water.is_empty() {
        return None;
    }
    let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("terrain vertices"),
        contents: bytemuck::cast_slice(&mesh.vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let indices = |list: &[u32]| {
        (!list.is_empty()).then(|| {
            (
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("terrain indices"),
                    contents: bytemuck::cast_slice(list),
                    usage: wgpu::BufferUsages::INDEX,
                }),
                list.len() as u32,
            )
        })
    };
    Some(GpuChunk {
        vertices,
        solid: indices(&mesh.solid),
        water: indices(&mesh.water),
    })
}

pub struct SmoothTerrain {
    solid: wgpu::RenderPipeline,
    water: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    params: wgpu::Buffer,
    bind_group: Option<wgpu::BindGroup>,
    chunks: std::collections::HashMap<IVec3, GpuChunk>,
    legacy_water: Vec<GpuChunk>,
    state: Option<(usize, u64)>,
}

impl SmoothTerrain {
    pub fn new(
        device: &wgpu::Device,
        globals_layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
    ) -> SmoothTerrain {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("smooth terrain"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/smooth.wgsl").into()),
        });
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("smooth terrain"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("smooth terrain"),
            bind_group_layouts: &[Some(globals_layout), Some(&layout)],
            immediate_size: 0,
        });
        let buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<SmoothVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &SmoothVertex::ATTRIBUTES,
        })];
        let make = |entry: &str, blend: Option<wgpu::BlendState>, depth_write: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: if depth_write {
                        Some(wgpu::Face::Back)
                    } else {
                        None
                    },
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: depth_format,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: crate::renderer::msaa_state(),
                multiview_mask: None,
                cache: None,
            })
        };
        let solid = make("fs_main", None, true);
        let water = make("fs_water", Some(wgpu::BlendState::ALPHA_BLENDING), false);
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("terrain params"),
            size: std::mem::size_of::<TerrainParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        SmoothTerrain {
            solid,
            water,
            layout,
            params,
            bind_group: None,
            chunks: std::collections::HashMap::new(),
            legacy_water: Vec::new(),
            state: None,
        }
    }

    pub fn clear(&mut self) {
        self.chunks.clear();
        self.state = None;
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty() && self.legacy_water.is_empty()
    }

    fn ensure_bind_group(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        platform_dir: &std::path::Path,
        white: &wgpu::TextureView,
    ) {
        if self.bind_group.is_some() {
            return;
        }
        let load = |name: &str| {
            crate::renderer::read_texture_gpu(
                &platform_dir.join("terrain").join(name),
                device
                    .features()
                    .contains(wgpu::Features::TEXTURE_COMPRESSION_BC),
            )
            .map(|img| {
                img.upload(device, queue, name, false)
                    .create_view(&wgpu::TextureViewDescriptor::default())
            })
        };
        let albedo = load("diffuse.dds");
        let specular = load("specular.dds");
        self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("smooth terrain"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(albedo.as_ref().unwrap_or(white)),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        specular.as_ref().unwrap_or(white),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.params.as_entire_binding(),
                },
            ],
        }));
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        water_color: [f32; 4],
        platform_dir: &std::path::Path,
        white: &wgpu::TextureView,
    ) {
        let scale = layer_scale(rbx_terrain::material_table());
        let params = TerrainParams {
            layer_scale: [scale[0], scale[1], 0.0, 0.0],
            water_color,
        };
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params));
        self.ensure_bind_group(device, queue, platform_dir, white);
    }

    pub fn set_legacy_water(&mut self, device: &wgpu::Device, meshes: Vec<SmoothMesh>) {
        self.legacy_water = meshes.iter().filter_map(|m| upload(device, m)).collect();
    }

    pub fn update(&mut self, device: &wgpu::Device, grid: &SmoothGrid) {
        let table = rbx_terrain::material_table();
        let identity = grid as *const SmoothGrid as usize;
        let generation = grid.changes.generation();
        if self.state == Some((identity, generation)) {
            return;
        }
        let mut dirty = Vec::new();
        let since = match self.state {
            Some((id, g)) if id == identity => Some(grid.changes.since(g)),
            _ => None,
        };
        match since {
            Some(rbx_terrain::changes::Changes::Since(list)) => {
                for (b, e) in list {
                    chunks_touching(*b, *e, &mut dirty);
                }
            }
            _ => {
                self.chunks.clear();
                for r in grid.non_empty_regions() {
                    chunks_touching(r.begin, r.end, &mut dirty);
                }
            }
        }
        self.state = Some((identity, generation));
        dirty.sort_by_key(|v| (v.x, v.y, v.z));
        dirty.dedup();
        for id in dirty {
            let mesh = chunk(grid, table, id);
            if mesh.solid.is_empty() && mesh.water.is_empty() {
                self.chunks.remove(&id);
                continue;
            }
            if let Some(gpu) = upload(device, &mesh) {
                self.chunks.insert(id, gpu);
            }
        }
    }

    pub fn draw_solid(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(bg) = &self.bind_group else {
            return;
        };
        pass.set_pipeline(&self.solid);
        pass.set_bind_group(1, bg, &[]);
        for c in self.chunks.values() {
            if let Some((buf, count)) = &c.solid {
                pass.set_vertex_buffer(0, c.vertices.slice(..));
                pass.set_index_buffer(buf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*count, 0, 0..1);
            }
        }
    }

    pub fn draw_water(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(bg) = &self.bind_group else {
            return;
        };
        pass.set_pipeline(&self.water);
        pass.set_bind_group(1, bg, &[]);
        for c in self.chunks.values().chain(self.legacy_water.iter()) {
            if let Some((buf, count)) = &c.water {
                pass.set_vertex_buffer(0, c.vertices.slice(..));
                pass.set_index_buffer(buf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*count, 0, 0..1);
            }
        }
    }
}
