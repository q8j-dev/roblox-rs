fn main() {
    let root = std::path::PathBuf::from("assets/inexperience");
    rbx_text::set_loader(move |url: &str| {
        let rel = url.strip_prefix("rbxasset://").unwrap_or(url);
        std::fs::read(root.join(rel)).ok()
    });
    let font = rbx_text::face(
        "rbxasset://LuaPackages/Packages/_Index/BuilderIcons/BuilderIcons/BuilderIcons.json",
        400,
        false,
    )
    .expect("face");
    let t = rbx_text::typesetter(font);
    let d = t.draw(
        "tilt",
        [38.0, 34.0],
        30.0,
        false,
        rbx_text::XAlign::Center,
        rbx_text::YAlign::Center,
        [0.0, 0.0],
        false,
    );
    for q in &d.quads {
        println!("{q:?}");
    }
    println!("size {:?} asc30 {}", d.size, t.ascender(30));
}
