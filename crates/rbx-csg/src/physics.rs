use rbx_math::{Quat, Vec3};

pub const PHYSICS_SERIAL_VERSION: i32 = 3;
const TAG: &[u8] = b"CSGPHS";
const BLOCK_KEY: &[u8] = b"BLOCK";

#[derive(Clone, Debug, PartialEq)]
pub struct Hull {
    pub translation: Vec3,
    pub rotation: Quat,
    pub vertices: Vec<Vec3>,
    pub indices: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PhysicsData {
    Missing,
    Block,
    OutOfDate(i32),
    Hulls(Vec<Hull>),
}

fn rd_u32(b: &[u8], pos: &mut usize) -> Option<u32> {
    let s = b.get(*pos..*pos + 4)?;
    *pos += 4;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn rd_floats(b: &[u8], pos: &mut usize, stride: usize) -> Option<Vec<f32>> {
    let s = b.get(*pos..*pos + stride)?;
    *pos += stride;
    Some(
        s.as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect(),
    )
}

pub fn parse(data: &[u8]) -> PhysicsData {
    if data.len() < TAG.len() + 4 || &data[..TAG.len()] != TAG {
        return PhysicsData::Missing;
    }
    let mut pos = TAG.len();
    let version = rd_u32(data, &mut pos).map_or(-1, |v| v as i32);
    if version == 0 && data.get(pos..pos + BLOCK_KEY.len()) == Some(BLOCK_KEY) {
        return PhysicsData::Block;
    }
    if version != PHYSICS_SERIAL_VERSION {
        return PhysicsData::OutOfDate(version);
    }
    let mut hulls = Vec::new();
    while pos < data.len() {
        let Some(hull) = read_hull(data, &mut pos) else {
            break;
        };
        hulls.push(hull);
    }
    if hulls.is_empty() {
        PhysicsData::Missing
    } else {
        PhysicsData::Hulls(hulls)
    }
}

fn read_hull(data: &[u8], pos: &mut usize) -> Option<Hull> {
    let ts = rd_u32(data, pos)? as usize;
    let t = rd_floats(data, pos, ts)?;
    let rs = rd_u32(data, pos)? as usize;
    let r = rd_floats(data, pos, rs)?;
    let num_floats = rd_u32(data, pos)? as usize;
    let stride = rd_u32(data, pos)? as usize;
    let fl = rd_floats(data, pos, num_floats.checked_mul(stride)?)?;
    let num_indices = rd_u32(data, pos)? as usize;
    let ib = data.get(*pos..*pos + num_indices.checked_mul(4)?)?;
    *pos += num_indices * 4;
    let indices = ib
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    let vertices = fl
        .as_chunks::<3>()
        .0
        .iter()
        .map(|c| Vec3::from_array(*c))
        .collect();
    let translation = Vec3::new(*t.first()?, *t.get(1)?, *t.get(2)?);
    let rotation = Quat::from_xyzw(*r.first()?, *r.get(1)?, *r.get(2)?, *r.get(3)?).normalize();
    Some(Hull {
        translation,
        rotation,
        vertices,
        indices,
    })
}

pub fn write(hulls: &[Hull]) -> Vec<u8> {
    let mut out = TAG.to_vec();
    out.extend_from_slice(&PHYSICS_SERIAL_VERSION.to_le_bytes());
    for h in hulls {
        out.extend_from_slice(&16u32.to_le_bytes());
        for v in [h.translation.x, h.translation.y, h.translation.z, 0.0] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&16u32.to_le_bytes());
        for v in [h.rotation.x, h.rotation.y, h.rotation.z, h.rotation.w] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&((h.vertices.len() * 3) as u32).to_le_bytes());
        out.extend_from_slice(&4u32.to_le_bytes());
        for v in &h.vertices {
            for c in v.to_array() {
                out.extend_from_slice(&c.to_le_bytes());
            }
        }
        out.extend_from_slice(&(h.indices.len() as u32).to_le_bytes());
        for i in &h.indices {
            out.extend_from_slice(&i.to_le_bytes());
        }
    }
    out
}

pub fn block_data() -> Vec<u8> {
    let mut out = TAG.to_vec();
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(BLOCK_KEY);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hulls_round_trip() {
        let h = Hull {
            translation: Vec3::new(1.0, 0.0, 0.0),
            rotation: Quat::IDENTITY,
            vertices: vec![Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z],
            indices: vec![0, 1, 2, 0, 2, 3],
        };
        assert_eq!(
            parse(&write(std::slice::from_ref(&h))),
            PhysicsData::Hulls(vec![h])
        );
        assert_eq!(parse(&block_data()), PhysicsData::Block);
    }
}
