use crate::gui::{GuiCmd, GuiItem, ImageUv, Rect, text_quads};
use crate::world_gui::WorldGui;
use bytemuck::{Pod, Zeroable};
use rbx_math::Vec2;
use std::collections::HashMap;
use wgpu::util::DeviceExt;

const ATLAS_SIZE: u32 = 2048;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct GuiVertex {
    pos: [f32; 4],
    uv: [f32; 2],
    color: [f32; 4],
    local: [f32; 2],
    shape: [f32; 4],
    radii: [f32; 4],
}

impl GuiVertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
        0 => Float32x4,
        1 => Float32x2,
        2 => Float32x4,
        3 => Float32x2,
        4 => Float32x4,
        5 => Float32x4
    ];
}

fn shape_data(item: &GuiItem, r: Rect) -> ([[f32; 2]; 4], [f32; 4], [f32; 4]) {
    match item.shape {
        Some(s) => {
            let cx = (s.rect.x0 + s.rect.x1) * 0.5;
            let cy = (s.rect.y0 + s.rect.y1) * 0.5;
            let local = [
                [r.x0 - cx, r.y0 - cy],
                [r.x1 - cx, r.y0 - cy],
                [r.x1 - cx, r.y1 - cy],
                [r.x0 - cx, r.y1 - cy],
            ];
            (
                local,
                [s.rect.width() * 0.5, s.rect.height() * 0.5, s.stroke, 1.0],
                s.radii,
            )
        }
        None => ([[0.0; 2]; 4], [0.0; 4], [0.0; 4]),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum TextureKey {
    White,
    Glyphs,
    Image(usize),
}

type Quad = (TextureKey, Rect, [f32; 4], [f32; 4]);

struct Batch {
    world: bool,
    texture: TextureKey,
    clip: Option<[u32; 4]>,
    start: u32,
    count: u32,
}

struct GlyphAtlas {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    entries: HashMap<(rbx_text::Font, char, u32), Option<[u32; 4]>>,
    cursor: (u32, u32),
    row: u32,
    full: bool,
}

impl GlyphAtlas {
    fn place(
        &mut self,
        queue: &wgpu::Queue,
        font: rbx_text::Font,
        ch: char,
        size: u32,
    ) -> Option<[u32; 4]> {
        if let Some(e) = self.entries.get(&(font, ch, size)) {
            return *e;
        }
        let result = rbx_text::typesetter(font)
            .rasterize(ch, size)
            .and_then(|b| {
                if b.width + 1 > ATLAS_SIZE || b.height + 1 > ATLAS_SIZE {
                    return None;
                }
                if self.cursor.0 + b.width + 1 > ATLAS_SIZE {
                    self.cursor = (0, self.cursor.1 + self.row + 1);
                    self.row = 0;
                }
                if self.cursor.1 + b.height + 1 > ATLAS_SIZE {
                    self.full = true;
                    return None;
                }
                let (x, y) = self.cursor;
                let rgba: Vec<u8> = b.data.iter().flat_map(|&a| [255, 255, 255, a]).collect();
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &self.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x, y, z: 0 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &rgba,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(b.width * 4),
                        rows_per_image: Some(b.height),
                    },
                    wgpu::Extent3d {
                        width: b.width,
                        height: b.height,
                        depth_or_array_layers: 1,
                    },
                );
                self.cursor.0 += b.width + 1;
                self.row = self.row.max(b.height);
                Some([x, y, b.width, b.height])
            });
        if result.is_some() || !self.full {
            self.entries.insert((font, ch, size), result);
        }
        result
    }

    fn reset_if_full(&mut self) {
        if self.full {
            self.entries.clear();
            self.cursor = (0, 0);
            self.row = 0;
            self.full = false;
        }
    }
}

struct LoadedImage {
    bind_group: wgpu::BindGroup,
    size: Vec2,
}

pub struct GuiRenderer {
    pipeline: wgpu::RenderPipeline,
    world_pipeline: wgpu::RenderPipeline,
    overlay_pipeline: wgpu::RenderPipeline,
    globals: wgpu::BindGroup,
    texture_layout: wgpu::BindGroupLayout,
    white: wgpu::BindGroup,
    atlas: GlyphAtlas,
    images: Vec<Option<LoadedImage>>,
    image_index: HashMap<String, usize>,
}

