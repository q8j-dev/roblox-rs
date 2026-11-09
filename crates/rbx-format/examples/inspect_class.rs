fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data = std::fs::read(&args[1]).unwrap();
    let tree = rbx_format::read(&data).unwrap();
    for (i, inst) in tree.instances.iter().enumerate() {
        if inst.class == args[2] {
            println!("#{i} {} parent={:?}", inst.class, inst.parent);
            for (n, v) in &inst.properties {
                println!("  {n} = {v:?}");
            }
        }
    }
}
