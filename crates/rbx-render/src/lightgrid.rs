use rbx_math::{CFrame, IVec3, Vec3};
use std::collections::HashMap;

pub const CHUNK_XZ: i32 = 32;
pub const CHUNK_Y: i32 = 16;
pub const CELL: f32 = 4.0;
pub const CHUNK_COUNT: IVec3 = IVec3::new(8, 4, 8);
const CELLS: usize = (CHUNK_XZ * CHUNK_XZ * CHUNK_Y) as usize;
const DIRECTIONAL_DIRTY: u32 = 4;
const SKYLIGHT_DIRTY: u32 = 4;
const BORDER_CUTOFF: u32 = 32;

const DIRTY_LOCAL: u32 = 1 << 2;
const DIRTY_SKYLIGHT: u32 = 1 << 3;
const DIRTY_GLOBAL: u32 = 1 << 4;
const DIRTY_UPLOAD: u32 = 1 << 5;
const DIRTY_OCCUPANCY: u32 = 1 << 6;
const DIRTY_ALL: u32 = DIRTY_OCCUPANCY | DIRTY_GLOBAL | DIRTY_SKYLIGHT | DIRTY_UPLOAD | DIRTY_LOCAL;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VoxelShape {
    Block,
    Sphere,
    Ellipsoid,
    CylinderX,
    CylinderY,
    Wedge,
    CornerWedge,
    Torso,
}

#[derive(Clone, Copy, Debug)]
pub struct VoxelPart {
    pub shape: VoxelShape,
    pub size: Vec3,
    pub cframe: CFrame,
    pub transparency: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LightKind {
    Point,
    Spot,
    Surface,
}

#[derive(Clone, Copy, Debug)]
pub struct LightSource {
    pub kind: LightKind,
    pub position: Vec3,
    pub color: [u8; 3],
    pub brightness: f32,
    pub range: f32,
    pub direction: Vec3,
    pub angle: f32,
    pub axis_u: Vec3,
    pub half_u: f32,
    pub axis_v: Vec3,
    pub half_v: f32,
}

fn cone_extents(c: Vec3, r: f32, a: Vec3, angle: f32) -> (Vec3, Vec3) {
    let axes = [
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(0.0, -1.0, 0.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    ];
    let s = (angle / 2.0).to_radians();
    let (sins, coss) = s.sin_cos();
    let mut e = [Vec3::ZERO; 6];
    for (k, d) in axes.iter().enumerate() {
        let da = d.dot(a);
        e[k] = if da >= coss {
            c + *d * r
        } else if da <= -sins {
            c
        } else {
            let aat_d = a * a.dot(*d);
            c + a * (r * coss) + (*d - aat_d).normalize_or_zero() * (r * sins)
        };
    }
    (
        Vec3::new(e[0].x, e[1].y, e[2].z),
        Vec3::new(e[3].x, e[4].y, e[5].z),
    )
}

impl LightSource {
    pub fn extents(&self) -> (Vec3, Vec3) {
        match self.kind {
            LightKind::Point => (
                self.position - Vec3::splat(self.range),
                self.position + Vec3::splat(self.range),
            ),
            LightKind::Spot => cone_extents(self.position, self.range, self.direction, self.angle),
            LightKind::Surface => {
                let u = self.axis_u * self.half_u;
                let v = self.axis_v * self.half_v;
                let mut lo = Vec3::splat(f32::MAX);
                let mut hi = Vec3::splat(f32::MIN);
                for c in [
                    self.position - u - v,
                    self.position + u - v,
                    self.position - u + v,
                    self.position + u + v,
                ] {
                    let (a, b) = cone_extents(c, self.range, self.direction, self.angle);
                    lo = lo.min(a);
                    hi = hi.max(b);
                }
                (lo, hi)
            }
        }
    }
}

pub struct LightInputs<'a> {
    pub focus: Vec3,
    pub parts: &'a [VoxelPart],
    pub lights: &'a [LightSource],
    pub terrain: &'a dyn Fn(IVec3, &mut [u8]),
    pub terrain_generation: u64,
    pub light_direction: Vec3,
    pub sky_ambient: [u8; 3],
    pub shadows: bool,
}

#[inline]
fn idx(x: i32, y: i32, z: i32) -> usize {
    ((y * CHUNK_XZ + z) * CHUNK_XZ + x) as usize
}

struct Chunk {
    index: IVec3,
    dirty: u32,
    signature: u64,
    occupancy: Vec<u8>,
    lighting: Vec<[u8; 4]>,
    skylight: Vec<u8>,
    average_global: [u32; CHUNK_Y as usize],
    average_skylight: [u32; CHUNK_Y as usize],
    average_weight: [u32; CHUNK_Y as usize],
    fringe_x: Vec<u8>,
    fringe_y: Vec<u8>,
    fringe_z: Vec<u8>,
    fringe_sky_y: Vec<u8>,
    fringe_sky_x0: Vec<u8>,
    fringe_sky_x1: Vec<u8>,
    fringe_sky_z0: Vec<u8>,
    fringe_sky_z1: Vec<u8>,
}

const FRINGE_XZ: usize = (CHUNK_Y * CHUNK_XZ) as usize;
const FRINGE_Y: usize = (CHUNK_XZ * CHUNK_XZ) as usize;

impl Chunk {
    fn new(index: IVec3) -> Chunk {
        Chunk {
            index,
            dirty: DIRTY_ALL,
            signature: u64::MAX,
            occupancy: vec![0; CELLS],
            lighting: vec![[0, 0, 0, 255]; CELLS],
            skylight: vec![0; CELLS],
            average_global: [0; CHUNK_Y as usize],
            average_skylight: [0; CHUNK_Y as usize],
            average_weight: [0; CHUNK_Y as usize],
            fringe_x: vec![255; FRINGE_XZ],
            fringe_y: vec![255; FRINGE_Y],
            fringe_z: vec![255; FRINGE_XZ],
            fringe_sky_y: vec![255; FRINGE_Y],
            fringe_sky_x0: vec![255; FRINGE_XZ],
            fringe_sky_x1: vec![255; FRINGE_XZ],
            fringe_sky_z0: vec![255; FRINGE_XZ],
            fringe_sky_z1: vec![255; FRINGE_XZ],
        }
    }

