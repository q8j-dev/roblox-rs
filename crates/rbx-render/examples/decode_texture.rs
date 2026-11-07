fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&args[1]).expect("read");
    let img = rbx_render::texture::decode(&bytes).expect("decode");
    image::save_buffer(
        &args[2],
        &img.levels[0],
        img.width,
        img.height,
        image::ColorType::Rgba8,
    )
    .expect("save");
    println!("{}x{} levels={}", img.width, img.height, img.levels.len());
}
