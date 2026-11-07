use rbx_math::Vec3;

#[derive(Clone, Debug, Default)]
pub struct FileMesh {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub min: Vec3,
    pub max: Vec3,
}

#[derive(Debug, thiserror::Error)]
pub enum MeshError {
    #[error("Error reading mesh data: {0}")]
    Bad(&'static str),
}

struct Text<'a> {
    s: &'a [u8],
    i: usize,
}

impl Text<'_> {
    fn skip_ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\r' | b'\n') {
            self.i += 1;
        }
    }

    fn token(&mut self, c: u8) -> Result<(), MeshError> {
        self.skip_ws();
        if self.s.get(self.i) != Some(&c) {
            return Err(MeshError::Bad("unexpected character"));
        }
        self.i += 1;
        Ok(())
    }

    fn number(&mut self, terminator: u8) -> Result<f32, MeshError> {
        self.skip_ws();
        let start = self.i;
        while self.i < self.s.len() && !matches!(self.s[self.i], b',' | b']') {
            self.i += 1;
        }
        let text = std::str::from_utf8(&self.s[start..self.i])
            .map_err(|_| MeshError::Bad("bad number"))?;
        let v: f32 = text
            .trim()
            .parse()
            .map_err(|_| MeshError::Bad("bad number"))?;
        if self.s.get(self.i) != Some(&terminator) {
            return Err(MeshError::Bad("unexpected separator"));
        }
        self.i += 1;
        Ok(v)
    }

    fn uint(&mut self) -> Result<u32, MeshError> {
        self.skip_ws();
        let start = self.i;
        while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
            self.i += 1;
        }
        std::str::from_utf8(&self.s[start..self.i])
            .ok()
            .and_then(|t| t.parse().ok())
            .ok_or(MeshError::Bad("bad face count"))
    }

    fn vec(&mut self) -> Result<[f32; 3], MeshError> {
        self.token(b'[')?;
        Ok([self.number(b',')?, self.number(b',')?, self.number(b']')?])
    }
}

fn read_v1(data: &[u8], scale: f32) -> Result<FileMesh, MeshError> {
    let mut t = Text { s: data, i: 0 };
    let faces = t.uint()?;
    let mut mesh = FileMesh::default();
    let mut index = std::collections::HashMap::new();
    for _ in 0..faces * 3 {
        let p = t.vec()?;
        let n = t.vec()?;
        let uv = t.vec()?;
        let normal = Vec3::new(n[0], n[1], n[2]).normalize_or_zero();
        let key = (
            (p[0] * scale).to_bits(),
            (p[1] * scale).to_bits(),
            (p[2] * scale).to_bits(),
            normal.x.to_bits(),
            normal.y.to_bits(),
            normal.z.to_bits(),
            uv[0].to_bits(),
            (1.0 - uv[1]).to_bits(),
        );
        let next = mesh.positions.len() as u32;
        let i = *index.entry(key).or_insert_with(|| {
            mesh.positions.push(Vec3::new(p[0], p[1], p[2]) * scale);
            mesh.normals.push(normal);
            mesh.uvs.push([uv[0], 1.0 - uv[1]]);
            next
        });
        mesh.indices.push(i);
    }
    Ok(mesh)
}

fn read_v2(data: &[u8]) -> Result<FileMesh, MeshError> {
    if data.len() < 12 {
        return Err(MeshError::Bad("offset is out of bounds"));
    }
    let header_size = u16::from_le_bytes([data[0], data[1]]) as usize;
    let vstride = data[2] as usize;
    let fstride = data[3] as usize;
    let nv = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let nf = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;
    if header_size != 12 || vstride != 36 || fstride != 12 {
        return Err(MeshError::Bad("incompatible stride"));
    }
    if nv == 0 || nf == 0 {
        return Err(MeshError::Bad("empty mesh"));
    }
    let need = 12 + nv * 36 + nf * 12;
    if data.len() < need {
        return Err(MeshError::Bad("offset is out of bounds"));
    }
    let f = |o: usize| f32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
    let mut mesh = FileMesh::default();
    for i in 0..nv {
        let o = 12 + i * 36;
        mesh.positions.push(Vec3::new(f(o), f(o + 4), f(o + 8)));
        mesh.normals
            .push(Vec3::new(f(o + 12), f(o + 16), f(o + 20)));
        mesh.uvs.push([f(o + 24), f(o + 28)]);
    }
    let base = 12 + nv * 36;
    for i in 0..nf * 3 {
        let o = base + i * 4;
        let idx = u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
        if idx as usize >= nv {
            return Err(MeshError::Bad("index value out of range"));
        }
        mesh.indices.push(idx);
    }
    Ok(mesh)
}

pub fn read(data: &[u8]) -> Result<FileMesh, MeshError> {
    let end = data
        .iter()
        .position(|&b| b == b'\n')
        .ok_or(MeshError::Bad("unknown version"))?;
    let version = &data[..end.min(12)];
    let body = &data[end + 1..];
    let mut mesh = match version {
        b"version 1.00" => read_v1(body, 0.5)?,
        b"version 1.01" => read_v1(body, 1.0)?,
        b"version 2.00" => read_v2(body)?,
        _ => return Err(MeshError::Bad("unknown version")),
    };
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for p in &mesh.positions {
        min = min.min(*p);
        max = max.max(*p);
    }
    if mesh.positions.is_empty() {
        min = Vec3::ZERO;
        max = Vec3::ZERO;
    }
    mesh.min = min;
    mesh.max = max;
    Ok(mesh)
}
