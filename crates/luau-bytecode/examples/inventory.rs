use luau_bytecode::disasm::{constant, disassemble};
use luau_bytecode::{Module, op};
use rbx_reflection::Variant;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data = std::fs::read(&args[1]).expect("read");
    let out = PathBuf::from(&args[2]);
    let filter = args.get(3).cloned();
    std::fs::create_dir_all(&out).expect("mkdir");
    let tree = rbx_format::read(&data).expect("parse");
    let name = |i: usize| -> String {
        tree.instances[i]
            .get("Name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    let path_of = |mut i: usize| -> String {
        let mut parts = vec![name(i)];
        while let Some(p) = tree.instances[i].parent {
            parts.push(name(p));
            i = p;
        }
        parts.reverse();
        parts.join("/")
    };
    let mut globals: BTreeMap<String, usize> = BTreeMap::new();
    let mut methods: BTreeMap<String, usize> = BTreeMap::new();
    let mut keys: BTreeMap<String, usize> = BTreeMap::new();
    let mut strings: BTreeMap<String, usize> = BTreeMap::new();
    let mut failed = 0;
    let mut mismatched = 0;
    let mut parsed = 0;
    for (i, inst) in tree.instances.iter().enumerate() {
        if !inst.class.ends_with("Script") {
            continue;
        }
        let Some(Variant::BinaryString(src)) = inst.get("Source") else {
            continue;
        };
        let path = path_of(i);
        let m = match Module::parse(src) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("{path}: {e}");
                failed += 1;
                continue;
            }
        };
        parsed += 1;
        if let Ok(back) = Module::parse_raw(&m.write(), false)
            && format!("{back:?}") != format!("{m:?}")
        {
            eprintln!("round-trip mismatch: {path}");
            mismatched += 1;
        }
        if let Some(f) = &filter
            && path.contains(f.as_str())
        {
            let file = out.join(format!("{}.dis", path.replace('/', "__")));
            std::fs::write(file, disassemble(&m)).expect("write");
        }
        for s in &m.strings {
            *strings
                .entry(String::from_utf8_lossy(s).into_owned())
                .or_default() += 1;
        }
        for p in 0..m.protos.len() {
            for (_, insn, aux) in m.instructions(p) {
                let o = op::op(insn);
                let k = match o {
                    op::GETIMPORT => Some((&mut globals, op::d(insn) as usize)),
                    op::GETGLOBAL | op::SETGLOBAL => {
                        Some((&mut globals, aux.unwrap_or(0) as usize))
                    }
                    op::NAMECALL => Some((&mut methods, aux.unwrap_or(0) as usize)),
                    op::GETTABLEKS | op::SETTABLEKS => Some((&mut keys, aux.unwrap_or(0) as usize)),
                    _ => None,
                };
                if let Some((map, k)) = k {
                    *map.entry(constant(&m, p, k).trim_matches('"').to_string())
                        .or_default() += 1;
                }
            }
        }
    }
    let dump = |file: &str, map: &BTreeMap<String, usize>| {
        let mut s = String::new();
        for (k, v) in map {
            s.push_str(&format!("{v}\t{k}\n"));
        }
        std::fs::write(out.join(file), s).expect("write");
    };
    dump("globals.txt", &globals);
    dump("methods.txt", &methods);
    dump("keys.txt", &keys);
    dump("strings.txt", &strings);
    println!(
        "parsed {parsed}, failed {failed}, round-trip mismatches {mismatched}, globals {}, methods {}, keys {}",
        globals.len(),
        methods.len(),
        keys.len()
    );
}
