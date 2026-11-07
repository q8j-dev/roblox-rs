use crate::legacy::{self, LegacyGrid};
use crate::smooth::{Cell, CellBox, MATERIAL_AIR, Region, SmoothGrid};
use rbx_math::IVec3;
use std::collections::HashSet;

fn copy_empty_legacy(
    target: &mut LegacyGrid,
    min: IVec3,
    max: IVec3,
    offset: IVec3,
    source_chunks: &[IVec3],
) {
    let set: HashSet<IVec3> = source_chunks.iter().copied().collect();
    let tmin = min + offset;
    let tmax = max + offset;
    for id in target.non_empty_chunks_in_region(tmin, tmax) {
        let lo = legacy::chunk_min(id).max(tmin);
        let hi = legacy::chunk_max(id).min(tmax);
        if offset == IVec3::ZERO {
            if set.contains(&id) {
                continue;
            }
            for y in lo.y..=hi.y {
                for z in lo.z..=hi.z {
                    for x in lo.x..=hi.x {
                        target.set_cell(
                            IVec3::new(x, y, z),
                            legacy::EMPTY_CELL,
                            legacy::MATERIAL_WATER,
                        );
                    }
                }
            }
        } else {
            for y in lo.y..=hi.y {
                for z in lo.z..=hi.z {
                    for x in lo.x..=hi.x {
                        let p = IVec3::new(x, y, z);
                        if !set.contains(&legacy::chunk_of(p - offset)) {
                            target.set_cell(p, legacy::EMPTY_CELL, legacy::MATERIAL_WATER);
                        }
                    }
                }
            }
        }
    }
}

pub fn copy_legacy(
    target: &mut LegacyGrid,
    source: &LegacyGrid,
    min: IVec3,
    max: IVec3,
    offset: IVec3,
    copy_empty: bool,
) {
    let chunks = source.non_empty_chunks_in_region(min, max);
    if copy_empty {
        copy_empty_legacy(target, min, max, offset, &chunks);
    }
    for id in chunks {
        let lo = legacy::chunk_min(id).max(min);
        let hi = legacy::chunk_max(id).min(max);
        let Some(chunk) = source.chunk(id) else {
            continue;
        };
        for y in lo.y..=hi.y {
            for z in lo.z..=hi.z {
                for x in lo.x..=hi.x {
                    let p = IVec3::new(x, y, z);
                    let cell = chunk.cell_at(p);
                    if copy_empty || !legacy::is_empty(cell) {
                        target.set_cell(p + offset, cell, chunk.material_at(p));
                    }
                }
            }
        }
    }
}

pub fn copy_smooth(
    target: &mut SmoothGrid,
    source: &SmoothGrid,
    min: IVec3,
    max: IVec3,
    offset: IVec3,
    copy_empty: bool,
) {
    let source_region = Region::new(min, max + IVec3::ONE);
    let regions = source.non_empty_regions_inside(&source_region);
    if copy_empty && target.is_allocated() {
        let set: HashSet<(IVec3, IVec3)> = regions.iter().map(|r| (r.begin, r.end)).collect();
        for tr in target.non_empty_regions_inside(&source_region.offset(offset)) {
            let sr = tr.offset(-offset);
            if !set.contains(&(sr.begin, sr.end)) {
                target.write(&tr, &CellBox::sized(tr.size()));
            }
        }
    }
    for sr in regions {
        let tr = sr.offset(offset);
        let mut sb = source.read(&sr, 0);
        if !copy_empty && target.is_allocated() {
            let tb = target.read(&tr, 0);
            if !tb.is_empty() {
                let s = tb.size();
                for y in 0..s.y {
                    for z in 0..s.z {
                        for x in 0..s.x {
                            if sb.get(x, y, z).material() == MATERIAL_AIR {
                                let t: Cell = tb.get(x, y, z);
                                sb.set(x, y, z, t);
                            }
                        }
                    }
                }
            }
        }
        target.write(&tr, &sb);
    }
}
