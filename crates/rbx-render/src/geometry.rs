use bytemuck::{Pod, Zeroable};
use rbx_math::{Vec2, Vec3, Vec4};

pub const TESSELATION_PIECE: f32 = 8.0;
pub const NO_OUTLINES: f32 = 10.0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub uv_studs: [f32; 2],
    pub tangent: [f32; 3],
    pub edge: [f32; 4],
}

impl Vertex {
    pub const ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x3,
        2 => Float32x2,
        3 => Float32x2,
        4 => Float32x3,
        5 => Float32x4,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Surface {
    Smooth,
    Glue,
    Weld,
    Studs,
    Inlet,
    Universal,
    Hinge,
    Motor,
    SteppingMotor,
    SmoothNoOutlines,
}

impl Surface {
    pub fn from_enum(v: u32) -> Surface {
        match v {
            1 => Surface::Glue,
            2 => Surface::Weld,
            3 => Surface::Studs,
            4 => Surface::Inlet,
            5 => Surface::Universal,
            6 => Surface::Hinge,
            7 => Surface::Motor,
            8 => Surface::SteppingMotor,
            10 => Surface::SmoothNoOutlines,
            _ => Surface::Smooth,
        }
    }

    fn is_no_surface(self) -> bool {
        matches!(self, Surface::Smooth | Surface::SmoothNoOutlines)
    }

    fn atlas(self) -> Option<(f32, f32)> {
        match self {
            Surface::Studs => Some((0.25, 0.0)),
            Surface::Weld | Surface::Glue => Some((0.25, 0.25)),
            Surface::Inlet => Some((0.25, 0.5)),
            Surface::Universal => Some((0.25, 0.75)),
            _ => None,
        }
    }
}

#[derive(Default)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub min: Vec3,
    pub max: Vec3,
}

pub struct BlockParams {
    pub size: Vec3,
    pub surfaces: [Surface; 6],
    pub ignore_studs: bool,
    pub surface_offset: Vec2,
    pub tiling: f32,
}

fn v3(v: Vec3) -> [f32; 3] {
    [v.x, v.y, v.z]
}

fn v2(v: Vec2) -> [f32; 2] {
    [v.x, v.y]
}

fn v4(v: Vec4) -> [f32; 4] {
    [v.x, v.y, v.z, v.w]
}

impl Mesh {
    fn push(
        &mut self,
        pos: Vec3,
        normal: Vec3,
        uv: Vec2,
        uv_studs: Vec2,
        tangent: Vec3,
        edge: Vec4,
    ) {
        self.vertices.push(Vertex {
            pos: v3(pos),
            normal: v3(normal),
            uv: v2(uv),
            uv_studs: v2(uv_studs),
            tangent: v3(tangent),
            edge: v4(edge),
        });
    }

    fn quad(&mut self, base: u32) {
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
    }

    fn tri(&mut self, a: u32, b: u32, c: u32) {
        self.indices.extend_from_slice(&[a, b, c]);
    }

    fn finish(mut self, size: Vec3) -> Mesh {
        self.min = -size * 0.5;
        self.max = size * 0.5;
        self
    }