    fn min_corner(&self) -> Vec3 {
        Vec3::new(
            (self.index.x * CHUNK_XZ) as f32,
            (self.index.y * CHUNK_Y) as f32,
            (self.index.z * CHUNK_XZ) as f32,
        ) * CELL
    }
}

fn extents_world(cframe: &CFrame, size: Vec3) -> Vec3 {
    let r = cframe.rotation;
    let axes = [r.x_axis, r.y_axis, r.z_axis];
    let s = [size.x, size.y, size.z];
    let mut out = Vec3::ZERO;
    for i in 0..3 {
        out.x += axes[i].x.abs() * s[i];
        out.y += axes[i].y.abs() * s[i];
        out.z += axes[i].z.abs() * s[i];
    }
    out
}

fn penetration(left: f32, right: f32, p: f32) -> f32 {
    if p <= left {
        return left - p;
    }
    if p >= right {
        return p - right;
    }
    -(p - left).min(right - p)
}

fn shape_distance(shape: VoxelShape, size: Vec3, p: Vec3) -> Vec3 {
    let h = size * 0.5;
    match shape {
        VoxelShape::Block => Vec3::new(p.x.abs() - h.x, p.y.abs() - h.y, p.z.abs() - h.z),
        VoxelShape::Sphere => {
            let d = p.length() - size.x / 2.0;
            Vec3::splat(d)
        }
        VoxelShape::Ellipsoid => {
            let r = h.max(Vec3::splat(1e-4));
            let k =
                ((p.x * p.x) / (r.x * r.x) + (p.y * p.y) / (r.y * r.y) + (p.z * p.z) / (r.z * r.z))
                    .sqrt();
            Vec3::new((k - 1.0) * r.x, (k - 1.0) * r.y, (k - 1.0) * r.z)
        }
        VoxelShape::CylinderX => {
            let d = (p.y * p.y + p.z * p.z).sqrt() - size.y.min(size.z) / 2.0;
            Vec3::new(p.x.abs() - h.x, d, d)
        }
        VoxelShape::CylinderY => {
            let d = (p.x * p.x + p.z * p.z).sqrt() - size.x.min(size.z) / 2.0;
            Vec3::new(d, p.y.abs() - h.y, d)
        }
        VoxelShape::Wedge => {
            let slope_yz = size.y / size.z.max(1e-4);
            let slope_zy = size.z / size.y.max(1e-4);
            Vec3::new(
                p.x.abs() - h.x,
                penetration(-h.y, p.z * slope_yz, p.y),
                penetration(p.y * slope_zy, h.z, p.z),
            )
        }
        VoxelShape::CornerWedge => {
            let slope_xy = size.x / size.y.max(1e-4);
            let slope_zy = size.z / size.y.max(1e-4);
            let slope_yz = size.y / size.z.max(1e-4);
            let slope_yx = size.y / size.x.max(1e-4);
            Vec3::new(
                penetration(p.y * slope_xy, h.x, p.x),
                penetration(-h.y, (-p.z * slope_yz).min(p.x * slope_yx), p.y),
                penetration(-h.z, -p.y * slope_zy, p.z),
            )
        }
        VoxelShape::Torso => {
            let inset = size.z * 0.3;
            let top = (h.x - inset).max(0.0);
            let cur = h.x + (top - h.x) * (p.y / h.y.max(1e-4) * 0.5 + 0.5);
            Vec3::new(p.x.abs() - cur, p.y.abs() - h.y, p.z.abs() - h.z)
        }
    }
}

fn fill_part(chunk: &mut Chunk, part: &VoxelPart) {
    let corner = chunk.min_corner();
    let ext = extents_world(&part.cframe, part.size);
    let low = part.cframe.translation - ext * 0.5;
    let high = part.cframe.translation + ext * 0.5;
    let lo = ((low - corner) / CELL).floor();
    let hi = ((high - corner) / CELL - Vec3::splat(0.01)).floor();
    let (x0, y0, z0) = (
        lo.x.max(0.0) as i32,
        lo.y.max(0.0) as i32,
        lo.z.max(0.0) as i32,
    );
    let (x1, y1, z1) = (
        (hi.x as i32).min(CHUNK_XZ - 1),
        (hi.y as i32).min(CHUNK_Y - 1),
        (hi.z as i32).min(CHUNK_XZ - 1),
    );
    if x0 > x1 || y0 > y1 || z0 > z1 {
        return;
    }
    let thickness = 255.5 * (1.0 - part.transparency.clamp(0.0, 1.0));
    let r = part.cframe.rotation;
    let axis_x = Vec3::new(r.x_axis.x, r.y_axis.x, r.z_axis.x);
    let axis_y = Vec3::new(r.x_axis.y, r.y_axis.y, r.z_axis.y);
    let axis_z = Vec3::new(r.x_axis.z, r.y_axis.z, r.z_axis.z);
    let origin = corner + Vec3::new(x0 as f32 + 0.5, y0 as f32 + 0.5, z0 as f32 + 0.5) * CELL;
    let local_corner = part.cframe.point_to_object_space(origin);
    if part.shape == VoxelShape::Block {
        let adjusted = part.size.max(Vec3::ONE);
        let clamped = (adjusted / CELL).min(Vec3::ONE);
        let half_offset = adjusted * (0.5 / CELL) + Vec3::splat(0.5);
        let start = local_corner / CELL;
        for y in y0..=y1 {
            for z in z0..=z1 {
                let mut l = start + axis_y * (y - y0) as f32 + axis_z * (z - z0) as f32;
                for x in x0..=x1 {
                    let fx = (half_offset.x - l.x.abs()).min(clamped.x).max(0.0);
                    let fy = (half_offset.y - l.y.abs()).min(clamped.y).max(0.0);
                    let fz = (half_offset.z - l.z.abs()).min(clamped.z).max(0.0);
                    let f = (fx * fy * fz * thickness) as i32;
                    let o = &mut chunk.occupancy[idx(x, y, z)];
                    *o = (*o as i32 + f).min(255) as u8;
                    l += axis_x;
                }
            }
        }
        return;
    }
    let clamped = (part.size / CELL).min(Vec3::ONE);
    for y in y0..=y1 {
        for z in z0..=z1 {
            let mut l = local_corner + (axis_y * (y - y0) as f32 + axis_z * (z - z0) as f32) * CELL;
            for x in x0..=x1 {
                let d = shape_distance(part.shape, part.size, l);
                let fx = (0.5 - d.x * 0.25).min(clamped.x).max(0.0);
                let fy = (0.5 - d.y * 0.25).min(clamped.y).max(0.0);
                let fz = (0.5 - d.z * 0.25).min(clamped.z).max(0.0);
                let f = (fx * fy * fz * thickness) as i32;
                let o = &mut chunk.occupancy[idx(x, y, z)];
                *o = (*o as i32 + f).min(255) as u8;
                l += axis_x * CELL;
            }
        }
    }
}

fn fixed_point(x: f32, y: f32, z: f32, bits: i32) -> IVec3 {
    let target = 1 << bits;
    let mut rx = (x.clamp(0.0, 1.0) * target as f32 + 0.5) as i32;
    let mut ry = (y.clamp(0.0, 1.0) * target as f32 + 0.5) as i32;
    let mut rz = (z.clamp(0.0, 1.0) * target as f32 + 0.5) as i32;
    rz = (rz + (target - (rx + ry + rz)) / 3).max(0);
    rx = (rx + (target - (rx + ry + rz)) / 2).max(0);
    ry = (ry + (target - (rx + ry + rz))).max(0);
    IVec3::new(rx, ry, rz)
}

fn contribution_z1(x: f32, y: f32) -> Vec3 {
    let wx = x / 2.0;
    let wy = y / 2.0;
    let corner = x * y / 6.0;
    let cx = wx - corner;
    let cy = wy - corner;
    Vec3::new(cx, cy, 1.0 - cx - cy)
}

pub fn light_contribution(d: Vec3, bits: i32) -> IVec3 {
    let (x, y, z) = (d.x.abs(), d.y.abs(), d.z.abs());
    if z >= y && z >= x {
        let r = contribution_z1(x / z, y / z);
        fixed_point(r.x, r.y, r.z, bits)
    } else if y >= z && y >= x {
        let r = contribution_z1(x / y, z / y);
        fixed_point(r.x, r.z, r.y, bits)
    } else {
        let r = contribution_z1(y / x, z / x);
        fixed_point(r.z, r.x, r.y, bits)
    }
}

fn local_lighting<'a>(chunk: &mut Chunk, lights: impl Iterator<Item = &'a LightSource>) {
    const SX: usize = (CHUNK_XZ + 2) as usize;
    const SY: usize = (CHUNK_Y + 2) as usize;
    let at = |x: usize, y: usize, z: usize| (y * SX + z) * SX + x;
    let mut scratch = vec![[0u32; 3]; SX * SX * SY];
    let corner = chunk.min_corner();
    let mut any = false;
    for l in lights {
        any = true;
        let position = (l.position - corner) / CELL;
        let range = l.range / CELL;
        let (lo, hi) = l.extents();
        let emin = ((lo - corner) / CELL + Vec3::ONE).floor().max(Vec3::ZERO);
        let emax = ((hi - corner) / CELL + Vec3::splat(0.99))
            .floor()
            .min(Vec3::new(
                (CHUNK_XZ + 1) as f32,
                (CHUNK_Y + 1) as f32,
                (CHUNK_XZ + 1) as f32,
            ));
        let inv_sq = 1.0 / (range * range).max(1e-6);
        let cone_cos = (l.angle / 2.0).to_radians().cos();
        let cone_scale = if cone_cos < 1.0 {
            1.0 / (1.0 - cone_cos)
        } else {
            0.0
        };
        let u = l.axis_u;
        let v = l.axis_v;
        let (hu, hv) = (l.half_u / CELL, l.half_v / CELL);
        for y in emin.y as usize..=emax.y as usize {
            for z in emin.z as usize..=emax.z as usize {
                for x in emin.x as usize..=emax.x as usize {
                    let cell = Vec3::new(x as f32 - 0.5, y as f32 - 0.5, z as f32 - 0.5);
                    let attenuation = match l.kind {
                        LightKind::Point => {
                            let d = 1.0 - (position - cell).length_squared() * inv_sq;
                            (d * l.brightness).clamp(0.0, 1.0)
                        }
                        LightKind::Spot => {
                            let d = 1.0 - (position - cell).length_squared() * inv_sq;
                            let c = (l.direction.dot((cell - position).normalize_or_zero())
                                - cone_cos)
                                * cone_scale;
                            (d * c.max(0.0) * l.brightness).clamp(0.0, 1.0)
                        }
                        LightKind::Surface => {
                            let pu = (cell - position).dot(u).clamp(-hu, hu);
                            let pv = (cell - position).dot(v).clamp(-hv, hv);
                            let projected = position + u * pu + v * pv;
                            let d = 1.0 - (projected - cell).length_squared() * inv_sq;
                            let c = (l.direction.dot((cell - projected).normalize_or_zero())
                                - cone_cos)
                                * cone_scale;
                            (d * c.max(0.0) * l.brightness).clamp(0.0, 1.0)
                        }
                    };
                    let factor = (attenuation * 65536.0 + 0.5) as u32;
                    let s = &mut scratch[at(x, y, z)];
                    for k in 0..3 {
                        s[k] = (s[k] + ((l.color[k] as u32 * factor) >> 16)).min(255);
                    }
                }
            }
        }
    }
    if !any {
        for l in chunk.lighting.iter_mut() {
            l[0] = 0;
            l[1] = 0;
            l[2] = 0;
        }
        return;
    }
    for x in 0..SX {
        for z in 0..SX {
            let mut prev = scratch[at(x, 0, z)];
            let mut cur = scratch[at(x, 1, z)];
            for y in 1..SY - 1 {
                let next = scratch[at(x, y + 1, z)];
                scratch[at(x, y, z)] = [0, 1, 2].map(|k| (prev[k] + cur[k] + next[k]) / 4);
                prev = cur;
                cur = next;
            }
        }
    }
    for y in 0..SY {
        for x in 0..SX {
            let mut prev = scratch[at(x, y, 0)];
            let mut cur = scratch[at(x, y, 1)];
            for z in 1..SX - 1 {
                let next = scratch[at(x, y, z + 1)];
                scratch[at(x, y, z)] = [0, 1, 2].map(|k| (prev[k] + cur[k] + next[k]) / 4);
                prev = cur;
                cur = next;
            }
        }
    }
    for y in 1..SY - 1 {
        for z in 1..SX - 1 {
            let mut prev = scratch[at(0, y, z)];
            let mut cur = scratch[at(1, y, z)];
            for x in 1..SX - 1 {
                let next = scratch[at(x + 1, y, z)];
                let value = [0, 1, 2].map(|k| ((prev[k] + cur[k] + next[k]) / 2) as u8);
                let l = &mut chunk.lighting[idx(x as i32 - 1, y as i32 - 1, z as i32 - 1)];
                l[0] = value[0];
                l[1] = value[1];
                l[2] = value[2];
                prev = cur;
                cur = next;
            }
        }
    }
}

