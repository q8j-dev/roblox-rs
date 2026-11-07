use crate::tree::{Tree, TreeInstance};
use crate::{BINARY_MAGIC, FormatError};
use rbx_math::{
    Axes, BrickColor, CFrame, Color3, ColorSequence, ColorSequenceKeypoint, Faces, Mat3,
    NumberRange, NumberSequence, NumberSequenceKeypoint, PhysicalProperties, Quat, Ray, Rect2D,
    UDim, UDim2, Vec2, Vec3, Vector2int16, Vector3int16,
};
use rbx_reflection::{Variant, VariantType};
use std::collections::{BTreeMap, HashMap};

const SIGNATURE: [u8; 6] = [0x89, 0xff, 0x0d, 0x0a, 0x1a, 0x0a];
const CHUNK_INST: [u8; 4] = *b"INST";
const CHUNK_PROP: [u8; 4] = *b"PROP";
const CHUNK_PRNT: [u8; 4] = *b"PRNT";
const CHUNK_END: [u8; 4] = *b"END\0";

const BPF_STRING: u8 = 1;
const BPF_BOOL: u8 = 2;
const BPF_INT: u8 = 3;
const BPF_FLOAT: u8 = 4;
const BPF_DOUBLE: u8 = 5;
const BPF_UDIM: u8 = 6;
const BPF_UDIM2: u8 = 7;
const BPF_RAY: u8 = 8;
const BPF_FACES: u8 = 9;
const BPF_AXES: u8 = 10;
const BPF_BRICKCOLOR: u8 = 11;
const BPF_COLOR3: u8 = 12;
const BPF_VECTOR2: u8 = 13;
const BPF_VECTOR3: u8 = 14;
const BPF_VECTOR2INT16: u8 = 15;
const BPF_CFRAME_MATRIX: u8 = 16;
const BPF_CFRAME_QUAT: u8 = 17;
const BPF_ENUM: u8 = 18;
const BPF_REF: u8 = 19;
const BPF_VECTOR3INT16: u8 = 20;
const BPF_NUMBER_SEQUENCE: u8 = 21;
const BPF_COLOR_SEQUENCE: u8 = 22;
const BPF_NUMBER_RANGE: u8 = 23;
const BPF_RECT2D: u8 = 24;
const BPF_PHYSICAL_PROPERTIES: u8 = 25;

struct Reader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Reader<'a> {
        Reader { data, offset: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], FormatError> {
        if self.offset + n > self.data.len() {
            return Err(FormatError::OutOfBounds(n));
        }
        let s = &self.data[self.offset..self.offset + n];
        self.offset += n;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, FormatError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, FormatError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn i16(&mut self) -> Result<i16, FormatError> {
        Ok(self.u16()? as i16)
    }

    fn u32(&mut self) -> Result<u32, FormatError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn f32(&mut self) -> Result<f32, FormatError> {
        Ok(f32::from_bits(self.u32()?))
    }

    fn f64(&mut self) -> Result<f64, FormatError> {
        let b = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Ok(f64::from_le_bytes(a))
    }

    fn vec3(&mut self) -> Result<Vec3, FormatError> {
        Ok(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }

    fn bytes(&mut self) -> Result<&'a [u8], FormatError> {
        let n = self.u32()? as usize;
        self.take(n)
    }

    fn string(&mut self) -> Result<String, FormatError> {
        Ok(String::from_utf8_lossy(self.bytes()?).into_owned())
    }

    fn interleaved(&mut self, count: usize) -> Result<Vec<u32>, FormatError> {
        let bytes = self.take(count * 4)?;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            out.push(
                ((bytes[i] as u32) << 24)
                    | ((bytes[count + i] as u32) << 16)
                    | ((bytes[count * 2 + i] as u32) << 8)
                    | bytes[count * 3 + i] as u32,
            );
        }
        Ok(out)
    }

    fn ints(&mut self, count: usize) -> Result<Vec<i32>, FormatError> {
        Ok(self
            .interleaved(count)?
            .into_iter()
            .map(decode_int)
            .collect())
    }

    fn floats(&mut self, count: usize) -> Result<Vec<f32>, FormatError> {
        Ok(self
            .interleaved(count)?
            .into_iter()
            .map(decode_float)
            .collect())
    }

    fn ids(&mut self, count: usize) -> Result<Vec<i32>, FormatError> {
        let mut v = self.ints(count)?;
        let mut last = 0i32;
        for x in v.iter_mut() {
            *x = x.wrapping_add(last);
            last = *x;
        }
        Ok(v)
    }
}

