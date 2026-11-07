use rbx_dom::DataModel;

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: tree_dump <place file> [name filter]");
        std::process::exit(2);
    };
    let filter = std::env::args().nth(2).unwrap_or_default();
    let mut dm = DataModel::new();
    let data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{path}: {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = dm.load_place(&data) {
        eprintln!("{path}: {e}");
        std::process::exit(1);
    }
    let root = dm.root();
    for d in dm.descendants(root) {
        let full = dm.full_name(d);
        if !full.contains(&filter) {
            continue;
        }
        let class = dm.class_of(d).unwrap_or("");
        let extra = if dm.is_a(d, "BaseScript") {
            let len = dm
                .get(d, "Source")
                .ok()
                .and_then(|v| v.as_str().map(str::len))
                .unwrap_or(0);
            format!(" disabled={} source_len={len}", dm.get_bool(d, "Disabled"))
        } else {
            String::new()
        };
        println!("{full} [{class}]{extra}");
    }
}
