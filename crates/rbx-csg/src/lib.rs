pub mod mesh;
pub mod ops;
pub mod physics;

pub use mesh::{CsgMesh, CsgVertex};
pub use physics::{Hull, PhysicsData};

pub const LOCAL_KEY_TAG: &[u8] = b"CSGK";

pub struct LcmRand {
    seed: u32,
}

impl Default for LcmRand {
    fn default() -> Self {
        LcmRand { seed: 1337 }
    }
}

impl LcmRand {
    pub fn value(&mut self) -> u32 {
        self.seed = self.seed.wrapping_mul(214_013).wrapping_add(2_531_011);
        (self.seed >> 16) & 0x7fff
    }
}

pub fn md5_hex(data: &[u8]) -> String {
    use md5::{Digest, Md5};
    let digest = Md5::digest(data);
    let mut out = String::with_capacity(32);
    for b in digest.iter() {
        out.push(char::from(b"0123456789abcdef"[(b >> 4) as usize]));
        out.push(char::from(b"0123456789abcdef"[(b & 15) as usize]));
    }
    out
}

pub fn is_hash_key(data: &[u8]) -> bool {
    data.len() > LOCAL_KEY_TAG.len() && data.starts_with(LOCAL_KEY_TAG)
}

pub fn hash_key_of(data: &[u8]) -> Option<&[u8]> {
    is_hash_key(data).then(|| &data[LOCAL_KEY_TAG.len()..])
}

pub fn make_hash_key(data: &[u8]) -> Vec<u8> {
    let mut out = LOCAL_KEY_TAG.to_vec();
    out.extend_from_slice(md5_hex(data).as_bytes());
    out
}