const SCRATCH_X: i32 = CHUNK_XZ + 2;
const SCRATCH_Y: i32 = CHUNK_Y + 2;

struct Scratch {
    data: Vec<u8>,
}

impl Scratch {
    fn new() -> Scratch {
        Scratch {
            data: vec![0; (SCRATCH_X * SCRATCH_X * SCRATCH_Y) as usize],
        }
    }

    #[inline]
    fn at(&mut self, x: i32, y: i32, z: i32) -> &mut u8 {
        let i = (((y + 1) * SCRATCH_X + (z + 1)) * SCRATCH_X + (x + 1)) as usize;
        &mut self.data[i]
    }

    #[inline]
    fn get(&self, x: i32, y: i32, z: i32) -> u8 {
        self.data[(((y + 1) * SCRATCH_X + (z + 1)) * SCRATCH_X + (x + 1)) as usize]
    }
}

fn update_fringe(target: &mut [u8], i: usize, value: u8, dirty: &mut u32) {
    if target[i] != value {
        *dirty += (target[i] as i32 - value as i32).unsigned_abs();
        target[i] = value;
    }
}

pub struct LightGrid {
    chunks: HashMap<IVec3, Chunk>,
    corner: Option<IVec3>,
    light_direction: Vec3,
    sky_ambient: [u8; 3],
    shadows: bool,
    terrain_generation: Option<u64>,
    border_global: u32,
    border_skylight: u32,
    scratch: Scratch,
    pub uploads: Vec<(IVec3, Vec<u8>)>,
}

