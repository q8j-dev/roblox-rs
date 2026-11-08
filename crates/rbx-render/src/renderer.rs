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
pub const SAMPLE_COUNT: u32 = 4;

pub fn msaa_state() -> wgpu::MultisampleState {
    wgpu::MultisampleState {
        count: SAMPLE_COUNT,
        ..Default::default()
    }
}
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
    pub lgrid_scale: [f32; 4],
    pub lgrid_offset: [f32; 4],
    pub lgrid_center: [f32; 4],
    pub lgrid_radius: [f32; 4],
    pub shadow_matrix: [[f32; 4]; 3],
    pub sky_color: [f32; 4],
    pub sky_color2: [f32; 4],
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
    pub async_loads: bool,
    load_tx: std::sync::mpsc::Sender<(Shading, Option<Image>)>,
    load_rx: std::sync::mpsc::Receiver<(Shading, Option<Image>)>,
    pub gui_scale: f32,
    terrain_meshes: Vec<GpuMesh>,
    terrain_generation: Option<u64>,
    smooth: crate::smooth::SmoothTerrain,
    gui: Option<crate::gui_pass::GuiRenderer>,
    compositor: crate::composite::Compositor,
    lightgrid: crate::lightgrid::LightGrid,
    shadow: crate::shadow::ShadowMap,
    shadow_pipeline: wgpu::RenderPipeline,
    shadow_globals_bg: wgpu::BindGroup,
    celestial_add: wgpu::RenderPipeline,
    celestial_alpha: wgpu::RenderPipeline,
    stars: crate::celestial::StarField,
    particles: crate::particles::ParticleSystem,
    particle_pass: crate::particle_pass::ParticlePass,
    adorn_pass: crate::adorn_pass::AdornPass,
    pub frame_dt: f32,
    lgrid_texture: wgpu::Texture,
    lgrid_active: bool,
    pub lighting_budget: usize,
    sky_bg: Option<(Vec<String>, wgpu::BindGroup)>,
    meshes: HashMap<MeshKey, GpuMesh>,
    depth: Option<(wgpu::Texture, wgpu::TextureView, u32, u32)>,
    msaa_color: Option<(wgpu::Texture, wgpu::TextureView)>,
    content_dir: PathBuf,
    platform_dir: PathBuf,
    white: wgpu::TextureView,
}

pub(crate) fn read_texture(path: &Path) -> Option<Image> {
    read_texture_gpu(path, false)
}