    pub fn block(p: &BlockParams) -> Mesh {
        const FACES: [(usize, usize, usize, usize, bool, bool); 6] = [
            (6, 7, 4, 5, false, true),
            (3, 7, 2, 6, true, false),
            (7, 3, 5, 1, false, false),
            (3, 2, 1, 0, false, true),
            (0, 4, 1, 5, true, false),
            (2, 6, 0, 4, false, false),
        ];
        let size = p.size;
        let tess = |v: f32| ((v / TESSELATION_PIECE).ceil().max(1.0)) as i32;
        let tz = tess(size.z);
        let ty = tess(size.y);
        let no_blanks = !p.surfaces.contains(&Surface::SmoothNoOutlines);
        let ext = size * 0.5;
        let o = -ext;
        let (cx, cy, cz) = (Vec3::X * size.x, Vec3::Y * size.y, Vec3::Z * size.z);
        let corners = [
            o,
            o + cz,
            o + cy,
            o + cy + cz,
            o + cx,
            o + cx + cz,
            o + cx + cy,
            o + cx + cy + cz,
        ];
        let normals = [Vec3::X, Vec3::Y, Vec3::Z, -Vec3::X, -Vec3::Y, -Vec3::Z];
        let tangents = [-Vec3::Z, -Vec3::X, Vec3::X, Vec3::Z, -Vec3::X, -Vec3::X];
        let faces_dir = [-Vec3::Y, -Vec3::Z, -Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Y];
        let mut mesh = Mesh::default();
        for face in 0..6 {
            let (s0, s1, e0, e1, dir_z, tangent_z) = FACES[face];
            let surface = p.surfaces[face];
            let ignore_tiling = surface.is_no_surface() || p.ignore_studs;
            let mut quads = if dir_z { tz } else { ty };
            if ignore_tiling {
                quads = 1;
            }
            let quad_length = if dir_z { size.z } else { size.y };
            let u_tiling = if tangent_z { size.z } else { size.x };
            let so = p.surface_offset;
            let mut uvs = [
                so + Vec2::new(u_tiling, 0.0),
                so,
                so + Vec2::new(u_tiling, TESSELATION_PIECE),
                so + Vec2::new(0.0, TESSELATION_PIECE),
            ];
            let atlas = if p.ignore_studs {
                None
            } else {
                surface.atlas()
            };
            let (scale_y, offset_y, tile_x) = match atlas {
                Some((s, o)) => (s, o, true),
                None => (0.0, 0.0, false),
            };
            let u_studs = if tile_x { u_tiling * 0.5 } else { 0.0 };
            let uv_studs = [
                Vec2::new(u_studs, offset_y),
                Vec2::new(0.0, offset_y),
                Vec2::new(u_studs, offset_y + scale_y),
                Vec2::new(0.0, offset_y + scale_y),
            ];
            let start_edge = if no_blanks {
                Vec4::ZERO
            } else {
                outline_offset(face, &p.surfaces)
            };
            let mut edge_offset = start_edge;
            let edge0 = Vec4::new(0.0, u_tiling, 0.0, quad_length);
            let edge1 = Vec4::new(u_tiling, 0.0, 0.0, quad_length);
            let world_dir = faces_dir[face] * TESSELATION_PIECE;
            let start0 = corners[s0];
            let start1 = corners[s1];
            let mut offset = Vec3::ZERO;
            let n = normals[face];
            let t = tangents[face];
            let tiling = p.tiling;
            for _ in 0..quads - 1 {
                let next = offset + world_dir;
                let next_edge =
                    edge_offset + Vec4::new(0.0, 0.0, TESSELATION_PIECE, -TESSELATION_PIECE);
                let base = mesh.vertices.len() as u32;
                mesh.push(
                    start0 + offset,
                    n,
                    uvs[0] * tiling,
                    uv_studs[0],
                    t,
                    edge0 + edge_offset,
                );
                mesh.push(
                    start1 + offset,
                    n,
                    uvs[1] * tiling,
                    uv_studs[1],
                    t,
                    edge1 + edge_offset,
                );
                mesh.push(
                    start0 + next,
                    n,
                    uvs[2] * tiling,
                    uv_studs[2],
                    t,
                    edge0 + next_edge,
                );
                mesh.push(
                    start1 + next,
                    n,
                    uvs[3] * tiling,
                    uv_studs[3],
                    t,
                    edge1 + next_edge,
                );
                mesh.quad(base);
                uvs[0].y = uvs[2].y;
                uvs[1].y = uvs[3].y;
                uvs[2].y += TESSELATION_PIECE;
                uvs[3].y += TESSELATION_PIECE;
                offset = next;
                edge_offset = next_edge;
            }
            let remainder = quad_length - (quads - 1) as f32 * TESSELATION_PIECE;
            let mult = (remainder / TESSELATION_PIECE).clamp(0.0, 1.0);
            let uv2 = Vec2::new(uvs[2].x, uvs[0].y + remainder);
            let uv3 = Vec2::new(uvs[3].x, uvs[1].y + remainder);
            let us2 = Vec2::new(uv_studs[2].x, offset_y + mult * scale_y);
            let us3 = Vec2::new(uv_studs[3].x, offset_y + mult * scale_y);
            let final_edge = start_edge + Vec4::new(0.0, 0.0, quad_length, -quad_length);
            let base = mesh.vertices.len() as u32;
            mesh.push(
                start0 + offset,
                n,
                uvs[0] * tiling,
                uv_studs[0],
                t,
                edge0 + edge_offset,
            );
            mesh.push(
                start1 + offset,
                n,
                uvs[1] * tiling,
                uv_studs[1],
                t,
                edge1 + edge_offset,
            );
            mesh.push(corners[e0], n, uv2 * tiling, us2, t, edge0 + final_edge);
            mesh.push(corners[e1], n, uv3 * tiling, us3, t, edge1 + final_edge);
            mesh.quad(base);
        }
        mesh.finish(size)
    }

