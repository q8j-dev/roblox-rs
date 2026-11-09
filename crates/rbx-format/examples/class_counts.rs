fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data = std::fs::read(&args[1]).unwrap();
    let tree = rbx_format::read(&data).unwrap();
    let mut counts = std::collections::BTreeMap::new();
    for inst in &tree.instances {
        let parent = inst
            .parent
            .map(|p| tree.instances[p].class.clone())
            .unwrap_or_default();
        *counts.entry((inst.class.clone(), parent)).or_insert(0) += 1;
    }
    for ((c, p), n) in counts {
        println!("{n:6} {c} (in {p})");
    }
}