fn encode_int(v: i32) -> u32 {
    ((v << 1) ^ (v >> 31)) as u32
}

fn decode_int(v: u32) -> i32 {
    ((v >> 1) as i32) ^ -((v & 1) as i32)
}

fn encode_float(v: f32) -> u32 {
    let i = v.to_bits();
    i.rotate_left(1)
}

fn decode_float(v: u32) -> f32 {
    f32::from_bits(v.rotate_right(1))
}

fn rotation_from_rows(r: [f32; 9]) -> Mat3 {
    CFrame::from_rows(r, Vec3::ZERO).rotation
}

fn read_rotation_exact(r: &mut Reader) -> Result<Mat3, FormatError> {
    let id = r.u8()?;
    if id != 0 {
        return Ok(CFrame::from_orient_id(id - 1).unwrap_or(Mat3::IDENTITY));
    }
    let mut rows = [0f32; 9];
    for v in rows.iter_mut() {
        *v = r.f32()?;
    }
    Ok(rotation_from_rows(rows))
}

fn read_rotation_quat(r: &mut Reader) -> Result<Mat3, FormatError> {
    let id = r.u8()?;
    if id != 0 {
        return Ok(CFrame::from_orient_id(id - 1).unwrap_or(Mat3::IDENTITY));
    }
    let q = Quat::from_xyzw(r.f32()?, r.f32()?, r.f32()?, r.f32()?);
    Ok(Mat3::from_quat(q.normalize()))
}

