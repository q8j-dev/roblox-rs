use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const MOVI_FOURCC_OFFSET: u64 = 220;
const KEYFRAME: u32 = 0x10;

pub struct VideoRecorder {
    sender: Option<Sender<Vec<u8>>>,
    worker: Option<JoinHandle<Result<u32, String>>>,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    interval: Duration,
    next: Instant,
}

impl VideoRecorder {
    pub fn start(path: PathBuf, width: u32, height: u32, fps: u32, quality: u8, bgra: bool) -> Result<VideoRecorder, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let file = File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let (sender, receiver) = channel();
        let worker = std::thread::Builder::new()
            .name("video recorder".into())
            .spawn(move || write_avi(file, width, height, fps, quality, bgra, receiver).map_err(|e| e.to_string()))
            .map_err(|e| e.to_string())?;
        Ok(VideoRecorder {
            sender: Some(sender),
            worker: Some(worker),
            path,
            width,
            height,
            interval: Duration::from_secs_f64(1.0 / fps.max(1) as f64),
            next: Instant::now(),
        })
    }

    pub fn wants_frame(&mut self) -> bool {
        let now = Instant::now();
        if now < self.next {
            return false;
        }
        self.next += self.interval;
        if self.next < now {
            self.next = now + self.interval;
        }
        true
    }

    pub fn push(&self, pixels: Vec<u8>) {
        if let Some(s) = &self.sender {
            let _ = s.send(pixels);
        }
    }

    pub fn finish(mut self) -> Result<(PathBuf, u32), String> {
        self.sender.take();
        let frames = match self.worker.take() {
            Some(w) => w.join().map_err(|_| "video writer panicked".to_string())??,
            None => 0,
        };
        Ok((self.path.clone(), frames))
    }
}