    pub fn wedge(size: Vec3, surfaces: &[Surface; 6], tiling: f32) -> Mesh {
        let e = size * 0.5;
        let p = |x: f32, y: f32, z: f32| Vec3::new(x * e.x, y * e.y, z * e.z);
        let mut mesh = Mesh::default();
        let slope_n = Vec3::new(0.0, size.z, -size.y).normalize_or(Vec3::Y);
        let faces: [(Vec3, Vec3, [Vec3; 4], usize); 4] = [
            (
                slope_n,
                Vec3::X,
                [
                    p(1.0, -1.0, -1.0),
                    p(-1.0, -1.0, -1.0),
                    p(1.0, 1.0, 1.0),
                    p(-1.0, 1.0, 1.0),
                ],
                1,
            ),
            (
                -Vec3::Y,
                Vec3::X,
                [
                    p(-1.0, -1.0, -1.0),
                    p(1.0, -1.0, -1.0),
                    p(-1.0, -1.0, 1.0),
                    p(1.0, -1.0, 1.0),
                ],
                4,
            ),
            (
                Vec3::Z,
                Vec3::X,
                [
                    p(-1.0, -1.0, 1.0),
                    p(1.0, -1.0, 1.0),
                    p(-1.0, 1.0, 1.0),
                    p(1.0, 1.0, 1.0),
                ],
                2,
            ),
            (Vec3::ZERO, Vec3::ZERO, [Vec3::ZERO; 4], 0),
        ];
        for (n, t, q, face) in faces.iter().take(3) {
            let base = mesh.vertices.len() as u32;
            let w = (q[1] - q[0]).length();
            let h = (q[2] - q[0]).length();
            let edge = edges_for(*face, surfaces);
            mesh.push(
                q[0],
                *n,
                Vec2::new(0.0, 0.0) * tiling,
                Vec2::ZERO,
                *t,
                edge + Vec4::new(0.0, w, 0.0, h),
            );
            mesh.push(
                q[1],
                *n,
                Vec2::new(w, 0.0) * tiling,
                Vec2::ZERO,
                *t,
                edge + Vec4::new(w, 0.0, 0.0, h),
            );
            mesh.push(
                q[2],
                *n,
                Vec2::new(0.0, h) * tiling,
                Vec2::ZERO,
                *t,
                edge + Vec4::new(0.0, w, h, 0.0),
            );
            mesh.push(
                q[3],
                *n,
                Vec2::new(w, h) * tiling,
                Vec2::ZERO,
                *t,
                edge + Vec4::new(w, 0.0, h, 0.0),
            );
            mesh.quad(base);
        }
        for (sx, face) in [(1.0f32, 0usize), (-1.0, 3)] {
            let n = Vec3::X * sx;
            let a = p(sx, -1.0, -1.0);
            let b = p(sx, -1.0, 1.0);
            let c = p(sx, 1.0, 1.0);
            let base = mesh.vertices.len() as u32;
            let edge = edges_for(face, surfaces);
            mesh.push(
                a,
                n,
                Vec2::new(0.0, 0.0),
                Vec2::ZERO,
                Vec3::Z,
                edge + Vec4::new(0.0, size.z, 0.0, size.y),
            );
            mesh.push(
                b,
                n,
                Vec2::new(size.z, 0.0) * tiling,
                Vec2::ZERO,
                Vec3::Z,
                edge + Vec4::new(size.z, 0.0, 0.0, size.y),
            );
            mesh.push(
                c,
                n,
                Vec2::new(size.z, size.y) * tiling,
                Vec2::ZERO,
                Vec3::Z,
                edge + Vec4::new(size.z, 0.0, size.y, 0.0),
            );
            if sx > 0.0 {
                mesh.tri(base, base + 1, base + 2);
            } else {
                mesh.tri(base, base + 2, base + 1);
            }
        }
        mesh.finish(size)
    }

