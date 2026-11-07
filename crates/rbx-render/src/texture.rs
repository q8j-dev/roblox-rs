use ddsfile::{D3DFormat, Dds, DxgiFormat};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PixelFormat {
    #[default]
    Rgba8,
    Bc1,
    Bc2,
    Bc3,
}

impl PixelFormat {
    fn block_bytes(self) -> u32 {
        match self {
            PixelFormat::Rgba8 => 4,
            PixelFormat::Bc1 => 8,
            PixelFormat::Bc2 | PixelFormat::Bc3 => 16,
        }
    }

    fn block_dim(self) -> u32 {
        match self {
            PixelFormat::Rgba8 => 1,
            _ => 4,
        }
    }

    fn wgpu(self, srgb: bool) -> wgpu::TextureFormat {
        use wgpu::TextureFormat as F;
        match (self, srgb) {
            (PixelFormat::Rgba8, false) => F::Rgba8Unorm,
            (PixelFormat::Rgba8, true) => F::Rgba8UnormSrgb,
            (PixelFormat::Bc1, false) => F::Bc1RgbaUnorm,
            (PixelFormat::Bc1, true) => F::Bc1RgbaUnormSrgb,
            (PixelFormat::Bc2, false) => F::Bc2RgbaUnorm,
            (PixelFormat::Bc2, true) => F::Bc2RgbaUnormSrgb,
            (PixelFormat::Bc3, false) => F::Bc3RgbaUnorm,
            (PixelFormat::Bc3, true) => F::Bc3RgbaUnormSrgb,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub levels: Vec<Vec<u8>>,
    pub format: PixelFormat,
}

#[derive(Debug, thiserror::Error)]
pub enum TextureError {
    #[error("dds: {0}")]
    Dds(#[from] ddsfile::Error),
    #[error("image: {0}")]
    Image(#[from] image::ImageError),
    #[error("unsupported texture format {0}")]
    Unsupported(String),
    #[error("decode: {0}")]
    Decode(&'static str),
}

#[derive(Clone, Copy)]
enum Block {
    Bc1,
    Bc2,
    Bc3,
    Rgba8,
    Bgra8,
}

impl Block {
    fn size(self, w: u32, h: u32) -> usize {
        let blocks = w.div_ceil(4) as usize * h.div_ceil(4) as usize;
        match self {
            Block::Bc1 => blocks * 8,
            Block::Bc2 | Block::Bc3 => blocks * 16,
            Block::Rgba8 | Block::Bgra8 => (w * h * 4) as usize,
        }
    }
}

fn decode_level(kind: Block, data: &[u8], w: u32, h: u32) -> Result<Vec<u8>, TextureError> {
    let (wu, hu) = (w as usize, h as usize);
    let mut out = vec![0u32; wu * hu];
    match kind {
        Block::Bc1 => {
            texture2ddecoder::decode_bc1a(data, wu, hu, &mut out).map_err(TextureError::Decode)?
        }
        Block::Bc2 => {
            texture2ddecoder::decode_bc2(data, wu, hu, &mut out).map_err(TextureError::Decode)?
        }
        Block::Bc3 => {
            texture2ddecoder::decode_bc3(data, wu, hu, &mut out).map_err(TextureError::Decode)?
        }
        Block::Rgba8 => return Ok(data[..wu * hu * 4].to_vec()),
        Block::Bgra8 => {
            let mut v = data[..wu * hu * 4].to_vec();
            for px in v.as_chunks_mut::<4>().0 {
                px.swap(0, 2);
            }
            return Ok(v);
        }
    }
    let mut bytes = Vec::with_capacity(wu * hu * 4);
    for px in out {
        let [b, g, r, a] = px.to_le_bytes();
        bytes.extend_from_slice(&[r, g, b, a]);
    }
    Ok(bytes)
}

pub fn decode_dds(bytes: &[u8]) -> Result<Vec<Image>, TextureError> {
    let dds = Dds::read(&mut std::io::Cursor::new(bytes))?;
    let kind = match (dds.get_d3d_format(), dds.get_dxgi_format()) {
        (Some(D3DFormat::DXT1), _) | (_, Some(DxgiFormat::BC1_UNorm)) => Block::Bc1,
        (Some(D3DFormat::DXT2 | D3DFormat::DXT3), _) | (_, Some(DxgiFormat::BC2_UNorm)) => {
            Block::Bc2
        }
        (Some(D3DFormat::DXT4 | D3DFormat::DXT5), _) | (_, Some(DxgiFormat::BC3_UNorm)) => {
            Block::Bc3
        }
        (Some(D3DFormat::A8B8G8R8), _) | (_, Some(DxgiFormat::R8G8B8A8_UNorm)) => Block::Rgba8,
        (Some(D3DFormat::A8R8G8B8 | D3DFormat::X8R8G8B8), _)
        | (_, Some(DxgiFormat::B8G8R8A8_UNorm)) => Block::Bgra8,
        (d3d, dxgi) => return Err(TextureError::Unsupported(format!("{d3d:?} {dxgi:?}"))),
    };
    let width = dds.get_width();
    let height = dds.get_height();
    let levels = dds.get_num_mipmap_levels().max(1);
    let layers = dds.get_num_array_layers().max(1);
    let faces = if dds.header.caps2.contains(ddsfile::Caps2::CUBEMAP) {
        6
    } else {
        1
    };
    let mut images = Vec::new();
    for layer in 0..(layers * faces).max(1) {
        let data = dds.get_data(layer.min(layers - 1)).unwrap_or(&dds.data);
        let mut offset = 0usize;
        let mut img = Image {
            width,
            height,
            levels: Vec::new(),
            format: PixelFormat::Rgba8,
        };
        if faces == 6 {
            let face_size: usize = (0..levels)
                .map(|l| kind.size((width >> l).max(1), (height >> l).max(1)))
                .sum();
            offset = face_size * layer as usize;
        }
        for l in 0..levels {
            let w = (width >> l).max(1);
            let h = (height >> l).max(1);
            let size = kind.size(w, h);
            let src = &dds.data;
            let slice = if faces == 6 {
                if offset + size > src.len() {
                    break;
                }
                &src[offset..offset + size]
            } else {
                if offset + size > data.len() {
                    break;
                }
                &data[offset..offset + size]
            };
            img.levels.push(decode_level(kind, slice, w, h)?);
            offset += size;
        }
        if img.levels.is_empty() {
            return Err(TextureError::Decode("dds has no image data"));
        }
        images.push(img);
        if faces == 1 {
            break;
        }
    }
    Ok(images)
}

fn decode_dds_compressed(bytes: &[u8]) -> Option<Image> {
    let dds = Dds::read(&mut std::io::Cursor::new(bytes)).ok()?;
    let (format, kind) = match (dds.get_d3d_format(), dds.get_dxgi_format()) {
        (Some(D3DFormat::DXT1), _) | (_, Some(DxgiFormat::BC1_UNorm)) => {
            (PixelFormat::Bc1, Block::Bc1)
        }
        (Some(D3DFormat::DXT2 | D3DFormat::DXT3), _) | (_, Some(DxgiFormat::BC2_UNorm)) => {
            (PixelFormat::Bc2, Block::Bc2)
        }
        (Some(D3DFormat::DXT4 | D3DFormat::DXT5), _) | (_, Some(DxgiFormat::BC3_UNorm)) => {
            (PixelFormat::Bc3, Block::Bc3)
        }
        _ => return None,
    };
    let width = dds.get_width();
    let height = dds.get_height();
    if dds.header.caps2.contains(ddsfile::Caps2::CUBEMAP)
        || dds.get_num_array_layers() > 1
        || width % 4 != 0
        || height % 4 != 0
    {
        return None;
    }
    let data = dds.get_data(0).unwrap_or(&dds.data);
    let mut offset = 0usize;
    let mut levels = Vec::new();
    for l in 0..dds.get_num_mipmap_levels().max(1) {
        let size = kind.size((width >> l).max(1), (height >> l).max(1));
        if offset + size > data.len() {
            break;
        }
        levels.push(data[offset..offset + size].to_vec());
        offset += size;
    }
    (!levels.is_empty()).then_some(Image {
        width,
        height,
        levels,
        format,
    })
}

pub fn decode_for_gpu(bytes: &[u8], compressed: bool) -> Result<Image, TextureError> {
    if compressed
        && bytes.starts_with(b"DDS ")
        && let Some(img) = decode_dds_compressed(bytes)
    {
        return Ok(img);
    }
    decode(bytes)
}

pub fn decode(bytes: &[u8]) -> Result<Image, TextureError> {
    if bytes.starts_with(b"DDS ") {
        return decode_dds(bytes)?
            .into_iter()
            .next()
            .ok_or(TextureError::Decode("empty dds"));
    }
    let img = image::load_from_memory(bytes)?.to_rgba8();
    let (width, height) = img.dimensions();
    Ok(Image {
        width,
        height,
        levels: vec![img.into_raw()],
        format: PixelFormat::Rgba8,
    }
    .with_mips())
}

impl Image {
    pub fn solid(rgba: [u8; 4]) -> Image {
        Image {
            width: 1,
            height: 1,
            levels: vec![rgba.to_vec()],
            format: PixelFormat::Rgba8,
        }
    }

    pub fn with_mips(mut self) -> Image {
        if self.levels.len() > 1 || self.format != PixelFormat::Rgba8 {
            return self;
        }
        let (mut w, mut h) = (self.width, self.height);
        while w > 1 || h > 1 {
            let prev = self.levels.last().cloned().unwrap_or_default();
            let nw = (w / 2).max(1);
            let nh = (h / 2).max(1);
            let mut next = vec![0u8; (nw * nh * 4) as usize];
            for y in 0..nh {
                for x in 0..nw {
                    let mut acc = [0u32; 4];
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(w - 1);
                        let sy = (y * 2 + dy).min(h - 1);
                        let i = ((sy * w + sx) * 4) as usize;
                        for (c, a) in acc.iter_mut().enumerate() {
                            *a += prev[i + c] as u32;
                        }
                    }
                    let o = ((y * nw + x) * 4) as usize;
                    for c in 0..4 {
                        next[o + c] = (acc[c] / 4) as u8;
                    }
                }
            }
            self.levels.push(next);
            w = nw;
            h = nh;
        }
        self
    }

    pub fn upload(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        label: &str,
        srgb: bool,
    ) -> wgpu::Texture {
        let format = self.format.wgpu(srgb);
        let dim = self.format.block_dim();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: self.levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, data) in self.levels.iter().enumerate() {
            let w = (self.width >> level).max(1).div_ceil(dim) * dim;
            let h = (self.height >> level).max(1).div_ceil(dim) * dim;
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w / dim * self.format.block_bytes()),
                    rows_per_image: Some(h / dim),
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
        }
        texture
    }
}

pub fn upload_cube(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    faces: &[Image; 6],
    label: &str,
) -> wgpu::Texture {
    let size = faces[0].width;
    let levels = faces.iter().map(|f| f.levels.len()).min().unwrap_or(1) as u32;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 6,
        },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (layer, face) in faces.iter().enumerate() {
        for level in 0..levels as usize {
            let w = (size >> level).max(1);
            let data = &face.levels[level];
            if data.len() < (w * w * 4) as usize || face.width != size {
                continue;
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer as u32,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w * 4),
                    rows_per_image: Some(w),
                },
                wgpu::Extent3d {
                    width: w,
                    height: w,
                    depth_or_array_layers: 1,
                },
            );
        }
    }
    texture
}
