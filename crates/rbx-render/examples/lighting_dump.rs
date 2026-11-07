use rbx_dom::DataModel;

fn main() {
    let path = std::env::args().nth(1).expect("place");
    let mut dm = DataModel::new();
    dm.load_place(&std::fs::read(&path).expect("read"))
        .expect("load");
    let l = dm.find_service("Lighting").expect("lighting");
    for p in [
        "Ambient",
        "OutdoorAmbient",
        "Brightness",
        "TimeOfDay",
        "ColorShift_Top",
        "ColorShift_Bottom",
        "GlobalShadows",
        "Outlines",
    ] {
        println!("{p} = {:?}", dm.get(l, p));
    }
    let s = rbx_render::scene::extract(&dm, 1.0);
    println!(
        "key {:?} fill {:?} ambient {:?}",
        s.lighting.key_color, s.lighting.fill_color, s.lighting.ambient
    );
}
