use crate::materials::{Deformation, MaterialTable};
use crate::smooth::{Cell, CellBox, MATERIAL_AIR, MATERIAL_WATER};
use rbx_math::{IVec3, Vec3};
use std::sync::LazyLock;

const VERTEX_INDEX: [[i32; 3]; 8] = [
    [0, 0, 0],
    [1, 0, 0],
    [1, 1, 0],
    [0, 1, 0],
    [0, 0, 1],
    [1, 0, 1],
    [1, 1, 1],
    [0, 1, 1],
];

const EDGE_VERTEX: [[[i32; 3]; 2]; 12] = [
    [[0, 0, 0], [1, 0, 0]],
    [[1, 0, 0], [1, 1, 0]],
    [[1, 1, 0], [0, 1, 0]],
    [[0, 1, 0], [0, 0, 0]],
    [[0, 0, 1], [1, 0, 1]],
    [[1, 0, 1], [1, 1, 1]],
    [[1, 1, 1], [0, 1, 1]],
    [[0, 1, 1], [0, 0, 1]],
    [[0, 0, 0], [0, 0, 1]],
    [[1, 0, 0], [1, 0, 1]],
    [[1, 1, 0], [1, 1, 1]],
    [[0, 1, 0], [0, 1, 1]],
];

static EDGE_TABLE: LazyLock<Vec<u16>> = LazyLock::new(|| {
    let mut table = vec![0u16; 81 * 81];
    for i0 in 0..81 {
        for i1 in 0..81 {
            let t = [
                [[i0 % 3, (i0 / 3) % 3], [(i0 / 9) % 3, (i0 / 27) % 3]],
                [[i1 % 3, (i1 / 3) % 3], [(i1 / 9) % 3, (i1 / 27) % 3]],
            ];
            let mut mask = 0u16;
            for (i, e) in EDGE_VERTEX.iter().enumerate() {
                let a = t[e[0][0] as usize][e[0][1] as usize][e[0][2] as usize];
                let b = t[e[1][0] as usize][e[1][1] as usize][e[1][2] as usize];
                if a != b {
                    mask |= 1 << i;
                }
            }
            table[i0 + 81 * i1] = mask;
        }
    }
    table
});

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub position: Vec3,
    pub border: bool,
    pub material: u8,
    pub seed: u16,
}

#[derive(Clone, Debug, Default)]
pub struct BasicMesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl BasicMesh {
    pub fn is_water(v0: &Vertex, v1: &Vertex, v2: &Vertex) -> bool {
        v0.material == MATERIAL_WATER
            || v1.material == MATERIAL_WATER
            || v2.material == MATERIAL_WATER
    }
}

#[derive(Clone, Copy)]
pub struct Options<'a> {
    pub materials: &'a MaterialTable,
    pub generate_water: bool,
}

#[derive(Clone, Copy, Default)]
struct GridVertex {
    tag: u8,
    occupancy: u8,
}

fn push_quad(ib: &mut Vec<u32>, vb: &[Vertex], v: [u32; 4], flip: bool) {
    let m = [
        vb[v[0] as usize].material,
        vb[v[1] as usize].material,
        vb[v[2] as usize].material,
        vb[v[3] as usize].material,
    ];
    let flipdiag = m[1] == m[3] && (m[1] == m[2] || m[1] == m[0]);
    const OFFSETS: [[[usize; 6]; 2]; 2] = [
        [[1, 0, 2, 2, 0, 3], [1, 2, 0, 0, 2, 3]],
        [[0, 3, 1, 1, 3, 2], [0, 1, 3, 3, 1, 2]],
    ];
    for o in OFFSETS[flipdiag as usize][flip as usize] {
        ib.push(v[o]);
    }
}

fn round(v: Vec3) -> Vec3 {
    let r = |f: f32| if f < 0.0 { (f - 0.5) as i32 } else { (f + 0.5) as i32 } as f32;
    Vec3::new(r(v.x), r(v.y), r(v.z))
}

fn avalanche(mut v: u32) -> u32 {
    v = v.wrapping_add(!(v << 15));
    v ^= v >> 10;
    v = v.wrapping_add(v << 3);
    v ^= v >> 6;
    v = v.wrapping_add(!(v << 11));
    v ^= v >> 16;
    v
}

fn hash_combine(seed: &mut u32, v: i32) {
    *seed ^= (v as u32)
        .wrapping_add(0x9e37_79b9)
        .wrapping_add(*seed << 6)
        .wrapping_add(*seed >> 2);
}