fn read_values(
    r: &mut Reader,
    name: &str,
    count: usize,
    id_count: usize,
) -> Result<Vec<Variant>, FormatError> {
    let format = r.u8()?;
    let mut out = Vec::with_capacity(count);
    match format {
        BPF_STRING => {
            for _ in 0..count {
                let b = r.bytes()?;
                out.push(match std::str::from_utf8(b) {
                    Ok(s) => Variant::String(s.to_string()),
                    Err(_) => Variant::BinaryString(b.to_vec()),
                });
            }
        }
        BPF_BOOL => {
            for _ in 0..count {
                out.push(Variant::Bool(r.u8()? != 0));
            }
        }
        BPF_INT => out.extend(r.ints(count)?.into_iter().map(Variant::Int)),
        BPF_FLOAT => out.extend(r.floats(count)?.into_iter().map(Variant::Float)),
        BPF_DOUBLE => {
            for _ in 0..count {
                out.push(Variant::Double(r.f64()?));
            }
        }
        BPF_UDIM => {
            let s = r.floats(count)?;
            let o = r.ints(count)?;
            out.extend((0..count).map(|i| Variant::UDim(UDim::new(s[i], o[i]))));
        }
        BPF_UDIM2 => {
            let sx = r.floats(count)?;
            let sy = r.floats(count)?;
            let ox = r.ints(count)?;
            let oy = r.ints(count)?;
            out.extend((0..count).map(|i| Variant::UDim2(UDim2::new(sx[i], ox[i], sy[i], oy[i]))));
        }
        BPF_RAY => {
            for _ in 0..count {
                let o = r.vec3()?;
                let d = r.vec3()?;
                out.push(Variant::Ray(Ray::new(o, d)));
            }
        }
        BPF_FACES => {
            for _ in 0..count {
                out.push(Variant::Faces(Faces(r.u8()?)));
            }
        }
        BPF_AXES => {
            for _ in 0..count {
                out.push(Variant::Axes(Axes(r.u8()?)));
            }
        }
        BPF_BRICKCOLOR => out.extend(
            r.interleaved(count)?
                .into_iter()
                .map(|v| Variant::BrickColor(BrickColor::new(v as i64))),
        ),
        BPF_COLOR3 => {
            let cr = r.floats(count)?;
            let cg = r.floats(count)?;
            let cb = r.floats(count)?;
            out.extend((0..count).map(|i| Variant::Color3(Color3::new(cr[i], cg[i], cb[i]))));
        }
        BPF_VECTOR2 => {
            let x = r.floats(count)?;
            let y = r.floats(count)?;
            out.extend((0..count).map(|i| Variant::Vector2(Vec2::new(x[i], y[i]))));
        }
        BPF_VECTOR3 => {
            let x = r.floats(count)?;
            let y = r.floats(count)?;
            let z = r.floats(count)?;
            out.extend((0..count).map(|i| Variant::Vector3(Vec3::new(x[i], y[i], z[i]))));
        }
        BPF_VECTOR2INT16 => {
            for _ in 0..count {
                out.push(Variant::Vector2int16(Vector2int16 {
                    x: r.i16()?,
                    y: r.i16()?,
                }));
            }
        }
        BPF_VECTOR3INT16 => {
            for _ in 0..count {
                out.push(Variant::Vector3int16(Vector3int16 {
                    x: r.i16()?,
                    y: r.i16()?,
                    z: r.i16()?,
                }));
            }
        }
        BPF_CFRAME_MATRIX | BPF_CFRAME_QUAT => {
            let mut rot = Vec::with_capacity(count);
            for _ in 0..count {
                rot.push(if format == BPF_CFRAME_MATRIX {
                    read_rotation_exact(r)?
                } else {
                    read_rotation_quat(r)?
                });
            }
            let tx = r.floats(count)?;
            let ty = r.floats(count)?;
            let tz = r.floats(count)?;
            out.extend(
                (0..count)
                    .map(|i| Variant::CFrame(CFrame::new(rot[i], Vec3::new(tx[i], ty[i], tz[i])))),
            );
        }
        BPF_ENUM => out.extend(r.interleaved(count)?.into_iter().map(Variant::Enum)),
        BPF_REF => {
            for v in r.ids(count)? {
                if v != -1 && (v < 0 || v as usize >= id_count) {
                    return Err(FormatError::InvalidId(v as i64));
                }
                out.push(Variant::Ref(if v == -1 {
                    None
                } else {
                    Some(Tree::id_of(v as usize))
                }));
            }
        }
        BPF_NUMBER_SEQUENCE => {
            for _ in 0..count {
                let n = r.u32()? as usize;
                let mut keypoints = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    keypoints.push(NumberSequenceKeypoint {
                        time: r.f32()?,
                        value: r.f32()?,
                        envelope: r.f32()?,
                    });
                }
                out.push(Variant::NumberSequence(NumberSequence { keypoints }));
            }
        }
        BPF_COLOR_SEQUENCE => {
            for _ in 0..count {
                let n = r.u32()? as usize;
                let mut keypoints = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    let time = r.f32()?;
                    let value = Color3::new(r.f32()?, r.f32()?, r.f32()?);
                    let envelope = r.f32()?;
                    keypoints.push(ColorSequenceKeypoint {
                        time,
                        value,
                        envelope,
                    });
                }
                out.push(Variant::ColorSequence(ColorSequence { keypoints }));
            }
        }
        BPF_NUMBER_RANGE => {
            for _ in 0..count {
                out.push(Variant::NumberRange(NumberRange::new(r.f32()?, r.f32()?)));
            }
        }
        BPF_RECT2D => {
            let x0 = r.floats(count)?;
            let y0 = r.floats(count)?;
            let x1 = r.floats(count)?;
            let y1 = r.floats(count)?;
            out.extend(
                (0..count).map(|i| Variant::Rect2D(Rect2D::xyxy(x0[i], y0[i], x1[i], y1[i]))),
            );
        }
        BPF_PHYSICAL_PROPERTIES => {
            for _ in 0..count {
                let custom = r.u8()? != 0;
                out.push(Variant::PhysicalProperties(if custom {
                    PhysicalProperties::custom(r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?)
                } else {
                    PhysicalProperties::default()
                }));
            }
        }
        other => return Err(FormatError::UnknownPropertyFormat(other, name.to_string())),
    }
    Ok(out)
}

fn read_chunk(data: &[u8], offset: &mut usize) -> Result<([u8; 4], Vec<u8>), FormatError> {
    let mut r = Reader::new(&data[*offset..]);
    let header = r.take(16).map_err(|_| FormatError::UnexpectedEof)?;
    let mut name = [0u8; 4];
    name.copy_from_slice(&header[..4]);
    let compressed = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
    let size = u32::from_le_bytes([header[8], header[9], header[10], header[11]]) as usize;
    *offset += 16;
    let payload = if size == 0 {
        Vec::new()
    } else if compressed == 0 {
        if *offset + size > data.len() {
            return Err(FormatError::UnexpectedEof);
        }
        let v = data[*offset..*offset + size].to_vec();
        *offset += size;
        v
    } else {
        if *offset + compressed > data.len() {
            return Err(FormatError::UnexpectedEof);
        }
        let src = &data[*offset..*offset + compressed];
        *offset += compressed;
        let mut out = vec![0u8; size];
        let n = lz4_flex::block::decompress_into(src, &mut out)
            .map_err(|_| FormatError::Decompress(0, size))?;
        if n != size {
            return Err(FormatError::Decompress(n, size));
        }
        out
    };
    Ok((name, payload))
}

