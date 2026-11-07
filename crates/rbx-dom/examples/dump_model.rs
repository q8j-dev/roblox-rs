use rbx_dom::{DataModel, InstanceId};

fn dump(dm: &DataModel, id: InstanceId, depth: usize) {
    let mut extra = String::new();
    for p in [
        "Texture",
        "MeshId",
        "MeshType",
        "BrickColor",
        "Size",
        "Face",
        "C0",
        "Part0",
        "Part1",
        "Shape",
    ] {
        if let Some(v) = dm.get_raw(id, p) {
            extra.push_str(&format!(" {p}={v:?}"));
        }
    }
    println!(
        "{}{} ({}){}",
        "  ".repeat(depth),
        dm.name_of(id),
        dm.class_of(id).unwrap_or("?"),
        extra
    );
    for &c in dm.children_of(id) {
        dump(dm, c, depth + 1);
    }
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: dump_model <file>");
    let data = std::fs::read(&path).expect("read");
    let mut dm = DataModel::new();
    let (roots, _) = dm.load_model(&data).expect("load");
    for r in roots {
        dump(&dm, r, 0);
    }
}