impl Default for LightGrid {
    fn default() -> Self {
        LightGrid::new()
    }
}

impl LightGrid {
    pub fn new() -> LightGrid {
        LightGrid {
            chunks: HashMap::new(),
            corner: None,
            light_direction: Vec3::new(0.0, -1.0, 0.0),
            sky_ambient: [0; 3],
            shadows: false,
            terrain_generation: None,
            border_global: 255,
            border_skylight: 0,
            scratch: Scratch::new(),
            uploads: Vec::new(),
        }
    }

    pub fn corner(&self) -> IVec3 {
        self.corner.unwrap_or(IVec3::ZERO)
    }

    pub fn grid_offset(&self) -> Vec3 {
        let c = self.corner();
        Vec3::new(
            (c.x * CHUNK_XZ) as f32,
            (c.y * CHUNK_Y) as f32,
            (c.z * CHUNK_XZ) as f32,
        ) * CELL
    }

    pub fn grid_size() -> Vec3 {
        Vec3::new(
            (CHUNK_COUNT.x * CHUNK_XZ) as f32,
            (CHUNK_COUNT.y * CHUNK_Y) as f32,
            (CHUNK_COUNT.z * CHUNK_XZ) as f32,
        ) * CELL
    }

    pub fn texture_size() -> IVec3 {
        IVec3::new(
            CHUNK_COUNT.x * CHUNK_XZ,
            CHUNK_COUNT.z * CHUNK_XZ,
            CHUNK_COUNT.y * CHUNK_Y,
        )
    }

    pub fn border_color(&self) -> [f32; 4] {
        let global = if self.border_global < BORDER_CUTOFF {
            0
        } else {
            255
        };
        let skylight = if self.border_skylight < BORDER_CUTOFF {
            0
        } else {
            255
        };
        let s = |c: u8| ((c as u32 * skylight + 255) >> 8) as f32 / 255.0;
        [
            s(self.sky_ambient[0]),
            s(self.sky_ambient[1]),
            s(self.sky_ambient[2]),
            global as f32 / 255.0,
        ]
    }