pub fn read_binary(data: &[u8]) -> Result<Tree, FormatError> {
    if data.len() < 32 {
        return Err(FormatError::UnexpectedEof);
    }
    if &data[..8] != BINARY_MAGIC {
        return Err(FormatError::UnrecognizedFormat);
    }
    if data[8..14] != SIGNATURE {
        return Err(FormatError::CorruptHeader);
    }
    let version = u16::from_le_bytes([data[14], data[15]]);
    if version != 0 {
        return Err(FormatError::UnsupportedVersion(version));
    }
    let type_count = u32::from_le_bytes([data[16], data[17], data[18], data[19]]) as usize;
    let object_count = u32::from_le_bytes([data[20], data[21], data[22], data[23]]) as usize;
    if object_count > data.len() * 8 + 1024 || type_count > data.len() + 1024 {
        return Err(FormatError::CorruptHeader);
    }

    let mut slots: Vec<Option<TreeInstance>> = vec![None; object_count];
    let mut types: Vec<Option<(String, Vec<usize>)>> = vec![None; type_count];
    let mut links: Vec<(usize, Option<usize>)> = Vec::new();
    let mut offset = 32;
    let mut done = false;

    while offset < data.len() {
        let (name, payload) = read_chunk(data, &mut offset)?;
        let mut r = Reader::new(&payload);
        match name {
            CHUNK_INST => {
                let index = r.u32()?;
                if index as usize >= type_count {
                    return Err(FormatError::TypeIndex(index));
                }
                if types[index as usize].is_some() {
                    return Err(FormatError::DuplicateType(index));
                }
                let class = r.string()?;
                let format = r.u8()?;
                if format > 1 {
                    return Err(FormatError::ObjectFormat(format));
                }
                let count = r.u32()? as usize;
                let ids = r.ids(count)?;
                let mut rooted = vec![false; count];
                if format == 1 {
                    for v in rooted.iter_mut() {
                        *v = r.u8()? != 0;
                    }
                }
                let mut members = Vec::with_capacity(count);
                for (i, &id) in ids.iter().enumerate() {
                    if id < 0 || id as usize >= object_count {
                        return Err(FormatError::InvalidId(id as i64));
                    }
                    if slots[id as usize].is_some() {
                        return Err(FormatError::DuplicateId(id as i64));
                    }
                    let mut inst = TreeInstance::new(class.clone());
                    inst.service = format == 1;
                    inst.service_rooted = format == 1 && rooted[i];
                    slots[id as usize] = Some(inst);
                    members.push(id as usize);
                }
                types[index as usize] = Some((class, members));
            }
            CHUNK_PROP => {
                let index = r.u32()?;
                if index as usize >= type_count {
                    return Err(FormatError::TypeIndex(index));
                }
                let prop = r.string()?;
                if let Some((_, members)) = &types[index as usize] {
                    let values = read_values(&mut r, &prop, members.len(), object_count)?;
                    for (id, value) in members.iter().zip(values) {
                        if let Some(inst) = slots[*id].as_mut() {
                            inst.properties.push((prop.clone(), value));
                        }
                    }
                }
            }
            CHUNK_PRNT => {
                let format = r.u8()?;
                if format != 0 {
                    return Err(FormatError::ParentFormat(format));
                }
                let count = r.u32()? as usize;
                let oids = r.ids(count)?;
                let pids = r.ids(count)?;
                for (o, p) in oids.into_iter().zip(pids) {
                    if o < 0 || o as usize >= object_count {
                        return Err(FormatError::InvalidId(o as i64));
                    }
                    let parent = if p == -1 {
                        None
                    } else if p < 0 || p as usize >= object_count {
                        return Err(FormatError::InvalidId(p as i64));
                    } else {
                        Some(p as usize)
                    };
                    links.push((o as usize, parent));
                }
            }
            CHUNK_END => {
                done = true;
                break;
            }
            _ => {}
        }
    }
    if !done {
        return Err(FormatError::UnexpectedEof);
    }

    let mut tree = Tree::new();
    let mut remap: Vec<Option<usize>> = vec![None; object_count];
    let mut order: Vec<usize> = Vec::with_capacity(object_count);
    for (id, slot) in slots.iter().enumerate() {
        if slot.is_some() {
            remap[id] = Some(order.len());
            order.push(id);
        }
    }
    for &id in &order {
        let inst = slots[id].take().unwrap_or_default();
        tree.instances.push(inst);
    }
    for inst in tree.instances.iter_mut() {
        for (_, v) in inst.properties.iter_mut() {
            if let Variant::Ref(Some(r)) = v {
                let old = Tree::index_of(*r);
                *v = Variant::Ref(remap.get(old).copied().flatten().map(Tree::id_of));
            }
        }
    }
    for (o, p) in links {
        let Some(child) = remap[o] else { continue };
        if tree.instances[child].parent.is_some() || tree.roots.contains(&child) {
            continue;
        }
        match p.and_then(|p| remap[p]) {
            Some(parent) if parent != child => {
                tree.instances[child].parent = Some(parent);
                tree.instances[parent].children.push(child);
            }
            _ => tree.roots.push(child),
        }
    }
    Ok(tree)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BinaryWriteOptions {
    pub no_compression: bool,
    pub inexact_cframe: bool,
}

struct Writer {
    data: Vec<u8>,
}

impl Writer {
    fn new() -> Writer {
        Writer { data: Vec::new() }
    }

    fn u8(&mut self, v: u8) {
        self.data.push(v);
    }

    fn u16(&mut self, v: u16) {
        self.data.extend_from_slice(&v.to_le_bytes());
    }

    fn u32(&mut self, v: u32) {
        self.data.extend_from_slice(&v.to_le_bytes());
    }

    fn f32(&mut self, v: f32) {
        self.data.extend_from_slice(&v.to_le_bytes());
    }

    fn f64(&mut self, v: f64) {
        self.data.extend_from_slice(&v.to_le_bytes());
    }

    fn vec3(&mut self, v: Vec3) {
        self.f32(v.x);
        self.f32(v.y);
        self.f32(v.z);
    }

    fn bytes(&mut self, b: &[u8]) {
        self.u32(b.len() as u32);
        self.data.extend_from_slice(b);
    }

    fn interleaved(&mut self, values: &[u32]) {
        for shift in [24, 16, 8, 0] {
            for v in values {
                self.data.push((v >> shift) as u8);
            }
        }
    }

    fn ints(&mut self, values: &[i32]) {
        let e: Vec<u32> = values.iter().map(|&v| encode_int(v)).collect();
        self.interleaved(&e);
    }

    fn floats(&mut self, values: &[f32]) {
        let e: Vec<u32> = values.iter().map(|&v| encode_float(v)).collect();
        self.interleaved(&e);
    }

    fn ids(&mut self, values: &[i32]) {
        let mut last = 0i32;
        let deltas: Vec<i32> = values
            .iter()
            .map(|&v| {
                let d = v.wrapping_sub(last);
                last = v;
                d
            })
            .collect();
        self.ints(&deltas);
    }
}

fn write_chunk(out: &mut Vec<u8>, name: [u8; 4], payload: &[u8], compress: bool) {
    out.extend_from_slice(&name);
    if compress {
        let compressed = lz4_flex::block::compress(payload);
        out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&compressed);
    } else {
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(payload);
    }
}

