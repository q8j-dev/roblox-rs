use crate::TerrainError;
use crate::changes::ChangeLog;
use rbx_math::{IVec3, Vec3};
use std::collections::HashMap;

pub const CHUNK_SIZE_LOG2: u32 = 5;
pub const CHUNK_SIZE: i32 = 1 << CHUNK_SIZE_LOG2;
const CHUNK_MIPS: usize = 4;

pub const MATERIAL_AIR: u8 = 0;
pub const MATERIAL_WATER: u8 = 1;
pub const MATERIAL_MAX: u8 = 63;
pub const OCCUPANCY_MAX: u8 = 255;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    material: u8,
    occupancy: u8,
}

impl Cell {
    pub const AIR: Cell = Cell {
        material: 0,
        occupancy: 0,
    };

    pub fn new(material: u8, occupancy: u8) -> Cell {
        Cell {
            material,
            occupancy: if material == MATERIAL_AIR {
                0
            } else {
                occupancy
            },
        }
    }

    pub fn material(&self) -> u8 {
        self.material
    }

    pub fn occupancy(&self) -> u8 {
        self.occupancy
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub begin: IVec3,
    pub end: IVec3,
}

impl Region {
    pub fn new(begin: IVec3, end: IVec3) -> Region {
        Region { begin, end }
    }

    pub fn cube(begin: IVec3, size: i32) -> Region {
        Region {
            begin,
            end: begin + IVec3::splat(size),
        }
    }

    pub fn from_chunk(id: IVec3, log2: u32) -> Region {
        Region::cube(id << log2 as i32, 1 << log2)
    }

    pub fn from_extents(min: Vec3, max: Vec3) -> Region {
        let vmin = min / crate::CELL_SIZE;
        let vmax = max / crate::CELL_SIZE;
        let b = IVec3::new(
            vmin.x.floor() as i32,
            vmin.y.floor() as i32,
            vmin.z.floor() as i32,
        );
        let e = IVec3::new(
            vmax.x.ceil() as i32,
            vmax.y.ceil() as i32,
            vmax.z.ceil() as i32,
        );
        Region::new(b, b.max(e))
    }

    pub fn size(&self) -> IVec3 {
        self.end - self.begin
    }

    pub fn is_empty(&self) -> bool {
        self.begin.x == self.end.x || self.begin.y == self.end.y || self.begin.z == self.end.z
    }

    pub fn aligned(&self, size: i32) -> bool {
        ((self.begin.x | self.begin.y | self.begin.z | self.end.x | self.end.y | self.end.z)
            & (size - 1))
            == 0
    }

    pub fn inside(&self, other: &Region) -> bool {
        let db = self.begin - other.begin;
        let de = other.end - self.end;
        (db.x | db.y | db.z | de.x | de.y | de.z) >= 0
    }

    pub fn intersect(&self, other: &Region) -> Region {
        let b = self.begin.max(other.begin);
        let e = self.end.min(other.end);
        Region::new(b, b.max(e))
    }

    pub fn expand(&self, size: i32) -> Region {
        Region::new(
            self.begin - IVec3::splat(size),
            self.end + IVec3::splat(size),
        )
    }

    pub fn expand_to_grid(&self, size: i32) -> Region {
        let mask = size - 1;
        Region::new(
            self.begin & IVec3::splat(!mask),
            (self.end + IVec3::splat(mask)) & IVec3::splat(!mask),
        )
    }

    pub fn offset(&self, offset: IVec3) -> Region {
        Region::new(self.begin + offset, self.end + offset)
    }

    pub fn downsample(&self, lod: u32) -> Region {
        let ar = self.expand_to_grid(1 << lod);
        Region::new(ar.begin >> lod as i32, ar.end >> lod as i32)
    }

    pub fn chunk_ids(&self, log2: u32) -> Vec<IVec3> {
        if self.is_empty() {
            return Vec::new();
        }
        let min = self.begin >> log2 as i32;
        let max = (self.end - IVec3::ONE) >> log2 as i32;
        let mut out = Vec::new();
        for z in min.z..=max.z {
            for y in min.y..=max.y {
                for x in min.x..=max.x {
                    out.push(IVec3::new(x, y, z));
                }
            }
        }
        out
    }