    fn chunk_of(p: Vec3) -> IVec3 {
        let c = (p / CELL).floor();
        IVec3::new(
            (c.x as i32).div_euclid(CHUNK_XZ),
            (c.y as i32).div_euclid(CHUNK_Y),
            (c.z as i32).div_euclid(CHUNK_XZ),
        )
    }

    fn recenter(&mut self, focus: Vec3) {
        let center = Self::chunk_of(focus);
        let corner = center - IVec3::new(CHUNK_COUNT.x / 2, CHUNK_COUNT.y / 2, CHUNK_COUNT.z / 2);
        if self.corner == Some(corner) {
            return;
        }
        self.corner = Some(corner);
        let end = corner + CHUNK_COUNT;
        self.chunks.retain(|i, _| {
            i.x >= corner.x
                && i.y >= corner.y
                && i.z >= corner.z
                && i.x < end.x
                && i.y < end.y
                && i.z < end.z
        });
        for y in corner.y..end.y {
            for z in corner.z..end.z {
                for x in corner.x..end.x {
                    let i = IVec3::new(x, y, z);
                    self.chunks.entry(i).or_insert_with(|| Chunk::new(i));
                }
            }
        }
    }

    fn mark_all(&mut self, flags: u32) {
        for c in self.chunks.values_mut() {
            c.dirty |= flags;
        }
    }

    pub fn update(&mut self, inputs: &LightInputs, budget: usize) {
        self.recenter(inputs.focus);
        if inputs.shadows != self.shadows {
            self.shadows = inputs.shadows;
            self.mark_all(DIRTY_GLOBAL | DIRTY_SKYLIGHT | DIRTY_UPLOAD);
        }
        if (inputs.light_direction - self.light_direction).length() > 1e-3 {
            self.light_direction = inputs.light_direction;
            self.mark_all(DIRTY_GLOBAL);
        }
        if self.terrain_generation != Some(inputs.terrain_generation) {
            self.terrain_generation = Some(inputs.terrain_generation);
            self.mark_all(DIRTY_ALL);
        }
        if inputs.sky_ambient != self.sky_ambient {
            self.sky_ambient = inputs.sky_ambient;
            self.mark_all(DIRTY_UPLOAD);
        }
        let mut buckets: HashMap<IVec3, Vec<usize>> = HashMap::new();
        let corner = self.corner();
        let end = corner + CHUNK_COUNT;
        for (i, p) in inputs.parts.iter().enumerate() {
            let ext = extents_world(&p.cframe, p.size);
            let a = Self::chunk_of(p.cframe.translation - ext * 0.5).max(corner);
            let b = Self::chunk_of(p.cframe.translation + ext * 0.5).min(end - IVec3::ONE);
            for y in a.y..=b.y {
                for z in a.z..=b.z {
                    for x in a.x..=b.x {
                        buckets.entry(IVec3::new(x, y, z)).or_default().push(i);
                    }
                }
            }
        }
        let mut light_buckets: HashMap<IVec3, Vec<usize>> = HashMap::new();
        for (i, l) in inputs.lights.iter().enumerate() {
            let (lo, hi) = l.extents();
            let a = Self::chunk_of(lo).max(corner);
            let b = Self::chunk_of(hi).min(end - IVec3::ONE);
            for y in a.y..=b.y {
                for z in a.z..=b.z {
                    for x in a.x..=b.x {
                        light_buckets
                            .entry(IVec3::new(x, y, z))
                            .or_default()
                            .push(i);
                    }
                }
            }
        }
        let mut terrain_scratch = vec![0u8; CELLS];
        for chunk in self.chunks.values_mut() {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            if let Some(list) = light_buckets.get(&chunk.index) {
                for &i in list {
                    let l = &inputs.lights[i];
                    (l.kind as u8).hash(&mut h);
                    l.position.to_array().map(f32::to_bits).hash(&mut h);
                    l.direction.to_array().map(f32::to_bits).hash(&mut h);
                    l.color.hash(&mut h);
                    [l.brightness, l.range, l.angle, l.half_u, l.half_v]
                        .map(f32::to_bits)
                        .hash(&mut h);
                }
            }
            if let Some(list) = buckets.get(&chunk.index) {
                for &i in list {
                    let p = &inputs.parts[i];
                    (p.shape as u8).hash(&mut h);
                    p.size.to_array().map(f32::to_bits).hash(&mut h);
                    p.cframe
                        .translation
                        .to_array()
                        .map(f32::to_bits)
                        .hash(&mut h);
                    p.cframe
                        .rotation
                        .to_cols_array()
                        .map(f32::to_bits)
                        .hash(&mut h);
                    p.transparency.to_bits().hash(&mut h);
                }
            }
            let sig = h.finish();
            if sig != chunk.signature {
                chunk.signature = sig;
                chunk.dirty |= DIRTY_ALL;
            }
        }
        let mut done = 0;
        while done < budget {
            let Some(index) = self.next_dirty() else {
                break;
            };
            self.process(
                index,
                inputs,
                buckets.get(&index).map(Vec::as_slice).unwrap_or(&[]),
                light_buckets.get(&index).map(Vec::as_slice).unwrap_or(&[]),
                &mut terrain_scratch,
            );
            done += 1;
        }
        self.update_border(inputs.focus);
    }

    fn next_dirty(&self) -> Option<IVec3> {
        let neg_x = self.light_direction.x < 0.0;
        let neg_z = self.light_direction.z < 0.0;
        self.chunks
            .values()
            .filter(|c| c.dirty != 0)
            .map(|c| c.index)
            .min_by_key(|i| {
                (
                    -i.y,
                    if neg_x { -i.x } else { i.x },
                    if neg_z { -i.z } else { i.z },
                )
            })
    }

