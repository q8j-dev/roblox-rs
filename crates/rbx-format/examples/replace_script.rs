use rbx_reflection::Variant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("usage: replace_script <place> <old source file> <new source file>");
        std::process::exit(2);
    }
    let old = std::fs::read_to_string(&args[2]).expect("old source");
    let new = std::fs::read_to_string(&args[3]).expect("new source");
    let data = std::fs::read(&args[1]).expect("read place");
    let binary = rbx_format::is_binary(&data);
    let mut tree = rbx_format::read(&data).expect("parse place");
    let mut count = 0;
    for inst in &mut tree.instances {
        let replacement = match inst.get("Source") {
            Some(Variant::String(s)) if *s == old => Variant::String(new.clone()),
            Some(Variant::ProtectedString(s)) if *s == old => Variant::ProtectedString(new.clone()),
            _ => continue,
        };
        inst.set("Source", replacement);
        count += 1;
    }
    if count == 0 {
        eprintln!("no script has that source");
        std::process::exit(1);
    }
    let out = if binary {
        rbx_format::write_binary(&tree, Default::default()).expect("write binary")
    } else {
        rbx_format::write_xml(&tree).into_bytes()
    };
    std::fs::write(&args[1], out).expect("write place");
    println!("replaced {count} script(s)");
}