impl Drop for VideoRecorder {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

pub fn video_path(home: &Path, stamp: &str) -> PathBuf {
    let folder = if cfg!(target_os = "macos") { "Movies" } else { "Videos" };
    home.join(folder).join("Roblox").join(format!("RobloxVideo{stamp}.avi"))
}

fn u32le(w: &mut impl Write, v: u32) -> std::io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

fn u16le(w: &mut impl Write, v: u16) -> std::io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

fn write_headers(w: &mut impl Write, width: u32, height: u32, fps: u32) -> std::io::Result<()> {
    w.write_all(b"RIFF")?;
    u32le(w, 0)?;
    w.write_all(b"AVI ")?;
    w.write_all(b"LIST")?;
    u32le(w, 192)?;
    w.write_all(b"hdrl")?;
    w.write_all(b"avih")?;
    u32le(w, 56)?;
    u32le(w, 1_000_000 / fps.max(1))?;
    u32le(w, 0)?;
    u32le(w, 0)?;
    u32le(w, KEYFRAME)?;
    u32le(w, 0)?;
    u32le(w, 0)?;
    u32le(w, 1)?;
    u32le(w, 0)?;
    u32le(w, width)?;
    u32le(w, height)?;
    for _ in 0..4 {
        u32le(w, 0)?;
    }
    w.write_all(b"LIST")?;
    u32le(w, 116)?;
    w.write_all(b"strl")?;
    w.write_all(b"strh")?;
    u32le(w, 56)?;
    w.write_all(b"vids")?;
    w.write_all(b"MJPG")?;
    u32le(w, 0)?;
    u16le(w, 0)?;
    u16le(w, 0)?;
    u32le(w, 0)?;
    u32le(w, 1)?;
    u32le(w, fps.max(1))?;
    u32le(w, 0)?;
    u32le(w, 0)?;
    u32le(w, 0)?;
    u32le(w, u32::MAX)?;
    u32le(w, 0)?;
    u16le(w, 0)?;
    u16le(w, 0)?;
    u16le(w, width as u16)?;
    u16le(w, height as u16)?;
    w.write_all(b"strf")?;
    u32le(w, 40)?;
    u32le(w, 40)?;
    u32le(w, width)?;
    u32le(w, height)?;
    u16le(w, 1)?;
    u16le(w, 24)?;
    w.write_all(b"MJPG")?;
    u32le(w, width * height * 3)?;
    for _ in 0..4 {
        u32le(w, 0)?;
    }
    w.write_all(b"LIST")?;
    u32le(w, 0)?;
    w.write_all(b"movi")
}

fn encode_jpeg(pixels: &[u8], width: u32, height: u32, quality: u8, bgra: bool) -> std::io::Result<Vec<u8>> {
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    for px in pixels.chunks_exact(4) {
        if bgra {
            rgb.extend_from_slice(&[px[2], px[1], px[0]]);
        } else {
            rgb.extend_from_slice(&[px[0], px[1], px[2]]);
        }
    }
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode(&rgb, width, height, image::ExtendedColorType::Rgb8)
        .map_err(std::io::Error::other)?;
    Ok(out)
}

fn write_avi(file: File, width: u32, height: u32, fps: u32, quality: u8, bgra: bool, frames: Receiver<Vec<u8>>) -> std::io::Result<u32> {
    let mut w = BufWriter::new(file);
    write_headers(&mut w, width, height, fps)?;
    let mut index: Vec<(u32, u32)> = Vec::new();
    let mut offset: u32 = 4;
    let mut largest = 0u32;
    for pixels in frames {
        if pixels.len() < (width * height * 4) as usize {
            continue;
        }
        let jpeg = encode_jpeg(&pixels, width, height, quality, bgra)?;
        let size = jpeg.len() as u32;
        w.write_all(b"00dc")?;
        u32le(&mut w, size)?;
        w.write_all(&jpeg)?;
        if size % 2 == 1 {
            w.write_all(&[0])?;
        }
        index.push((offset, size));
        largest = largest.max(size);
        offset += 8 + size + size % 2;
    }
    let movi_size = offset;
    w.write_all(b"idx1")?;
    u32le(&mut w, index.len() as u32 * 16)?;
    for (off, size) in &index {
        w.write_all(b"00dc")?;
        u32le(&mut w, KEYFRAME)?;
        u32le(&mut w, *off)?;
        u32le(&mut w, *size)?;
    }
    let end = w.stream_position()?;
    let count = index.len() as u32;
    let patch = |pos: u64, v: u32, w: &mut BufWriter<File>| -> std::io::Result<()> {
        w.seek(SeekFrom::Start(pos))?;
        u32le(w, v)
    };
    patch(4, (end - 8) as u32, &mut w)?;
    patch(48, count, &mut w)?;
    patch(60, largest, &mut w)?;
    patch(140, count, &mut w)?;
    patch(144, largest, &mut w)?;
    patch(MOVI_FOURCC_OFFSET - 4, movi_size, &mut w)?;
    w.flush()?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_playable_avi_layout() {
        let dir = std::env::temp_dir().join(format!("rbx-video-{}", std::process::id()));
        let path = dir.join("test.avi");
        let rec = VideoRecorder::start(path.clone(), 16, 8, 30, 80, false).expect("start");
        for i in 0..3u8 {
            rec.push(vec![i * 40; 16 * 8 * 4]);
        }
        let (out, frames) = rec.finish().expect("finish");
        assert_eq!(frames, 3);
        let data = std::fs::read(&out).expect("read");
        assert_eq!(&data[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize, data.len() - 8);
        assert_eq!(&data[8..12], b"AVI ");
        assert_eq!(&data[MOVI_FOURCC_OFFSET as usize..MOVI_FOURCC_OFFSET as usize + 4], b"movi");
        assert_eq!(u32::from_le_bytes(data[48..52].try_into().unwrap()), 3);
        assert_eq!(&data[224..228], b"00dc");
        let jpeg_len = u32::from_le_bytes(data[228..232].try_into().unwrap()) as usize;
        assert_eq!(&data[232..234], &[0xFF, 0xD8]);
        assert!(jpeg_len > 2);
        let _ = std::fs::remove_dir_all(dir);
    }
}
