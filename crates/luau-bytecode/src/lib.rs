pub mod disasm;
pub mod op;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("unexpected end of bytecode at offset {0}")]
    Eof(usize),
    #[error("unsupported bytecode version {0}")]
    Version(u8),
    #[error("unsupported type info version {0}")]
    TypesVersion(u8),
    #[error("unknown constant kind {0}")]
    Constant(u8),
    #[error("compile error: {0}")]
    Compile(String),
}

pub const HEADER_LEN: usize = 8;

#[derive(Clone, Debug, PartialEq)]
pub enum Constant {
    Nil,
    Bool(bool),
    Number(f64),
    Vector([f32; 4]),
    String(u32),
    Import(u32),
    Table(Vec<u32>),
    TableWithConstants(Vec<(u32, i32)>),
    Closure(u32),
    Integer(i64),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Local {
    pub name: u32,
    pub start_pc: u32,
    pub end_pc: u32,
    pub reg: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DebugInfo {
    pub locals: Vec<Local>,
    pub upvalues: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LineInfo {
    pub gap_log2: u8,
    pub deltas: Vec<u8>,
    pub abs: Vec<i32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Proto {
    pub max_stack: u8,
    pub num_params: u8,
    pub num_upvalues: u8,
    pub is_vararg: bool,
    pub flags: u8,
    pub type_info: Vec<u8>,
    pub code: Vec<u32>,
    pub constants: Vec<Constant>,
    pub children: Vec<u32>,
    pub line_defined: u32,
    pub debug_name: u32,
    pub line_info: Option<LineInfo>,
    pub debug: Option<DebugInfo>,
}

impl Proto {
    pub fn line(&self, pc: usize) -> Option<i32> {
        let li = self.line_info.as_ref()?;
        let mut acc = 0u8;
        for d in &li.deltas[..=pc.min(li.deltas.len().saturating_sub(1))] {
            acc = acc.wrapping_add(*d);
        }
        Some(li.abs.get(pc >> li.gap_log2)? + acc as i32)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Module {
    pub version: u8,
    pub types_version: u8,
    pub strings: Vec<Vec<u8>>,
    pub userdata_types: Vec<(u8, u32)>,
    pub protos: Vec<Proto>,
    pub main: u32,
}

struct Reader<'a> {
    d: &'a [u8],
    o: usize,
}

impl Reader<'_> {
    fn u8(&mut self) -> Result<u8, Error> {
        let v = *self.d.get(self.o).ok_or(Error::Eof(self.o))?;
        self.o += 1;
        Ok(v)
    }

    fn take(&mut self, n: usize) -> Result<&[u8], Error> {
        let s = self.d.get(self.o..self.o + n).ok_or(Error::Eof(self.o))?;
        self.o += n;
        Ok(s)
    }

    fn u32(&mut self) -> Result<u32, Error> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn varint(&mut self) -> Result<u32, Error> {
        Ok(self.varint64()? as u32)
    }

    fn varint64(&mut self) -> Result<u64, Error> {
        let mut r = 0u64;
        let mut s = 0;
        loop {
            let b = self.u8()?;
            r |= ((b & 127) as u64) << s;
            s += 7;
            if b & 128 == 0 || s >= 64 {
                return Ok(r);
            }
        }
    }
}

pub fn has_roblox_header(data: &[u8]) -> bool {
    data.len() > HEADER_LEN && !(3..=14).contains(&data[0]) && (3..=14).contains(&data[HEADER_LEN])
}

impl Module {
    pub fn parse(data: &[u8]) -> Result<Module, Error> {
        if has_roblox_header(data) {
            Module::parse_raw(&data[HEADER_LEN..], true)
        } else {
            Module::parse_raw(data, false)
        }
    }

    pub fn parse_raw(data: &[u8], encoded: bool) -> Result<Module, Error> {
        let mut r = Reader { d: data, o: 0 };
        let version = r.u8()?;
        if !(3..=9).contains(&version) {
            return Err(Error::Version(version));
        }
        let types_version = if version >= 4 { r.u8()? } else { 0 };
        if types_version > 3 {
            return Err(Error::TypesVersion(types_version));
        }
        let n = r.varint()? as usize;
        let mut strings = Vec::with_capacity(n);
        for _ in 0..n {
            let l = r.varint()? as usize;
            strings.push(r.take(l)?.to_vec());
        }
        let mut userdata_types = Vec::new();
        if types_version == 3 {
            let mut index = r.u8()?;
            while index != 0 {
                userdata_types.push((index, r.varint()?));
                index = r.u8()?;
            }
        }
        let count = r.varint()? as usize;
        let mut protos = Vec::with_capacity(count);
        for _ in 0..count {
            let max_stack = r.u8()?;
            let num_params = r.u8()?;
            let num_upvalues = r.u8()?;
            let is_vararg = r.u8()? != 0;
            let mut flags = 0;
            let mut type_info = Vec::new();
            if version >= 4 {
                flags = r.u8()?;
                let n = r.varint()? as usize;
                type_info = r.take(n)?.to_vec();
            }
            let n = r.varint()? as usize;
            let mut code = Vec::with_capacity(n);
            for _ in 0..n {
                code.push(r.u32()?);
            }
            if encoded {
                let mut pc = 0;
                while pc < code.len() {
                    let o = op::decode(code[pc] as u8);
                    code[pc] = (code[pc] & !0xff) | o as u32;
                    pc += op::length(o);
                }
            }
            let n = r.varint()? as usize;
            let mut constants = Vec::with_capacity(n);
            for _ in 0..n {
                let kind = r.u8()?;
                constants.push(match kind {
                    0 => Constant::Nil,
                    1 => Constant::Bool(r.u8()? != 0),
                    2 => Constant::Number(f64::from_le_bytes(r.take(8)?.try_into().unwrap())),
                    3 => Constant::String(r.varint()?),
                    4 => Constant::Import(r.u32()?),
                    5 => {
                        let n = r.varint()? as usize;
                        let mut keys = Vec::with_capacity(n);
                        for _ in 0..n {
                            keys.push(r.varint()?);
                        }
                        Constant::Table(keys)
                    }
                    6 => Constant::Closure(r.varint()?),
                    7 => {
                        let mut v = [0f32; 4];
                        for x in v.iter_mut() {
                            *x = f32::from_le_bytes(r.take(4)?.try_into().unwrap());
                        }
                        Constant::Vector(v)
                    }
                    8 => {
                        let n = r.varint()? as usize;
                        let mut keys = Vec::with_capacity(n);
                        for _ in 0..n {
                            let k = r.varint()?;
                            keys.push((k, r.u32()? as i32));
                        }
                        Constant::TableWithConstants(keys)
                    }
                    9 => {
                        let neg = r.u8()? != 0;
                        let m = r.varint64()?;
                        Constant::Integer(if neg {
                            (!m).wrapping_add(1) as i64
                        } else {
                            m as i64
                        })
                    }
                    k => return Err(Error::Constant(k)),
                });
            }
            let n = r.varint()? as usize;
            let mut children = Vec::with_capacity(n);
            for _ in 0..n {
                children.push(r.varint()?);
            }
            let line_defined = r.varint()?;
            let debug_name = r.varint()?;
            let line_info = if r.u8()? != 0 {
                let gap_log2 = r.u8()?;
                let deltas = r.take(code.len())?.to_vec();
                let intervals = if code.is_empty() {
                    0
                } else {
                    ((code.len() - 1) >> gap_log2) + 1
                };
                let mut abs = Vec::with_capacity(intervals);
                let mut last = 0i32;
                for _ in 0..intervals {
                    last = last.wrapping_add(r.u32()? as i32);
                    abs.push(last);
                }
                Some(LineInfo {
                    gap_log2,
                    deltas,
                    abs,
                })
            } else {
                None
            };
            let debug = if r.u8()? != 0 {
                let n = r.varint()? as usize;
                let mut locals = Vec::with_capacity(n);
                for _ in 0..n {
                    locals.push(Local {
                        name: r.varint()?,
                        start_pc: r.varint()?,
                        end_pc: r.varint()?,
                        reg: r.u8()?,
                    });
                }
                let n = r.varint()? as usize;
                let mut upvalues = Vec::with_capacity(n);
                for _ in 0..n {
                    upvalues.push(r.varint()?);
                }
                Some(DebugInfo { locals, upvalues })
            } else {
                None
            };
            protos.push(Proto {
                max_stack,
                num_params,
                num_upvalues,
                is_vararg,
                flags,
                type_info,
                code,
                constants,
                children,
                line_defined,
                debug_name,
                line_info,
                debug,
            });
        }
        let main = r.varint()?;
        Ok(Module {
            version,
            types_version,
            strings,
            userdata_types,
            protos,
            main,
        })
    }

    pub fn string(&self, index: u32) -> Option<&[u8]> {
        if index == 0 {
            None
        } else {
            self.strings.get(index as usize - 1).map(|s| s.as_slice())
        }
    }

    pub fn str(&self, index: u32) -> String {
        self.string(index)
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .unwrap_or_default()
    }

    pub fn write(&self) -> Vec<u8> {
        let mut w = Writer(Vec::new());
        w.u8(self.version);
        if self.version >= 4 {
            w.u8(self.types_version);
        }
        w.varint(self.strings.len() as u64);
        for s in &self.strings {
            w.varint(s.len() as u64);
            w.0.extend_from_slice(s);
        }
        if self.types_version == 3 {
            for (i, name) in &self.userdata_types {
                w.u8(*i);
                w.varint(*name as u64);
            }
            w.u8(0);
        }
        w.varint(self.protos.len() as u64);
        for p in &self.protos {
            w.u8(p.max_stack);
            w.u8(p.num_params);
            w.u8(p.num_upvalues);
            w.u8(p.is_vararg as u8);
            if self.version >= 4 {
                w.u8(p.flags);
                w.varint(p.type_info.len() as u64);
                w.0.extend_from_slice(&p.type_info);
            }
            w.varint(p.code.len() as u64);
            for c in &p.code {
                w.0.extend_from_slice(&c.to_le_bytes());
            }
            w.varint(p.constants.len() as u64);
            for k in &p.constants {
                match k {
                    Constant::Nil => w.u8(0),
                    Constant::Bool(b) => {
                        w.u8(1);
                        w.u8(*b as u8);
                    }
                    Constant::Number(n) => {
                        w.u8(2);
                        w.0.extend_from_slice(&n.to_le_bytes());
                    }
                    Constant::String(s) => {
                        w.u8(3);
                        w.varint(*s as u64);
                    }
                    Constant::Import(i) => {
                        w.u8(4);
                        w.0.extend_from_slice(&i.to_le_bytes());
                    }
                    Constant::Table(keys) => {
                        w.u8(5);
                        w.varint(keys.len() as u64);
                        for k in keys {
                            w.varint(*k as u64);
                        }
                    }
                    Constant::Closure(f) => {
                        w.u8(6);
                        w.varint(*f as u64);
                    }
                    Constant::Vector(v) => {
                        w.u8(7);
                        for x in v {
                            w.0.extend_from_slice(&x.to_le_bytes());
                        }
                    }
                    Constant::TableWithConstants(keys) => {
                        w.u8(8);
                        w.varint(keys.len() as u64);
                        for (k, c) in keys {
                            w.varint(*k as u64);
                            w.0.extend_from_slice(&c.to_le_bytes());
                        }
                    }
                    Constant::Integer(v) => {
                        w.u8(9);
                        w.u8((*v < 0) as u8);
                        w.varint(v.unsigned_abs());
                    }
                }
            }
            w.varint(p.children.len() as u64);
            for c in &p.children {
                w.varint(*c as u64);
            }
            w.varint(p.line_defined as u64);
            w.varint(p.debug_name as u64);
            match &p.line_info {
                Some(li) => {
                    w.u8(1);
                    w.u8(li.gap_log2);
                    w.0.extend_from_slice(&li.deltas);
                    let mut last = 0i32;
                    for a in &li.abs {
                        w.0.extend_from_slice(&a.wrapping_sub(last).to_le_bytes());
                        last = *a;
                    }
                }
                None => w.u8(0),
            }
            match &p.debug {
                Some(d) => {
                    w.u8(1);
                    w.varint(d.locals.len() as u64);
                    for l in &d.locals {
                        w.varint(l.name as u64);
                        w.varint(l.start_pc as u64);
                        w.varint(l.end_pc as u64);
                        w.u8(l.reg);
                    }
                    w.varint(d.upvalues.len() as u64);
                    for u in &d.upvalues {
                        w.varint(*u as u64);
                    }
                }
                None => w.u8(0),
            }
        }
        w.varint(self.main as u64);
        w.0
    }

    pub fn instructions(&self, proto: usize) -> Vec<(usize, u32, Option<u32>)> {
        let code = &self.protos[proto].code;
        let mut out = Vec::new();
        let mut pc = 0;
        while pc < code.len() {
            let o = op::op(code[pc]);
            let aux = (op::length(o) == 2).then(|| code.get(pc + 1).copied().unwrap_or(0));
            out.push((pc, code[pc], aux));
            pc += op::length(o);
        }
        out
    }
}

struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }

    fn varint(&mut self, mut v: u64) {
        loop {
            let b = (v & 127) as u8;
            v >>= 7;
            if v == 0 {
                self.0.push(b);
                return;
            }
            self.0.push(b | 128);
        }
    }
}

pub fn to_vm_bytecode(data: &[u8]) -> Result<Vec<u8>, Error> {
    Ok(Module::parse(data)?.write())
}
