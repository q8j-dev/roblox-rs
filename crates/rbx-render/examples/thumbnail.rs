fn main() {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "rbxthumb://type=Avatar&id=1&w=150&h=150".into());
    let out = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "thumbnail.png".into());
    let png = rbx_render::thumbnail::fetch(
        &url,
        std::path::Path::new("assets/content"),
        std::path::Path::new("assets/platform-content/pc"),
    )
    .expect("thumbnail");
    std::fs::write(&out, png).expect("write");
    println!("wrote {out}");
}
