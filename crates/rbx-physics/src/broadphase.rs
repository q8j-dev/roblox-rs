use parry3d::bounding_volume::{Aabb, BoundingVolume};
use std::collections::HashMap;

const MAX_CELLS_PER_AXIS: i64 = 48;

pub struct Grid {
    cell: f32,
    cells: HashMap<(i32, i32, i32), Vec<usize>>,
    large: Vec<usize>,
    stamp: Vec<u32>,
    epoch: u32,
}

impl Grid {
    pub fn new(cell: f32) -> Grid {
        Grid {
            cell,
            cells: HashMap::new(),
            large: Vec::new(),
            stamp: Vec::new(),
            epoch: 0,
        }
    }

    fn range(&self, aabb: &Aabb) -> Option<([i32; 3], [i32; 3])> {
        let mut lo = [0i32; 3];
        let mut hi = [0i32; 3];
        for k in 0..3 {
            let a = aabb.mins[k];
            let b = aabb.maxs[k];
            if !a.is_finite() || !b.is_finite() {
                return None;
            }
            let l = (a / self.cell).floor() as i64;
            let h = (b / self.cell).floor() as i64;
            if h - l > MAX_CELLS_PER_AXIS {
                return None;
            }
            lo[k] = l.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
            hi[k] = h.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        }
        Some((lo, hi))
    }

    pub fn clear(&mut self) {
        self.cells.clear();
        self.large.clear();
    }

    pub fn insert(&mut self, idx: usize, aabb: &Aabb) {
        if idx >= self.stamp.len() {
            self.stamp.resize(idx + 1, 0);
        }
        match self.range(aabb) {
            Some((lo, hi)) => {
                for x in lo[0]..=hi[0] {
                    for y in lo[1]..=hi[1] {
                        for z in lo[2]..=hi[2] {
                            self.cells.entry((x, y, z)).or_default().push(idx);
                        }
                    }
                }
            }
            None => self.large.push(idx),
        }
    }

    pub fn query(&mut self, aabb: &Aabb, aabbs: &[Aabb], out: &mut Vec<usize>) {
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.stamp.fill(0);
            self.epoch = 1;
        }
        let epoch = self.epoch;
        let mut consider = |i: usize, stamp: &mut Vec<u32>| {
            if i < stamp.len() && stamp[i] != epoch {
                stamp[i] = epoch;
                if i < aabbs.len() && aabbs[i].intersects(aabb) {
                    out.push(i);
                }
            }
        };
        for &i in &self.large {
            consider(i, &mut self.stamp);
        }
        match self.range(aabb) {
            Some((lo, hi)) => {
                for x in lo[0]..=hi[0] {
                    for y in lo[1]..=hi[1] {
                        for z in lo[2]..=hi[2] {
                            if let Some(list) = self.cells.get(&(x, y, z)) {
                                for &i in list {
                                    consider(i, &mut self.stamp);
                                }
                            }
                        }
                    }
                }
            }
            None => {
                let all: Vec<usize> = self.cells.values().flatten().copied().collect();
                for i in all {
                    consider(i, &mut self.stamp);
                }
            }
        }
    }
}

pub fn sweep_pairs(items: &[(usize, Aabb)], out: &mut Vec<(usize, usize)>) {
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by(|&a, &b| items[a].1.mins.x.total_cmp(&items[b].1.mins.x));
    for (k, &i) in order.iter().enumerate() {
        let (ia, aa) = &items[i];
        for &j in &order[k + 1..] {
            let (ib, ab) = &items[j];
            if ab.mins.x > aa.maxs.x {
                break;
            }
            if aa.intersects(ab) {
                out.push((*ia, *ib));
            }
        }
    }
}