    pub fn chunk_count(&self, log2: u32) -> u64 {
        if self.is_empty() {
            return 0;
        }
        let min = self.begin >> log2 as i32;
        let max = (self.end - IVec3::ONE) >> log2 as i32;
        let d = (max - min + IVec3::ONE).as_i64vec3();
        (d.x * d.y * d.z) as u64
    }
}

#[derive(Clone, Debug, Default)]
pub struct CellBox {
    size: IVec3,
    data: Option<Vec<Cell>>,
}

impl CellBox {
    pub fn new(x: i32, y: i32, z: i32) -> CellBox {
        CellBox {
            size: IVec3::new(x, y, z),
            data: None,
        }
    }

    pub fn sized(size: IVec3) -> CellBox {
        CellBox::new(size.x, size.y, size.z)
    }

    fn index(&self, x: i32, y: i32, z: i32) -> usize {
        (x + self.size.x * z + self.size.x * self.size.z * y) as usize
    }

    pub fn get(&self, x: i32, y: i32, z: i32) -> Cell {
        match &self.data {
            Some(d) => d[self.index(x, y, z)],
            None => Cell::AIR,
        }
    }

    pub fn set(&mut self, x: i32, y: i32, z: i32, cell: Cell) {
        let i = self.index(x, y, z);
        self.alloc()[i] = cell;
    }

    fn alloc(&mut self) -> &mut Vec<Cell> {
        let n = (self.size.x * self.size.y * self.size.z) as usize;
        self.data.get_or_insert_with(|| vec![Cell::AIR; n])
    }

    pub fn row(&self, x: i32, y: i32, z: i32, len: i32) -> &[Cell] {
        let i = self.index(x, y, z);
        match &self.data {
            Some(d) => &d[i..i + len as usize],
            None => &[],
        }
    }

    pub fn row_mut(&mut self, x: i32, y: i32, z: i32, len: i32) -> &mut [Cell] {
        let i = self.index(x, y, z);
        &mut self.alloc()[i..i + len as usize]
    }

    pub fn size(&self) -> IVec3 {
        self.size
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_none()
    }
}

fn has_solid(row: &[Cell]) -> bool {
    row.iter().any(|c| c.material != MATERIAL_AIR)
}

fn box_has_solid(b: &CellBox) -> bool {
    match &b.data {
        Some(d) => has_solid(d),
        None => false,
    }
}

fn copy_cells(
    target: &mut CellBox,
    target_region: &Region,
    source: &CellBox,
    source_region: &Region,
) -> bool {
    let region = source_region.intersect(target_region);
    if region.is_empty() || (source.is_empty() && target.is_empty()) {
        return false;
    }
    let so = region.begin - source_region.begin;
    let to = region.begin - target_region.begin;
    let size = region.size();
    let mut dirty = false;
    for y in 0..size.y {
        for z in 0..size.z {
            if source.is_empty() {
                let row = target.row_mut(to.x, to.y + y, to.z + z, size.x);
                if dirty || has_solid(row) {
                    row.fill(Cell::AIR);
                    dirty = true;
                }
            } else {
                let src = source.row(so.x, so.y + y, so.z + z, size.x);
                if !target.is_empty() || has_solid(src) {
                    let row = target.row_mut(to.x, to.y + y, to.z + z, size.x);
                    if dirty || row != src {
                        row.copy_from_slice(src);
                        dirty = true;
                    }
                }
            }
        }
    }
    dirty
}

fn count_cells(b: &CellBox, lod: u32) -> u32 {
    match &b.data {
        Some(d) => d
            .iter()
            .filter(|c| c.material != MATERIAL_AIR)
            .map(|c| ((c.occupancy as u32 + 1) << (lod * 3)) >> 8)
            .sum(),
        None => 0,
    }
}

#[derive(Clone, Copy)]
struct Merged {
    material: u8,
    occupancy: u32,
}

impl Merged {
    fn of(c: Cell) -> Merged {
        Merged {
            material: c.material,
            occupancy: c.occupancy as u32,
        }
    }

