use crate::geometry::{BlockParams, Mesh, Vertex};
use crate::scene::{self, PartDraw, Scene, Shading, Shape};
use crate::texture::{self, Image};
use bytemuck::{Pod, Zeroable};
use rbx_dom::DataModel;
use rbx_math::{Mat3, Vec2, Vec3};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use wgpu::util::DeviceExt;

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub const SHADING_DISTANCE: f32 = 300.0;
const OUTLINE_THICKNESS: f32 = 40.0;
const OUTLINE_BRIGHTNESS_MIN: f32 = 50.0;
const OUTLINE_BRIGHTNESS_MAX: f32 = 160.0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Globals {
    pub view_projection: [[f32; 4]; 4],
    pub view_right: [f32; 4],
    pub view_up: [f32; 4],
    pub view_dir: [f32; 4],
    pub camera_position: [f32; 4],
    pub ambient_color: [f32; 4],
    pub lamp0_color: [f32; 4],
    pub lamp0_dir: [f32; 4],
    pub lamp1_color: [f32; 4],
    pub fog_color: [f32; 4],
    pub fog_params: [f32; 4],
    pub light_border: [f32; 4],
    pub fade_distance_glow: [f32; 4],
    pub outline_brightness_shadow: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct InstanceData {
    pub rows: [[f32; 4]; 3],
    pub color: [f32; 4],
    pub extra: [f32; 4],
}

impl InstanceData {
    const ATTRIBUTES: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
        6 => Float32x4,
        7 => Float32x4,
        8 => Float32x4,
        9 => Float32x4,
        10 => Float32x4,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct MeshKey {
    shape: Shape,
    size: [u32; 3],
    surfaces: [u8; 6],
    ignore_studs: bool,
    tiling: u32,
    file: u64,
    decal: u8,
}

struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
}

pub struct Renderer {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub format: wgpu::TextureFormat,
    globals: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    opaque: wgpu::RenderPipeline,
    transparent: wgpu::RenderPipeline,
    sky: wgpu::RenderPipeline,
    material_layout: wgpu::BindGroupLayout,
    sky_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    materials: HashMap<Shading, wgpu::BindGroup>,
    image_names: Vec<String>,
    image_index: HashMap<String, u32>,
    file_meshes: HashMap<String, Option<crate::filemesh::FileMesh>>,
    pub asset_dirs: Vec<PathBuf>,
    sky_bg: Option<(Vec<String>, wgpu::BindGroup)>,
    meshes: HashMap<MeshKey, GpuMesh>,
    depth: Option<(wgpu::Texture, wgpu::TextureView, u32, u32)>,
    content_dir: PathBuf,
    platform_dir: PathBuf,
    white: wgpu::TextureView,
}

fn read_texture(path: &Path) -> Option<Image> {
    let bytes = std::fs::read(path).ok()?;
    match texture::decode(&bytes) {
        Ok(img) => Some(img),
        Err(e) => {
            tracing::warn!("{}: {e}", path.display());
            None
        }
    }
}

impl Renderer {
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        content_dir: &Path,
        platform_dir: &Path,
    ) -> Renderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("default"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/default.wgsl").into()),
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("linear"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
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
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let sky_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let studs = read_texture(&platform_dir.join("textures").join("studs.dds"))
            .unwrap_or_else(|| Image::solid([128, 128, 128, 255]));
        let studs_view = studs
            .upload(&device, &queue, "studs", false)
            .create_view(&wgpu::TextureViewDescriptor::default());
        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: globals.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&studs_view),
                },
            ],
        });
        let part_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("parts"),
            bind_group_layouts: &[Some(&globals_layout), Some(&material_layout)],
            immediate_size: 0,
        });
        let sky_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sky"),
            bind_group_layouts: &[Some(&globals_layout), Some(&sky_layout)],
            immediate_size: 0,
        });
        let buffers = [
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &Vertex::ATTRIBUTES,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<InstanceData>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &InstanceData::ATTRIBUTES,
            }),
        ];
        let make_parts = |blend: Option<wgpu::BlendState>, depth_write: bool, label: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&part_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
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
                    cull_mode: Some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let opaque = make_parts(None, true, "opaque");
        let transparent = make_parts(Some(wgpu::BlendState::ALPHA_BLENDING), false, "transparent");
        let sky = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky"),
            layout: Some(&sky_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_sky"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_sky"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let white = Image::solid([255, 255, 255, 255])
            .upload(&device, &queue, "white", false)
            .create_view(&wgpu::TextureViewDescriptor::default());
        Renderer {
            device,
            queue,
            format,
            globals,
            globals_bg,
            opaque,
            transparent,
            sky,
            material_layout,
            sky_layout,
            sampler,
            materials: HashMap::new(),
            image_names: Vec::new(),
            image_index: HashMap::new(),
            file_meshes: HashMap::new(),
            asset_dirs: Vec::new(),
            sky_bg: None,
            meshes: HashMap::new(),
            depth: None,
            content_dir: content_dir.to_path_buf(),
            platform_dir: platform_dir.to_path_buf(),
            white,
        }
    }

    fn material_group(&mut self, shading: Shading) -> &wgpu::BindGroup {
        if !self.materials.contains_key(&shading) {
            let view = match shading {
                Shading::Textured(m) => scene::material_folder(m)
                    .and_then(|f| {
                        read_texture(
                            &self
                                .platform_dir
                                .join("textures")
                                .join(f)
                                .join("diffuse.dds"),
                        )
                    })
                    .map(|img| {
                        img.upload(&self.device, &self.queue, "material", false)
                            .create_view(&wgpu::TextureViewDescriptor::default())
                    }),
                Shading::Image(i) => self
                    .image_names
                    .get(i as usize)
                    .and_then(|u| self.read_content(u))
                    .and_then(|b| texture::decode(&b).ok())
                    .map(|img| {
                        img.upload(&self.device, &self.queue, "image", false)
                            .create_view(&wgpu::TextureViewDescriptor::default())
                    }),
                _ => None,
            };
            let view = view.unwrap_or_else(|| self.white.clone());
            let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("material"),
                layout: &self.material_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                }],
            });
            self.materials.insert(shading, bg);
        }
        &self.materials[&shading]
    }

    fn image(&mut self, url: &str) -> Shading {
        if let Some(&i) = self.image_index.get(url) {
            return Shading::Image(i);
        }
        let i = self.image_names.len() as u32;
        self.image_names.push(url.to_string());
        self.image_index.insert(url.to_string(), i);
        Shading::Image(i)
    }

    fn read_content(&self, url: &str) -> Option<Vec<u8>> {
        if let Some(p) = self.resolve_content(url) {
            return std::fs::read(p).ok();
        }
        let lower = url.trim().to_ascii_lowercase();
        let id: u64 = if let Some(rest) = lower.strip_prefix("rbxassetid://") {
            rest.trim().parse().ok()?
        } else {
            let at = lower.find("id=")?;
            lower[at + 3..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .ok()?
        };
        self.asset_dirs
            .iter()
            .find_map(|d| std::fs::read(d.join(id.to_string())).ok())
    }

    fn file_mesh(&mut self, url: &str) -> Option<&crate::filemesh::FileMesh> {
        if !self.file_meshes.contains_key(url) {
            let mesh = self
                .read_content(url)
                .and_then(|b| match crate::filemesh::read(&b) {
                    Ok(m) => Some(m),
                    Err(e) => {
                        tracing::warn!("{url}: {e}");
                        None
                    }
                });
            self.file_meshes.insert(url.to_string(), mesh);
        }
        self.file_meshes.get(url).and_then(Option::as_ref)
    }

    fn upload_mesh(&mut self, key: MeshKey, mesh: &Mesh) {
        let vertices = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh vertices"),
                contents: bytemuck::cast_slice(&mesh.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let indices = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh indices"),
                contents: bytemuck::cast_slice(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        self.meshes.insert(
            key,
            GpuMesh {
                vertices,
                indices,
                count: mesh.indices.len() as u32,
            },
        );
    }

    fn decal_mesh(&mut self, size: Vec3, face: usize, tile: Option<(f32, f32)>) -> MeshKey {
        let key = MeshKey {
            shape: Shape::Block,
            size: [size.x.to_bits(), size.y.to_bits(), size.z.to_bits()],
            surfaces: [0; 6],
            ignore_studs: true,
            tiling: tile.map_or(0, |t| t.0.to_bits() ^ t.1.to_bits().rotate_left(7)),
            file: 0,
            decal: face as u8 + 1,
        };
        if !self.meshes.contains_key(&key) {
            let mesh = Mesh::decal(size, face, tile);
            self.upload_mesh(key, &mesh);
        }
        key
    }

    fn resolve_content(&self, url: &str) -> Option<PathBuf> {
        let lower = url.to_ascii_lowercase();
        let rest = lower.strip_prefix("rbxasset://")?;
        let rel = &url[url.len() - rest.len()..];
        let mut path = self.content_dir.clone();
        for part in rel
            .split(['/', '\\'])
            .filter(|s| !s.is_empty() && *s != "..")
        {
            path.push(part);
        }
        if path.is_file() {
            return Some(path);
        }
        let mut alt = self.platform_dir.clone();
        for part in rel
            .split(['/', '\\'])
            .filter(|s| !s.is_empty() && *s != "..")
        {
            alt.push(part);
        }
        alt.is_file().then_some(alt)
    }

    fn sky_group(&mut self, faces: Option<&[String; 6]>) -> &wgpu::BindGroup {
        let default: [String; 6] = [
            "rbxasset://textures/sky/sky512_rt.tex",
            "rbxasset://textures/sky/sky512_lf.tex",
            "rbxasset://textures/sky/sky512_bk.tex",
            "rbxasset://textures/sky/sky512_ft.tex",
            "rbxasset://textures/sky/sky512_up.tex",
            "rbxasset://textures/sky/sky512_dn.tex",
        ]
        .map(String::from);
        let wanted: Vec<String> = match faces {
            Some(f) if f.iter().all(|s| !s.is_empty()) => f.to_vec(),
            _ => default.to_vec(),
        };
        let stale = self.sky_bg.as_ref().is_none_or(|(k, _)| *k != wanted);
        if stale {
            let mut images: Vec<Image> = wanted
                .iter()
                .zip(default.iter())
                .map(|(u, d)| {
                    self.resolve_content(u)
                        .and_then(|p| read_texture(&p))
                        .or_else(|| self.resolve_content(d).and_then(|p| read_texture(&p)))
                        .unwrap_or_else(|| Image::solid([128, 160, 220, 255]))
                })
                .collect();
            let size = images.iter().map(|i| i.width).max().unwrap_or(1);
            for img in images.iter_mut() {
                if img.width != size || img.height != size {
                    *img = resize(img, size);
                }
            }
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("sky"),
                size: wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: 6,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            for (layer, img) in images.iter().enumerate() {
                self.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: 0,
                            y: 0,
                            z: layer as u32,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &img.levels[0],
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(size * 4),
                        rows_per_image: Some(size),
                    },
                    wgpu::Extent3d {
                        width: size,
                        height: size,
                        depth_or_array_layers: 1,
                    },
                );
            }
            let view = texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            });
            let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sky"),
                layout: &self.sky_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                }],
            });
            self.sky_bg = Some((wanted, bg));
        }
        match &self.sky_bg {
            Some((_, bg)) => bg,
            None => unreachable!(),
        }
    }

    fn mesh_for(&mut self, part: &PartDraw) -> (MeshKey, Vec3) {
        let size = part.size * part.mesh_scale;
        let ignore_studs = !matches!(part.shading, Shading::Plastic);
        let tiling = match part.shading {
            Shading::Textured(m) => scene::tiling(m),
            _ => 1.0,
        };
        let file_hash = part.file.as_ref().map_or(0, |f| {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            f.url.hash(&mut h);
            [
                f.scale.x.to_bits(),
                f.scale.y.to_bits(),
                f.scale.z.to_bits(),
            ]
            .hash(&mut h);
            h.finish() | 1
        });
        let key = MeshKey {
            shape: part.shape,
            size: [size.x.to_bits(), size.y.to_bits(), size.z.to_bits()],
            surfaces: part.surfaces.map(|s| s as u8),
            ignore_studs,
            tiling: tiling.to_bits(),
            file: file_hash,
            decal: 0,
        };
        if !self.meshes.contains_key(&key) {
            let file_mesh = match &part.file {
                Some(f) => {
                    let scale = f.scale;
                    let url = f.url.clone();
                    self.file_mesh(&url).map(|m| Mesh::from_file(m, scale))
                }
                None => None,
            };
            let block = |ignore_studs| {
                Mesh::block(&BlockParams {
                    size,
                    surfaces: part.surfaces,
                    ignore_studs,
                    surface_offset: Vec2::ZERO,
                    tiling,
                })
            };
            let mesh = match part.shape {
                Shape::File => file_mesh.unwrap_or_else(|| block(true)),
                Shape::Block => block(ignore_studs),
                Shape::Wedge => Mesh::wedge(size, &part.surfaces, tiling),
                Shape::CornerWedge => Mesh::corner_wedge(size, &part.surfaces, tiling),
                Shape::Ball => Mesh::sphere(size, tiling),
                Shape::Cylinder => Mesh::cylinder(size, tiling),
            };
            self.upload_mesh(key, &mesh);
        }
        (key, size)
    }

    fn ensure_depth(&mut self, width: u32, height: u32) {
        if self
            .depth
            .as_ref()
            .is_some_and(|d| d.2 == width && d.3 == height)
        {
            return;
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.depth = Some((texture, view, width, height));
    }

    pub fn render(&mut self, dm: &DataModel, target: &wgpu::TextureView, width: u32, height: u32) {
        let aspect = width.max(1) as f32 / height.max(1) as f32;
        let scene = scene::extract(dm, aspect);
        self.render_scene(&scene, target, width, height);
    }

    pub fn render_scene(
        &mut self,
        scene: &Scene,
        target: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) {
        let width = width.max(1);
        let height = height.max(1);
        self.ensure_depth(width, height);
        let aspect = width as f32 / height as f32;
        let cam = &scene.camera;
        let rot: Mat3 = cam.cframe.rotation;
        let eye = cam.cframe.translation;
        let look = -rot.z_axis;
        let vp = mat_mul(
            &perspective(cam.fov_y, aspect, cam.near, cam.far),
            &look_to(eye, look, rot.y_axis),
        );
        let tan_y = (cam.fov_y * 0.5).tan();
        let l = &scene.lighting;
        let fov_coeff = 1.0 / tan_y;
        let outline_scaling =
            fov_coeff * height as f32 * (10.0 / OUTLINE_THICKNESS) * 0.5 / SHADING_DISTANCE;
        let (mul, add) = if l.outlines {
            (
                (OUTLINE_BRIGHTNESS_MAX - OUTLINE_BRIGHTNESS_MIN) / 255.0,
                OUTLINE_BRIGHTNESS_MIN / 255.0,
            )
        } else {
            (0.0, 1.0)
        };
        let fog_range = l.fog_end - l.fog_start;
        let globals = Globals {
            view_projection: vp,
            view_right: [rot.x_axis.x, rot.x_axis.y, rot.x_axis.z, tan_y * aspect],
            view_up: [rot.y_axis.x, rot.y_axis.y, rot.y_axis.z, tan_y],
            view_dir: [look.x, look.y, look.z, 0.0],
            camera_position: [eye.x, eye.y, eye.z, 1.0],
            ambient_color: [l.ambient.r, l.ambient.g, l.ambient.b, 1.0],
            lamp0_color: [l.key_color.r, l.key_color.g, l.key_color.b, 1.0],
            lamp0_dir: [l.key_direction.x, l.key_direction.y, l.key_direction.z, 0.0],
            lamp1_color: [l.fill_color.r, l.fill_color.g, l.fill_color.b, 1.0],
            fog_color: [l.fog_color.r, l.fog_color.g, l.fog_color.b, 1.0],
            fog_params: [
                0.0,
                l.fog_start,
                l.fog_end,
                if fog_range != 0.0 {
                    1.0 / fog_range
                } else {
                    0.0
                },
            ],
            light_border: [0.0, 0.0, 0.0, 1.0],
            fade_distance_glow: [
                SHADING_DISTANCE,
                1.0 / SHADING_DISTANCE,
                outline_scaling,
                1.0,
            ],
            outline_brightness_shadow: [mul, add, 0.0, l.shadow_intensity],
        };
        self.queue
            .write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));

        let mut opaque: HashMap<(Shading, MeshKey), Vec<InstanceData>> = HashMap::new();
        let mut transparent: Vec<(f32, Shading, MeshKey, InstanceData)> = Vec::new();
        for part in &scene.parts {
            let (key, size) = self.mesh_for(part);
            let shading = match &part.texture {
                Some(t) if part.shape == Shape::File => self.image(t),
                _ => part.shading,
            };
            let kind = match shading {
                Shading::Plastic | Shading::SmoothPlastic => 0.0,
                Shading::Textured(_) => 1.0,
                Shading::Neon => 2.0,
                Shading::Image(_) => 3.0,
            };
            let rows = scene::instance_rows(&part.cframe, Vec3::ONE, part.mesh_offset);
            let data = InstanceData {
                rows,
                color: part.color,
                extra: [part.specular.0, part.specular.1, part.reflectance, kind],
            };
            let d = (part.cframe.translation - eye).length_squared();
            if part.color[3] < 1.0 {
                transparent.push((d, shading, key, data));
            } else {
                opaque.entry((shading, key)).or_default().push(data);
            }
            for decal in &part.decals {
                let decal_shading = self.image(&decal.texture);
                let decal_key = if part.shape == Shape::File {
                    key
                } else {
                    self.decal_mesh(size, decal.face, decal.tile)
                };
                let decal_data = InstanceData {
                    rows,
                    color: [1.0, 1.0, 1.0, (1.0 - decal.transparency) * part.color[3]],
                    extra: [part.specular.0, part.specular.1, 0.0, 3.0],
                };
                transparent.push((d - 0.001, decal_shading, decal_key, decal_data));
            }
        }
        transparent.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let shadings: Vec<Shading> = opaque
            .keys()
            .map(|k| k.0)
            .chain(transparent.iter().map(|t| t.1))
            .collect();
        for s in shadings {
            self.material_group(s);
        }
        self.sky_group(scene.sky_faces.as_ref());

        let mut batches: Vec<(Shading, MeshKey, u32, u32)> = Vec::new();
        let mut instances: Vec<InstanceData> = Vec::new();
        for ((shading, key), list) in &opaque {
            batches.push((*shading, *key, instances.len() as u32, list.len() as u32));
            instances.extend_from_slice(list);
        }
        let transparent_start = instances.len() as u32;
        for t in &transparent {
            instances.push(t.3);
        }
        let instance_buffer = (!instances.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("instances"),
                    contents: bytemuck::cast_slice(&instances),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        {
            let clear = l.clear_color;
            let depth_view = match &self.depth {
                Some(d) => &d.1,
                None => return,
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear.r as f64,
                            g: clear.g as f64,
                            b: clear.b as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.globals_bg, &[]);
            if let Some(buf) = &instance_buffer {
                pass.set_pipeline(&self.opaque);
                pass.set_vertex_buffer(1, buf.slice(..));
                for (shading, key, start, count) in &batches {
                    let mesh = &self.meshes[key];
                    pass.set_bind_group(1, &self.materials[shading], &[]);
                    pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                    pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.count, 0, *start..start + count);
                }
            }
            if let Some((_, sky)) = &self.sky_bg {
                pass.set_pipeline(&self.sky);
                pass.set_bind_group(1, sky, &[]);
                pass.draw(0..3, 0..1);
            }
            if let Some(buf) = &instance_buffer
                && !transparent.is_empty()
            {
                pass.set_pipeline(&self.transparent);
                pass.set_bind_group(0, &self.globals_bg, &[]);
                pass.set_vertex_buffer(1, buf.slice(..));
                for (i, (_, shading, key, _)) in transparent.iter().enumerate() {
                    let mesh = &self.meshes[key];
                    pass.set_bind_group(1, &self.materials[shading], &[]);
                    pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                    pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                    let index = transparent_start + i as u32;
                    pass.draw_indexed(0..mesh.count, 0, index..index + 1);
                }
            }
        }
        self.queue.submit([encoder.finish()]);
        let _ = &self.sampler;
    }
}

