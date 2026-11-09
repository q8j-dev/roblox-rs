use crate::shape::ShapeKind;
use crate::terrain::TerrainCollider;
use crate::world::{GRAVITY, PhysicalMode, PhysicsWorld};
use rbx_dom::rbx_terrain::{CELL_SIZE, legacy};
use rbx_math::{CFrame, IVec3, Vec3};

pub const WATER_DENSITY: f32 = 1.0;
pub const KERNEL_DT: f32 = 1.0 / (240.0 * 19.0);

pub use rbx_dom::specific_gravity;

#[derive(Clone, Copy)]
struct Extents {
    low: Vec3,
    high: Vec3,
}

impl Extents {
    fn vv(a: Vec3, b: Vec3) -> Extents {
        Extents {
            low: a.min(b),
            high: a.max(b),
        }
    }

    fn corner_index(i: usize) -> (usize, usize, usize) {
        (i / 4, (i / 2) % 2, i % 2)
    }

    fn corner(&self, i: usize) -> Vec3 {
        let (x, y, z) = Self::corner_index(i);
        Vec3::new(
            if x == 0 { self.low.x } else { self.high.x },
            if y == 0 { self.low.y } else { self.high.y },
            if z == 0 { self.low.z } else { self.high.z },
        )
    }

    fn center(&self) -> Vec3 {
        (self.low + self.high) * 0.5
    }

    fn bottom_center(&self) -> Vec3 {
        let c = self.center();
        Vec3::new(c.x, self.low.y, c.z)
    }

    fn face_center(&self, face: usize) -> Vec3 {
        let mut c = self.center();
        let axis = face % 3;
        c[axis] = if face < 3 {
            self.high[axis]
        } else {
            self.low[axis]
        };
        c
    }

    fn transformed(self, cf: &CFrame) -> Extents {
        let center = self.center();
        let half = (self.high - self.low) * 0.5;
        let nc = *cf * center;
        let r = cf.rotation;
        let row = |i: usize| Vec3::new(r.x_axis[i].abs(), r.y_axis[i].abs(), r.z_axis[i].abs());
        let nh = Vec3::new(row(0).dot(half), row(1).dot(half), row(2).dot(half));
        Extents {
            low: nc - nh,
            high: nc + nh,
        }
    }
}

struct Water<'a> {
    terrain: &'a TerrainCollider,
}

impl Water<'_> {
    fn cell_has_water(&self, pos: IVec3) -> bool {
        self.terrain.water_at_cell(pos)
    }

    fn under(&self, p: Vec3) -> bool {
        self.cell_has_water(legacy::world_to_cell_floor(p))
    }

    fn submerged_level(&self, p: Vec3, max_search: Vec3) -> Option<f32> {
        let mut pos = legacy::world_to_cell_floor(p);
        if !self.cell_has_water(pos) {
            return None;
        }
        let end = legacy::world_to_cell_floor(max_search);
        pos.y += 1;
        while pos.y <= end.y && self.cell_has_water(pos) {
            pos.y += 1;
        }
        Some(pos.y as f32 * CELL_SIZE)
    }

    fn above_water(&self, p: Vec3, min_y: f32) -> Option<f32> {
        let mut pos = legacy::world_to_cell_floor(p);
        let min_y = min_y.trunc();
        let mut level;
        loop {
            level = pos.y as f32 * CELL_SIZE;
            pos.y -= 1;
            if self.cell_has_water(pos) || level < min_y {
                break;
            }
        }
        if level >= min_y { Some(level) } else { None }
    }

    fn velocity(&self, p: Vec3) -> Vec3 {
        let Some(g) = &self.terrain.legacy else {
            return Vec3::ZERO;
        };
        let cell = g.get_water_cell(legacy::world_to_cell_floor(p));
        if legacy::is_empty(cell) {
            return Vec3::ZERO;
        }
        let magnitude = 16.0 * legacy::water_force(cell) as f32;
        let dir = legacy::water_direction(cell);
        let mut v = Vec3::ZERO;
        v[(dir >> 1) as usize] = if dir & 1 != 0 { magnitude } else { -magnitude };
        v
    }

    fn extents_band(&self, e: &Extents) -> (f32, f32) {
        let mut float_d;
        let mut sink_d = 0.0;
        if let Some(level) = self.submerged_level(e.low, e.high) {
            float_d = level - e.low.y;
            if self.under(e.high) {
                sink_d = -1.0;
            } else if let Some(l2) = self.above_water(e.high, e.low.y) {
                sink_d = e.high.y - l2;
            } else if self.under(e.bottom_center()) {
                sink_d = float_d;
            } else {
                float_d = -1.0;
            }
        } else if let Some(level) = self.submerged_level(e.high, e.high) {
            float_d = level - e.low.y;
            if let Some(l2) = self.above_water(e.low, e.low.y - float_d) {
                sink_d = e.high.y - l2;
            } else if self.under(e.bottom_center()) {
                sink_d = float_d;
            } else {
                float_d = -1.0;
            }
        } else {
            float_d = 0.0;
            for i in 0..7 {
                let c = e.corner(i);
                if c != e.low && c != e.high && self.under(c) {
                    float_d += 1.0;
                }
            }
            if float_d > 0.0 {
                if self.under(e.center()) {
                    float_d += 1.0;
                }
                for i in 0..7 {
                    if self.under((e.corner(i) + e.center()) / 2.0) {
                        float_d += 1.0;
                    }
                }
                sink_d = 17.0 - float_d;
            } else {
                float_d = -1.0;
            }
        }
        (float_d, sink_d)
    }
}

