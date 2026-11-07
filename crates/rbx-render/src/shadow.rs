use bytemuck::{Pod, Zeroable};
use rbx_math::Vec3;
use wgpu::util::DeviceExt;

pub const SHADOW_SIZE: u32 = 1024;
const TEXEL_SIZE: f32 = 0.2;
const DEPTH_RADIUS: f32 = 64.0;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BlurParams {
    step: [f32; 4],
}

pub struct ShadowMap {
    color: [wgpu::Texture; 2],
    views: [wgpu::TextureView; 2],
    depth: wgpu::TextureView,
    blur: wgpu::RenderPipeline,
    blur_groups: [wgpu::BindGroup; 2],
}

pub fn shadow_matrix(center: Vec3, light_travel: Vec3) -> [[f32; 4]; 3] {
    let radius = SHADOW_SIZE as f32 * TEXEL_SIZE / 2.0;
    let z = light_travel.normalize_or(Vec3::NEG_Y);
    let up = if z.dot(Vec3::Y).abs() > 0.99 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    let x = up.cross(z).normalize_or(Vec3::X);
    let y = z.cross(x);
    let round = (SHADOW_SIZE / 2) as f32;
    let origin_x = x.dot(-center) / radius;
    let origin_y = y.dot(-center) / radius;
    let off_x = (origin_x * round).floor() / round - origin_x;
    let off_y = (origin_y * round).floor() / round - origin_y;
    let row = |axis: Vec3, scale: f32, add: f32| {
        [
            axis.x * scale,
            axis.y * scale,
            axis.z * scale,
            -axis.dot(center) * scale + add,
        ]
    };
    let cx = row(x, 1.0 / radius, off_x);
    let cy = row(y, 1.0 / radius, off_y);
    let cz = row(z, 1.0 / DEPTH_RADIUS, 0.5);
    [
        [cx[0] * 0.5, cx[1] * 0.5, cx[2] * 0.5, cx[3] * 0.5 + 0.5],
        [-cy[0] * 0.5, -cy[1] * 0.5, -cy[2] * 0.5, -cy[3] * 0.5 + 0.5],
        cz,
    ]
}

impl ShadowMap {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mask: Option<crate::texture::Image>,
    ) -> ShadowMap {
        let make_color = |label: &str| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: SHADOW_SIZE,
                    height: SHADOW_SIZE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let color = [make_color("shadow map"), make_color("shadow blur")];
        let views = [
            color[0].create_view(&wgpu::TextureViewDescriptor::default()),
            color[1].create_view(&wgpu::TextureViewDescriptor::default()),
        ];
        let depth = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("shadow depth"),
                size: wgpu::Extent3d {
                    width: SHADOW_SIZE,
                    height: SHADOW_SIZE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: crate::renderer::DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default());
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shadow blur"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/shadow_blur.wgsl").into()),
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
            label: Some("shadow blur"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
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
                texture_entry(2),
                texture_entry(3),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow blur"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let blur = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow blur"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow blur"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let white = crate::texture::Image::solid([255, 255, 255, 255])
            .upload(device, queue, "shadow white", false)
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mask_view = match mask {
            Some(img) => img
                .upload(device, queue, "shadow mask", false)
                .create_view(&wgpu::TextureViewDescriptor::default()),
            None => crate::texture::Image::solid([0, 0, 0, 255])
                .upload(device, queue, "shadow mask", false)
                .create_view(&wgpu::TextureViewDescriptor::default()),
        };
        let texel = 1.0 / SHADOW_SIZE as f32;
        let group = |step: [f32; 4], source: &wgpu::TextureView, mask: &wgpu::TextureView| {
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("shadow blur params"),
                contents: bytemuck::bytes_of(&BlurParams { step }),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("shadow blur"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(source),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(mask),
                    },
                ],
            })
        };
        let blur_groups = [
            group([texel, 0.0, 0.0, 0.0], &views[0], &white),
            group([0.0, texel, 0.0, 0.0], &views[1], &mask_view),
        ];
        ShadowMap {
            color,
            views,
            depth,
            blur,
            blur_groups,
        }
    }

    pub fn view(&self) -> &wgpu::TextureView {
        &self.views[0]
    }

    pub fn texture(&self) -> &wgpu::Texture {
        &self.color[0]
    }

    pub fn begin<'a>(&'a self, encoder: &'a mut wgpu::CommandEncoder) -> wgpu::RenderPass<'a> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shadow casters"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.views[0],
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 1.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }

    pub fn blur(&self, encoder: &mut wgpu::CommandEncoder) {
        for (target, group) in [
            (&self.views[1], &self.blur_groups[0]),
            (&self.views[0], &self.blur_groups[1]),
        ] {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow blur"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.blur);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}
