pub const NAMES: [&str; 95] = [
    "NOP",
    "BREAK",
    "LOADNIL",
    "LOADB",
    "LOADN",
    "LOADK",
    "MOVE",
    "GETGLOBAL",
    "SETGLOBAL",
    "GETUPVAL",
    "SETUPVAL",
    "CLOSEUPVALS",
    "GETIMPORT",
    "GETTABLE",
    "SETTABLE",
    "GETTABLEKS",
    "SETTABLEKS",
    "GETTABLEN",
    "SETTABLEN",
    "NEWCLOSURE",
    "NAMECALL",
    "CALL",
    "RETURN",
    "JUMP",
    "JUMPBACK",
    "JUMPIF",
    "JUMPIFNOT",
    "JUMPIFEQ",
    "JUMPIFLE",
    "JUMPIFLT",
    "JUMPIFNOTEQ",
    "JUMPIFNOTLE",
    "JUMPIFNOTLT",
    "ADD",
    "SUB",
    "MUL",
    "DIV",
    "MOD",
    "POW",
    "ADDK",
    "SUBK",
    "MULK",
    "DIVK",
    "MODK",
    "POWK",
    "AND",
    "OR",
    "ANDK",
    "ORK",
    "CONCAT",
    "NOT",
    "MINUS",
    "LENGTH",
    "NEWTABLE",
    "DUPTABLE",
    "SETLIST",
    "FORNPREP",
    "FORNLOOP",
    "FORGLOOP",
    "FORGPREP_INEXT",
    "FASTCALL3",
    "FORGPREP_NEXT",
    "NATIVECALL",
    "GETVARARGS",
    "DUPCLOSURE",
    "PREPVARARGS",
    "LOADKX",
    "JUMPX",
    "FASTCALL",
    "COVERAGE",
    "CAPTURE",
    "SUBRK",
    "DIVRK",
    "FASTCALL1",
    "FASTCALL2",
    "FASTCALL2K",
    "FORGPREP",
    "JUMPXEQKNIL",
    "JUMPXEQKB",
    "JUMPXEQKN",
    "JUMPXEQKS",
    "IDIV",
    "IDIVK",
    "GETUDATAKS",
    "SETUDATAKS",
    "NAMECALLUDATA",
    "NEWCLASSMEMBER",
    "CALLFB",
    "CMPPROTO",
    "FASTPCALL",
    "NEWCLASS",
    "CONSTRUCT",
    "FINCONSTRUCT",
    "GETPRIVATEFIELD",
    "SETPRIVATEFIELD",
];

pub const NOP: u8 = 0;
pub const LOADNIL: u8 = 2;
pub const LOADB: u8 = 3;
pub const LOADN: u8 = 4;
pub const LOADK: u8 = 5;
pub const MOVE: u8 = 6;
pub const GETGLOBAL: u8 = 7;
pub const SETGLOBAL: u8 = 8;
pub const GETUPVAL: u8 = 9;
pub const SETUPVAL: u8 = 10;
pub const CLOSEUPVALS: u8 = 11;
pub const GETIMPORT: u8 = 12;
pub const GETTABLE: u8 = 13;
pub const SETTABLE: u8 = 14;
pub const GETTABLEKS: u8 = 15;
pub const SETTABLEKS: u8 = 16;
pub const GETTABLEN: u8 = 17;
pub const SETTABLEN: u8 = 18;
pub const NEWCLOSURE: u8 = 19;
pub const NAMECALL: u8 = 20;
pub const CALL: u8 = 21;
pub const RETURN: u8 = 22;
pub const JUMP: u8 = 23;
pub const JUMPBACK: u8 = 24;
pub const JUMPIF: u8 = 25;
pub const JUMPIFNOT: u8 = 26;
pub const JUMPIFEQ: u8 = 27;
pub const JUMPIFLE: u8 = 28;
pub const JUMPIFLT: u8 = 29;
pub const JUMPIFNOTEQ: u8 = 30;
pub const JUMPIFNOTLE: u8 = 31;
pub const JUMPIFNOTLT: u8 = 32;
pub const ADD: u8 = 33;
pub const POW: u8 = 38;
pub const ADDK: u8 = 39;
pub const POWK: u8 = 44;
pub const AND: u8 = 45;
pub const OR: u8 = 46;
pub const ANDK: u8 = 47;
pub const ORK: u8 = 48;
pub const CONCAT: u8 = 49;
pub const NOT: u8 = 50;
pub const MINUS: u8 = 51;
pub const LENGTH: u8 = 52;
pub const NEWTABLE: u8 = 53;
pub const DUPTABLE: u8 = 54;
pub const SETLIST: u8 = 55;
pub const FORNPREP: u8 = 56;
pub const FORNLOOP: u8 = 57;
pub const FORGLOOP: u8 = 58;
pub const FORGPREP_INEXT: u8 = 59;
pub const FASTCALL3: u8 = 60;
pub const FORGPREP_NEXT: u8 = 61;
pub const GETVARARGS: u8 = 63;
pub const DUPCLOSURE: u8 = 64;
pub const PREPVARARGS: u8 = 65;
pub const LOADKX: u8 = 66;
pub const JUMPX: u8 = 67;
pub const FASTCALL: u8 = 68;
pub const COVERAGE: u8 = 69;
pub const CAPTURE: u8 = 70;
pub const SUBRK: u8 = 71;
pub const DIVRK: u8 = 72;
pub const FASTCALL1: u8 = 73;
pub const FASTCALL2: u8 = 74;
pub const FASTCALL2K: u8 = 75;
pub const FORGPREP: u8 = 76;
pub const JUMPXEQKNIL: u8 = 77;
pub const JUMPXEQKB: u8 = 78;
pub const JUMPXEQKN: u8 = 79;
pub const JUMPXEQKS: u8 = 80;
pub const IDIV: u8 = 81;
pub const IDIVK: u8 = 82;

pub fn name(op: u8) -> &'static str {
    NAMES.get(op as usize).copied().unwrap_or("?")
}

pub fn length(op: u8) -> usize {
    match op {
        7
        | 8
        | 12
        | 15
        | 16
        | 20
        | 27..=32
        | 53
        | 55
        | 58
        | 66
        | 74
        | 75
        | 60
        | 77..=80
        | 83..=88
        | 90
        | 91
        | 93
        | 94 => 2,
        _ => 1,
    }
}

pub fn decode(raw: u8) -> u8 {
    raw.wrapping_mul(203)
}

pub fn encode(op: u8) -> u8 {
    op.wrapping_mul(227)
}

pub fn op(insn: u32) -> u8 {
    insn as u8
}

pub fn a(insn: u32) -> u8 {
    (insn >> 8) as u8
}

pub fn b(insn: u32) -> u8 {
    (insn >> 16) as u8
}

pub fn c(insn: u32) -> u8 {
    (insn >> 24) as u8
}

pub fn d(insn: u32) -> i32 {
    (insn as i32) >> 16
}

pub fn e(insn: u32) -> i32 {
    (insn as i32) >> 8
}