fn binary_format(ty: VariantType) -> Option<u8> {
    Some(match ty {
        VariantType::String
        | VariantType::ProtectedString
        | VariantType::BinaryString
        | VariantType::Content => BPF_STRING,
        VariantType::Bool => BPF_BOOL,
        VariantType::Int => BPF_INT,
        VariantType::Float => BPF_FLOAT,
        VariantType::Double => BPF_DOUBLE,
        VariantType::UDim => BPF_UDIM,
        VariantType::UDim2 => BPF_UDIM2,
        VariantType::Ray => BPF_RAY,
        VariantType::Faces => BPF_FACES,
        VariantType::Axes => BPF_AXES,
        VariantType::BrickColor => BPF_BRICKCOLOR,
        VariantType::Color3 => BPF_COLOR3,
        VariantType::Vector2 => BPF_VECTOR2,
        VariantType::Vector3 => BPF_VECTOR3,
        VariantType::Vector2int16 => BPF_VECTOR2INT16,
        VariantType::Vector3int16 => BPF_VECTOR3INT16,
        VariantType::CFrame => BPF_CFRAME_MATRIX,
        VariantType::Enum => BPF_ENUM,
        VariantType::Ref => BPF_REF,
        VariantType::NumberSequence => BPF_NUMBER_SEQUENCE,
        VariantType::ColorSequence => BPF_COLOR_SEQUENCE,
        VariantType::NumberRange => BPF_NUMBER_RANGE,
        VariantType::Rect2D => BPF_RECT2D,
        VariantType::PhysicalProperties => BPF_PHYSICAL_PROPERTIES,
        _ => return None,
    })
}