    fn process(
        &mut self,
        index: IVec3,
        inputs: &LightInputs,
        parts: &[usize],
        lights: &[usize],
        terrain: &mut [u8],
    ) {
        let Some(mut chunk) = self.chunks.remove(&index) else {
            return;
        };
        let dirty = chunk.dirty;
        chunk.dirty = 0;
        if dirty & DIRTY_OCCUPANCY != 0 {
            terrain.fill(0);
            (inputs.terrain)(index, terrain);
            chunk.occupancy.copy_from_slice(terrain);
            for &i in parts {
                fill_part(&mut chunk, &inputs.parts[i]);
            }
        }
        if dirty & (DIRTY_OCCUPANCY | DIRTY_LOCAL) != 0 {
            local_lighting(&mut chunk, lights.iter().map(|&i| &inputs.lights[i]));
        }
        if dirty & (DIRTY_OCCUPANCY | DIRTY_GLOBAL) != 0 {
            if self.shadows {
                self.directional(&mut chunk);
            } else {
                for l in chunk.lighting.iter_mut() {
                    l[3] = 255;
                }
            }
        }
        if dirty & (DIRTY_OCCUPANCY | DIRTY_SKYLIGHT) != 0 {
            if self.shadows {
                self.skylight(&mut chunk);
            } else {
                chunk.skylight.fill(0);
            }
        }
        self.average(&mut chunk);
        self.uploads.push((index, self.composite(&chunk)));
        self.chunks.insert(index, chunk);
    }

    fn neighbor_fringe(&self, index: IVec3, pick: fn(&Chunk) -> &Vec<u8>) -> Option<&Vec<u8>> {
        self.chunks.get(&index).map(pick)
    }

    fn mark(&mut self, index: IVec3, flags: u32) {
        if let Some(c) = self.chunks.get_mut(&index) {
            c.dirty |= flags;
        }
    }

    fn directional(&mut self, chunk: &mut Chunk) {
        let dir = self.light_direction;
        let neg_x = dir.x < 0.0;
        let neg_z = dir.z < 0.0;
        let contrib = light_contribution(dir, 8);
        let step_x = if neg_x { 1 } else { -1 };
        let step_y = 1;
        let step_z = if neg_z { 1 } else { -1 };
        let index = chunk.index;
        let full_xz = vec![255u8; FRINGE_XZ];
        let full_y = vec![255u8; FRINGE_Y];
        let in_x = self
            .neighbor_fringe(index + IVec3::new(step_x, 0, 0), |c| &c.fringe_x)
            .cloned()
            .unwrap_or_else(|| full_xz.clone());
        let in_y = self
            .neighbor_fringe(index + IVec3::new(0, step_y, 0), |c| &c.fringe_y)
            .cloned()
            .unwrap_or_else(|| full_y.clone());
        let in_z = self
            .neighbor_fringe(index + IVec3::new(0, 0, step_z), |c| &c.fringe_z)
            .cloned()
            .unwrap_or(full_xz);
        let s = &mut self.scratch;
        for y in 0..CHUNK_Y {
            for z in 0..CHUNK_XZ {
                *s.at(if neg_x { CHUNK_XZ } else { -1 }, y, z) = in_x[(y * CHUNK_XZ + z) as usize];
            }
        }
        for z in 0..CHUNK_XZ {
            for x in 0..CHUNK_XZ {
                *s.at(x, CHUNK_Y, z) = in_y[(z * CHUNK_XZ + x) as usize];
            }
        }
        for y in 0..CHUNK_Y {
            for x in 0..CHUNK_XZ {
                *s.at(x, y, if neg_z { CHUNK_XZ } else { -1 }) = in_z[(y * CHUNK_XZ + x) as usize];
            }
        }
        for yi in 0..CHUNK_Y {
            let y = CHUNK_Y - 1 - yi;
            for zi in 0..CHUNK_XZ {
                let z = if neg_z { CHUNK_XZ - 1 - zi } else { zi };
                for xi in 0..CHUNK_XZ {
                    let x = if neg_x { CHUNK_XZ - 1 - xi } else { xi };
                    let ix = s.get(x + step_x, y, z) as i32;
                    let iy = s.get(x, y + step_y, z) as i32;
                    let iz = s.get(x, y, z + step_z) as i32;
                    let result = (ix * contrib.x + iy * contrib.y + iz * contrib.z + 255) >> 8;
                    let i = idx(x, y, z);
                    chunk.lighting[i][3] = result.min(255) as u8;
                    *s.at(x, y, z) = (result - chunk.occupancy[i] as i32).max(0) as u8;
                }
            }
        }
        let (mut dx, mut dy, mut dz) = (0u32, 0u32, 0u32);
        for y in 0..CHUNK_Y {
            for z in 0..CHUNK_XZ {
                let v = s.get(if neg_x { 0 } else { CHUNK_XZ - 1 }, y, z);
                update_fringe(&mut chunk.fringe_x, (y * CHUNK_XZ + z) as usize, v, &mut dx);
            }
        }
        for z in 0..CHUNK_XZ {
            for x in 0..CHUNK_XZ {
                let v = s.get(x, 0, z);
                update_fringe(&mut chunk.fringe_y, (z * CHUNK_XZ + x) as usize, v, &mut dy);
            }
        }
        for y in 0..CHUNK_Y {
            for x in 0..CHUNK_XZ {
                let v = s.get(x, y, if neg_z { 0 } else { CHUNK_XZ - 1 });
                update_fringe(&mut chunk.fringe_z, (y * CHUNK_XZ + x) as usize, v, &mut dz);
            }
        }
        if dx > DIRECTIONAL_DIRTY {
            self.mark(index - IVec3::new(step_x, 0, 0), DIRTY_GLOBAL);
        }
        if dy > DIRECTIONAL_DIRTY {
            self.mark(index - IVec3::new(0, step_y, 0), DIRTY_GLOBAL);
        }
        if dz > DIRECTIONAL_DIRTY {
            self.mark(index - IVec3::new(0, 0, step_z), DIRTY_GLOBAL);
        }
    }

