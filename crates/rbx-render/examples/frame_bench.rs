use rbx_dom::{DataModel, Variant};
use rbx_render::{Offscreen, wgpu};
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: frame_bench <place> [frames] [width] [height]");
        std::process::exit(2);
    }
    let frames: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(300);
    let width: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1920);
    let height: u32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(1080);
    let data = std::fs::read(&args[1]).expect("read place");
    let mut dm = DataModel::new();
    dm.load_place(&data).expect("load place");
    dm.gui_view.screen = rbx_math::Vec2::new(width as f32, height as f32);
    let content = PathBuf::from("assets/content");
    let platform = PathBuf::from("assets/platform-content/pc");
    let mut off = Offscreen::new(width, height, &content, &platform).expect("gpu");
    let texture = off
        .renderer
        .device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("bench"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: off.renderer.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let cam = dm
        .find_service("Workspace")
        .and_then(|ws| dm.get_ref(ws, "CurrentCamera"));
    let base = cam
        .and_then(|c| dm.get(c, "CFrame").ok())
        .and_then(|v| v.as_cframe());
    let mut cpu = Vec::with_capacity(frames);
    let mut total = Vec::with_capacity(frames);
    for i in 0..frames + 30 {
        if let (Some(c), Some(b)) = (cam, base) {
            let t = i as f32 * 0.01;
            let cf = rbx_math::CFrame::new(
                b.rotation * rbx_math::Mat3::from_rotation_y(t.sin() * 0.2),
                b.translation,
            );
            dm.store_silent(c, "CFrame", Variant::CFrame(cf));
        }
        let start = Instant::now();
        off.renderer.render(&dm, &view, width, height);
        let submitted = start.elapsed();
        let _ = off
            .renderer
            .device
            .poll(wgpu::PollType::wait_indefinitely());
        if i >= 30 {
            cpu.push(submitted.as_secs_f64() * 1000.0);
            total.push(start.elapsed().as_secs_f64() * 1000.0);
        }
    }
    let stats = |v: &mut Vec<f64>| {
        v.sort_by(|a, b| a.total_cmp(b));
        let avg = v.iter().sum::<f64>() / v.len().max(1) as f64;
        (
            avg,
            v[v.len() / 2],
            v[(v.len() * 99 / 100).min(v.len() - 1)],
            v[v.len() - 1],
        )
    };
    let (a, m, p99, mx) = stats(&mut cpu);
    println!("cpu   avg {a:.2} ms  median {m:.2}  p99 {p99:.2}  max {mx:.2}");
    let (a, m, p99, mx) = stats(&mut total);
    println!("frame avg {a:.2} ms  median {m:.2}  p99 {p99:.2}  max {mx:.2}");
}
