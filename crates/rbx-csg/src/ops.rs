use crate::mesh::{CsgMesh, CsgVertex};
use boolmesh::prelude::*;
use rbx_math::{CFrame, Vec2, Vec3, Vec4};
use std::f32::consts::PI;
use std::ops::{Add, Mul};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rgb(pub f32, pub f32, pub f32);

impl Add for Rgb {
    type Output = Rgb;
    fn add(self, o: Rgb) -> Rgb {
        Rgb(self.0 + o.0, self.1 + o.1, self.2 + o.2)
    }
}

impl Mul<f32> for Rgb {
    type Output = Rgb;
    fn mul(self, k: f32) -> Rgb {
        Rgb(self.0 * k, self.1 * k, self.2 * k)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Primitive {
    Block,
    Wedge,
    CornerWedge,
    Ball,
    Cylinder,
}

#[derive(Clone, Debug, Default)]
pub struct Solid {
    pub positions: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    pub colors: Vec<[u8; 3]>,
}

impl Solid {
    fn push_quad(&mut self, a: u32, b: u32, c: u32, d: u32, color: [u8; 3]) {
        self.triangles.push([a, b, c]);
        self.triangles.push([a, c, d]);
        self.colors.push(color);
        self.colors.push(color);
    }

    fn push_tri(&mut self, a: u32, b: u32, c: u32, color: [u8; 3]) {
        self.triangles.push([a, b, c]);
        self.colors.push(color);
    }

    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        if self.positions.is_empty() {
            return None;
        }
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for p in &self.positions {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        Some((lo, hi))
    }

    fn transformed(mut self, cframe: CFrame) -> Solid {
        for p in &mut self.positions {
            *p = cframe.point_to_world_space(*p);
        }
        self
    }
}

pub fn primitive(kind: Primitive, size: Vec3, cframe: CFrame, color: [u8; 3]) -> Solid {
    let h = size * 0.5;
    let mut s = Solid::default();
    match kind {
        Primitive::Block => {
            for i in 0..8 {
                s.positions.push(Vec3::new(
                    if i & 1 != 0 { h.x } else { -h.x },
                    if i & 2 != 0 { h.y } else { -h.y },
                    if i & 4 != 0 { h.z } else { -h.z },
                ));
            }
            s.push_quad(1, 3, 7, 5, color);
            s.push_quad(0, 4, 6, 2, color);
            s.push_quad(2, 6, 7, 3, color);
            s.push_quad(0, 1, 5, 4, color);
            s.push_quad(4, 5, 7, 6, color);
            s.push_quad(0, 2, 3, 1, color);
        }
        Primitive::Wedge => {
            s.positions = vec![
                Vec3::new(h.x, h.y, h.z),
                Vec3::new(h.x, -h.y, h.z),
                Vec3::new(h.x, -h.y, -h.z),
                Vec3::new(-h.x, h.y, h.z),
                Vec3::new(-h.x, -h.y, h.z),
                Vec3::new(-h.x, -h.y, -h.z),
            ];
            s.push_quad(0, 3, 4, 1, color);
            s.push_quad(1, 4, 5, 2, color);
            s.push_quad(0, 2, 5, 3, color);
            s.push_tri(0, 1, 2, color);
            s.push_tri(3, 5, 4, color);
        }
        Primitive::CornerWedge => {
            s.positions = vec![
                Vec3::new(h.x, -h.y, h.z),
                Vec3::new(h.x, -h.y, -h.z),
                Vec3::new(-h.x, -h.y, -h.z),
                Vec3::new(-h.x, -h.y, h.z),
                Vec3::new(h.x, h.y, -h.z),
            ];
            s.push_quad(0, 3, 2, 1, color);
            s.push_tri(0, 1, 4, color);
            s.push_tri(1, 2, 4, color);
            s.push_tri(2, 3, 4, color);
            s.push_tri(3, 0, 4, color);
        }
        Primitive::Ball => {
            let r = h.x.min(h.y).min(h.z);
            let rings = 12u32;
            let segments = 24u32;
            s.positions.push(Vec3::new(0.0, r, 0.0));
            for i in 1..rings {
                let phi = PI * i as f32 / rings as f32;
                for j in 0..segments {
                    let theta = 2.0 * PI * j as f32 / segments as f32;
                    s.positions.push(Vec3::new(
                        r * phi.sin() * theta.cos(),
                        r * phi.cos(),
                        r * phi.sin() * theta.sin(),
                    ));
                }
            }
            s.positions.push(Vec3::new(0.0, -r, 0.0));
            let bottom = s.positions.len() as u32 - 1;
            let at = |i: u32, j: u32| 1 + (i - 1) * segments + j % segments;
            for j in 0..segments {
                s.push_tri(0, at(1, j + 1), at(1, j), color);
                s.push_tri(bottom, at(rings - 1, j), at(rings - 1, j + 1), color);
            }
            for i in 1..rings - 1 {
                for j in 0..segments {
                    s.push_quad(
                        at(i, j),
                        at(i, j + 1),
                        at(i + 1, j + 1),
                        at(i + 1, j),
                        color,
                    );
                }
            }
        }
        Primitive::Cylinder => {
            let r = h.y.min(h.z);
            let segments = 24u32;
            for side in [-h.x, h.x] {
                for j in 0..segments {
                    let theta = 2.0 * PI * j as f32 / segments as f32;
                    s.positions
                        .push(Vec3::new(side, r * theta.cos(), r * theta.sin()));
                }
            }
            s.positions.push(Vec3::new(-h.x, 0.0, 0.0));
            s.positions.push(Vec3::new(h.x, 0.0, 0.0));
            let left = 2 * segments;
            let right = left + 1;
            for j in 0..segments {
                let a = j;
                let b = (j + 1) % segments;
                s.push_quad(a, b, segments + b, segments + a, color);
                s.push_tri(left, b, a, color);
                s.push_tri(right, segments + a, segments + b, color);
            }
        }
    }
    s.transformed(cframe)
}

pub fn from_csg_mesh(mesh: &CsgMesh, scale: Vec3, cframe: CFrame, color: Option<[u8; 3]>) -> Solid {
    let mut s = Solid::default();
    let mut weld: std::collections::HashMap<(u32, u32, u32), u32> = Default::default();
    let mut remap = Vec::with_capacity(mesh.vertices.len());
    for v in &mesh.vertices {
        let p = cframe.point_to_world_space(v.pos * scale);
        let key = (p.x.to_bits(), p.y.to_bits(), p.z.to_bits());
        let next = s.positions.len() as u32;
        let i = *weld.entry(key).or_insert_with(|| {
            s.positions.push(p);
            next
        });
        remap.push(i);
    }
    for tri in mesh.indices.as_chunks::<3>().0 {
        if tri.iter().any(|&i| i as usize >= remap.len()) {
            continue;
        }
        let c = color.unwrap_or_else(|| {
            let v = &mesh.vertices[tri[0] as usize];
            [v.color[0], v.color[1], v.color[2]]
        });
        s.push_tri(
            remap[tri[0] as usize],
            remap[tri[1] as usize],
            remap[tri[2] as usize],
            c,
        );
    }
    s
}

fn to_manifold(s: &Solid) -> Result<Manifold<Rgb>, String> {
    let pos: Vec<f64> = s
        .positions
        .iter()
        .flat_map(|p| [p.x as f64, p.y as f64, p.z as f64])
        .collect();
    let idx: Vec<usize> = s
        .triangles
        .iter()
        .flat_map(|t| t.map(|i| i as usize))
        .collect();
    let vals: Vec<Rgb> = s
        .colors
        .iter()
        .flat_map(|c| {
            let v = Rgb(c[0] as f32, c[1] as f32, c[2] as f32);
            [v, v, v]
        })
        .collect();
    Manifold::new_with_values(&pos, &idx, &vals)
}

fn from_manifold(m: &Manifold<Rgb>) -> Solid {
    let mut s = Solid {
        positions: m.ps.iter().map(|p| Vec3::new(p.x, p.y, p.z)).collect(),
        ..Default::default()
    };
    for (f, h) in m.hs.chunks(3).enumerate() {
        let (Some(a), Some(b), Some(c)) = (h[0].tail(), h[1].tail(), h[2].tail()) else {
            continue;
        };
        let rgb = m
            .vals
            .get(3 * f)
            .copied()
            .unwrap_or(Rgb(163.0, 162.0, 165.0));
        let to = |v: f32| v.round().clamp(0.0, 255.0) as u8;
        s.push_tri(
            a as u32,
            b as u32,
            c as u32,
            [to(rgb.0), to(rgb.1), to(rgb.2)],
        );
    }
    s
}

fn boolean(a: &Solid, b: &Solid, op: OpType) -> Result<Solid, String> {
    let ma = to_manifold(a)?;
    let mb = to_manifold(b)?;
    let out = compute_boolean(&ma, &mb, op)?;
    Ok(from_manifold(&out))
}

pub fn union(a: &Solid, b: &Solid) -> Result<Solid, String> {
    boolean(a, b, OpType::Add)
}

pub fn subtract(a: &Solid, b: &Solid) -> Result<Solid, String> {
    boolean(a, b, OpType::Subtract)
}

fn uv_kind(n: Vec3) -> u8 {
    let a = n.abs();
    if a.x >= a.y && a.x >= a.z {
        if n.x >= 0.0 { 1 } else { 4 }
    } else if a.y >= a.z {
        if n.y >= 0.0 { 2 } else { 5 }
    } else if n.z >= 0.0 {
        3
    } else {
        6
    }
}

fn box_uv(kind: u8, p: Vec3) -> Vec2 {
    match kind {
        1 => Vec2::new(-p.z, -p.y),
        4 => Vec2::new(p.z, -p.y),
        2 => Vec2::new(-p.x, -p.z),
        5 => Vec2::new(p.x, -p.z),
        3 => Vec2::new(p.x, -p.y),
        _ => Vec2::new(-p.x, -p.y),
    }
}

pub fn to_csg_mesh(s: &Solid, origin: Vec3) -> CsgMesh {
    let face_normal = |t: &[u32; 3]| {
        let a = s.positions[t[0] as usize];
        let b = s.positions[t[1] as usize];
        let c = s.positions[t[2] as usize];
        (b - a).cross(c - a).normalize_or_zero()
    };
    let normals: Vec<Vec3> = s.triangles.iter().map(face_normal).collect();
    let mut around: Vec<Vec<usize>> = vec![Vec::new(); s.positions.len()];
    for (f, t) in s.triangles.iter().enumerate() {
        for &i in t {
            around[i as usize].push(f);
        }
    }
    let smooth = 35f32.to_radians().cos();
    let mut mesh = CsgMesh::default();
    for (f, t) in s.triangles.iter().enumerate() {
        let n = normals[f];
        if n == Vec3::ZERO {
            continue;
        }
        let kind = uv_kind(n);
        let c = s.colors.get(f).copied().unwrap_or([163, 162, 165]);
        for &i in t {
            let mut sum = Vec3::ZERO;
            for &g in &around[i as usize] {
                if normals[g].dot(n) >= smooth {
                    sum += normals[g];
                }
            }
            let normal = sum.normalize_or(n);
            let pos = s.positions[i as usize] - origin;
            let tangent = if normal.y.abs() < 0.99 {
                Vec3::Y.cross(normal).normalize_or(Vec3::X)
            } else {
                Vec3::X
            };
            mesh.indices.push(mesh.vertices.len() as u32);
            mesh.vertices.push(CsgVertex {
                pos,
                normal,
                color: [c[0], c[1], c[2], 255],
                extra: [kind, 0, 0, 0],
                uv: box_uv(kind, pos),
                uv_studs: Vec2::ZERO,
                uv_decal: Vec2::ZERO,
                tangent,
                edge_distances: Vec4::ZERO,
            });
        }
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(s: &Solid) -> f32 {
        s.triangles
            .iter()
            .map(|t| {
                let a = s.positions[t[0] as usize];
                let b = s.positions[t[1] as usize];
                let c = s.positions[t[2] as usize];
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }

    #[test]
    fn primitives_are_closed_and_outward() {
        for kind in [
            Primitive::Block,
            Primitive::Wedge,
            Primitive::CornerWedge,
            Primitive::Ball,
            Primitive::Cylinder,
        ] {
            let s = primitive(
                kind,
                Vec3::new(4.0, 2.0, 2.0),
                CFrame::IDENTITY,
                [255, 0, 0],
            );
            assert!(to_manifold(&s).is_ok(), "{kind:?} is not manifold");
            assert!(volume(&s) > 0.0, "{kind:?} faces inward");
        }
    }

    #[test]
    fn union_of_overlapping_blocks_keeps_both_volumes() {
        let a = primitive(
            Primitive::Block,
            Vec3::splat(2.0),
            CFrame::IDENTITY,
            [255, 0, 0],
        );
        let b = primitive(
            Primitive::Block,
            Vec3::splat(2.0),
            CFrame::from_translation(Vec3::new(1.0, 0.0, 0.0)),
            [0, 0, 255],
        );
        let u = union(&a, &b).expect("union");
        assert!((volume(&u) - 12.0).abs() < 1e-3, "volume {}", volume(&u));
        assert!(u.colors.contains(&[255, 0, 0]) && u.colors.contains(&[0, 0, 255]));
        let d = subtract(&a, &b).expect("subtract");
        assert!((volume(&d) - 4.0).abs() < 1e-3, "volume {}", volume(&d));
        let mesh = to_csg_mesh(&u, Vec3::new(0.5, 0.0, 0.0));
        let bytes = mesh.to_binary(&[7; 16]);
        let back = CsgMesh::from_binary(&bytes).expect("round trip");
        assert_eq!(back.indices.len(), mesh.indices.len());
    }
}