fn resize(img: &Image, size: u32) -> Image {
    let mut out = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let sx = (x * img.width / size).min(img.width - 1);
            let sy = (y * img.height / size).min(img.height - 1);
            let si = ((sy * img.width + sx) * 4) as usize;
            let di = ((y * size + x) * 4) as usize;
            out[di..di + 4].copy_from_slice(&img.levels[0][si..si + 4]);
        }
    }
    Image {
        width: size,
        height: size,
        levels: vec![out],
    }
}

fn look_to(eye: Vec3, dir: Vec3, up: Vec3) -> [[f32; 4]; 4] {
    let f = dir.normalize_or(Vec3::NEG_Z);
    let s = f.cross(up).normalize_or(Vec3::X);
    let u = s.cross(f);
    [
        [s.x, u.x, -f.x, 0.0],
        [s.y, u.y, -f.y, 0.0],
        [s.z, u.z, -f.z, 0.0],
        [-s.dot(eye), -u.dot(eye), f.dot(eye), 1.0],
    ]
}

fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> [[f32; 4]; 4] {
    let f = 1.0 / (fov_y * 0.5).tan();
    let r = far / (near - far);
    [
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, r, -1.0],
        [0.0, 0.0, r * near, 0.0],
    ]
}

fn mat_mul(a: &[[f32; 4]; 4], b: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut out = [[0.0f32; 4]; 4];
    for (c, col) in out.iter_mut().enumerate() {
        for (r, v) in col.iter_mut().enumerate() {
            *v = (0..4).map(|k| a[k][r] * b[c][k]).sum();
        }
    }
    out
}
