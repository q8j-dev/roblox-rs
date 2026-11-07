use crate::character::{
    CANVAS_HEIGHT, CANVAS_WIDTH, COMPOSITE_HEIGHT, COMPOSITE_WIDTH, CompositeDesc, LayerMode,
};
use crate::filemesh::FileMesh;
use crate::texture::Image;
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LayerUniform {
    color: [f32; 4],
    canvas: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LayerVertex {
    position: [f32; 3],
    uv: [f32; 2],
}

pub struct Compositor {
    blit: wgpu::RenderPipeline,
    blend: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    white: wgpu::TextureView,
}

impl Compositor {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Compositor {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("composite"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/composite.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composite layer"),
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("composite"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2];
        let buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LayerVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attributes,
        })];
        let make = |entry: &str, blend: Option<wgpu::BlendState>| {
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
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let blit = make("fs_blit", None);
        let blend = make(
            "fs_blend",
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("composite"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let white = Image::solid([255, 255, 255, 255])
            .upload(device, queue, "composite white", false)
            .create_view(&wgpu::TextureViewDescriptor::default());
        Compositor {
            blit,
            blend,
            layout,
            sampler,
            white,
        }
    }

    pub fn render(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        desc: &CompositeDesc,
        mesh: &mut dyn FnMut(&str) -> Option<FileMesh>,
        texture: &mut dyn FnMut(&str) -> Option<Image>,
    ) -> wgpu::TextureView {
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("character composite"),
            size: wgpu::Extent3d {
                width: COMPOSITE_WIDTH,
                height: COMPOSITE_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        struct Draw {
            vertices: wgpu::Buffer,
            indices: wgpu::Buffer,
            count: u32,
            group: wgpu::BindGroup,
            blend: bool,
        }
        let mut draws = Vec::new();
        for layer in &desc.layers {
            let Some(m) = mesh(&layer.mesh) else {
                continue;
            };
            let tex_view = match (&layer.texture, layer.mode) {
                (Some(url), _) => match texture(url) {
                    Some(img) => img
                        .upload(device, queue, "composite layer", false)
                        .create_view(&wgpu::TextureViewDescriptor::default()),
                    None => continue,
                },
                (None, _) => self.white.clone(),
            };
            let vertices: Vec<LayerVertex> = m
                .positions
                .iter()
                .zip(m.uvs.iter())
                .map(|(p, uv)| LayerVertex {
                    position: [p.x, p.y, p.z],
                    uv: *uv,
                })
                .collect();
            let uniform = LayerUniform {
                color: [layer.color[0], layer.color[1], layer.color[2], 1.0],
                canvas: [2.0 / CANVAS_WIDTH, 2.0 / CANVAS_HEIGHT, 0.0, 0.0],
            };
            let ubuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("composite layer"),
                contents: bytemuck::bytes_of(&uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("composite layer"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: ubuf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&tex_view),
                    },
                ],
            });
            draws.push(Draw {
                vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("composite vertices"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
                indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("composite indices"),
                    contents: bytemuck::cast_slice(&m.indices),
                    usage: wgpu::BufferUsages::INDEX,
                }),
                count: m.indices.len() as u32,
                group,
                blend: layer.mode == LayerMode::Blend,
            });
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("composite"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("composite"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.5,
                            g: 0.5,
                            b: 0.5,
                            a: 0.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            for d in &draws {
                pass.set_pipeline(if d.blend { &self.blend } else { &self.blit });
                pass.set_bind_group(0, &d.group, &[]);
                pass.set_vertex_buffer(0, d.vertices.slice(..));
                pass.set_index_buffer(d.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..d.count, 0, 0..1);
            }
        }
        queue.submit([encoder.finish()]);
        view
    }
}