fn compute_seed(p: IVec3) -> u32 {
    let mut r = 0u32;
    hash_combine(&mut r, p.x);
    hash_combine(&mut r, p.y);
    hash_combine(&mut r, p.z);
    avalanche(r)
}

fn compute_point(
    smooth: Vec3,
    cell_size: f32,
    materials: &MaterialTable,
    material: u8,
    seed: u32,
) -> Vec3 {
    let m = materials.material(material);
    let center = Vec3::splat(cell_size * 0.5);
    match m.deformation {
        Deformation::Shift => {
            let r = Vec3::new(
                (seed & 255) as f32 / 255.0,
                ((seed >> 8) & 255) as f32 / 255.0,
                ((seed >> 16) & 255) as f32 / 255.0,
            );
            smooth + (r * 2.0 - Vec3::ONE) * (m.parameter * cell_size)
        }
        Deformation::Cubify => smooth + (center - smooth) * m.parameter,
        Deformation::Quantize => round((smooth - center) / m.parameter) * m.parameter + center,
        Deformation::Barrel => Vec3::new(
            smooth.x,
            smooth.y + (center.y - smooth.y) * m.parameter,
            smooth.z,
        ),
        Deformation::Water => Vec3::new(smooth.x, smooth.y - m.parameter, smooth.z),
        Deformation::None => smooth,
    }
}

fn reduce(a: (u32, u8), b: (u32, u8)) -> (u32, u8) {
    if a.1 == b.1 {
        (a.0 + b.0, a.1)
    } else if a.0 != b.0 {
        if a.0 > b.0 { a } else { b }
    } else if a.1 < b.1 {
        a
    } else {
        b
    }
}

struct Dims {
    x: i32,
    y: i32,
    z: i32,
    xz: i32,
}

impl Dims {
    fn at(&self, x: i32, y: i32, z: i32) -> usize {
        (x + self.xz * y + self.x * z) as usize
    }
}

#[allow(clippy::too_many_arguments)]
fn generate_vertex(
    gv: &[GridVertex],
    b: &CellBox,
    d: &Dims,
    offset: IVec3,
    lod: u32,
    options: &Options,
    x: i32,
    y: i32,
    z: i32,
    edgemask: u16,
) -> Vertex {
    let cell_size = (1u32 << lod) as f32;
    let corner = offset.as_vec3() + Vec3::new(x as f32, y as f32, z as f32) * cell_size;
    let mut count = 0usize;
    let mut avg = Vec3::ZERO;
    for (i, e) in EDGE_VERTEX.iter().enumerate() {
        if edgemask & (1 << i) == 0 {
            continue;
        }
        let g0 = gv[d.at(x + e[0][0], y + e[0][1], z + e[0][2])];
        let g1 = gv[d.at(x + e[1][0], y + e[1][1], z + e[1][2])];
        let scale = 1.0 / 256.0;
        let t = if g0.tag > g1.tag {
            (g0.occupancy as f32 + 1.0) * scale
        } else {
            1.0 - (g1.occupancy as f32 + 1.0) * scale
        };
        let p0 = Vec3::new(e[0][0] as f32, e[0][1] as f32, e[0][2] as f32) * cell_size;
        let p1 = Vec3::new(e[1][0] as f32, e[1][1] as f32, e[1][2] as f32) * cell_size;
        avg += p0 + (p1 - p0) * t;
        count += 1;
    }
    let mut vmat = [(0u32, 0u8); 8];
    let mut vcount = 0;
    for vi in VERTEX_INDEX {
        let c: Cell = b.get(x + vi[0], y + vi[1], z + vi[2]);
        if c.material() > MATERIAL_WATER {
            vmat[vcount] = (c.occupancy() as u32 + 1, c.material());
            vcount += 1;
        }
    }
    let material = if vcount > 0 {
        let mut r = reduce(reduce(vmat[0], vmat[1]), reduce(vmat[2], vmat[3]));
        if vcount > 4 {
            r = reduce(
                r,
                reduce(reduce(vmat[4], vmat[5]), reduce(vmat[6], vmat[7])),
            );
        }
        r.1
    } else {
        MATERIAL_WATER
    };
    let border = x == 0 || x == d.x - 2 || y == 0 || y == d.y - 2 || z == 0 || z == d.z - 2;
    let seed = compute_seed(IVec3::new(x, y, z) + (offset >> lod as i32));
    let point = compute_point(
        avg / count as f32,
        cell_size,
        options.materials,
        material,
        seed,
    );
    let position = corner + point.clamp(Vec3::ZERO, Vec3::splat(cell_size)) + Vec3::splat(0.5);
    Vertex {
        position,
        border,
        material,
        seed: seed as u16,
    }
}

