use rbx_reflection::Variant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("usage: patch_script <place> <old text> <new text>");
        std::process::exit(2);
    }
    let path = &args[1];
    let data = std::fs::read(path).expect("read place");
    let binary = rbx_format::is_binary(&data);
    let mut tree = rbx_format::read(&data).expect("parse place");
    let mut count = 0;
    for inst in &mut tree.instances {
        let (patched, protected) = match inst.get("Source") {
            Some(Variant::String(s)) => (s.replace(args[2].as_str(), &args[3]), false),
            Some(Variant::ProtectedString(s)) => (s.replace(args[2].as_str(), &args[3]), true),
            _ => continue,
        };
        if inst.get("Source").and_then(|v| v.as_str()) == Some(patched.as_str()) {
            continue;
        }
        inst.set(
            "Source",
            if protected {
                Variant::ProtectedString(patched)
            } else {
                Variant::String(patched)
            },
        );
        count += 1;
    }
    if count == 0 {
        eprintln!("no script contains the text");
        std::process::exit(1);
    }
    let out = if binary {
        rbx_format::write_binary(&tree, Default::default()).expect("write binary")
    } else {
        rbx_format::write_xml(&tree).into_bytes()
    };
    std::fs::write(path, out).expect("write place");
    println!("patched {count} script(s)");
}
