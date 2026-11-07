use crate::particles::{Appearance, Blend, ParticleSystem, ParticleVertex, Shader};
use bytemuck::{Pod, Zeroable};
use rbx_dom::InstanceId;
use std::collections::HashMap;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    modulate: [f32; 4],
    throttle: [f32; 4],
}

struct EmitterGpu {
    appearance: Appearance,
    buffer: wgpu::Buffer,
    group: wgpu::BindGroup,
}

pub struct ParticlePass {
    shader: wgpu::ShaderModule,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    pipelines: HashMap<(Shader, Blend), wgpu::RenderPipeline>,
    format: wgpu::TextureFormat,
    tex_sampler: wgpu::Sampler,
    strip_sampler: wgpu::Sampler,
    textures: HashMap<String, (wgpu::TextureView, u32)>,
    black: wgpu::TextureView,
    emitters: HashMap<(InstanceId, usize), EmitterGpu>,
}

fn blend_state(b: Blend) -> wgpu::BlendState {
    use wgpu::BlendFactor as F;
    let (src, dst) = match b {
        Blend::None => (F::One, F::Zero),
        Blend::Additive => (F::One, F::One),
        Blend::Multiplicative => (F::Dst, F::Zero),
        Blend::AlphaBlend => (F::SrcAlpha, F::OneMinusSrcAlpha),
        Blend::Premultiplied => (F::One, F::OneMinusSrcAlpha),
        Blend::AlphaOne => (F::SrcAlpha, F::One),
    };
    let c = wgpu::BlendComponent {
        src_factor: src,
        dst_factor: dst,
        operation: wgpu::BlendOperation::Add,
    };
    wgpu::BlendState { color: c, alpha: c }
}

impl ParticlePass {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        globals_layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
    ) -> ParticlePass {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("particles"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/particles.wgsl").into()),
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
        let sampler_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("particle emitter"),
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
                sampler_entry(1),
                sampler_entry(2),
                texture_entry(3),
                texture_entry(4),
                texture_entry(5),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("particles"),
            bind_group_layouts: &[Some(globals_layout), Some(&layout)],
            immediate_size: 0,
        });
        let tex_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("particle texture"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let strip_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("particle strip"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let black = crate::texture::Image::solid([0, 0, 0, 0])
            .upload(device, queue, "particle black", false)
            .create_view(&wgpu::TextureViewDescriptor::default());
        ParticlePass {
            shader,
            layout,
            pipeline_layout,
            pipelines: HashMap::new(),
            format,
            tex_sampler,
            strip_sampler,
            textures: HashMap::new(),
            black,
            emitters: HashMap::new(),
        }
    }

    fn pipeline(&mut self, device: &wgpu::Device, shader: Shader, blend: Blend) {
        if self.pipelines.contains_key(&(shader, blend)) {
            return;
        }
        let entry = match shader {
            Shader::Add => "fs_add",
            Shader::Modulate => "fs_modulate",
            Shader::Crazy => "fs_crazy",
            Shader::CrazySparkles => "fs_crazy_sparkles",
            Shader::Custom => "fs_custom",
        };
        let buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ParticleVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ParticleVertex::ATTRIBUTES,
        })];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(entry),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &buffers,
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.format,
                    blend: Some(blend_state(blend)),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: crate::renderer::DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: crate::renderer::msaa_state(),
            multiview_mask: None,
            cache: None,
        });
        self.pipelines.insert((shader, blend), pipeline);
    }

    fn texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        url: &str,
        load: &dyn Fn(&str) -> Option<crate::texture::Image>,
    ) -> (wgpu::TextureView, u32) {
        if url.is_empty() {
            return (self.black.clone(), 256);
        }
        if let Some(t) = self.textures.get(url) {
            return t.clone();
        }
        let entry = match load(url) {
            Some(img) => {
                let h = img.height;
                (
                    img.upload(device, queue, "particle", false)
                        .create_view(&wgpu::TextureViewDescriptor::default()),
                    h,
                )
            }
            None => (self.black.clone(), 256),
        };
        self.textures.insert(url.to_string(), entry.clone());
        entry
    }

    pub fn missing(
        &self,
        system: &ParticleSystem,
        draws: &[(InstanceId, usize, u32, u32)],
    ) -> Vec<String> {
        let mut out = Vec::new();
        for &(id, k, _, _) in draws {
            if let Some(e) = system.emitter(id, k) {
                for url in [
                    &e.appearance.main,
                    &e.appearance.color_strip,
                    &e.appearance.alpha_strip,
                ] {
                    if !url.is_empty()
                        && !self.textures.contains_key(url.as_str())
                        && !out.contains(url)
                    {
                        out.push(url.clone());
                    }
                }
            }
        }
        out
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        system: &mut ParticleSystem,
        draws: &[(InstanceId, usize, u32, u32)],
        load: &dyn Fn(&str) -> Option<crate::texture::Image>,
    ) {
        let live: std::collections::HashSet<(InstanceId, usize)> =
            draws.iter().map(|d| (d.0, d.1)).collect();
        self.emitters.retain(|k, _| live.contains(k));
        for &(id, k, _, _) in draws {
            let Some(e) = system.emitter(id, k) else {
                continue;
            };
            let appearance = e.appearance.clone();
            let params = Params {
                modulate: e.modulate,
                throttle: [0.0, 0.0, 0.0, e.blend_ratio],
            };
            self.pipeline(device, appearance.shader, appearance.blend);
            let rebuild = self
                .emitters
                .get(&(id, k))
                .is_none_or(|g| g.appearance != appearance);
            if rebuild {
                let (main, _) = self.texture(device, queue, &appearance.main, load);
                let (strip, height) = self.texture(device, queue, &appearance.color_strip, load);
                let (alpha, _) = self.texture(device, queue, &appearance.alpha_strip, load);
                system.set_strip_height(&appearance.color_strip, height);
                let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("particle params"),
                    contents: bytemuck::bytes_of(&params),
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                });
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("particle emitter"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.tex_sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&self.strip_sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::TextureView(&main),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: wgpu::BindingResource::TextureView(&strip),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: wgpu::BindingResource::TextureView(&alpha),
                        },
                    ],
                });
                self.emitters.insert(
                    (id, k),
                    EmitterGpu {
                        appearance,
                        buffer,
                        group,
                    },
                );
            } else if let Some(g) = self.emitters.get(&(id, k)) {
                queue.write_buffer(&g.buffer, 0, bytemuck::bytes_of(&params));
            }
        }
    }

    pub fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        buffer: &wgpu::Buffer,
        draws: &[(InstanceId, usize, u32, u32)],
    ) {
        pass.set_vertex_buffer(0, buffer.slice(..));
        for &(id, k, start, count) in draws {
            let Some(g) = self.emitters.get(&(id, k)) else {
                continue;
            };
            let Some(p) = self
                .pipelines
                .get(&(g.appearance.shader, g.appearance.blend))
            else {
                continue;
            };
            pass.set_pipeline(p);
            pass.set_bind_group(1, &g.group, &[]);
            pass.draw(start..start + count, 0..1);
        }
    }
}
