use crate::lightgrid::{CHUNK_XZ, CHUNK_Y};
use rbx_math::IVec3;
use rbx_terrain::legacy;
use rbx_terrain::smooth::{MATERIAL_WATER, Region};

const BLOCK_TABLE: [u8; 8] = [255, 128, 42, 213, 128, 0, 0, 0];

pub fn fill(terrain: Option<&rbx_dom::TerrainData>, chunk: IVec3, out: &mut [u8]) {
    let Some(t) = terrain else {
        return;
    };
    let base = IVec3::new(chunk.x * CHUNK_XZ, chunk.y * CHUNK_Y, chunk.z * CHUNK_XZ);
    let index = |x: i32, y: i32, z: i32| ((y * CHUNK_XZ + z) * CHUNK_XZ + x) as usize;
    if let Some(grid) = t.smooth.as_ref().filter(|g| g.is_allocated()) {
        let region = Region::new(base, base + IVec3::new(CHUNK_XZ, CHUNK_Y, CHUNK_XZ));
        let cells = grid.read(&region, 0);
        if !cells.is_empty() {
            for y in 0..CHUNK_Y {
                for z in 0..CHUNK_XZ {
                    for x in 0..CHUNK_XZ {
                        let c = cells.get(x, y, z);
                        if c.material() > MATERIAL_WATER {
                            let o = &mut out[index(x, y, z)];
                            *o = o.saturating_add(c.occupancy());
                        }
                    }
                }
            }
        }
    }
    if let Some(grid) = t.legacy.as_ref().filter(|g| g.non_empty_cell_count() > 0) {
        let lo = base;
        let hi = base + IVec3::new(CHUNK_XZ - 1, CHUNK_Y - 1, CHUNK_XZ - 1);
        if grid.non_empty_chunks_in_region(lo, hi).is_empty() {
            return;
        }
        for y in 0..CHUNK_Y {
            for z in 0..CHUNK_XZ {
                for x in 0..CHUNK_XZ {
                    let cell = grid.get_cell(base + IVec3::new(x, y, z));
                    if legacy::is_empty(cell) {
                        continue;
                    }
                    let v = BLOCK_TABLE[(legacy::block(cell) & 7) as usize];
                    let o = &mut out[index(x, y, z)];
                    *o = o.saturating_add(v);
                }
            }
        }
    }
}
