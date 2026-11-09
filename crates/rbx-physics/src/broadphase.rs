use parry3d::bounding_volume::{Aabb, BoundingVolume};
use rbx_dom::InstanceId;
use rbx_math::Vec3;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

const MAX_CELLS_PER_AXIS: i64 = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Placement {
    Cells([i32; 3], [i32; 3]),
    Large,
}

struct Entry {
    aabb: Aabb,
    placement: Placement,
}

pub struct SpatialIndex {
    cell: f32,
    cells: HashMap<(i32, i32, i32), Vec<InstanceId>>,
    large: HashSet<InstanceId>,
    entries: HashMap<InstanceId, Entry>,
}

impl SpatialIndex {
    pub fn new(cell: f32) -> SpatialIndex {
        SpatialIndex {
            cell,
            cells: HashMap::default(),
            large: HashSet::default(),
            entries: HashMap::default(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn placement(&self, aabb: &Aabb) -> Placement {
        let mut lo = [0i32; 3];
        let mut hi = [0i32; 3];
        for k in 0..3 {
            let a = aabb.mins[k];
            let b = aabb.maxs[k];
            if !a.is_finite() || !b.is_finite() {
                return Placement::Large;
            }
            let l = (a / self.cell).floor() as i64;
            let h = (b / self.cell).floor() as i64;
            if h - l > MAX_CELLS_PER_AXIS {
                return Placement::Large;
            }
            lo[k] = l.clamp(i32::MIN as i64 + 1, i32::MAX as i64 - 1) as i32;
            hi[k] = h.clamp(i32::MIN as i64 + 1, i32::MAX as i64 - 1) as i32;
        }
        Placement::Cells(lo, hi)
    }

    fn unplace(&mut self, id: InstanceId, p: Placement) {
        match p {
            Placement::Large => {
                self.large.remove(&id);
            }
            Placement::Cells(lo, hi) => {
                for x in lo[0]..=hi[0] {
                    for y in lo[1]..=hi[1] {
                        for z in lo[2]..=hi[2] {
                            if let Some(v) = self.cells.get_mut(&(x, y, z)) {
                                if let Some(i) = v.iter().position(|e| *e == id) {
                                    v.swap_remove(i);
                                }
                                if v.is_empty() {
                                    self.cells.remove(&(x, y, z));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn place(&mut self, id: InstanceId, p: Placement) {
        match p {
            Placement::Large => {
                self.large.insert(id);
            }
            Placement::Cells(lo, hi) => {
                for x in lo[0]..=hi[0] {
                    for y in lo[1]..=hi[1] {
                        for z in lo[2]..=hi[2] {
                            self.cells.entry((x, y, z)).or_default().push(id);
                        }
                    }
                }
            }
        }
    }

    pub fn update(&mut self, id: InstanceId, aabb: Aabb) {
        let p = self.placement(&aabb);
        match self.entries.get_mut(&id) {
            Some(e) => {
                e.aabb = aabb;
                if e.placement != p {
                    let old = e.placement;
                    e.placement = p;
                    self.unplace(id, old);
                    self.place(id, p);
                }
            }
            None => {
                self.entries.insert(id, Entry { aabb, placement: p });
                self.place(id, p);
            }
        }
    }

    pub fn remove(&mut self, id: InstanceId) {
        if let Some(e) = self.entries.remove(&id) {
            self.unplace(id, e.placement);
        }
    }

    pub fn aabb(&self, id: InstanceId) -> Option<Aabb> {
        self.entries.get(&id).map(|e| e.aabb)
    }

    pub fn query(&self, aabb: &Aabb, out: &mut Vec<InstanceId>) {
        let start = out.len();
        let push = |id: InstanceId, out: &mut Vec<InstanceId>| {
            if let Some(e) = self.entries.get(&id)
                && e.aabb.intersects(aabb)
            {
                out.push(id);
            }
        };
        for &id in &self.large {
            push(id, out);
        }
        match self.placement(aabb) {
            Placement::Cells(lo, hi) => {
                for x in lo[0]..=hi[0] {
                    for y in lo[1]..=hi[1] {
                        for z in lo[2]..=hi[2] {
                            if let Some(v) = self.cells.get(&(x, y, z)) {
                                for &id in v {
                                    push(id, out);
                                }
                            }
                        }
                    }
                }
            }
            Placement::Large => {
                for (&id, e) in &self.entries {
                    if e.aabb.intersects(aabb) {
                        out.push(id);
                    }
                }
            }
        }
        let found = &mut out[start..];
        found.sort_unstable();
        let mut n = 0;
        for i in 0..found.len() {
            if i == 0 || found[i] != found[n - 1] {
                found[n] = found[i];
                n += 1;
            }
        }
        out.truncate(start + n);
    }

    pub fn query_ray(&self, origin: Vec3, dir: Vec3, max: f32, out: &mut Vec<InstanceId>) {
        let mut seen: HashSet<InstanceId> = HashSet::default();
        for &id in &self.large {
            if seen.insert(id) {
                out.push(id);
            }
        }
        if !origin.is_finite() || !dir.is_finite() || !max.is_finite() {
            return;
        }
        let inv = |d: f32| {
            if d.abs() < 1e-12 {
                f32::INFINITY
            } else {
                1.0 / d
            }
        };
        let mut cell = [
            (origin.x / self.cell).floor() as i64,
            (origin.y / self.cell).floor() as i64,
            (origin.z / self.cell).floor() as i64,
        ];
        let step = [
            dir.x.signum() as i64,
            dir.y.signum() as i64,
            dir.z.signum() as i64,
        ];
        let mut t_max = [0.0f32; 3];
        let mut t_delta = [0.0f32; 3];
        for k in 0..3 {
            let d = dir[k];
            let id = inv(d);
            t_delta[k] = (self.cell * id).abs();
            let boundary = if d > 0.0 {
                (cell[k] + 1) as f32 * self.cell
            } else {
                cell[k] as f32 * self.cell
            };
            t_max[k] = if d.abs() < 1e-12 {
                f32::INFINITY
            } else {
                (boundary - origin[k]) * id
            };
        }
        let max_steps = 4096;
        for _ in 0..max_steps {
            let key = (cell[0] as i32, cell[1] as i32, cell[2] as i32);
            if let Some(v) = self.cells.get(&key) {
                for &id in v {
                    if seen.insert(id) {
                        out.push(id);
                    }
                }
            }
            let k = if t_max[0] < t_max[1] {
                if t_max[0] < t_max[2] { 0 } else { 2 }
            } else if t_max[1] < t_max[2] {
                1
            } else {
                2
            };
            if t_max[k] > max {
                break;
            }
            cell[k] += step[k];
            t_max[k] += t_delta[k];
        }
    }
}