    fn skylight(&mut self, chunk: &mut Chunk) {
        let mut curve = [0u8; 256];
        for (i, c) in curve.iter_mut().enumerate() {
            let r1 = 255 - i as u32;
            let r2 = r1 * r1;
            let r4 = r2 * r2;
            *c = (252 - (r4 >> 24)) as u8;
        }
        let index = chunk.index;
        let get = |o: IVec3, pick: fn(&Chunk) -> &Vec<u8>, n: usize| -> Vec<u8> {
            self.chunks
                .get(&(index + o))
                .map(pick)
                .cloned()
                .unwrap_or_else(|| vec![255; n])
        };
        let in_y = get(IVec3::new(0, 1, 0), |c| &c.fringe_sky_y, FRINGE_Y);
        let in_x0 = get(IVec3::new(-1, 0, 0), |c| &c.fringe_sky_x1, FRINGE_XZ);
        let in_x1 = get(IVec3::new(1, 0, 0), |c| &c.fringe_sky_x0, FRINGE_XZ);
        let in_z0 = get(IVec3::new(0, 0, -1), |c| &c.fringe_sky_z1, FRINGE_XZ);
        let in_z1 = get(IVec3::new(0, 0, 1), |c| &c.fringe_sky_z0, FRINGE_XZ);
        let s = &mut self.scratch;
        for z in 0..CHUNK_XZ {
            for x in 0..CHUNK_XZ {
                *s.at(x, CHUNK_Y, z) = in_y[(z * CHUNK_XZ + x) as usize];
            }
        }
        for y in 0..CHUNK_Y {
            for z in 0..CHUNK_XZ {
                *s.at(-1, y, z) = in_x0[(y * CHUNK_XZ + z) as usize];
                *s.at(CHUNK_XZ, y, z) = in_x1[(y * CHUNK_XZ + z) as usize];
            }
            for x in 0..CHUNK_XZ {
                *s.at(x, y, -1) = in_z0[(y * CHUNK_XZ + x) as usize];
                *s.at(x, y, CHUNK_XZ) = in_z1[(y * CHUNK_XZ + x) as usize];
            }
        }
        let ty = CHUNK_Y;
        let n = CHUNK_XZ;
        for x in 0..n {
            let a = (s.get(x, ty - 1, -1) as u32 + s.get(x, ty, 0) as u32).div_ceil(2);
            *s.at(x, ty, -1) = a as u8;
            let b = (s.get(x, ty - 1, n) as u32 + s.get(x, ty, n - 1) as u32).div_ceil(2);
            *s.at(x, ty, n) = b as u8;
        }
        for z in 0..n {
            let a = (s.get(-1, ty - 1, z) as u32 + s.get(0, ty, z) as u32).div_ceil(2);
            *s.at(-1, ty, z) = a as u8;
            let b = (s.get(n, ty - 1, z) as u32 + s.get(n - 1, ty, z) as u32).div_ceil(2);
            *s.at(n, ty, z) = b as u8;
        }
        for y in 0..=ty {
            let a = (s.get(-1, y, 0) as u32 + s.get(0, y, -1) as u32).div_ceil(2);
            *s.at(-1, y, -1) = a as u8;
            let b = (s.get(n, y, 0) as u32 + s.get(n - 1, y, -1) as u32).div_ceil(2);
            *s.at(n, y, -1) = b as u8;
            let c = (s.get(-1, y, n - 1) as u32 + s.get(0, y, n) as u32).div_ceil(2);
            *s.at(-1, y, n) = c as u8;
            let d = (s.get(n, y, n - 1) as u32 + s.get(n - 1, y, n) as u32).div_ceil(2);
            *s.at(n, y, n) = d as u8;
        }
        for yi in 0..CHUNK_Y {
            let y = CHUNK_Y - 1 - yi;
            for z in 0..n {
                for x in 0..n {
                    let sum = s.get(x + 1, y + 1, z + 1) as u32
                        + s.get(x, y + 1, z + 1) as u32
                        + s.get(x - 1, y + 1, z + 1) as u32
                        + s.get(x + 1, y + 1, z) as u32
                        + s.get(x - 1, y + 1, z) as u32
                        + s.get(x + 1, y + 1, z - 1) as u32
                        + s.get(x, y + 1, z - 1) as u32
                        + s.get(x - 1, y + 1, z - 1) as u32;
                    let result = ((sum + 7) >> 3) as i32;
                    let i = idx(x, y, z);
                    chunk.skylight[i] = curve[result.min(255) as usize];
                    *s.at(x, y, z) = (result - chunk.occupancy[i] as i32).max(0) as u8;
                }
            }
        }
        let mut d = [0u32; 5];
        for z in 0..n {
            for x in 0..n {
                let v = s.get(x, 0, z);
                update_fringe(&mut chunk.fringe_sky_y, (z * n + x) as usize, v, &mut d[0]);
            }
        }
        for y in 0..CHUNK_Y {
            for z in 0..n {
                let v0 = s.get(0, y, z);
                update_fringe(
                    &mut chunk.fringe_sky_x0,
                    (y * n + z) as usize,
                    v0,
                    &mut d[1],
                );
                let v1 = s.get(n - 1, y, z);
                update_fringe(
                    &mut chunk.fringe_sky_x1,
                    (y * n + z) as usize,
                    v1,
                    &mut d[2],
                );
            }
            for x in 0..n {
                let v0 = s.get(x, y, 0);
                update_fringe(
                    &mut chunk.fringe_sky_z0,
                    (y * n + x) as usize,
                    v0,
                    &mut d[3],
                );
                let v1 = s.get(x, y, n - 1);
                update_fringe(
                    &mut chunk.fringe_sky_z1,
                    (y * n + x) as usize,
                    v1,
                    &mut d[4],
                );
            }
        }
        let offsets = [
            IVec3::new(0, -1, 0),
            IVec3::new(-1, 0, 0),
            IVec3::new(1, 0, 0),
            IVec3::new(0, 0, -1),
            IVec3::new(0, 0, 1),
        ];
        for (k, o) in offsets.iter().enumerate() {
            if d[k] > SKYLIGHT_DIRTY {
                self.mark(index + *o, DIRTY_SKYLIGHT);
            }
        }
    }

