use crate::{LcmRand, md5_hex};
use rbx_math::{Vec2, Vec3, Vec4};

const HEADER: &[u8] = b"CSGMDL";
const VERSION: i32 = 2;
const SALT_SIZE: usize = 16;
const HASH_SIZE: usize = 16;
pub const VERTEX_STRIDE: usize = 84;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CsgVertex {
    pub pos: Vec3,
    pub normal: Vec3,
    pub color: [u8; 4],
    pub extra: [u8; 4],
    pub uv: Vec2,
    pub uv_studs: Vec2,
    pub uv_decal: Vec2,
    pub tangent: Vec3,
    pub edge_distances: Vec4,
}

impl CsgVertex {
    fn write(&self, out: &mut Vec<u8>) {
        let f = |out: &mut Vec<u8>, v: f32| out.extend_from_slice(&v.to_le_bytes());
        for v in self.pos.to_array() {
            f(out, v);
        }
        for v in self.normal.to_array() {
            f(out, v);
        }
        out.extend_from_slice(&self.color);
        out.extend_from_slice(&self.extra);
        for v in self
            .uv
            .to_array()
            .into_iter()
            .chain(self.uv_studs.to_array())
            .chain(self.uv_decal.to_array())
        {
            f(out, v);
        }
        for v in self.tangent.to_array() {
            f(out, v);
        }
        for v in self.edge_distances.to_array() {
            f(out, v);
        }
    }

    fn read(b: &[u8]) -> CsgVertex {
        let f = |i: usize| f32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        CsgVertex {
            pos: Vec3::new(f(0), f(4), f(8)),
            normal: Vec3::new(f(12), f(16), f(20)),
            color: [b[24], b[25], b[26], b[27]],
            extra: [b[28], b[29], b[30], b[31]],
            uv: Vec2::new(f(32), f(36)),
            uv_studs: Vec2::new(f(40), f(44)),
            uv_decal: Vec2::new(f(48), f(52)),
            tangent: Vec3::new(f(56), f(60), f(64)),
            edge_distances: Vec4::new(f(68), f(72), f(76), f(80)),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct CsgMesh {
    pub vertices: Vec<CsgVertex>,
    pub indices: Vec<u32>,
    pub bad_mesh: bool,
}

fn xor_buffer(buffer: &mut [u8]) {
    let mut rng = LcmRand::default();
    let key: Vec<u8> = (0..31).map(|_| (rng.value() % 127) as u8).collect();
    for (i, b) in buffer.iter_mut().enumerate() {
        *b ^= key[i % key.len()];
    }
}

impl CsgMesh {
    pub fn is_not_empty(&self) -> bool {
        !self.vertices.is_empty() && !self.indices.is_empty()
    }

    fn raw_bytes(&self) -> Vec<u8> {
        let mut out =
            Vec::with_capacity(self.vertices.len() * VERTEX_STRIDE + self.indices.len() * 4);
        for v in &self.vertices {
            v.write(&mut out);
        }
        for i in &self.indices {
            out.extend_from_slice(&i.to_le_bytes());
        }
        out
    }

    pub fn create_hash(&self, salt: &[u8; SALT_SIZE]) -> [u8; HASH_SIZE + SALT_SIZE] {
        let mut buf = self.raw_bytes();
        buf.extend_from_slice(salt);
        let n = buf.len();
        let mut rng = LcmRand::default();
        for i in 0..n {
            let j = rng.value() as usize % n;
            buf.swap(i, j);
        }
        let hex = md5_hex(&buf);
        let mut out = [0u8; HASH_SIZE + SALT_SIZE];
        out[..HASH_SIZE].copy_from_slice(&hex.as_bytes()[..HASH_SIZE]);
        out[HASH_SIZE..].copy_from_slice(salt);
        out
    }

    pub fn to_binary(&self, salt: &[u8; SALT_SIZE]) -> Vec<u8> {
        let mut out = HEADER.to_vec();
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&self.create_hash(salt));
        out.extend_from_slice(&(self.vertices.len() as u32).to_le_bytes());
        out.extend_from_slice(&(VERTEX_STRIDE as u32).to_le_bytes());
        for v in &self.vertices {
            v.write(&mut out);
        }
        out.extend_from_slice(&(self.indices.len() as u32).to_le_bytes());
        for i in &self.indices {
            out.extend_from_slice(&i.to_le_bytes());
        }
        xor_buffer(&mut out);
        out
    }

    pub fn from_binary(data: &[u8]) -> Option<CsgMesh> {
        let mut buf = data.to_vec();
        xor_buffer(&mut buf);
        let mut r = Reader { data: &buf, pos: 0 };
        if r.take(HEADER.len())? != HEADER {
            return None;
        }
        if r.i32()? != VERSION {
            return None;
        }
        let hash = r.take(HASH_SIZE + SALT_SIZE)?.to_vec();
        let num_vertices = r.u32()? as usize;
        let stride = r.u32()? as usize;
        if stride != VERTEX_STRIDE {
            return None;
        }
        let vbytes = r.take_lossy(num_vertices.saturating_mul(stride));
        let vertices: Vec<CsgVertex> = vbytes.chunks_exact(stride).map(CsgVertex::read).collect();
        let num_indices = r.u32().unwrap_or(0) as usize;
        let ibytes = r.take_lossy(num_indices.saturating_mul(4));
        let indices: Vec<u32> = ibytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes(*c))
            .collect();
        let mut mesh = CsgMesh {
            vertices,
            indices,
            bad_mesh: false,
        };
        let mut salt = [0u8; SALT_SIZE];
        salt.copy_from_slice(&hash[HASH_SIZE..]);
        if mesh.create_hash(&salt)[..] != hash[..] {
            mesh.bad_mesh = true;
        }
        Some(mesh)
    }

    pub fn positions(&self) -> Vec<Vec3> {
        self.vertices.iter().map(|v| v.pos).collect()
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }

    fn take_lossy(&mut self, n: usize) -> &'a [u8] {
        let end = self.pos.saturating_add(n).min(self.data.len());
        let s = &self.data[self.pos.min(end)..end];
        self.pos = end;
        s
    }

    fn u32(&mut self) -> Option<u32> {
        let b = self.take(4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn i32(&mut self) -> Option<i32> {
        self.u32().map(|v| v as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_hash_valid() {
        let v = CsgVertex {
            pos: Vec3::new(1.0, 2.0, 3.0),
            extra: [1, 0, 0, 0],
            ..Default::default()
        };
        let mesh = CsgMesh {
            vertices: vec![v, v, v],
            indices: vec![0, 1, 2],
            bad_mesh: false,
        };
        let bin = mesh.to_binary(&[7u8; 16]);
        let back = CsgMesh::from_binary(&bin).expect("decode");
        assert!(!back.bad_mesh);
        assert_eq!(back.vertices, mesh.vertices);
        assert_eq!(back.indices, mesh.indices);
    }
}