pub(crate) fn read_texture_gpu(path: &Path, compressed: bool) -> Option<Image> {
    let bytes = rbx_base::vfs::read(path).ok()?;
    match texture::decode_for_gpu(&bytes, compressed) {
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
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let shadow_mask = read_texture(&content_dir.join("textures").join("shadowmask.png"));
        let shadow = crate::shadow::ShadowMap::new(&device, &queue, shadow_mask);
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow map"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let lgrid_size = crate::lightgrid::LightGrid::texture_size();
        let lgrid_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("light grid"),
            size: wgpu::Extent3d {
                width: lgrid_size.x as u32,
                height: lgrid_size.y as u32,
                depth_or_array_layers: lgrid_size.z as u32,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let lgrid_view = lgrid_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let lgrid_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("light grid"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
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
        let compressed = device
            .features()
            .contains(wgpu::Features::TEXTURE_COMPRESSION_BC);
        let studs = read_texture_gpu(&platform_dir.join("textures").join("studs.dds"), compressed)
            .unwrap_or_else(|| Image::solid([128, 128, 128, 255]));
        let studs_view = studs
            .upload(&device, &queue, "studs", false)
            .create_view(&wgpu::TextureViewDescriptor::default());
        let shadow_placeholder = Image::solid([255, 0, 0, 0])
            .upload(&device, &queue, "shadow placeholder", false)
            .create_view(&wgpu::TextureViewDescriptor::default());
        let make_globals = |shadow_view: &wgpu::TextureView, label: &str| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
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
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&lgrid_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::Sampler(&lgrid_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::TextureView(shadow_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::Sampler(&shadow_sampler),
                    },
                ],
            })
        };
        let globals_bg = make_globals(shadow.view(), "globals");
        let shadow_globals_bg = make_globals(&shadow_placeholder, "shadow globals");
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
                multisample: crate::renderer::msaa_state(),
                multiview_mask: None,
                cache: None,
            })
        };
        let shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow casters"),
            bind_group_layouts: &[Some(&globals_layout)],
            immediate_size: 0,
        });
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow casters"),
            layout: Some(&shadow_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_shadow"),
                compilation_options: Default::default(),
                buffers: &buffers,
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_shadow"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: crate::shadow::SHADOW_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let celestial_buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<crate::celestial::CelestialVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &crate::celestial::CelestialVertex::ATTRIBUTES,
        })];
        let make_celestial = |blend: wgpu::BlendState, label: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&part_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_celestial"),
                    compilation_options: Default::default(),
                    buffers: &celestial_buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_celestial"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: crate::renderer::msaa_state(),
                multiview_mask: None,
                cache: None,
            })
        };
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let particle_pass =
            crate::particle_pass::ParticlePass::new(&device, &queue, &globals_layout, format);
        let celestial_add = make_celestial(additive, "sun and moon");
        let celestial_alpha = make_celestial(wgpu::BlendState::ALPHA_BLENDING, "stars");
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
            multisample: crate::renderer::msaa_state(),
            multiview_mask: None,
            cache: None,
        });
        let smooth =
            crate::smooth::SmoothTerrain::new(&device, &globals_layout, format, DEPTH_FORMAT);
        let gui = Some(crate::gui_pass::GuiRenderer::new(&device, &queue, format));
        let compositor = crate::composite::Compositor::new(&device, &queue);
        let white = Image::solid([255, 255, 255, 255])
            .upload(&device, &queue, "white", false)
            .create_view(&wgpu::TextureViewDescriptor::default());
        let adorn_pass =
            crate::adorn_pass::AdornPass::new(&device, &globals_layout, format, &white);
        let (load_tx, load_rx) = std::sync::mpsc::channel();
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
            async_loads: false,
            load_tx,
            load_rx,
            gui_scale: 1.0,
            terrain_meshes: Vec::new(),
            terrain_generation: None,
            smooth,
            gui,
            compositor,
            lightgrid: crate::lightgrid::LightGrid::new(),
            shadow,
            shadow_pipeline,
            shadow_globals_bg,
            celestial_add,
            celestial_alpha,
            stars: crate::celestial::StarField::default(),
            particles: crate::particles::ParticleSystem::default(),
            particle_pass,
            adorn_pass,
            frame_dt: 0.0,
            lgrid_texture,
            lgrid_active: false,
            lighting_budget: 24,
            sky_bg: None,
            meshes: HashMap::new(),
            depth: None,
            msaa_color: None,
            content_dir: content_dir.to_path_buf(),
            platform_dir: platform_dir.to_path_buf(),
            white,
        }
    }

    fn texture_job(&self, shading: Shading) -> Option<Box<dyn FnOnce() -> Option<Image> + Send>> {
        let compressed = self.compressed();
        let textures = self.platform_dir.join("textures");
        match shading {
            Shading::Textured(m) => {
                let folder = scene::material_folder(m)?;
                let path = textures.join(folder).join("diffuse.dds");
                Some(Box::new(move || read_texture_gpu(&path, compressed)))
            }
            Shading::Terrain => {
                let path = textures.join("terrain").join("diffuse.dds");
                Some(Box::new(move || read_texture_gpu(&path, compressed)))
            }
            Shading::Image(i) => {
                let url = self.image_names.get(i as usize)?.clone();
                let content = self.content_dir.clone();
                let platform = self.platform_dir.clone();
                let assets = self.asset_dirs.clone();
                Some(Box::new(move || {
                    read_content_in(&content, &platform, &assets, &url)
                        .and_then(|b| texture::decode_for_gpu(&b, compressed).ok())
                }))
            }
            _ => None,
        }
    }

    fn texture_bind_group(&self, view: &wgpu::TextureView) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("material"),
            layout: &self.material_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            }],
        })
    }

    fn poll_texture_loads(&mut self) {
        while let Ok((shading, image)) = self.load_rx.try_recv() {
            let view = image
                .map(|img| {
                    img.upload(&self.device, &self.queue, "material", false)
                        .create_view(&wgpu::TextureViewDescriptor::default())
                })
                .unwrap_or_else(|| self.white.clone());
            let bg = self.texture_bind_group(&view);
            self.materials.insert(shading, bg);
        }
    }

    fn material_group(&mut self, shading: Shading) -> &wgpu::BindGroup {
        if self.async_loads
            && !self.materials.contains_key(&shading)
            && let Some(job) = self.texture_job(shading)
        {
            let tx = self.load_tx.clone();
            rbx_base::jobs::spawn(move || {
                let _ = tx.send((shading, job()));
            });
            let placeholder = self.texture_bind_group(&self.white.clone());
            self.materials.insert(shading, placeholder);
        }
        if !self.materials.contains_key(&shading) {
            let view = match shading {
                Shading::Textured(m) => scene::material_folder(m)
                    .and_then(|f| {
                        read_texture_gpu(
                            &self
                                .platform_dir
                                .join("textures")
                                .join(f)
                                .join("diffuse.dds"),
                            self.compressed(),
                        )
                    })
                    .map(|img| {
                        img.upload(&self.device, &self.queue, "material", false)
                            .create_view(&wgpu::TextureViewDescriptor::default())
                    }),
                Shading::Terrain => read_texture_gpu(
                    &self
                        .platform_dir
                        .join("textures")
                        .join("terrain")
                        .join("diffuse.dds"),
                    self.compressed(),
                )
                .map(|img| {
                    img.upload(&self.device, &self.queue, "terrain", false)
                        .create_view(&wgpu::TextureViewDescriptor::default())
                }),
                Shading::Image(i) => self
                    .image_names
                    .get(i as usize)
                    .and_then(|u| self.read_content(u))
                    .and_then(|b| texture::decode_for_gpu(&b, self.compressed()).ok())
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

    fn update_lightgrid(&mut self, dm: &DataModel, scene: &Scene) {
        let l = &scene.lighting;
        if !l.global_shadows && scene.lights.is_empty() {
            self.lgrid_active = false;
            return;
        }
        self.lgrid_active = true;
        let parts: Vec<crate::lightgrid::VoxelPart> = scene
            .parts
            .iter()
            .filter_map(|p| {
                p.voxel.map(|shape| crate::lightgrid::VoxelPart {
                    shape,
                    size: p.size,
                    cframe: p.cframe,
                    transparency: p.voxel_transparency,
                })
            })
            .collect();
        let terrain = dm.workspace_terrain().and_then(|t| dm.terrain(t));
        let generation = terrain.map_or(0, |t| {
            t.smooth.as_ref().map_or(0, |g| g.changes.generation())
                ^ t.legacy
                    .as_ref()
                    .map_or(0, |g| g.changes.generation() << 32)
                ^ (t as *const rbx_dom::TerrainData as usize as u64)
        });
        let fill = |chunk: rbx_math::IVec3, out: &mut [u8]| {
            crate::lightgrid_terrain::fill(terrain, chunk, out);
        };
        let to8 = |c: f32| (c.clamp(0.0, 1.0) * 255.0) as u8;
        let inputs = crate::lightgrid::LightInputs {
            focus: scene.focus,
            parts: &parts,
            lights: &scene.lights,
            terrain: &fill,
            terrain_generation: generation,
            light_direction: l.key_direction,
            sky_ambient: [
                to8(l.sky_ambient.r),
                to8(l.sky_ambient.g),
                to8(l.sky_ambient.b),
            ],
            shadows: l.global_shadows,
        };
        self.lightgrid.update(&inputs, self.lighting_budget);
        for (index, data) in self.lightgrid.uploads.drain(..) {
            let w = crate::lightgrid::LightGrid::wrapped(index);
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.lgrid_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: (w.x * crate::lightgrid::CHUNK_XZ) as u32,
                        y: (w.z * crate::lightgrid::CHUNK_XZ) as u32,
                        z: (w.y * crate::lightgrid::CHUNK_Y) as u32,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(crate::lightgrid::CHUNK_XZ as u32 * 4),
                    rows_per_image: Some(crate::lightgrid::CHUNK_XZ as u32),
                },
                wgpu::Extent3d {
                    width: crate::lightgrid::CHUNK_XZ as u32,
                    height: crate::lightgrid::CHUNK_XZ as u32,
                    depth_or_array_layers: crate::lightgrid::CHUNK_Y as u32,
                },
            );
        }
    }

    fn update_composites(&mut self, composites: &[crate::character::CompositeDesc]) {
        for desc in composites {
            let shading = self.image(&desc.url);
            if self.materials.contains_key(&shading) {
                continue;
            }
            let mut meshes: HashMap<String, Option<crate::filemesh::FileMesh>> = HashMap::new();
            for l in &desc.layers {
                if !meshes.contains_key(&l.mesh) {
                    let m = self.file_mesh(&l.mesh).cloned();
                    meshes.insert(l.mesh.clone(), m);
                }
            }
            let mut textures: HashMap<String, Option<Image>> = HashMap::new();
            for l in &desc.layers {
                if let Some(t) = &l.texture
                    && !textures.contains_key(t)
                {
                    let img = self.read_content(t).and_then(|b| texture::decode(&b).ok());
                    textures.insert(t.clone(), img);
                }
            }
            let view = self.compositor.render(
                &self.device,
                &self.queue,
                desc,
                &mut |m| meshes.get(m).cloned().flatten(),
                &mut |t| textures.get(t).cloned().flatten(),
            );
            let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("character composite"),
                layout: &self.material_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                }],
            });
            self.materials.insert(shading, bg);
        }
    }

    pub fn particle_count(&self) -> usize {
        self.particles.particle_count()
    }

    fn compressed(&self) -> bool {
        self.device
            .features()
            .contains(wgpu::Features::TEXTURE_COMPRESSION_BC)
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
        read_content_in(&self.content_dir, &self.platform_dir, &self.asset_dirs, url)
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
        resolve_content_in(&self.content_dir, &self.platform_dir, url)
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
            f.uv.map(|r| r.map(f32::to_bits)).hash(&mut h);
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
                    let uv = f.uv;
                    self.file_mesh(&url).map(|m| {
                        let mut mesh = Mesh::from_file(m, scale);
                        if let Some(r) = uv {
                            for v in &mut mesh.vertices {
                                v.uv = [v.uv[0] * r[2] + r[0], v.uv[1] * r[3] + r[1]];
                            }
                        }
                        mesh
                    })
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
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size,
            mip_level_count: 1,
            sample_count: SAMPLE_COUNT,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.depth = Some((texture, view, width, height));
        let color = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("msaa color"),
            size,
            mip_level_count: 1,
            sample_count: SAMPLE_COUNT,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
        self.msaa_color = Some((color, color_view));
    }

    pub fn render(&mut self, dm: &DataModel, target: &wgpu::TextureView, width: u32, height: u32) {
        self.poll_texture_loads();
        let aspect = width.max(1) as f32 / height.max(1) as f32;
        let scene = scene::extract(dm, aspect);
        self.update_terrain(dm);
        self.update_lightgrid(dm, &scene);
        self.render_scene(&scene, target, width, height);
        let world = crate::world_gui::extract(dm);
        let mut items = world.on_top;
        items.extend(crate::gui::extract(dm));
        let cam = &scene.camera;
        let vp = mat_mul(
            &perspective(cam.fov_y, aspect, cam.near, cam.far),
            &look_to(
                cam.cframe.translation,
                -cam.cframe.rotation.z_axis,
                cam.cframe.rotation.y_axis,
            ),
        );
        if let Some(mut gui) = self.gui.take() {
            let load = |url: &str| self.read_content(url);
            gui.render(
                &self.device,
                &self.queue,
                self.msaa_color.as_ref().map(|c| &c.1).unwrap_or(target),
                Some(target),
                self.depth.as_ref().map(|d| &d.1),
                (width.max(1), height.max(1)),
                self.gui_scale,
                &items,
                &world.world,
                &vp,
                &load,
            );
            self.gui = Some(gui);
        }
    }

    fn update_terrain(&mut self, dm: &DataModel) {
        let Some(t) = dm.workspace_terrain() else {
            self.terrain_meshes.clear();
            self.terrain_generation = None;
            self.smooth.clear();
            self.smooth.set_legacy_water(&self.device, Vec::new());
            return;
        };
        let color = dm
            .get(t, "WaterColor")
            .ok()
            .and_then(|v| v.as_color3())
            .unwrap_or(rbx_math::Color3::new(0.05, 0.33, 0.36));
        let transparency = dm
            .get(t, "WaterTransparency")
            .ok()
            .and_then(|v| v.as_f32())
            .unwrap_or(0.3);
        self.smooth.prepare(
            &self.device,
            &self.queue,
            [
                color.r,
                color.g,
                color.b,
                1.0 - transparency.clamp(0.0, 1.0),
            ],
            &self.platform_dir,
            &self.white,
        );
        let data = dm.terrain(t);
        match data
            .and_then(|d| d.smooth.as_ref())
            .filter(|g| g.is_allocated())
        {
            Some(grid) => self.smooth.update(&self.device, grid),
            None => self.smooth.clear(),
        }
        let grid = data
            .and_then(|d| d.legacy.as_ref())
            .filter(|g| g.non_empty_cell_count() > 0);
        let Some(grid) = grid else {
            if self.terrain_generation.is_some() {
                self.smooth.set_legacy_water(&self.device, Vec::new());
            }
            self.terrain_meshes.clear();
            self.terrain_generation = None;
            return;
        };
        let generation = grid.changes.generation();
        if self.terrain_generation == Some(generation) {
            return;
        }
        self.terrain_generation = Some(generation);
        self.terrain_meshes.clear();
        let mut water = Vec::new();
        for chunk in grid.non_empty_chunks() {
            let w = crate::terrain::legacy_water(grid, chunk);
            if !w.water.is_empty() {
                water.push(w);
            }
            let mesh = crate::terrain::legacy_chunk(grid, chunk);
            if mesh.indices.is_empty() {
                continue;
            }
            let vertices = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("terrain vertices"),
                    contents: bytemuck::cast_slice(&mesh.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            let indices = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("terrain indices"),
                    contents: bytemuck::cast_slice(&mesh.indices),
                    usage: wgpu::BufferUsages::INDEX,
                });
            self.terrain_meshes.push(GpuMesh {
                vertices,
                indices,
                count: mesh.indices.len() as u32,
            });
        }
        self.smooth.set_legacy_water(&self.device, water);
        if !self.terrain_meshes.is_empty() {
            self.material_group(Shading::Terrain);
        }
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
            light_border: if self.lgrid_active {
                self.lightgrid.border_color()
            } else {
                [0.0, 0.0, 0.0, 1.0]
            },
            fade_distance_glow: [
                SHADING_DISTANCE,
                1.0 / SHADING_DISTANCE,
                outline_scaling,
                1.0,
            ],
            outline_brightness_shadow: [mul, add, 0.0, l.shadow_intensity],
            lgrid_scale: [0.0; 4],
            lgrid_offset: [0.0; 4],
            lgrid_center: [0.0; 4],
            lgrid_radius: [-1.0, -1.0, -1.0, 0.0],
            shadow_matrix: crate::shadow::shadow_matrix(scene.focus, l.key_direction),
            sky_color: [l.sky_color.r, l.sky_color.g, l.sky_color.b, 1.0],
            sky_color2: [l.sky_color2.r, l.sky_color2.g, l.sky_color2.b, 1.0],
        };
        let mut globals = globals;
        if self.lgrid_active {
            let size = crate::lightgrid::LightGrid::grid_size();
            let offset = self.lightgrid.grid_offset();
            let scale = Vec3::new(1.0 / size.x, 1.0 / size.y, 1.0 / size.z);
            let center = (offset + size * 0.5) * scale;
            let radius = size * 0.5 * 0.95 * scale;
            globals.lgrid_scale = [scale.x, scale.y, scale.z, 0.0];
            globals.lgrid_center = [center.x, center.y, center.z, 0.0];
            globals.lgrid_radius = [radius.x, radius.y, radius.z, 0.0];
        }
        self.queue
            .write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));

        let mut opaque: HashMap<(Shading, MeshKey), Vec<InstanceData>> = HashMap::new();
        let mut casters: HashMap<MeshKey, Vec<InstanceData>> = HashMap::new();
        let mut transparent: Vec<(f32, Shading, MeshKey, InstanceData)> = Vec::new();
        self.update_composites(&scene.composites);
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
                Shading::Terrain => 4.0,
            };
            let rows = scene::instance_rows(&part.cframe, Vec3::ONE, part.mesh_offset);
            let data = InstanceData {
                rows,
                color: part.color,
                extra: [
                    part.specular.0,
                    part.specular.1,
                    part.reflectance,
                    kind + if part.caster { 16.0 } else { 0.0 },
                ],
            };
            let d = (part.cframe.translation - eye).length_squared();
            if part.caster && part.color[3] > 0.0 {
                casters.entry(key).or_default().push(data);
            }
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
        let cel = &scene.celestial;
        let mut sun_moon: Vec<crate::celestial::CelestialVertex> = Vec::new();
        let mut star_vertices: Vec<crate::celestial::CelestialVertex> = Vec::new();
        let sun_shading = self.image(&cel.sun_texture);
        let moon_shading = self.image(&cel.moon_texture);
        self.material_group(sun_shading);
        self.material_group(moon_shading);
        self.material_group(Shading::SmoothPlastic);
        let right = rot.x_axis;
        let up = rot.y_axis;
        let mut moon_start = 0u32;
        if cel.shown {
            let sun_color = 0.8 * ((l.sun_position.y + 0.1) * 10.0).clamp(0.0, 1.0);
            let size = cel.sun_size * 23.809_525;
            for _ in 0..3 {
                crate::celestial::quad(
                    eye + l.sun_position * crate::celestial::SKY_SIZE,
                    right * size,
                    up * size,
                    [sun_color, sun_color, sun_color, 1.0],
                    &mut sun_moon,
                );
            }
            moon_start = sun_moon.len() as u32;
            let moon_color = (1.0 - l.sky_color.g + 0.1).min(1.0);
            let size = cel.moon_size * 23.809_525;
            crate::celestial::quad(
                eye + l.moon_position * crate::celestial::SKY_SIZE,
                right * size,
                up * size,
                [moon_color, moon_color, moon_color, 1.0],
                &mut sun_moon,
            );
        }
        self.stars.update(cel.stars, cel.shown, l.moon_naive.y);
        let world_per_pixel = 2.0 * tan_y * 2500.0 / height as f32;
        self.stars
            .vertices(eye, right, up, world_per_pixel, &mut star_vertices);
        let make_buffer = |device: &wgpu::Device, v: &[crate::celestial::CelestialVertex]| {
            (!v.is_empty()).then(|| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("celestial"),
                    contents: bytemuck::cast_slice(v),
                    usage: wgpu::BufferUsages::VERTEX,
                })
            })
        };
        let cam_dir = -rot.z_axis;
        self.particles
            .update(&scene.effects, self.frame_dt, eye, cam_dir);
        let (particle_vertices, particle_draws) = self.particles.draw_list(right, up, eye, cam_dir);
        if !particle_draws.is_empty() {
            let wanted = self.particle_pass.missing(&self.particles, &particle_draws);
            let loaded: HashMap<String, Option<Image>> = wanted
                .into_iter()
                .map(|u| {
                    let img = self.read_content(&u).and_then(|b| texture::decode(&b).ok());
                    (u, img)
                })
                .collect();
            let load = |url: &str| loaded.get(url).cloned().flatten();
            self.particle_pass.prepare(
                &self.device,
                &self.queue,
                &mut self.particles,
                &particle_draws,
                &load,
            );
        }
        let adorn_textures = self.adorn_pass.missing_textures(&scene.adorns);
        let adorn_images: HashMap<String, Option<Image>> = adorn_textures
            .into_iter()
            .map(|u| {
                let img = self.read_content(&u).and_then(|b| texture::decode(&b).ok());
                (u, img)
            })
            .collect();
        self.adorn_pass.prepare(
            &self.device,
            &self.queue,
            &scene.adorns,
            eye,
            cam.fov_y,
            (width, height),
            &|url: &str| adorn_images.get(url).cloned().flatten(),
        );
        let particle_buffer = (!particle_vertices.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("particles"),
                    contents: bytemuck::cast_slice(&particle_vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let sun_moon_buffer = make_buffer(&self.device, &sun_moon);
        let star_buffer = make_buffer(&self.device, &star_vertices);

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
            let mut caster_instances: Vec<InstanceData> = Vec::new();
            let mut caster_batches: Vec<(MeshKey, u32, u32)> = Vec::new();
            for (key, list) in &casters {
                caster_batches.push((*key, caster_instances.len() as u32, list.len() as u32));
                caster_instances.extend_from_slice(list);
            }
            let caster_buffer = (!caster_instances.is_empty()).then(|| {
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("shadow casters"),
                        contents: bytemuck::cast_slice(&caster_instances),
                        usage: wgpu::BufferUsages::VERTEX,
                    })
            });
            {
                let mut pass = self.shadow.begin(&mut encoder);
                if let Some(buf) = &caster_buffer {
                    pass.set_pipeline(&self.shadow_pipeline);
                    pass.set_bind_group(0, &self.shadow_globals_bg, &[]);
                    pass.set_vertex_buffer(1, buf.slice(..));
                    for (key, start, count) in &caster_batches {
                        let mesh = &self.meshes[key];
                        pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                        pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..mesh.count, 0, *start..start + count);
                    }
                }
            }
            if caster_buffer.is_some() {
                self.shadow.blur(&mut encoder);
            }
        }
        {
            let clear = l.clear_color;
            let depth_view = match &self.depth {
                Some(d) => &d.1,
                None => return,
            };
            let Some((_, msaa_view)) = &self.msaa_color else {
                return;
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: msaa_view,
                    depth_slice: None,
                    resolve_target: Some(target),
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
            if !self.terrain_meshes.is_empty() {
                let terrain_instance = InstanceData {
                    rows: [
                        [1.0, 0.0, 0.0, 0.0],
                        [0.0, 1.0, 0.0, 0.0],
                        [0.0, 0.0, 1.0, 0.0],
                    ],
                    color: [1.0; 4],
                    extra: [0.0, 1.0, 0.0, 4.0],
                };
                let buf = self
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("terrain instance"),
                        contents: bytemuck::bytes_of(&terrain_instance),
                        usage: wgpu::BufferUsages::VERTEX,
                    });
                pass.set_pipeline(&self.opaque);
                pass.set_bind_group(1, &self.materials[&Shading::Terrain], &[]);
                pass.set_vertex_buffer(1, buf.slice(..));
                for mesh in &self.terrain_meshes {
                    pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                    pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.count, 0, 0..1);
                }
            }
            if !self.smooth.is_empty() {
                self.smooth.draw_solid(&mut pass);
            }
            if let Some((_, sky)) = &self.sky_bg {
                pass.set_pipeline(&self.sky);
                pass.set_bind_group(1, sky, &[]);
                pass.draw(0..3, 0..1);
            }
            if let Some(buf) = &sun_moon_buffer {
                pass.set_pipeline(&self.celestial_add);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.set_bind_group(1, &self.materials[&sun_shading], &[]);
                pass.draw(0..moon_start, 0..1);
                pass.set_bind_group(1, &self.materials[&moon_shading], &[]);
                pass.draw(moon_start..sun_moon.len() as u32, 0..1);
            }
            if let Some(buf) = &star_buffer {
                pass.set_pipeline(&self.celestial_alpha);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.set_bind_group(1, &self.materials[&Shading::SmoothPlastic], &[]);
                pass.draw(0..star_vertices.len() as u32, 0..1);
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
            if !self.smooth.is_empty() {
                pass.set_bind_group(0, &self.globals_bg, &[]);
                self.smooth.draw_water(&mut pass);
            }
            if let Some(buf) = &particle_buffer {
                pass.set_bind_group(0, &self.globals_bg, &[]);
                self.particle_pass.draw(&mut pass, buf, &particle_draws);
            }
            pass.set_bind_group(0, &self.globals_bg, &[]);
            self.adorn_pass.draw(&mut pass);
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
        format: crate::texture::PixelFormat::Rgba8,
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

fn resolve_content_in(content_dir: &Path, platform_dir: &Path, url: &str) -> Option<PathBuf> {
    let lower = url.to_ascii_lowercase();
    let rest = lower.strip_prefix("rbxasset://")?;
    let rel = &url[url.len() - rest.len()..];
    let mut path = content_dir.to_path_buf();
    for part in rel
        .split(['/', '\\'])
        .filter(|s| !s.is_empty() && *s != "..")
    {
        path.push(part);
    }
    if rbx_base::vfs::is_file(&path) {
        return Some(path);
    }
    let mut alt = platform_dir.to_path_buf();
    for part in rel
        .split(['/', '\\'])
        .filter(|s| !s.is_empty() && *s != "..")
    {
        alt.push(part);
    }
    rbx_base::vfs::is_file(&alt).then_some(alt)
}

fn read_content_in(
    content_dir: &Path,
    platform_dir: &Path,
    asset_dirs: &[PathBuf],
    url: &str,
) -> Option<Vec<u8>> {
    if let Some(p) = resolve_content_in(content_dir, platform_dir, url) {
        return rbx_base::vfs::read(p).ok();
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
    asset_dirs
        .iter()
        .find_map(|d| rbx_base::vfs::read(d.join(id.to_string())).ok())
}