fn generate_indices(ib: &mut Vec<u32>, vb: &[Vertex], gv: &[GridVertex], gp: &[u32], d: &Dims) {
    for y in 1..d.y - 1 {
        for z in 1..d.z - 1 {
            for x in 1..d.x - 1 {
                let v000 = gv[d.at(x, y, z)];
                let v100 = gv[d.at(x + 1, y, z)];
                let v010 = gv[d.at(x, y + 1, z)];
                let v001 = gv[d.at(x, y, z + 1)];
                if v000.tag != v100.tag {
                    push_quad(
                        ib,
                        vb,
                        [
                            gp[d.at(x, y, z)],
                            gp[d.at(x, y - 1, z)],
                            gp[d.at(x, y - 1, z - 1)],
                            gp[d.at(x, y, z - 1)],
                        ],
                        v000.tag > v100.tag,
                    );
                }
                if v000.tag != v010.tag {
                    push_quad(
                        ib,
                        vb,
                        [
                            gp[d.at(x, y, z)],
                            gp[d.at(x - 1, y, z)],
                            gp[d.at(x - 1, y, z - 1)],
                            gp[d.at(x, y, z - 1)],
                        ],
                        v000.tag < v010.tag,
                    );
                }
                if v000.tag != v001.tag {
                    push_quad(
                        ib,
                        vb,
                        [
                            gp[d.at(x, y, z)],
                            gp[d.at(x - 1, y, z)],
                            gp[d.at(x - 1, y - 1, z)],
                            gp[d.at(x, y - 1, z)],
                        ],
                        v000.tag > v001.tag,
                    );
                }
            }
        }
    }
}

pub fn generate_geometry(b: &CellBox, offset: IVec3, lod: u32, options: &Options) -> BasicMesh {
    let mut result = BasicMesh::default();
    if b.is_empty() {
        return result;
    }
    let s = b.size();
    let d = Dims {
        x: s.x,
        y: s.y,
        z: s.z,
        xz: s.x * s.z,
    };
    let total = (s.x * s.y * s.z) as usize;
    let cutoff = if options.generate_water {
        MATERIAL_AIR
    } else {
        MATERIAL_WATER
    };
    let mut gv = vec![GridVertex::default(); total];
    for y in 0..s.y {
        for z in 0..s.z {
            for x in 0..s.x {
                let c = b.get(x, y, z);
                let tag = if c.material() <= cutoff {
                    0
                } else if c.material() == MATERIAL_WATER {
                    1
                } else {
                    2
                };
                gv[d.at(x, y, z)] = GridVertex {
                    tag,
                    occupancy: c.occupancy(),
                };
            }
        }
    }
    let table = &*EDGE_TABLE;
    let mut gp = vec![0u32; total];
    for y in 0..s.y - 1 {
        for z in 0..s.z - 1 {
            let oyz = (d.xz * y + d.x * z) as usize;
            let sx = d.x as usize;
            let sxz = d.xz as usize;
            let mut tagi0 = gv[oyz].tag as usize
                + 3 * (gv[oyz + sx].tag as usize
                    + 3 * (gv[oyz + sxz].tag as usize + 3 * gv[oyz + sxz + sx].tag as usize));
            for x in 0..s.x - 1 {
                let n = oyz + x as usize + 1;
                let tagi1 = gv[n].tag as usize
                    + 3 * gv[n + sx].tag as usize
                    + 9 * gv[n + sxz].tag as usize
                    + 27 * gv[n + sxz + sx].tag as usize;
                let edgemask = table[tagi0 + 81 * tagi1];
                tagi0 = tagi1;
                if edgemask != 0 {
                    let v = generate_vertex(&gv, b, &d, offset, lod, options, x, y, z, edgemask);
                    gp[d.at(x, y, z)] = result.vertices.len() as u32;
                    result.vertices.push(v);
                }
            }
        }
    }
    let vertices = std::mem::take(&mut result.vertices);
    generate_indices(&mut result.indices, &vertices, &gv, &gp, &d);
    result.vertices = vertices;
    result
}