    fn average(&self, chunk: &mut Chunk) {
        for y in 0..CHUNK_Y {
            let (mut g, mut sk, mut w) = (0u32, 0u32, 0u32);
            for z in 0..CHUNK_XZ {
                for x in 0..CHUNK_XZ {
                    let i = idx(x, y, z);
                    let weight = 255 - chunk.occupancy[i] as u32;
                    g += chunk.lighting[i][3] as u32 * weight;
                    sk += chunk.skylight[i] as u32 * weight;
                    w += weight;
                }
            }
            chunk.average_global[y as usize] = g;
            chunk.average_skylight[y as usize] = sk;
            chunk.average_weight[y as usize] = w;
        }
    }

    fn composite(&self, chunk: &Chunk) -> Vec<u8> {
        let mut out = Vec::with_capacity(CELLS * 4);
        let sky = self.sky_ambient;
        let plain = !self.shadows || sky == [0, 0, 0];
        for (i, l) in chunk.lighting.iter().enumerate() {
            if plain {
                out.extend_from_slice(l);
            } else {
                let s = chunk.skylight[i] as u32;
                let add = |c: u8, k: u8| {
                    let b = (s * k as u32 + 255) >> 8;
                    (c as u32 + b).min(255) as u8
                };
                out.extend_from_slice(&[
                    add(l[0], sky[0]),
                    add(l[1], sky[1]),
                    add(l[2], sky[2]),
                    l[3],
                ]);
            }
        }
        out
    }

    fn update_border(&mut self, focus: Vec3) {
        let corner = self.corner();
        let end = corner + CHUNK_COUNT;
        let cell_y = (focus.y / CELL).floor() as i32;
        let (mut g, mut s, mut w) = (0u64, 0u64, 0u64);
        for c in self.chunks.values() {
            let border = c.index.x == corner.x
                || c.index.z == corner.z
                || c.index.x == end.x - 1
                || c.index.z == end.z - 1;
            if !border {
                continue;
            }
            let local = cell_y - c.index.y * CHUNK_Y;
            for off in 1..=2 {
                let y = local + off;
                if (0..CHUNK_Y).contains(&y) {
                    g += c.average_global[y as usize] as u64;
                    s += c.average_skylight[y as usize] as u64;
                    w += c.average_weight[y as usize] as u64;
                }
            }
        }
        if w > 0 {
            self.border_global = g.div_ceil(w) as u32;
            self.border_skylight = s.div_ceil(w) as u32;
        }
    }

    pub fn wrapped(index: IVec3) -> IVec3 {
        IVec3::new(
            index.x.rem_euclid(CHUNK_COUNT.x),
            index.y.rem_euclid(CHUNK_COUNT.y),
            index.z.rem_euclid(CHUNK_COUNT.z),
        )
    }

    pub fn pending(&self) -> usize {
        self.chunks.values().filter(|c| c.dirty != 0).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contribution_sums_to_256() {
        for d in [
            Vec3::new(0.3, -1.0, 0.2),
            Vec3::new(-0.9, -0.2, 0.4),
            Vec3::new(0.0, -1.0, 0.0),
        ] {
            let c = light_contribution(d.normalize(), 8);
            assert_eq!(c.x + c.y + c.z, 256, "{d:?} {c:?}");
        }
    }

    #[test]
    fn block_casts_shadow_below() {
        let mut grid = LightGrid::new();
        let parts = [VoxelPart {
            shape: VoxelShape::Block,
            size: Vec3::new(40.0, 4.0, 40.0),
            cframe: CFrame::from_translation(Vec3::new(0.0, 40.0, 0.0)),
            transparency: 0.0,
        }];
        let terrain = |_: IVec3, _: &mut [u8]| {};
        let inputs = LightInputs {
            focus: Vec3::ZERO,
            parts: &parts,
            lights: &[],
            terrain: &terrain,
            terrain_generation: 0,
            light_direction: Vec3::new(0.0, -1.0, 0.0),
            sky_ambient: [128, 128, 128],
            shadows: true,
        };
        grid.update(&inputs, usize::MAX);
        let below = grid
            .chunks
            .get(&LightGrid::chunk_of(Vec3::new(0.0, 20.0, 0.0)))
            .unwrap();
        let cell = (Vec3::new(0.0, 20.0, 0.0) / CELL).floor();
        let local = IVec3::new(cell.x as i32, cell.y as i32, cell.z as i32)
            - IVec3::new(
                below.index.x * CHUNK_XZ,
                below.index.y * CHUNK_Y,
                below.index.z * CHUNK_XZ,
            );
        let i = idx(local.x, local.y, local.z);
        assert!(below.lighting[i][3] < 40, "{}", below.lighting[i][3]);
        let open = grid
            .chunks
            .get(&LightGrid::chunk_of(Vec3::new(200.0, 20.0, 200.0)))
            .unwrap();
        assert!(open.lighting.iter().all(|l| l[3] > 200));
    }
}