fn texture_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("gui texture"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(view),
        }],
    })
}

impl GuiRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> GuiRenderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gui"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/gui.wgsl").into()),
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gui globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gui texture"),
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
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gui"),
            bind_group_layouts: &[Some(&globals_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let make_pipeline = |depth: Option<wgpu::DepthStencilState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("gui"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<GuiVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &GuiVertex::ATTRIBUTES,
                    })],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: depth,
                multisample: crate::renderer::msaa_state(),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipeline = make_pipeline(None);
        let world_pipeline = make_pipeline(Some(wgpu::DepthStencilState {
            format: crate::renderer::DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }));
        let overlay_pipeline = make_pipeline(Some(wgpu::DepthStencilState {
            format: crate::renderer::DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }));
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("gui"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let globals = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gui globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Sampler(&sampler),
            }],
        });
        let white_view = crate::texture::Image::solid([255, 255, 255, 255])
            .upload(device, queue, "gui white", false)
            .create_view(&wgpu::TextureViewDescriptor::default());
        let white = texture_group(device, &texture_layout, &white_view);
        let atlas_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let atlas_view = atlas_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let atlas = GlyphAtlas {
            bind_group: texture_group(device, &texture_layout, &atlas_view),
            texture: atlas_texture,
            entries: HashMap::new(),
            cursor: (0, 0),
            row: 0,
            full: false,
        };
        GuiRenderer {
            pipeline,
            world_pipeline,
            overlay_pipeline,
            globals,
            texture_layout,
            white,
            atlas,
            images: Vec::new(),
            image_index: HashMap::new(),
        }
    }

    fn image(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        url: &str,
        load: &dyn Fn(&str) -> Option<Vec<u8>>,
    ) -> Option<usize> {
        let index = match self.image_index.get(url) {
            Some(&i) => i,
            None => {
                let loaded = load(url)
                    .and_then(|b| match crate::texture::decode(&b) {
                        Ok(img) => Some(img),
                        Err(e) => {
                            tracing::warn!("{url}: {e}");
                            None
                        }
                    })
                    .map(|img| {
                        let view = img
                            .upload(device, queue, "gui image", false)
                            .create_view(&wgpu::TextureViewDescriptor::default());
                        LoadedImage {
                            bind_group: texture_group(device, &self.texture_layout, &view),
                            size: Vec2::new(img.width as f32, img.height as f32),
                        }
                    });
                let i = self.images.len();
                self.images.push(loaded);
                self.image_index.insert(url.to_string(), i);
                i
            }
        };
        self.images[index].as_ref().map(|_| index)
    }

    fn quads(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        item: &GuiItem,
        scale: f32,
        world: bool,
        load: &dyn Fn(&str) -> Option<Vec<u8>>,
    ) -> Vec<Quad> {
        let mut quads: Vec<Quad> = Vec::new();
        match &item.cmd {
            GuiCmd::Rect { rect, color } => {
                quads.push((TextureKey::White, *rect, [0.0, 0.0, 1.0, 1.0], *color))
            }
            GuiCmd::Border { rect, size, color } => {
                let s = *size;
                let uv = [0.0, 0.0, 1.0, 1.0];
                quads.push((
                    TextureKey::White,
                    Rect {
                        x0: rect.x0 - s,
                        y0: rect.y0 - s,
                        x1: rect.x1 + s,
                        y1: rect.y0,
                    },
                    uv,
                    *color,
                ));
                quads.push((
                    TextureKey::White,
                    Rect {
                        x0: rect.x0 - s,
                        y0: rect.y1,
                        x1: rect.x1 + s,
                        y1: rect.y1 + s,
                    },
                    uv,
                    *color,
                ));
                quads.push((
                    TextureKey::White,
                    Rect {
                        x0: rect.x0 - s,
                        y0: rect.y0,
                        x1: rect.x0,
                        y1: rect.y1,
                    },
                    uv,
                    *color,
                ));
                quads.push((
                    TextureKey::White,
                    Rect {
                        x0: rect.x1,
                        y0: rect.y0,
                        x1: rect.x1 + s,
                        y1: rect.y1,
                    },
                    uv,
                    *color,
                ));
            }
            GuiCmd::Image {
                rect,
                url,
                uv,
                color,
            } => {
                if let Some(i) = self.image(device, queue, url, load) {
                    let size = self.images[i].as_ref().map(|l| l.size).unwrap_or(Vec2::ONE);
                    let uv = match uv {
                        ImageUv::Full => [0.0, 0.0, 1.0, 1.0],
                        ImageUv::Pixels { offset, size: s } => [
                            offset.x / size.x,
                            offset.y / size.y,
                            (offset.x + s.x) / size.x,
                            (offset.y + s.y) / size.y,
                        ],
                    };
                    quads.push((TextureKey::Image(i), *rect, uv, *color));
                }
            }
            GuiCmd::Thirds {
                rect,
                url,
                edge,
                min,
                color,
            } => {
                if let Some(i) = self.image(device, queue, url, load) {
                    let scale = 1.0f32
                        .min(rect.width() / min.x.max(1.0))
                        .min(rect.height() / min.y.max(1.0));
                    let ex = edge.x * scale;
                    let ey = edge.y * scale;
                    let xs = [rect.x0, rect.x0 + ex, rect.x1 - ex, rect.x1];
                    let ys = [rect.y0, rect.y0 + ey, rect.y1 - ey, rect.y1];
                    for row in 0..3 {
                        for col in 0..3 {
                            let r = Rect {
                                x0: xs[col],
                                y0: ys[row],
                                x1: xs[col + 1],
                                y1: ys[row + 1],
                            };
                            if r.width() > 0.0 && r.height() > 0.0 {
                                let u0 = col as f32 / 3.0;
                                let v0 = row as f32 / 3.0;
                                quads.push((
                                    TextureKey::Image(i),
                                    r,
                                    [u0, v0, u0 + 1.0 / 3.0, v0 + 1.0 / 3.0],
                                    *color,
                                ));
                            }
                        }
                    }
                }
            }
            GuiCmd::Slice {
                rect,
                url,
                center,
                uv,
                color,
            } => {
                if let Some(i) = self.image(device, queue, url, load) {
                    let size = self.images[i].as_ref().map(|l| l.size).unwrap_or(Vec2::ONE);
                    let (bounds_min, bounds_max) = match uv {
                        ImageUv::Full => (Vec2::ZERO, size),
                        ImageUv::Pixels { offset, size: s } => (*offset, *offset + *s),
                    };
                    let c0 = Vec2::new(center.min.x.max(0.0), center.min.y.max(0.0));
                    let c1 = Vec2::new(center.max.x, center.max.y);
                    let left = c0.x;
                    let top = c0.y;
                    let right = (bounds_max.x - bounds_min.x - c1.x).max(0.0);
                    let bottom = (bounds_max.y - bounds_min.y - c1.y).max(0.0);
                    let xs = [
                        rect.x0,
                        rect.x0 + left,
                        (rect.x1 - right).max(rect.x0 + left),
                        rect.x1,
                    ];
                    let ys = [
                        rect.y0,
                        rect.y0 + top,
                        (rect.y1 - bottom).max(rect.y0 + top),
                        rect.y1,
                    ];
                    let us = [
                        bounds_min.x,
                        bounds_min.x + c0.x,
                        bounds_min.x + c1.x,
                        bounds_max.x,
                    ];
                    let vs = [
                        bounds_min.y,
                        bounds_min.y + c0.y,
                        bounds_min.y + c1.y,
                        bounds_max.y,
                    ];
                    for row in 0..3 {
                        for col in 0..3 {
                            let r = Rect {
                                x0: xs[col],
                                y0: ys[row],
                                x1: xs[col + 1],
                                y1: ys[row + 1],
                            };
                            if r.width() > 0.0 && r.height() > 0.0 {
                                quads.push((
                                    TextureKey::Image(i),
                                    r,
                                    [
                                        us[col] / size.x,
                                        vs[row] / size.y,
                                        us[col + 1] / size.x,
                                        vs[row + 1] / size.y,
                                    ],
                                    *color,
                                ));
                            }
                        }
                    }
                }
            }
            GuiCmd::Text(cmd) => {
                let draw = text_quads(cmd);
                let typesetter = rbx_text::typesetter(cmd.font);
                for q in &draw.quads {
                    let target_em = if world {
                        2f32.powf(((q.em * scale).max(1.0).log2() * 4.0).round() / 4.0)
                    } else {
                        q.em * scale
                    };
                    let pad = typesetter.em(0);
                    let px = (target_em - pad).round().max(1.0) as u32;
                    let raster_em = typesetter.em(px);
                    let Some([x, y, w, h]) = self.atlas.place(queue, cmd.font, q.ch, px) else {
                        continue;
                    };
                    let g = typesetter.glyph(q.ch, px);
                    let f = q.em / raster_em;
                    let pad = rbx_text::FONT_PADDING;
                    let base = if q.stroke {
                        cmd.stroke.unwrap_or(cmd.color)
                    } else {
                        cmd.color
                    };
                    let color = [base[0], base[1], base[2], base[3] * draw.alpha];
                    let a = ATLAS_SIZE as f32;
                    let snap = |v: f32| {
                        if world {
                            v
                        } else {
                            (v * scale).round() / scale
                        }
                    };
                    let left = snap(q.pen[0] + (g.xoffset as f32 - pad) * f);
                    let top = snap(q.pen[1] - (g.yoffset as f32 + pad) * f);
                    quads.push((
                        TextureKey::Glyphs,
                        Rect::xywh(left, top, w as f32 * f, h as f32 * f),
                        [
                            x as f32 / a,
                            y as f32 / a,
                            (x + w) as f32 / a,
                            (y + h) as f32 / a,
                        ],
                        color,
                    ));
                }
            }
        }
        quads
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::TextureView,
        resolve: Option<&wgpu::TextureView>,
        depth: Option<&wgpu::TextureView>,
        (width, height): (u32, u32),
        scale: f32,
        items: &[GuiItem],
        world: &[WorldGui],
        view_projection: &[[f32; 4]; 4],
        load: &dyn Fn(&str) -> Option<Vec<u8>>,
    ) {
        let world = if depth.is_some() { world } else { &[] };
        if items.is_empty() && world.is_empty() {
            return;
        }
        self.atlas.reset_if_full();
        let mut vertices: Vec<GuiVertex> = Vec::new();
        let mut batches: Vec<Batch> = Vec::new();
        let full = [0, 0, width, height];
        let push = |vertices: &mut Vec<GuiVertex>,
                    batches: &mut Vec<Batch>,
                    world: bool,
                    texture: TextureKey,
                    clip: Option<[u32; 4]>,
                    corners: [([f32; 4], [f32; 2]); 4],
                    color: [f32; 4],
                    shape: ([[f32; 2]; 4], [f32; 4], [f32; 4])| {
            let start = vertices.len() as u32;
            for i in [0usize, 1, 2, 0, 2, 3] {
                vertices.push(GuiVertex {
                    pos: corners[i].0,
                    uv: corners[i].1,
                    color,
                    local: shape.0[i],
                    shape: shape.1,
                    radii: shape.2,
                });
            }
            match batches.last_mut() {
                Some(b)
                    if b.world == world
                        && b.texture == texture
                        && b.clip == clip
                        && b.start + b.count == start =>
                {
                    b.count += 6;
                }
                _ => batches.push(Batch {
                    world,
                    texture,
                    clip,
                    start,
                    count: 6,
                }),
            }
        };
        let m = view_projection;
        for layer in world {
            let to_clip = |p: Vec2| {
                let w = layer.origin + layer.x_axis * p.x + layer.y_axis * p.y;
                [0, 1, 2, 3].map(|r| m[0][r] * w.x + m[1][r] * w.y + m[2][r] * w.z + m[3][r])
            };
            for item in &layer.items {
                for (texture, r, uv, color) in
                    self.quads(device, queue, item, layer.raster_scale, true, load)
                {
                    let (r, uv) = if item.rotation == 0.0 {
                        match item.clip {
                            Some(c) => match clip_quad(r, uv, c) {
                                Some(v) => v,
                                None => continue,
                            },
                            None => (r, uv),
                        }
                    } else {
                        (r, uv)
                    };
                    let corners = rotated_corners(item, r, uv).map(|(p, t)| (to_clip(p), t));
                    let sd = shape_data(item, r);
                    push(
                        &mut vertices,
                        &mut batches,
                        true,
                        texture,
                        None,
                        corners,
                        color,
                        sd,
                    );
                }
            }
        }
        let (lw, lh) = (width as f32 / scale, height as f32 / scale);
        let to_ndc = |p: Vec2| [p.x / lw * 2.0 - 1.0, 1.0 - p.y / lh * 2.0, 0.0, 1.0];
        for item in items {
            let clip = item.clip.map(|c| {
                let x0 = (c.x0 * scale).round().max(0.0).min(width as f32) as u32;
                let y0 = (c.y0 * scale).round().max(0.0).min(height as f32) as u32;
                let x1 = (c.x1 * scale).round().max(0.0).min(width as f32) as u32;
                let y1 = (c.y1 * scale).round().max(0.0).min(height as f32) as u32;
                [x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0)]
            });
            if clip.is_some_and(|c| c[2] == 0 || c[3] == 0) {
                continue;
            }
            for (texture, r, uv, color) in self.quads(device, queue, item, scale, false, load) {
                let corners = rotated_corners(item, r, uv).map(|(p, t)| (to_ndc(p), t));
                let sd = shape_data(item, r);
                push(
                    &mut vertices,
                    &mut batches,
                    false,
                    texture,
                    clip,
                    corners,
                    color,
                    sd,
                );
            }
        }
        if vertices.is_empty() {
            return;
        }
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("gui vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("gui") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("gui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: resolve,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: depth.map(|view| {
                    wgpu::RenderPassDepthStencilAttachment {
                        view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.globals, &[]);
            pass.set_vertex_buffer(0, buffer.slice(..));
            let mut current: Option<bool> = None;
            for b in &batches {
                let group = match b.texture {
                    TextureKey::White => &self.white,
                    TextureKey::Glyphs => &self.atlas.bind_group,
                    TextureKey::Image(i) => match &self.images[i] {
                        Some(l) => &l.bind_group,
                        None => continue,
                    },
                };
                if current != Some(b.world) {
                    let pipeline = if b.world {
                        &self.world_pipeline
                    } else if depth.is_some() {
                        &self.overlay_pipeline
                    } else {
                        &self.pipeline
                    };
                    pass.set_pipeline(pipeline);
                    current = Some(b.world);
                }
                let [x, y, w, h] = b.clip.unwrap_or(full);
                pass.set_scissor_rect(x, y, w, h);
                pass.set_bind_group(1, group, &[]);
                pass.draw(b.start..b.start + b.count, 0..1);
            }
        }
        queue.submit([encoder.finish()]);
    }
}