    fn merge(a: Merged, b: Merged) -> Merged {
        if a.material == b.material {
            Merged {
                material: a.material,
                occupancy: a.occupancy + b.occupancy,
            }
        } else if a.occupancy != b.occupancy {
            if a.occupancy > b.occupancy { a } else { b }
        } else if a.material > b.material {
            a
        } else {
            b
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn downsample_cell(
    c000: Cell,
    c001: Cell,
    c010: Cell,
    c011: Cell,
    c100: Cell,
    c101: Cell,
    c110: Cell,
    c111: Cell,
) -> Cell {
    let occ = (c000.occupancy as u32
        + c001.occupancy as u32
        + c010.occupancy as u32
        + c011.occupancy as u32
        + c100.occupancy as u32
        + c101.occupancy as u32
        + c110.occupancy as u32
        + c111.occupancy as u32)
        .div_ceil(8);
    let m00 = Merged::merge(Merged::of(c000), Merged::of(c001));
    let m01 = Merged::merge(Merged::of(c010), Merged::of(c011));
    let m10 = Merged::merge(Merged::of(c100), Merged::of(c101));
    let m11 = Merged::merge(Merged::of(c110), Merged::of(c111));
    let m = Merged::merge(Merged::merge(m00, m01), Merged::merge(m10, m11));
    Cell::new(m.material, occ as u8)
}

fn downsample_cells(target: &mut CellBox, target_region: &Region, source: &CellBox) {
    if target_region.is_empty() || (source.is_empty() && target.is_empty()) {
        return;
    }
    let size = target_region.size();
    let so = target_region.begin * 2;
    let to = target_region.begin;
    for y in 0..size.y {
        for z in 0..size.z {
            if source.is_empty() {
                target
                    .row_mut(to.x, to.y + y, to.z + z, size.x)
                    .fill(Cell::AIR);
            } else {
                let w = size.x * 2;
                let r00: Vec<Cell> = source.row(so.x, so.y + y * 2, so.z + z * 2, w).to_vec();
                let r10: Vec<Cell> = source.row(so.x, so.y + y * 2 + 1, so.z + z * 2, w).to_vec();
                let r01: Vec<Cell> = source.row(so.x, so.y + y * 2, so.z + z * 2 + 1, w).to_vec();
                let r11: Vec<Cell> = source
                    .row(so.x, so.y + y * 2 + 1, so.z + z * 2 + 1, w)
                    .to_vec();
                let row = target.row_mut(to.x, to.y + y, to.z + z, size.x);
                for x in 0..size.x as usize {
                    row[x] = downsample_cell(
                        r00[x * 2],
                        r00[x * 2 + 1],
                        r10[x * 2],
                        r10[x * 2 + 1],
                        r01[x * 2],
                        r01[x * 2 + 1],
                        r11[x * 2],
                        r11[x * 2 + 1],
                    );
                }
            }
        }
    }
}

#[derive(Clone)]
struct Chunk {
    data: [CellBox; CHUNK_MIPS],
    volume: u32,
}

impl Chunk {
    fn new() -> Chunk {
        Chunk {
            data: std::array::from_fn(|mip| {
                let s = CHUNK_SIZE >> mip;
                CellBox::new(s, s, s)
            }),
            volume: 0,
        }
    }
}

#[derive(Clone, Default)]
pub struct SmoothGrid {
    chunks: HashMap<IVec3, Chunk>,
    volume: u32,
    pub changes: ChangeLog,
}

impl SmoothGrid {
    pub fn new() -> SmoothGrid {
        SmoothGrid::default()
    }

    pub fn is_allocated(&self) -> bool {
        !self.chunks.is_empty()
    }

    pub fn non_empty_cell_count_approx(&self) -> u32 {
        self.volume
    }

    pub fn read(&self, region: &Region, lod: u32) -> CellBox {
        if (lod as usize) < CHUNK_MIPS {
            let rl = region.downsample(lod);
            let mut result = CellBox::sized(rl.size());
            for cid in region.chunk_ids(CHUNK_SIZE_LOG2) {
                if let Some(chunk) = self.chunks.get(&cid) {
                    let cr = Region::from_chunk(cid, CHUNK_SIZE_LOG2 - lod);
                    copy_cells(&mut result, &rl, &chunk.data[lod as usize], &cr);
                }
            }
            result
        } else {
            let mut result = self.read(region, CHUNK_MIPS as u32 - 1);
            for _ in (CHUNK_MIPS as u32 - 1)..lod {
                let s = result.size() / 2;
                let mut next = CellBox::sized(s);
                downsample_cells(&mut next, &Region::new(IVec3::ZERO, s), &result);
                result = next;
            }
            result
        }
    }

    pub fn write(&mut self, region: &Region, b: &CellBox) {
        for cid in region.chunk_ids(CHUNK_SIZE_LOG2) {
            let exists = self.chunks.contains_key(&cid);
            if b.is_empty() && !exists {
                continue;
            }
            let cr = Region::from_chunk(cid, CHUNK_SIZE_LOG2);
            if b.is_empty() && cr.inside(region) {
                if let Some(c) = self.chunks.remove(&cid) {
                    self.volume -= c.volume;
                }
                self.changes.push(cr.begin, cr.end);
                continue;
            }
            let chunk = self.chunks.entry(cid).or_insert_with(Chunk::new);
            let dirty = copy_cells(&mut chunk.data[0], &cr, b, region);
            if dirty {
                let updated = region.intersect(&cr);
                self.changes.push(updated.begin, updated.end);
                let local = updated.offset(-cr.begin);
                for mip in 1..CHUNK_MIPS {
                    let mr = local.downsample(mip as u32);
                    let (lo, hi) = chunk.data.split_at_mut(mip);
                    downsample_cells(&mut hi[0], &mr, &lo[mip - 1]);
                }
                let cells = count_cells(&chunk.data[CHUNK_MIPS - 1], CHUNK_MIPS as u32 - 1);
                self.volume = self.volume - chunk.volume + cells;
                chunk.volume = cells;
            }
            if !box_has_solid(&chunk.data[CHUNK_MIPS - 1]) {
                let v = chunk.volume;
                self.volume -= v;
                self.chunks.remove(&cid);
            }
        }
    }

    pub fn get_cell(&self, x: i32, y: i32, z: i32) -> Cell {
        let p = IVec3::new(x, y, z);
        let id = p >> CHUNK_SIZE_LOG2 as i32;
        let o = id << CHUNK_SIZE_LOG2 as i32;
        match self.chunks.get(&id) {
            Some(c) => c.data[0].get(x - o.x, y - o.y, z - o.z),
            None => Cell::AIR,
        }
    }

    pub fn non_empty_regions(&self) -> Vec<Region> {
        let mut ids: Vec<IVec3> = self.chunks.keys().copied().collect();
        ids.sort_by_key(|v| (v.x, v.y, v.z));
        ids.into_iter()
            .map(|id| Region::from_chunk(id, CHUNK_SIZE_LOG2))
            .collect()
    }

    pub fn non_empty_regions_inside(&self, region: &Region) -> Vec<Region> {
        let mut out = Vec::new();
        if region.chunk_count(CHUNK_SIZE_LOG2) < self.chunks.len() as u64 * 2 {
            for cid in region.chunk_ids(CHUNK_SIZE_LOG2) {
                if self.chunks.contains_key(&cid) {
                    out.push(region.intersect(&Region::from_chunk(cid, CHUNK_SIZE_LOG2)));
                }
            }
        } else {
            for cr in self.non_empty_regions() {
                let r = region.intersect(&cr);
                if !r.is_empty() {
                    out.push(r);
                }
            }
        }
        out
    }

    pub fn clear(&mut self) {
        for r in self.non_empty_regions() {
            self.changes.push(r.begin, r.end);
        }
        self.chunks.clear();
        self.volume = 0;
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut out = vec![1u8, CHUNK_SIZE_LOG2 as u8];
        let mut ids: Vec<IVec3> = self
            .chunks
            .iter()
            .filter(|(_, c)| !c.data[0].is_empty())
            .map(|(k, _)| *k)
            .collect();
        ids.sort_by_key(|v| (v.x, v.y, v.z));
        let mut last = IVec3::ZERO;
        for id in ids {
            let d = id - last;
            for i in (0..4).rev() {
                out.push((d.x >> (i * 8)) as u8);
                out.push((d.y >> (i * 8)) as u8);
                out.push((d.z >> (i * 8)) as u8);
            }
            encode_chunk(&mut out, &self.chunks[&id].data[0]);
            last = id;
        }
        out
    }

    pub fn deserialize(&mut self, data: &[u8]) -> Result<(), TerrainError> {
        if data.is_empty() {
            return Ok(());
        }
        let mut off = 0usize;
        let version = read_u8(data, &mut off)? as i8;
        if version != 1 {
            return Err(TerrainError::Decode("unsupported version".into()));
        }
        let log2 = read_u8(data, &mut off)? as u32;
        if log2 > 8 {
            return Err(TerrainError::Decode("malformed chunk size".into()));
        }
        let size = 1i32 << log2;
        let mut last = IVec3::ZERO;
        let mut cells = Vec::new();
        while off < data.len() {
            for i in (0..4).rev() {
                last.x = last
                    .x
                    .wrapping_add(((read_u8(data, &mut off)? as u32) << (i * 8)) as i32);
                last.y = last
                    .y
                    .wrapping_add(((read_u8(data, &mut off)? as u32) << (i * 8)) as i32);
                last.z = last
                    .z
                    .wrapping_add(((read_u8(data, &mut off)? as u32) << (i * 8)) as i32);
            }
            let mut b = CellBox::new(size, size, size);
            decode_chunk(data, &mut off, &mut b, &mut cells)?;
            self.write(&Region::cube(last << log2 as i32, size), &b);
        }
        Ok(())
    }
}

fn read_u8(data: &[u8], off: &mut usize) -> Result<u8, TerrainError> {
    let v = *data
        .get(*off)
        .ok_or_else(|| TerrainError::Decode(format!("unexpected end at offset {}", *off)))?;
    *off += 1;
    Ok(v)
}

fn encode_run(out: &mut Vec<u8>, cell: Cell, count: u32) {
    let need_count = count != 1;
    let need_occ = cell.material != MATERIAL_AIR && cell.occupancy != OCCUPANCY_MAX;
    out.push(cell.material | ((need_occ as u8) << 6) | ((need_count as u8) << 7));
    if need_occ {
        out.push(cell.occupancy);
    }
    if need_count {
        out.push((count - 1) as u8);
    }
}

fn encode_chunk(out: &mut Vec<u8>, b: &CellBox) {
    let s = b.size();
    let n = (s.x * s.y * s.z) as usize;
    let air = vec![Cell::AIR; n];
    let cells: &[Cell] = b.data.as_deref().unwrap_or(&air);
    let mut last = Cell::AIR;
    let mut count = 0u32;
    for &c in cells {
        if count < 256 && (c == last || count == 0) {
            last = c;
            count += 1;
        } else {
            encode_run(out, last, count);
            last = c;
            count = 1;
        }
    }
    if count > 0 {
        encode_run(out, last, count);
    }
}

fn decode_chunk(
    data: &[u8],
    off: &mut usize,
    result: &mut CellBox,
    cells: &mut Vec<Cell>,
) -> Result<(), TerrainError> {
    let s = result.size();
    let n = (s.x * s.y * s.z) as usize;
    cells.clear();
    cells.resize(n, Cell::AIR);
    let mut o = 0usize;
    while o < n {
        let meta = read_u8(data, off)?;
        let occ = if meta & 0x40 != 0 {
            read_u8(data, off)?
        } else {
            OCCUPANCY_MAX
        };
        let count = if meta & 0x80 != 0 {
            read_u8(data, off)? as usize + 1
        } else {
            1
        };
        if o + count > n {
            return Err(TerrainError::Decode(format!(
                "chunk overflow at {} cells",
                o + count
            )));
        }
        let cell = Cell::new(meta & 0x3f, occ);
        cells[o..o + count].fill(cell);
        o += count;
    }
    if cells.iter().any(|c| c.material != MATERIAL_AIR) {
        result.alloc().copy_from_slice(cells);
    } else {
        result.data = None;
    }
    Ok(())
}