fn linear_ratio(float_d: f32, sink_d: f32) -> f32 {
    if float_d <= 0.0 {
        0.0
    } else if sink_d <= 0.0 {
        1.0
    } else {
        float_d / (float_d + sink_d)
    }
}

impl PhysicsWorld {
    pub(crate) fn buoyancy_forces(&mut self) {
        self.terrain.buoyant.clear();
        if !self.terrain.is_active() || self.terrain.water_parts.is_empty() {
            return;
        }
        let mut parts: Vec<_> = self.terrain.water_parts.iter().copied().collect();
        parts.sort();
        let viscosity = self.water_viscosity;
        let mut applied: Vec<(usize, Vec3, Vec3)> = Vec::new();
        let mut buoyant = Vec::new();
        {
            let water = Water {
                terrain: &self.terrain,
            };
            for id in parts {
                let Some(&pi) = self.index.get(&id) else {
                    continue;
                };
                let p = &self.parts[pi];
                let ai = p.assembly;
                if ai >= self.assemblies.len()
                    || self.assemblies[ai].anchored
                    || self.assemblies[ai].sleeping
                {
                    continue;
                }
                let cf = p.cframe;
                let size = p.shape.size;
                let local = Extents {
                    low: -size * 0.5,
                    high: size * 0.5,
                };
                let world_ext = local.transformed(&cf);
                let is_ball = p.shape.kind == ShapeKind::Ball;
                let colliding = if is_ball {
                    let c = cf.translation;
                    water.under(c) || (0..6).any(|f| water.under(c + local.face_center(f)))
                } else {
                    water.under(world_ext.center())
                        || (0..8).any(|i| water.under(world_ext.corner(i)))
                };
                if !colliding {
                    continue;
                }
                buoyant.push(id);
                let full = match self.mode {
                    PhysicalMode::New => p.shape.volume * GRAVITY * WATER_DENSITY,
                    PhysicalMode::Legacy => {
                        let sg = specific_gravity(p.material);
                        if sg > 0.0 { p.mass * GRAVITY / sg } else { 0.0 }
                    }
                };
                let (v_part, _) = self.part_velocity(pi);
                let mut connectors: Vec<(Vec3, f32, Vec3)> = Vec::new();
                if is_ball {
                    let r = size.x / 2.0;
                    let c = cf.translation;
                    let bottom = c - Vec3::Y * r;
                    let top = c + Vec3::Y * r;
                    let (fd, sd) = if let Some(level) = water.submerged_level(bottom, top) {
                        let fd = level - bottom.y;
                        if water.under(top) {
                            (fd, -1.0)
                        } else {
                            let l2 = water.above_water(top, bottom.y).unwrap_or(level);
                            (fd, top.y - l2)
                        }
                    } else if let Some(level) = water.above_water(top, bottom.y) {
                        (top.y - level, level - bottom.y)
                    } else {
                        let f = [
                            Vec3::new(-r, 0.0, 0.0),
                            Vec3::new(r, 0.0, 0.0),
                            Vec3::new(0.0, 0.0, -r),
                            Vec3::new(0.0, 0.0, r),
                        ]
                        .iter()
                        .filter(|o| water.under(c + **o))
                        .count() as f32;
                        (f, 4.0 - f)
                    };
                    let ratio = if fd <= 0.0 {
                        0.0
                    } else if sd <= 0.0 {
                        1.0
                    } else {
                        let k = fd / (fd + sd);
                        k * k * (3.0 - 2.0 * k)
                    };
                    let cross = std::f32::consts::PI * r * r;
                    let mut wv = water.velocity(c);
                    if wv == Vec3::ZERO {
                        for f in 0..6 {
                            wv = water.velocity(c + local.face_center(f));
                            if wv != Vec3::ZERO {
                                break;
                            }
                        }
                    }
                    connectors.push((Vec3::ZERO, ratio, wv));
                    let force = Self::connector_force(
                        full,
                        1,
                        viscosity,
                        ratio,
                        v_part,
                        wv,
                        &cf,
                        Vec3::splat(cross) / 8.0,
                    );
                    applied.push((pi, cf.translation, force));
                    continue;
                }
                let mut ratios = [0.0f32; 8];
                for (i, r) in ratios.iter_mut().enumerate() {
                    let voxel = Extents::vv(Vec3::ZERO, local.corner(i)).transformed(&cf);
                    let (fd, sd) = water.extents_band(&voxel);
                    *r = linear_ratio(fd, sd);
                }
                let mut cross = Vec3::new(size.y * size.z, size.x * size.z, size.x * size.y);
                match p.shape.kind {
                    ShapeKind::Cylinder => {
                        let rr = size.y / 2.0;
                        cross.x = std::f32::consts::PI * rr * rr;
                        let start = (ratios[0] + ratios[1] + ratios[2] + ratios[3]) * 0.5 - 1.0;
                        let end = (ratios[4] + ratios[5] + ratios[6] + ratios[7]) * 0.5 - 1.0;
                        let vert = cf.rotation.x_axis.normalize_or_zero().dot(Vec3::Y).abs();
                        let horiz = |x: f32| {
                            let x = x.clamp(-1.0, 1.0);
                            0.5 + (x.asin() + x * (1.0 - x * x).sqrt()) / std::f32::consts::PI
                        };
                        let (hs, he) = (horiz(start), horiz(end));
                        for (i, r) in ratios.iter_mut().enumerate() {
                            *r = vert * *r + (1.0 - vert) * if i < 4 { hs } else { he };
                        }
                    }
                    ShapeKind::Wedge => {
                        cross.x = size.y * size.z / 2.0;
                        const W: [f32; 8] = [1.0, 2.0, 0.0, 1.0, 1.0, 2.0, 0.0, 1.0];
                        for (r, w) in ratios.iter_mut().zip(W) {
                            *r *= w;
                        }
                    }
                    ShapeKind::CornerWedge => {
                        cross.x = size.y * size.z / 2.0;
                        cross.z = size.x * size.y / 2.0;
                        const W: [f32; 8] = [1.5, 1.0, 0.0, 0.0, 3.0, 1.5, 1.0, 0.0];
                        for (r, w) in ratios.iter_mut().zip(W) {
                            *r *= w;
                        }
                    }
                    _ => {}
                }
                for (i, ratio) in ratios.iter().enumerate() {
                    let offset = local.corner(i) / 2.0;
                    let world_point = cf * offset;
                    let wv = water.velocity(world_point);
                    let rel = v_part - wv;
                    let rel_obj = cf.rotation.transpose() * rel;
                    let (ix, iy, iz) = Extents::corner_index(i);
                    let pick = |sign: bool, idx: usize, value: f32| {
                        if (sign as usize) ^ idx == 0 {
                            value / 4.0
                        } else {
                            0.0
                        }
                    };
                    let cs = Vec3::new(
                        pick(rel_obj.x > 0.0, ix, cross.x),
                        pick(rel_obj.y > 0.0, iy, cross.y),
                        pick(rel_obj.z > 0.0, iz, cross.z),
                    );
                    let force =
                        Self::connector_force(full, 8, viscosity, *ratio, v_part, wv, &cf, cs);
                    applied.push((pi, world_point, force));
                }
            }
        }
        for (pi, point, force) in applied {
            let ai = self.parts[pi].assembly;
            let asm = &mut self.assemblies[ai];
            let com = asm.position;
            asm.force += force;
            asm.torque += (point - com).cross(force);
        }
        self.terrain.buoyant.extend(buoyant);
    }

    #[allow(clippy::too_many_arguments)]
    fn connector_force(
        full: f32,
        n: usize,
        viscosity: f32,
        ratio: f32,
        v_part: Vec3,
        water_v: Vec3,
        cf: &CFrame,
        cross: Vec3,
    ) -> Vec3 {
        if ratio <= 0.0 {
            return Vec3::ZERO;
        }
        let partial = Vec3::new(0.0, full / n as f32, 0.0);
        let k = -viscosity / KERNEL_DT / n as f32;
        let rel = v_part - water_v;
        if rel == Vec3::ZERO {
            return partial * ratio;
        }
        let rel_obj = cf.rotation.transpose() * rel;
        let visc_obj = rel_obj * cross;
        let linear = (cf.rotation * visc_obj) * k;
        (partial + linear) * ratio
    }
}