fn rotated_corners(item: &GuiItem, r: Rect, uv: [f32; 4]) -> [(Vec2, [f32; 2]); 4] {
    let corners = [
        (Vec2::new(r.x0, r.y0), [uv[0], uv[1]]),
        (Vec2::new(r.x1, r.y0), [uv[2], uv[1]]),
        (Vec2::new(r.x1, r.y1), [uv[2], uv[3]]),
        (Vec2::new(r.x0, r.y1), [uv[0], uv[3]]),
    ];
    if item.rotation == 0.0 {
        return corners;
    }
    let (s, c) = item.rotation.to_radians().sin_cos();
    let pivot = item.pivot;
    corners.map(|(p, t)| {
        let d = p - pivot;
        (pivot + Vec2::new(d.x * c - d.y * s, d.x * s + d.y * c), t)
    })
}

fn clip_quad(r: Rect, uv: [f32; 4], c: Rect) -> Option<(Rect, [f32; 4])> {
    let x0 = r.x0.max(c.x0);
    let y0 = r.y0.max(c.y0);
    let x1 = r.x1.min(c.x1);
    let y1 = r.y1.min(c.y1);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let w = r.width();
    let h = r.height();
    let lerp_u = |x: f32| {
        if w == 0.0 {
            uv[0]
        } else {
            uv[0] + (uv[2] - uv[0]) * (x - r.x0) / w
        }
    };
    let lerp_v = |y: f32| {
        if h == 0.0 {
            uv[1]
        } else {
            uv[1] + (uv[3] - uv[1]) * (y - r.y0) / h
        }
    };
    Some((
        Rect { x0, y0, x1, y1 },
        [lerp_u(x0), lerp_v(y0), lerp_u(x1), lerp_v(y1)],
    ))
}
