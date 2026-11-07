use rbx_dom::DataModel;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: get_prop <class> <property> <place>...");
        std::process::exit(2);
    }
    let (class, prop) = (&args[1], &args[2]);
    for path in &args[3..] {
        let Ok(data) = std::fs::read(path) else {
            continue;
        };
        let mut dm = DataModel::new();
        if dm.load_place(&data).is_err() {
            continue;
        }
        for id in dm.descendants(dm.root()) {
            if dm.class_of(id) == Some(class.as_str()) {
                println!("{path}: {} = {:?}", dm.name_of(id), dm.get(id, prop));
            }
        }
    }
}
