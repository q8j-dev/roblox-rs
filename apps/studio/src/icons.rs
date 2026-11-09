use egui::{Rect, TextureHandle, TextureId, pos2};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct Icons {
    root: PathBuf,
    cache: HashMap<PathBuf, Option<TextureHandle>>,
    disabled: HashMap<PathBuf, Option<TextureHandle>>,
    classes: Option<(TextureHandle, usize)>,
    strips: HashMap<String, Option<(TextureHandle, usize)>>,
}

fn decode(bytes: &[u8]) -> Option<egui::ColorImage> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
}

impl Icons {
    pub fn new(root: &Path) -> Icons {
        Icons {
            root: root.to_path_buf(),
            cache: HashMap::new(),
            disabled: HashMap::new(),
            classes: None,
            strips: HashMap::new(),
        }
    }

    pub fn get(&mut self, ctx: &egui::Context, path: &Path) -> Option<TextureHandle> {
        let full = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        self.cache
            .entry(full.clone())
            .or_insert_with(|| {
                let bytes = std::fs::read(&full).ok()?;
                let img = decode(&bytes)?;
                Some(ctx.load_texture(
                    full.display().to_string(),
                    img,
                    egui::TextureOptions::LINEAR,
                ))
            })
            .clone()
    }

    pub fn studio(&mut self, ctx: &egui::Context, rel: &str) -> Option<TextureHandle> {
        self.get(ctx, Path::new(rel))
    }

    pub fn studio_disabled(&mut self, ctx: &egui::Context, rel: &str) -> Option<TextureHandle> {
        let full = self.root.join(rel);
        self.disabled
            .entry(full.clone())
            .or_insert_with(|| {
                let bytes = std::fs::read(&full).ok()?;
                let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
                let size = [img.width() as usize, img.height() as usize];
                let mut px = img.into_raw();
                for p in px.as_chunks_mut::<4>().0 {
                    let g = (p[0] as u32 * 11 + p[1] as u32 * 16 + p[2] as u32 * 5) / 32;
                    let v = (128 + g / 2).min(255) as u8;
                    p[0] = v;
                    p[1] = v;
                    p[2] = v;
                }
                let img = egui::ColorImage::from_rgba_unmultiplied(size, &px);
                Some(ctx.load_texture(
                    format!("{}#disabled", full.display()),
                    img,
                    egui::TextureOptions::LINEAR,
                ))
            })
            .clone()
    }

    pub fn class(&mut self, ctx: &egui::Context, index: usize) -> Option<(TextureId, Rect)> {
        if self.classes.is_none() {
            let bytes = std::fs::read(self.root.join("images/ClassImages.PNG")).ok()?;
            let img = decode(&bytes)?;
            let count = img.size[0] / 16;
            let tex = ctx.load_texture("ClassImages", img, egui::TextureOptions::NEAREST);
            self.classes = Some((tex, count.max(1)));
        }
        let (tex, count) = self.classes.as_ref()?;
        let i = index.min(count - 1) as f32;
        let w = 1.0 / *count as f32;
        Some((
            tex.id(),
            Rect::from_min_max(pos2(i * w, 0.0), pos2((i + 1.0) * w, 1.0)),
        ))
    }

    pub fn strip(
        &mut self,
        ctx: &egui::Context,
        rel: &str,
        index: usize,
        size: usize,
    ) -> Option<(TextureId, Rect)> {
        let root = self.root.clone();
        let entry = self
            .strips
            .entry(rel.to_string())
            .or_insert_with(|| {
                let bytes = std::fs::read(root.join(rel)).ok()?;
                let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
                let (w, h) = (img.width() as usize, img.height() as usize);
                let mut px = img.into_raw();
                let count = (w / size.max(1)).max(1);
                for i in 0..count {
                    let key = [px[i * size * 4], px[i * size * 4 + 1], px[i * size * 4 + 2]];
                    for y in 0..h.min(size) {
                        for x in i * size..((i + 1) * size).min(w) {
                            let o = (y * w + x) * 4;
                            if px[o] == key[0] && px[o + 1] == key[1] && px[o + 2] == key[2] {
                                px[o + 3] = 0;
                            }
                        }
                    }
                }
                let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &px);
                Some((
                    ctx.load_texture(rel.to_string(), image, egui::TextureOptions::NEAREST),
                    count,
                ))
            })
            .clone();
        let (tex, count) = entry?;
        let i = index.min(count - 1) as f32;
        let w = 1.0 / count as f32;
        Some((
            tex.id(),
            Rect::from_min_max(pos2(i * w, 0.0), pos2((i + 1.0) * w, 1.0)),
        ))
    }
}