    pub fn corner_wedge(size: Vec3, surfaces: &[Surface; 6], tiling: f32) -> Mesh {
        let e = size * 0.5;
        let p = |x: f32, y: f32, z: f32| Vec3::new(x * e.x, y * e.y, z * e.z);
        let top = p(1.0, 1.0, -1.0);
        let b = [
            p(-1.0, -1.0, -1.0),
            p(1.0, -1.0, -1.0),
            p(1.0, -1.0, 1.0),
            p(-1.0, -1.0, 1.0),
        ];
        let mut mesh = Mesh::default();
        let tris: [(Vec3, Vec3, Vec3, usize); 4] = [
            (b[1], b[2], top, 0),
            (b[0], b[1], top, 5),
            (b[2], b[3], top, 2),
            (b[3], b[0], top, 3),
        ];
        for (a, bb, c, face) in tris {
            let n = (bb - a).cross(c - a).normalize_or(Vec3::Y);
            let n = if n.dot((a + bb + c) / 3.0) < 0.0 {
                -n
            } else {
                n
            };
            let base = mesh.vertices.len() as u32;
            let edge = edges_for(face, surfaces) + Vec4::splat(NO_OUTLINES * 0.5);
            let t = (bb - a).normalize_or(Vec3::X);
            mesh.push(a, n, Vec2::ZERO, Vec2::ZERO, t, edge);
            mesh.push(
                bb,
                n,
                Vec2::new((bb - a).length(), 0.0) * tiling,
                Vec2::ZERO,
                t,
                edge,
            );
            mesh.push(
                c,
                n,
                Vec2::new(0.0, (c - a).length()) * tiling,
                Vec2::ZERO,
                t,
                edge,
            );
            if n.dot((bb - a).cross(c - a)) > 0.0 {
                mesh.tri(base, base + 1, base + 2);
            } else {
                mesh.tri(base, base + 2, base + 1);
            }
        }
        let base = mesh.vertices.len() as u32;
        let edge = edges_for(4, surfaces);
        for (i, v) in b.iter().enumerate() {
            let uv = Vec2::new((v.x + e.x) * tiling, (v.z + e.z) * tiling);
            let ed = edge
                + match i {
                    0 => Vec4::new(0.0, size.x, 0.0, size.z),
                    1 => Vec4::new(size.x, 0.0, 0.0, size.z),
                    2 => Vec4::new(size.x, 0.0, size.z, 0.0),
                    _ => Vec4::new(0.0, size.x, size.z, 0.0),
                };
            mesh.push(*v, -Vec3::Y, uv, Vec2::ZERO, Vec3::X, ed);
        }
        mesh.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        mesh.finish(size)
    }