fn write_rotation_exact(w: &mut Writer, m: &Mat3) {
    let cf = CFrame::new(*m, Vec3::ZERO);
    match cf.orient_id() {
        Some(id) => w.u8(id + 1),
        None => {
            w.u8(0);
            for v in cf.rows() {
                w.f32(v);
            }
        }
    }
}

fn write_rotation_quat(w: &mut Writer, m: &Mat3) {
    let cf = CFrame::new(*m, Vec3::ZERO);
    match cf.orient_id() {
        Some(id) => w.u8(id + 1),
        None => {
            w.u8(0);
            let q = cf.to_quaternion().normalize();
            w.f32(q.x);
            w.f32(q.y);
            w.f32(q.z);
            w.f32(q.w);
        }
    }
}

fn write_values(
    w: &mut Writer,
    name: &str,
    ty: VariantType,
    values: &[&Variant],
    id_of: &dyn Fn(usize) -> i32,
    options: BinaryWriteOptions,
) -> Result<(), FormatError> {
    let format =
        binary_format(ty).ok_or_else(|| FormatError::UnsupportedBinaryType(name.to_string()))?;
    let format = if format == BPF_CFRAME_MATRIX && options.inexact_cframe {
        BPF_CFRAME_QUAT
    } else {
        format
    };
    w.u8(format);
    macro_rules! each {
        ($pat:pat => $e:expr, $default:expr) => {
            values
                .iter()
                .map(|v| match v {
                    $pat => $e,
                    _ => $default,
                })
                .collect::<Vec<_>>()
        };
    }
    match ty {
        VariantType::String
        | VariantType::ProtectedString
        | VariantType::Content
        | VariantType::BinaryString => {
            for v in values {
                match v {
                    Variant::BinaryString(b) => w.bytes(b),
                    other => w.bytes(other.as_str().unwrap_or("").as_bytes()),
                }
            }
        }
        VariantType::Bool => {
            for v in values {
                w.u8(v.as_bool().unwrap_or(false) as u8);
            }
        }
        VariantType::Int => w.ints(&each!(Variant::Int(x) => *x, 0)),
        VariantType::Float => w.floats(&each!(Variant::Float(x) => *x, 0.0)),
        VariantType::Double => {
            for v in values {
                w.f64(v.as_f64().unwrap_or(0.0));
            }
        }
        VariantType::UDim => {
            let u = each!(Variant::UDim(x) => *x, UDim::default());
            w.floats(&u.iter().map(|x| x.scale).collect::<Vec<_>>());
            w.ints(&u.iter().map(|x| x.offset).collect::<Vec<_>>());
        }
        VariantType::UDim2 => {
            let u = each!(Variant::UDim2(x) => *x, UDim2::default());
            w.floats(&u.iter().map(|x| x.x.scale).collect::<Vec<_>>());
            w.floats(&u.iter().map(|x| x.y.scale).collect::<Vec<_>>());
            w.ints(&u.iter().map(|x| x.x.offset).collect::<Vec<_>>());
            w.ints(&u.iter().map(|x| x.y.offset).collect::<Vec<_>>());
        }
        VariantType::Ray => {
            for r in each!(Variant::Ray(x) => *x, Ray::default()) {
                w.vec3(r.origin);
                w.vec3(r.direction);
            }
        }
        VariantType::Faces => {
            for f in each!(Variant::Faces(x) => x.0, 0) {
                w.u8(f);
            }
        }
        VariantType::Axes => {
            for f in each!(Variant::Axes(x) => x.0, 0) {
                w.u8(f);
            }
        }
        VariantType::BrickColor => {
            w.interleaved(&each!(Variant::BrickColor(x) => x.number() as u32, BrickColor::DEFAULT.number() as u32))
        }
        VariantType::Color3 => {
            let c = each!(Variant::Color3(x) => *x, Color3::default());
            w.floats(&c.iter().map(|x| x.r).collect::<Vec<_>>());
            w.floats(&c.iter().map(|x| x.g).collect::<Vec<_>>());
            w.floats(&c.iter().map(|x| x.b).collect::<Vec<_>>());
        }
        VariantType::Vector2 => {
            let c = each!(Variant::Vector2(x) => *x, Vec2::ZERO);
            w.floats(&c.iter().map(|x| x.x).collect::<Vec<_>>());
            w.floats(&c.iter().map(|x| x.y).collect::<Vec<_>>());
        }
        VariantType::Vector3 => {
            let c = each!(Variant::Vector3(x) => *x, Vec3::ZERO);
            w.floats(&c.iter().map(|x| x.x).collect::<Vec<_>>());
            w.floats(&c.iter().map(|x| x.y).collect::<Vec<_>>());
            w.floats(&c.iter().map(|x| x.z).collect::<Vec<_>>());
        }
        VariantType::Vector2int16 => {
            for v in each!(Variant::Vector2int16(x) => *x, Vector2int16::default()) {
                w.u16(v.x as u16);
                w.u16(v.y as u16);
            }
        }
        VariantType::Vector3int16 => {
            for v in each!(Variant::Vector3int16(x) => *x, Vector3int16::default()) {
                w.u16(v.x as u16);
                w.u16(v.y as u16);
                w.u16(v.z as u16);
            }
        }
        VariantType::CFrame => {
            let c = each!(Variant::CFrame(x) => *x, CFrame::IDENTITY);
            for cf in &c {
                if options.inexact_cframe {
                    write_rotation_quat(w, &cf.rotation);
                } else {
                    write_rotation_exact(w, &cf.rotation);
                }
            }
            w.floats(&c.iter().map(|x| x.translation.x).collect::<Vec<_>>());
            w.floats(&c.iter().map(|x| x.translation.y).collect::<Vec<_>>());
            w.floats(&c.iter().map(|x| x.translation.z).collect::<Vec<_>>());
        }
        VariantType::Enum => w.interleaved(&each!(Variant::Enum(x) => *x, 0)),
        VariantType::Ref => {
            let ids = each!(Variant::Ref(Some(r)) => id_of(Tree::index_of(*r)), -1);
            w.ids(&ids);
        }
        VariantType::NumberSequence => {
            for v in values {
                let keys = match v {
                    Variant::NumberSequence(s) => s.keypoints.clone(),
                    _ => NumberSequence::default().keypoints,
                };
                w.u32(keys.len() as u32);
                for k in keys {
                    w.f32(k.time);
                    w.f32(k.value);
                    w.f32(k.envelope);
                }
            }
        }
        VariantType::ColorSequence => {
            for v in values {
                let keys = match v {
                    Variant::ColorSequence(s) => s.keypoints.clone(),
                    _ => ColorSequence::default().keypoints,
                };
                w.u32(keys.len() as u32);
                for k in keys {
                    w.f32(k.time);
                    w.f32(k.value.r);
                    w.f32(k.value.g);
                    w.f32(k.value.b);
                    w.f32(k.envelope);
                }
            }
        }
        VariantType::NumberRange => {
            for r in each!(Variant::NumberRange(x) => *x, NumberRange::default()) {
                w.f32(r.min);
                w.f32(r.max);
            }
        }
        VariantType::Rect2D => {
            let c = each!(Variant::Rect2D(x) => *x, Rect2D::default());
            w.floats(&c.iter().map(|x| x.min.x).collect::<Vec<_>>());
            w.floats(&c.iter().map(|x| x.min.y).collect::<Vec<_>>());
            w.floats(&c.iter().map(|x| x.max.x).collect::<Vec<_>>());
            w.floats(&c.iter().map(|x| x.max.y).collect::<Vec<_>>());
        }
        VariantType::PhysicalProperties => {
            for v in values {
                match v {
                    Variant::PhysicalProperties(PhysicalProperties { custom: Some(c) }) => {
                        w.u8(1);
                        w.f32(c.density);
                        w.f32(c.friction);
                        w.f32(c.elasticity);
                        w.f32(c.friction_weight);
                        w.f32(c.elasticity_weight);
                    }
                    _ => w.u8(0),
                }
            }
        }
        _ => return Err(FormatError::UnsupportedBinaryType(name.to_string())),
    }
    Ok(())
}

