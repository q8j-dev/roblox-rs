use crate::renderer::Renderer;
use rbx_dom::DataModel;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum OffscreenError {
    #[error("no graphics adapter: {0}")]
    Adapter(String),
    #[error("device: {0}")]
    Device(String),
    #[error("readback failed: {0}")]
    Readback(String),
}

pub struct Offscreen {
    pub renderer: Renderer,
    target: wgpu::Texture,
    width: u32,
    height: u32,
}

pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

pub fn create_device() -> Result<(wgpu::Device, wgpu::Queue), OffscreenError> {
    let instance = crate::create_instance();
    let request = |fallback: bool| {
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: fallback,
            compatible_surface: None,
            apply_limit_buckets: false,
        }))
    };
    let adapter = request(false)
        .or_else(|_| request(true))
        .map_err(|e| OffscreenError::Adapter(e.to_string()))?;
    pollster::block_on(adapter.request_device(&crate::device_descriptor(&adapter)))
        .map_err(|e| OffscreenError::Device(e.to_string()))
}

impl Offscreen {
    pub fn new(
        width: u32,
        height: u32,
        content: &Path,
        platform: &Path,
    ) -> Result<Offscreen, OffscreenError> {
        let (device, queue) = create_device()?;
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let renderer = Renderer::new(device, queue, FORMAT, content, platform);
        Ok(Offscreen {
            renderer,
            target,
            width,
            height,
        })
    }

    pub fn render(&mut self, dm: &DataModel) -> Result<Vec<u8>, OffscreenError> {
        let view = self
            .target
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.renderer.render(dm, &view, self.width, self.height);
        self.read()
    }

    fn read(&self) -> Result<Vec<u8>, OffscreenError> {
        let device = &self.renderer.device;
        let queue = &self.renderer.queue;
        let row = self.width * 4;
        let padded =
            row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (padded * self.height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("readback"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| OffscreenError::Readback(e.to_string()))?;
        rx.recv()
            .map_err(|e| OffscreenError::Readback(e.to_string()))?
            .map_err(|e| OffscreenError::Readback(e.to_string()))?;
        let data = slice
            .get_mapped_range()
            .map_err(|e| OffscreenError::Readback(e.to_string()))?;
        let mut out = Vec::with_capacity((row * self.height) as usize);
        for y in 0..self.height {
            let start = (y * padded) as usize;
            out.extend_from_slice(&data[start..start + row as usize]);
        }
        drop(data);
        buffer.unmap();
        Ok(out)
    }

    pub fn save_png(&mut self, dm: &DataModel, path: &Path) -> Result<(), OffscreenError> {
        let pixels = self.render(dm)?;
        image::save_buffer(
            path,
            &pixels,
            self.width,
            self.height,
            image::ColorType::Rgba8,
        )
        .map_err(|e| OffscreenError::Readback(e.to_string()))
    }
}