    pub fn sphere(size: Vec3, tiling: f32) -> Mesh {
        let r = size.x.min(size.y).min(size.z) * 0.5;
        let rings = 16u32;
        let segments = 24u32;
        let mut mesh = Mesh::default();
        for i in 0..=rings {
            let v = i as f32 / rings as f32;
            let theta = v * std::f32::consts::PI;
            for j in 0..=segments {
                let u = j as f32 / segments as f32;
                let phi = u * std::f32::consts::TAU;
                let n = Vec3::new(
                    theta.sin() * phi.cos(),
                    theta.cos(),
                    theta.sin() * phi.sin(),
                );
                let t = Vec3::new(-phi.sin(), 0.0, phi.cos());
                mesh.push(
                    n * r,
                    n,
                    Vec2::new(u * r * std::f32::consts::TAU, v * r * std::f32::consts::PI) * tiling,
                    Vec2::ZERO,
                    t,
                    Vec4::splat(NO_OUTLINES),
                );
            }
        }
        for i in 0..rings {
            for j in 0..segments {
                let a = i * (segments + 1) + j;
                let b = a + segments + 1;
                mesh.indices
                    .extend_from_slice(&[a, a + 1, b, b, a + 1, b + 1]);
            }
        }
        mesh.finish(Vec3::splat(r * 2.0))
    }

    pub fn cylinder(size: Vec3, tiling: f32) -> Mesh {
        let r = size.y.min(size.z) * 0.5;
        let h = size.x * 0.5;
        let segments = 24u32;
        let mut mesh = Mesh::default();
        for j in 0..=segments {
            let u = j as f32 / segments as f32;
            let a = u * std::f32::consts::TAU;
            let n = Vec3::new(0.0, a.cos(), a.sin());
            for (k, x) in [-h, h].into_iter().enumerate() {
                mesh.push(
                    Vec3::new(x, n.y * r, n.z * r),
                    n,
                    Vec2::new(u * r * std::f32::consts::TAU, k as f32 * size.x) * tiling,
                    Vec2::ZERO,
                    Vec3::X,
                    Vec4::splat(NO_OUTLINES),
                );
            }
        }
        for j in 0..segments {
            let a = j * 2;
            mesh.indices
                .extend_from_slice(&[a, a + 2, a + 1, a + 1, a + 2, a + 3]);
        }
        for sx in [-1.0f32, 1.0] {
            let center = mesh.vertices.len() as u32;
            let n = Vec3::X * sx;
            mesh.push(
                Vec3::new(sx * h, 0.0, 0.0),
                n,
                Vec2::ZERO,
                Vec2::ZERO,
                Vec3::Z,
                Vec4::splat(NO_OUTLINES),
            );
            for j in 0..=segments {
                let a = j as f32 / segments as f32 * std::f32::consts::TAU;
                let p = Vec3::new(sx * h, a.cos() * r, a.sin() * r);
                mesh.push(
                    p,
                    n,
                    Vec2::new(p.z, p.y) * tiling,
                    Vec2::ZERO,
                    Vec3::Z,
                    Vec4::splat(NO_OUTLINES),
                );
            }
            for j in 0..segments {
                let a = center + 1 + j;
                if sx > 0.0 {
                    mesh.tri(center, a, a + 1);
                } else {
                    mesh.tri(center, a + 1, a);
                }
            }
        }
        mesh.finish(Vec3::new(size.x, r * 2.0, r * 2.0))
    }
}