pub fn write_binary(tree: &Tree, options: BinaryWriteOptions) -> Result<Vec<u8>, FormatError> {
    let order = tree.preorder();
    let mut ids: HashMap<usize, i32> = HashMap::with_capacity(order.len());
    for (i, &index) in order.iter().enumerate() {
        ids.insert(index, i as i32);
    }
    let mut types: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for &index in &order {
        types
            .entry(tree.instances[index].class.as_str())
            .or_default()
            .push(index);
    }

    let mut out = Vec::new();
    out.extend_from_slice(BINARY_MAGIC);
    out.extend_from_slice(&SIGNATURE);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(types.len() as u32).to_le_bytes());
    out.extend_from_slice(&(order.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 8]);
    let compress = !options.no_compression;

    for (type_index, (class, members)) in types.iter().enumerate() {
        let mut w = Writer::new();
        w.u32(type_index as u32);
        w.bytes(class.as_bytes());
        let service = members.iter().any(|&m| tree.instances[m].service);
        w.u8(service as u8);
        w.u32(members.len() as u32);
        let member_ids: Vec<i32> = members.iter().map(|m| ids[m]).collect();
        w.ids(&member_ids);
        if service {
            for &m in members {
                w.u8(tree.instances[m].parent.is_none() as u8);
            }
        }
        write_chunk(&mut out, CHUNK_INST, &w.data, compress);
    }

    let id_of = |index: usize| ids.get(&index).copied().unwrap_or(-1);
    for (type_index, (_, members)) in types.iter().enumerate() {
        let mut names: Vec<&str> = Vec::new();
        let mut kinds: HashMap<&str, VariantType> = HashMap::new();
        for &m in members {
            for (n, v) in &tree.instances[m].properties {
                if !kinds.contains_key(n.as_str()) {
                    names.push(n.as_str());
                    kinds.insert(n.as_str(), v.ty());
                }
            }
        }
        names.sort_unstable();
        for name in names {
            let ty = kinds[name];
            if binary_format(ty).is_none() {
                continue;
            }
            let defaults = Variant::default_for(ty);
            let format = binary_format(ty);
            let values: Vec<&Variant> = members
                .iter()
                .map(|&m| {
                    tree.instances[m]
                        .get(name)
                        .filter(|v| binary_format(v.ty()) == format)
                        .unwrap_or(&defaults)
                })
                .collect();
            let mut w = Writer::new();
            w.u32(type_index as u32);
            w.bytes(name.as_bytes());
            write_values(&mut w, name, ty, &values, &id_of, options)?;
            write_chunk(&mut out, CHUNK_PROP, &w.data, compress);
        }
    }

    {
        let post = tree.postorder();
        let mut w = Writer::new();
        w.u8(0);
        w.u32(post.len() as u32);
        let oids: Vec<i32> = post.iter().map(|&i| id_of(i)).collect();
        let pids: Vec<i32> = post
            .iter()
            .map(|&i| tree.instances[i].parent.map(id_of).unwrap_or(-1))
            .collect();
        w.ids(&oids);
        w.ids(&pids);
        write_chunk(&mut out, CHUNK_PRNT, &w.data, compress);
    }
    write_chunk(&mut out, CHUNK_END, b"</roblox>", false);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_and_float_encoding() {
        for v in [0, 1, -1, 12345, i32::MIN, i32::MAX] {
            assert_eq!(decode_int(encode_int(v)), v);
        }
        for v in [0.0f32, -1.5, 3.25e10, f32::MIN_POSITIVE] {
            assert_eq!(decode_float(encode_float(v)), v);
        }
    }

    #[test]
    fn round_trip_small_tree() {
        let mut tree = Tree::new();
        let mut model = TreeInstance::new("Model");
        model.set("Name", Variant::String("M".into()));
        let m = tree.push(model, None);
        let mut part = TreeInstance::new("Part");
        part.set("Name", Variant::String("P".into()));
        part.set("size", Variant::Vector3(Vec3::new(4.0, 1.2, 2.0)));
        part.set(
            "CFrame",
            Variant::CFrame(CFrame::from_axis_angle(Vec3::Y, 0.3)),
        );
        part.set("BrickColor", Variant::BrickColor(BrickColor::new(21)));
        part.set("Material", Variant::Enum(256));
        let p = tree.push(part, Some(m));
        tree.instances[m].set("PrimaryPart", Variant::Ref(Some(Tree::id_of(p))));
        let bytes = write_binary(&tree, BinaryWriteOptions::default()).expect("write");
        let back = read_binary(&bytes).expect("read");
        assert_eq!(back.instances.len(), 2);
        assert_eq!(back.roots.len(), 1);
        let root = &back.instances[back.roots[0]];
        assert_eq!(root.class, "Model");
        let child = &back.instances[root.children[0]];
        assert_eq!(
            child.get("size"),
            Some(&Variant::Vector3(Vec3::new(4.0, 1.2, 2.0)))
        );
        assert_eq!(child.get("Material"), Some(&Variant::Enum(256)));
        let pp = root.get("PrimaryPart").and_then(|v| v.as_ref()).flatten();
        assert_eq!(pp.map(Tree::index_of), Some(root.children[0]));
    }
}
