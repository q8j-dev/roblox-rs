use crate::adorn::{AdornMesh, Adorns, Material, Shape};
use bytemuck::{Pod, Zeroable};
use rbx_math::Vec3;
use std::collections::HashMap;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct AdornVertex {
    position: [f32; 3],
    uv: [f32; 2],
    normal: [f32; 3],
}

impl AdornVertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x3];

    fn new(p: Vec3, n: Vec3) -> AdornVertex {
        AdornVertex {
            position: p.to_array(),
            uv: [0.0, 0.0],
            normal: n.to_array(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct AdornInstance {
    rows: [[f32; 4]; 3],
    color: [f32; 4],
    params: [f32; 4],
}

impl AdornInstance {
    const ATTRIBUTES: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
        3 => Float32x4,
        4 => Float32x4,
        5 => Float32x4,
        6 => Float32x4,
        7 => Float32x4
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum MeshKey {
    Box,
    Cylinder,
    AaCylinder,
    Sphere,
    Cone,
    Quad,
    Line,
}

struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct PipelineKey {
    blend: bool,
    depth_write: bool,
    always: bool,
    lines: bool,
    cull: bool,
}

struct Draw {
    key: PipelineKey,
    mesh: MeshKey,
    texture: Option<String>,
    instance: u32,
}

pub struct AdornPass {
    shader: wgpu::ShaderModule,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    pipelines: HashMap<PipelineKey, wgpu::RenderPipeline>,
    meshes: HashMap<MeshKey, GpuMesh>,
    sampler: wgpu::Sampler,
    pixel: wgpu::Buffer,
    white: wgpu::BindGroup,
    textures: HashMap<String, Option<wgpu::BindGroup>>,
    frame: Option<(wgpu::Buffer, Vec<Draw>)>,
}

fn fix_winding(vertices: &[AdornVertex], indices: &mut [u16], outward: impl Fn(usize) -> Vec3) {
    for tri in indices.chunks_mut(3) {
        let p = |i: u16| Vec3::from_array(vertices[i as usize].position);
        let n = (p(tri[1]) - p(tri[0])).cross(p(tri[2]) - p(tri[0]));
        let dir = outward(tri[0] as usize) + outward(tri[1] as usize) + outward(tri[2] as usize);
        if n.dot(dir) < 0.0 {
            tri.swap(1, 2);
        }
    }
}

fn box_mesh() -> (Vec<AdornVertex>, Vec<u16>) {
    let mut v = Vec::new();
    let mut i = Vec::new();
    for axis in 0..3 {
        for sign in [1.0f32, -1.0] {
            let mut n = Vec3::ZERO;
            n[axis] = sign;
            let u = {
                let mut u = Vec3::ZERO;
                u[(axis + 1) % 3] = 1.0;
                u
            };
            let w = n.cross(u);
            let base = v.len() as u16;
            for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                v.push(AdornVertex::new(n + u * a + w * b, n));
            }
            i.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    fix_winding(&v, &mut i, |k| Vec3::from_array(v[k].normal));
    (v, i)
}

fn cylinder_mesh(sides: usize, zero_normal_bottom: bool) -> (Vec<AdornVertex>, Vec<u16>) {
    let mut v = vec![
        AdornVertex::new(Vec3::NEG_X, Vec3::NEG_X),
        AdornVertex::new(Vec3::X, Vec3::X),
    ];
    for side in 0..sides {
        let a = side as f32 / sides as f32 * std::f32::consts::TAU;
        let r = Vec3::new(0.0, a.sin(), a.cos());
        v.push(AdornVertex::new(r - Vec3::X, r));
        v.push(AdornVertex::new(r + Vec3::X, r));
        v.push(AdornVertex::new(r - Vec3::X, Vec3::NEG_X));
        v.push(AdornVertex::new(r + Vec3::X, Vec3::X));
    }
    let mut i = Vec::new();
    for side in 0..sides {
        let s0 = (2 + side * 4) as u16;
        let s1 = (2 + ((side + 1) % sides) * 4) as u16;
        i.extend_from_slice(&[s0, s0 + 1, s1 + 1, s0, s1 + 1, s1]);
        i.extend_from_slice(&[0, s0 + 2, s1 + 2, 1, s1 + 3, s0 + 3]);
    }
    fix_winding(&v, &mut i, |k| Vec3::from_array(v[k].normal));
    if zero_normal_bottom {
        v[0].normal = [0.0; 3];
        v[1].normal = [0.0; 3];
        for side in 0..sides {
            let base = 2 + side * 4;
            v[base + 2].normal = v[base].normal;
            v[base + 3].normal = v[base].normal;
        }
    }
    (v, i)
}

fn sphere_mesh() -> (Vec<AdornVertex>, Vec<u16>) {
    let (su, sv) = (18usize, 9usize);
    let du = std::f32::consts::TAU / su as f32;
    let dv = std::f32::consts::PI / sv as f32;
    let mut avg = Vec3::ZERO;
    for k in 0..4 {
        let u = if k & 1 != 0 { du } else { 0.0 };
        let w = if k & 2 != 0 { dv } else { 0.0 };
        avg += Vec3::new(w.sin() * u.cos(), w.sin() * u.sin(), w.cos());
    }
    let radius = 1.0 / (avg / 4.0).length();
    let mut v = Vec::new();
    for u in 0..su {
        for w in 0..=sv {
            let (a, b) = (u as f32 * du, w as f32 * dv);
            let p = Vec3::new(b.sin() * a.cos(), b.sin() * a.sin(), b.cos());
            v.push(AdornVertex::new(p * radius, p));
        }
    }
    let mut i = Vec::new();
    for u in 0..su {
        let u1 = (u + 1) % su;
        for w in 0..sv {
            let a = (u * (sv + 1) + w) as u16;
            let b = (u1 * (sv + 1) + w + 1) as u16;
            let c = (u1 * (sv + 1) + w) as u16;
            let d = (u * (sv + 1) + w + 1) as u16;
            i.extend_from_slice(&[a, b, c, a, d, b]);
        }
    }
    fix_winding(&v, &mut i, |k| Vec3::from_array(v[k].position));
    (v, i)
}

fn cone_mesh() -> (Vec<AdornVertex>, Vec<u16>) {
    let sides = 12usize;
    let mut v = vec![AdornVertex::new(Vec3::ZERO, Vec3::NEG_X)];
    let mut i = Vec::new();
    for side in 0..sides {
        let a0 = side as f32 / sides as f32 * std::f32::consts::TAU;
        let a1 = (side + 1) as f32 / sides as f32 * std::f32::consts::TAU;
        let c = Vec3::new(0.0, a0.sin(), a0.cos());
        let n = Vec3::new(0.0, a1.sin(), a1.cos());
        let base = v.len() as u16;
        v.push(AdornVertex::new(c, c));
        v.push(AdornVertex::new(n, n));
        v.push(AdornVertex::new(Vec3::X, (c + n).normalize()));
        v.push(AdornVertex::new(c, Vec3::NEG_X));
        v.push(AdornVertex::new(n, Vec3::NEG_X));
        i.extend_from_slice(&[base, base + 2, base + 1, 0, base + 3, base + 4]);
    }
    fix_winding(&v, &mut i, |k| {
        let p = Vec3::from_array(v[k].position);
        let n = Vec3::from_array(v[k].normal);
        if n.x < -0.5 {
            Vec3::NEG_X
        } else {
            p - Vec3::new(0.4, 0.0, 0.0)
        }
    });
    (v, i)
}

fn quad_mesh() -> (Vec<AdornVertex>, Vec<u16>) {
    let v: Vec<AdornVertex> = [(-1.0f32, 1.0f32), (1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)]
        .iter()
        .map(|&(x, y)| AdornVertex {
            position: [x, y, 0.0],
            uv: [(x + 1.0) / 2.0, (1.0 - y) / 2.0],
            normal: [0.0, 0.0, 1.0],
        })
        .collect();
    (v, vec![0, 1, 2, 0, 2, 3])
}

fn line_mesh() -> (Vec<AdornVertex>, Vec<u16>) {
    (
        vec![
            AdornVertex::new(Vec3::NEG_X, Vec3::ZERO),
            AdornVertex::new(Vec3::X, Vec3::ZERO),
        ],
        vec![0, 1],
    )
}

fn mesh_key(m: &AdornMesh) -> MeshKey {
    match m.shape {
        Shape::Box => MeshKey::Box,
        Shape::Cylinder => MeshKey::Cylinder,
        Shape::Line => MeshKey::AaCylinder,
        Shape::Sphere => MeshKey::Sphere,
        Shape::Cone => MeshKey::Cone,
        Shape::Quad => MeshKey::Quad,
    }
}

impl AdornPass {
    pub fn new(
        device: &wgpu::Device,
        globals_layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
        white: &wgpu::TextureView,
    ) -> AdornPass {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("adorn"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/adorn.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("adorn"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
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
            label: Some("adorn"),
            bind_group_layouts: &[Some(globals_layout), Some(&layout)],
            immediate_size: 0,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("adorn"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let pixel = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("adorn pixel info"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let make_group = |view: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("adorn"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: pixel.as_entire_binding(),
                    },
                ],
            })
        };
        let white = make_group(white);
        let mut meshes = HashMap::new();
        for (key, (v, i)) in [
            (MeshKey::Box, box_mesh()),
            (MeshKey::Cylinder, cylinder_mesh(12, false)),
            (MeshKey::AaCylinder, cylinder_mesh(12, true)),
            (MeshKey::Sphere, sphere_mesh()),
            (MeshKey::Cone, cone_mesh()),
            (MeshKey::Quad, quad_mesh()),
            (MeshKey::Line, line_mesh()),
        ] {
            let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("adorn vertices"),
                contents: bytemuck::cast_slice(&v),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let mut padded = i.clone();
            if padded.len() % 2 == 1 {
                padded.push(0);
            }
            let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("adorn indices"),
                contents: bytemuck::cast_slice(&padded),
                usage: wgpu::BufferUsages::INDEX,
            });
            meshes.insert(
                key,
                GpuMesh {
                    vertices,
                    indices,
                    count: i.len() as u32,
                },
            );
        }
        AdornPass {
            shader,
            layout,
            pipeline_layout,
            format,
            pipelines: HashMap::new(),
            meshes,
            sampler,
            pixel,
            white,
            textures: HashMap::new(),
            frame: None,
        }
    }

    fn pipeline(&mut self, device: &wgpu::Device, key: PipelineKey) {
        if self.pipelines.contains_key(&key) {
            return;
        }
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("adorn"),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[
                    Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<AdornVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &AdornVertex::ATTRIBUTES,
                    }),
                    Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<AdornInstance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &AdornInstance::ATTRIBUTES,
                    }),
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.format,
                    blend: key.blend.then_some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: if key.lines {
                    wgpu::PrimitiveTopology::LineList
                } else {
                    wgpu::PrimitiveTopology::TriangleList
                },
                cull_mode: key.cull.then_some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: crate::renderer::DEPTH_FORMAT,
                depth_write_enabled: Some(key.depth_write),
                depth_compare: Some(if key.always {
                    wgpu::CompareFunction::Always
                } else {
                    wgpu::CompareFunction::LessEqual
                }),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: crate::renderer::msaa_state(),
            multiview_mask: None,
            cache: None,
        });
        self.pipelines.insert(key, pipeline);
    }

    pub fn missing_textures(&self, adorns: &Adorns) -> Vec<String> {
        let mut out: Vec<String> = adorns
            .meshes
            .iter()
            .filter_map(|m| m.texture.clone())
            .filter(|t| !self.textures.contains_key(t))
            .collect();
        out.sort();
        out.dedup();
        out
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        adorns: &Adorns,
        camera: Vec3,
        fov_y: f32,
        (width, height): (u32, u32),
        load: &dyn Fn(&str) -> Option<crate::texture::Image>,
    ) {
        self.frame = None;
        if adorns.is_empty() {
            return;
        }
        for url in self.missing_textures(adorns) {
            let group = load(&url).map(|img| {
                let view = img
                    .upload(device, queue, "adorn image", true)
                    .create_view(&wgpu::TextureViewDescriptor::default());
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("adorn image"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: self.pixel.as_entire_binding(),
                        },
                    ],
                })
            });
            self.textures.insert(url, group);
        }
        let (w, h) = (width.max(1) as f32, height.max(1) as f32);
        queue.write_buffer(
            &self.pixel,
            0,
            bytemuck::cast_slice(&[(fov_y * 0.5).tan() / h, w, h / w, 0.0]),
        );
        let mut opaque = Vec::new();
        let mut transparent = Vec::new();
        let mut layers: Vec<Vec<(f32, usize)>> = (0..=10).map(|_| Vec::new()).collect();
        for (i, m) in adorns.meshes.iter().enumerate() {
            if m.texture
                .as_ref()
                .is_some_and(|t| self.textures.get(t).is_none_or(|g| g.is_none()))
            {
                continue;
            }
            let d = (camera - m.translation).length_squared();
            if (0..=10).contains(&m.z_index) {
                layers[m.z_index as usize].push((d, i));
            } else if m.color[3] < 1.0 || m.material == Material::AaLine {
                transparent.push((d, i));
            } else {
                opaque.push(i);
            }
        }
        opaque.sort_by_key(|&i| adorns.meshes[i].material as u8);
        transparent.sort_by(|a, b| b.0.total_cmp(&a.0));
        for layer in &mut layers {
            layer.sort_by(|a, b| b.0.total_cmp(&a.0));
        }
        let mut instances = Vec::new();
        let mut draws = Vec::new();
        let push_mesh = |m: &AdornMesh,
                         key: PipelineKey,
                         instances: &mut Vec<AdornInstance>,
                         draws: &mut Vec<Draw>| {
            let instance = instances.len() as u32;
            instances.push(AdornInstance {
                rows: m.rows(),
                color: m.color,
                params: [m.material as u8 as f32, m.thickness, 0.0, 0.0],
            });
            draws.push(Draw {
                key,
                mesh: mesh_key(m),
                texture: m.texture.clone(),
                instance,
            });
        };
        let key = |blend: bool, depth_write: bool, always: bool, m: &AdornMesh| PipelineKey {
            blend,
            depth_write,
            always,
            lines: false,
            cull: m.shape != Shape::Quad && m.material != Material::Outline,
        };
        for &i in &opaque {
            let m = &adorns.meshes[i];
            push_mesh(m, key(false, true, false, m), &mut instances, &mut draws);
        }
        for &(_, i) in &transparent {
            let m = &adorns.meshes[i];
            push_mesh(m, key(true, true, false, m), &mut instances, &mut draws);
        }
        for line in &adorns.lines {
            let instance = instances.len() as u32;
            let half = (line.b - line.a) * 0.5;
            let c = (line.a + line.b) * 0.5;
            instances.push(AdornInstance {
                rows: [
                    [half.x, 0.0, 0.0, c.x],
                    [half.y, 0.0, 0.0, c.y],
                    [half.z, 0.0, 0.0, c.z],
                ],
                color: line.color,
                params: [Material::NoLighting as u8 as f32, 0.0, 0.0, 0.0],
            });
            draws.push(Draw {
                key: PipelineKey {
                    blend: line.color[3] < 1.0,
                    depth_write: true,
                    always: false,
                    lines: true,
                    cull: false,
                },
                mesh: MeshKey::Line,
                texture: None,
                instance,
            });
        }
        for layer in &layers {
            for &(_, i) in layer {
                let m = &adorns.meshes[i];
                push_mesh(
                    m,
                    key(true, false, m.always_on_top, m),
                    &mut instances,
                    &mut draws,
                );
            }
        }
        if instances.is_empty() {
            return;
        }
        let keys: Vec<PipelineKey> = draws.iter().map(|d| d.key).collect();
        for k in keys {
            self.pipeline(device, k);
        }
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("adorn instances"),
            contents: bytemuck::cast_slice(&instances),
            usage: wgpu::BufferUsages::VERTEX,
        });
        self.frame = Some((buffer, draws));
    }

    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        let Some((buffer, draws)) = &self.frame else {
            return;
        };
        pass.set_vertex_buffer(1, buffer.slice(..));
        let mut current_key = None;
        let mut current_mesh = None;
        let mut current_texture: Option<Option<&String>> = None;
        for d in draws {
            if current_key != Some(d.key) {
                pass.set_pipeline(&self.pipelines[&d.key]);
                current_key = Some(d.key);
            }
            if current_texture != Some(d.texture.as_ref()) {
                let group = match &d.texture {
                    Some(t) => match self.textures.get(t) {
                        Some(Some(g)) => g,
                        _ => continue,
                    },
                    None => &self.white,
                };
                pass.set_bind_group(1, group, &[]);
                current_texture = Some(d.texture.as_ref());
            }
            let mesh = &self.meshes[&d.mesh];
            if current_mesh != Some(d.mesh) {
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint16);
                current_mesh = Some(d.mesh);
            }
            pass.draw_indexed(0..mesh.count, 0, d.instance..d.instance + 1);
        }
    }
}