impl Mesh {
    pub fn from_file(file: &crate::filemesh::FileMesh, scale: Vec3) -> Mesh {
        let mut mesh = Mesh::default();
        for i in 0..file.positions.len() {
            let n = (file.normals[i] / scale).normalize_or_zero();
            let t = if n.y.abs() < 0.99 {
                Vec3::Y.cross(n).normalize_or(Vec3::X)
            } else {
                Vec3::X
            };
            let uv = file.uvs[i];
            mesh.push(
                file.positions[i] * scale,
                n,
                Vec2::new(uv[0], uv[1]),
                Vec2::ZERO,
                t,
                Vec4::splat(NO_OUTLINES),
            );
        }
        for tri in file.indices.chunks_exact(3) {
            mesh.indices.extend_from_slice(&[tri[0], tri[1], tri[2]]);
        }
        mesh.min = file.min * scale;
        mesh.max = file.max * scale;
        mesh
    }

    pub fn decal(size: Vec3, face: usize, tile: Option<(f32, f32)>) -> Mesh {
        const FACES: [[usize; 4]; 6] = [
            [6, 7, 4, 5],
            [3, 7, 2, 6],
            [7, 3, 5, 1],
            [3, 2, 1, 0],
            [5, 1, 4, 0],
            [2, 6, 0, 4],
        ];
        let normals = [Vec3::X, Vec3::Y, Vec3::Z, -Vec3::X, -Vec3::Y, -Vec3::Z];
        let e = size * 0.5;
        let o = -e;
        let (cx, cy, cz) = (Vec3::X * size.x, Vec3::Y * size.y, Vec3::Z * size.z);
        let corners = [
            o,
            o + cz,
            o + cy,
            o + cy + cz,
            o + cx,
            o + cx + cz,
            o + cx + cy,
            o + cx + cy + cz,
        ];
        let face = face.min(5);
        let n = normals[face];
        let uv = match tile {
            Some((u, v)) => match face {
                0 => Vec2::new(size.z / u, size.y / v),
                1 => Vec2::new(-size.x / u, size.z / v),
                2 => Vec2::new(size.x / u, size.y / v),
                3 => Vec2::new(-size.z / u, size.y / v),
                4 => Vec2::new(size.x / u, -size.z / v),
                _ => Vec2::new(-size.x / u, size.y / v),
            },
            None => Vec2::ONE,
        };
        let lift = n * 0.002;
        let mut mesh = Mesh::default();
        let f = FACES[face];
        let edge = Vec4::splat(NO_OUTLINES);
        mesh.push(
            corners[f[0]] + lift,
            n,
            Vec2::new(uv.x, 0.0),
            Vec2::ZERO,
            Vec3::X,
            edge,
        );
        mesh.push(
            corners[f[1]] + lift,
            n,
            Vec2::new(0.0, 0.0),
            Vec2::ZERO,
            Vec3::X,
            edge,
        );
        mesh.push(
            corners[f[2]] + lift,
            n,
            Vec2::new(uv.x, uv.y),
            Vec2::ZERO,
            Vec3::X,
            edge,
        );
        mesh.push(
            corners[f[3]] + lift,
            n,
            Vec2::new(0.0, uv.y),
            Vec2::ZERO,
            Vec3::X,
            edge,
        );
        mesh.quad(0);
        mesh.finish(size)
    }
}

fn edges_for(face: usize, surfaces: &[Surface; 6]) -> Vec4 {
    if surfaces[face] == Surface::SmoothNoOutlines {
        Vec4::splat(NO_OUTLINES)
    } else {
        Vec4::ZERO
    }
}

fn outline_offset(face: usize, surfaces: &[Surface; 6]) -> Vec4 {
    if surfaces[face] == Surface::SmoothNoOutlines {
        return Vec4::splat(NO_OUTLINES);
    }
    const ADJ: [[usize; 4]; 6] = [
        [5, 2, 1, 4],
        [3, 0, 2, 5],
        [0, 3, 1, 4],
        [2, 5, 1, 4],
        [3, 0, 5, 2],
        [3, 0, 1, 4],
    ];
    let f = |i: usize| {
        if surfaces[ADJ[face][i]] == Surface::SmoothNoOutlines {
            NO_OUTLINES
        } else {
            0.0
        }
    };
    Vec4::new(f(0), f(1), f(2), f(3))
}
