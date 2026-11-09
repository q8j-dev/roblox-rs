use crate::op::{self, *};
use crate::{Constant, Module};
use std::fmt::Write;

pub fn import_path(m: &Module, proto: usize, id: u32) -> String {
    let count = id >> 30;
    let ks = [(id >> 20) & 1023, (id >> 10) & 1023, id & 1023];
    let p = &m.protos[proto];
    ks[..count as usize]
        .iter()
        .map(|&k| match p.constants.get(k as usize) {
            Some(Constant::String(s)) => m.str(*s),
            _ => "?".to_string(),
        })
        .collect::<Vec<_>>()
        .join(".")
}

pub fn constant(m: &Module, proto: usize, k: usize) -> String {
    match m.protos[proto].constants.get(k) {
        Some(Constant::Nil) => "nil".into(),
        Some(Constant::Bool(b)) => b.to_string(),
        Some(Constant::Number(n)) => n.to_string(),
        Some(Constant::Integer(n)) => format!("{n}i"),
        Some(Constant::String(s)) => format!("{:?}", m.str(*s)),
        Some(Constant::Import(i)) => import_path(m, proto, *i),
        Some(Constant::Vector(v)) => format!("vector({}, {}, {})", v[0], v[1], v[2]),
        Some(Constant::Table(keys)) => format!("table[{}]", keys.len()),
        Some(Constant::TableWithConstants(keys)) => format!("table[{}]", keys.len()),
        Some(Constant::Closure(f)) => format!("closure#{f}"),
        None => format!("K{k}?"),
    }
}

pub fn proto_name(m: &Module, proto: usize) -> String {
    let p = &m.protos[proto];
    match m.string(p.debug_name) {
        Some(s) => String::from_utf8_lossy(s).into_owned(),
        None if proto == m.main as usize => "main".into(),
        None => format!("anon#{proto}"),
    }
}

pub fn disassemble(m: &Module) -> String {
    let mut out = String::new();
    for i in 0..m.protos.len() {
        let p = &m.protos[i];
        let _ = writeln!(
            out,
            "function {} #{} (params {}, upvalues {}, stack {}{}) line {}",
            proto_name(m, i),
            i,
            p.num_params,
            p.num_upvalues,
            p.max_stack,
            if p.is_vararg { ", vararg" } else { "" },
            p.line_defined
        );
        for (pc, insn, aux) in m.instructions(i) {
            let o = op::op(insn);
            let (a, b, c, d) = (op::a(insn), op::b(insn), op::c(insn), op::d(insn));
            let detail = match o {
                LOADK => format!("R{a} = {}", constant(m, i, d as usize)),
                LOADKX => format!("R{a} = {}", constant(m, i, aux.unwrap_or(0) as usize)),
                LOADN => format!("R{a} = {d}"),
                LOADB => format!("R{a} = {} jump {c}", b != 0),
                MOVE => format!("R{a} = R{b}"),
                GETIMPORT => format!("R{a} = {}", constant(m, i, d as usize)),
                GETGLOBAL => format!("R{a} = _G[{}]", constant(m, i, aux.unwrap_or(0) as usize)),
                SETGLOBAL => format!("_G[{}] = R{a}", constant(m, i, aux.unwrap_or(0) as usize)),
                GETTABLEKS => format!("R{a} = R{b}.{}", constant(m, i, aux.unwrap_or(0) as usize)),
                SETTABLEKS => format!("R{b}.{} = R{a}", constant(m, i, aux.unwrap_or(0) as usize)),
                NAMECALL => format!("R{a} = R{b}:{}", constant(m, i, aux.unwrap_or(0) as usize)),
                CALL => format!("R{a}(.. {}) -> {}", b as i32 - 1, c as i32 - 1),
                RETURN => format!("return R{a} .. {}", b as i32 - 1),
                GETUPVAL => format!("R{a} = U{b}"),
                SETUPVAL => format!("U{b} = R{a}"),
                NEWCLOSURE => format!(
                    "R{a} = closure {}",
                    p.children
                        .get(d as usize)
                        .map(|&c| proto_name(m, c as usize))
                        .unwrap_or_default()
                ),
                DUPCLOSURE => format!("R{a} = closure {}", constant(m, i, d as usize)),
                CAPTURE => format!("capture {a} {b}"),
                JUMP | JUMPBACK | JUMPIF | JUMPIFNOT | FORNPREP | FORNLOOP | FORGPREP
                | FORGPREP_INEXT | FORGPREP_NEXT => {
                    format!("R{a} -> {}", pc as i32 + 1 + d)
                }
                JUMPIFEQ..=JUMPIFNOTLT => {
                    format!("R{a} R{} -> {}", aux.unwrap_or(0), pc as i32 + 1 + d)
                }
                JUMPXEQKNIL..=JUMPXEQKS => format!(
                    "R{a} {}{} -> {}",
                    if aux.unwrap_or(0) >> 31 != 0 {
                        "not "
                    } else {
                        ""
                    },
                    if o == JUMPXEQKNIL {
                        "nil".to_string()
                    } else if o == JUMPXEQKB {
                        (aux.unwrap_or(0) & 1 != 0).to_string()
                    } else {
                        constant(m, i, (aux.unwrap_or(0) & 0xffffff) as usize)
                    },
                    pc as i32 + 1 + d
                ),
                JUMPX => format!("-> {}", pc as i32 + 1 + op::e(insn)),
                ADDK..=POWK | ANDK | ORK | IDIVK => {
                    format!("R{a} = R{b} op {}", constant(m, i, c as usize))
                }
                SUBRK | DIVRK => format!("R{a} = {} op R{c}", constant(m, i, b as usize)),
                DUPTABLE => format!("R{a} = {}", constant(m, i, d as usize)),
                FASTCALL | FASTCALL1 | FASTCALL2 | FASTCALL2K | FASTCALL3 => {
                    format!("builtin {a} -> {}", pc as i32 + 1 + c as i32)
                }
                _ => format!("{a} {b} {c}"),
            };
            let line = p.line(pc).map(|l| format!("{l:>5} ")).unwrap_or_default();
            let _ = writeln!(out, "  {line}{pc:>5} {:<14} {detail}", op::name(o));
        }
    }
    out
}
