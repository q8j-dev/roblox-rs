use crate::TerrainError;
use crate::changes::ChangeLog;
use rbx_math::{IVec3, Vec3};
use std::collections::HashMap;
use std::sync::LazyLock;

pub const CHUNK_X: i32 = 32;
pub const CHUNK_Y: i32 = 16;
pub const CHUNK_Z: i32 = 32;
const CHUNK_SHIFT: IVec3 = IVec3::new(5, 4, 5);
const CHUNK_MASK: IVec3 = IVec3::new(31, 15, 31);
pub const CHUNK_CELLS: usize = (CHUNK_X * CHUNK_Y * CHUNK_Z) as usize;
pub const EXTENT_RADIUS: i32 = 32000;

pub const MATERIAL_EMPTY: u8 = 0;
pub const MATERIAL_WATER: u8 = 17;
pub const MATERIAL_UNSPECIFIED: u8 = 255;
pub const MAX_MATERIALS: u8 = 18;

pub const BLOCK_SOLID: u8 = 0;
pub const BLOCK_VERTICAL_WEDGE: u8 = 1;
pub const BLOCK_CORNER_WEDGE: u8 = 2;
pub const BLOCK_INVERSE_CORNER_WEDGE: u8 = 3;
pub const BLOCK_HORIZONTAL_WEDGE: u8 = 4;
pub const BLOCK_EMPTY: u8 = 5;

pub const FORCE_NONE: u8 = 0;
pub const DIRECTION_NEG_Y: u8 = 2;
pub const MAX_FORCES: u8 = 5;
pub const MAX_DIRECTIONS: u8 = 6;

pub const EMPTY_CELL: u8 = BLOCK_EMPTY << 3;
pub const WATER_ON_WEDGE_CELL: u8 = water_cell(FORCE_NONE, DIRECTION_NEG_Y);

pub const fn block(cell: u8) -> u8 {
    (cell >> 3) & 7
}

pub const fn orientation(cell: u8) -> u8 {
    cell >> 6
}

pub const fn solid_cell(block: u8, orientation: u8) -> u8 {
    ((block & 7) << 3) | ((orientation & 3) << 6)
}

pub const fn water_cell(force: u8, direction: u8) -> u8 {
    let raw = force as u32 * MAX_DIRECTIONS as u32 + direction as u32 + 1;
    ((raw & 7) as u8) | (BLOCK_EMPTY << 3) | ((((raw >> 3) & 3) as u8) << 6)
}

fn water_data(cell: u8) -> u32 {
    ((((cell >> 6) as u32) << 3) | (cell & 7) as u32).wrapping_sub(1)
}

pub fn water_force(cell: u8) -> u8 {
    ((water_data(cell) / MAX_DIRECTIONS as u32) % MAX_FORCES as u32) as u8
}

pub fn water_direction(cell: u8) -> u8 {
    (water_data(cell) % MAX_DIRECTIONS as u32) as u8
}

pub const fn is_empty(cell: u8) -> bool {
    cell == EMPTY_CELL
}

pub const fn is_explicit_water(cell: u8) -> bool {
    !is_empty(cell) && block(cell) == BLOCK_EMPTY
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    PlusX = 0,
    PlusZ = 1,
    MinusX = 2,
    MinusZ = 3,
    PlusY = 4,
    MinusY = 5,
}

pub const SKIP_TOP_RIGHT: u8 = 0;
pub const SKIP_TOP_LEFT: u8 = 1;
pub const SKIP_BOTTOM_LEFT: u8 = 2;
pub const SKIP_BOTTOM_RIGHT: u8 = 3;
pub const SKIP_ALL: u8 = 4;
pub const SKIP_NONE: u8 = 5;

const UNORIENTED_FACES: [[u8; 6]; 6] = [
    [SKIP_NONE; 6],
    [
        SKIP_TOP_LEFT,
        SKIP_ALL,
        SKIP_TOP_RIGHT,
        SKIP_NONE,
        SKIP_ALL,
        SKIP_NONE,
    ],
    [
        SKIP_TOP_LEFT,
        SKIP_ALL,
        SKIP_ALL,
        SKIP_TOP_RIGHT,
        SKIP_ALL,
        SKIP_TOP_LEFT,
    ],
    [
        SKIP_NONE,
        SKIP_TOP_LEFT,
        SKIP_TOP_RIGHT,
        SKIP_NONE,
        SKIP_BOTTOM_LEFT,
        SKIP_NONE,
    ],
    [
        SKIP_NONE,
        SKIP_ALL,
        SKIP_ALL,
        SKIP_NONE,
        SKIP_BOTTOM_LEFT,
        SKIP_TOP_LEFT,
    ],
    [SKIP_ALL; 6],
];

fn rotate_corner(corner: u8, orient: u8) -> u8 {
    if corner < 4 {
        (corner + orient) % 4
    } else {
        corner
    }
}

fn compute_oriented_face(wedge: &[u8; 6], f: usize, orient: u8) -> u8 {
    if f < 4 {
        return wedge[(f + orient as usize) % 4];
    }
    let basis = wedge[f];
    if basis == SKIP_NONE || basis == SKIP_ALL {
        return basis;
    }
    let o = if f == Face::MinusY as usize {
        (4 - orient) % 4
    } else {
        orient
    };
    rotate_corner(basis, o)
}

static ORIENTED_FACES: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let mut map = vec![0u8; 256 * 6];
    for i in 0..8u8 {
        let wedge = if i >= 6 {
            &UNORIENTED_FACES[0]
        } else {
            &UNORIENTED_FACES[i as usize]
        };
        for j in 0..8u8 {
            for k in 0..4u8 {
                for l in 0..6 {
                    let index = solid_cell(i, k) | j;
                    map[index as usize * 6 + l] = compute_oriented_face(wedge, l, k);
                }
            }
        }
    }
    map
});

pub fn oriented_face(cell: u8, f: Face) -> u8 {
    ORIENTED_FACES[cell as usize * 6 + f as usize]
}

pub fn is_wedge_side_not_full(cell: u8, f: Face) -> bool {
    oriented_face(cell, f) != SKIP_NONE
}

const OPPOSITE_FACE: [Face; 6] = [
    Face::MinusX,
    Face::MinusZ,
    Face::PlusX,
    Face::PlusZ,
    Face::MinusY,
    Face::PlusY,
];
const PRIMARY_FACE: [Face; 4] = [Face::PlusZ, Face::PlusX, Face::MinusZ, Face::MinusX];
const SECONDARY_FACE: [Face; 4] = [Face::MinusX, Face::PlusZ, Face::PlusX, Face::MinusZ];
const PRIMARY_OFFSET: [IVec3; 4] = [
    IVec3::new(0, 0, 1),
    IVec3::new(1, 0, 0),
    IVec3::new(0, 0, -1),
    IVec3::new(-1, 0, 0),
];
const SECONDARY_OFFSET: [IVec3; 4] = [
    IVec3::new(-1, 0, 0),
    IVec3::new(0, 0, 1),
    IVec3::new(1, 0, 0),
    IVec3::new(0, 0, -1),
];

pub fn chunk_of(pos: IVec3) -> IVec3 {
    pos >> CHUNK_SHIFT
}

pub fn chunk_min(id: IVec3) -> IVec3 {
    id << CHUNK_SHIFT
}

pub fn chunk_max(id: IVec3) -> IVec3 {
    chunk_min(id) + CHUNK_MASK
}

fn index_of(pos: IVec3) -> usize {
    let r = pos & CHUNK_MASK;
    (r.x + r.z * CHUNK_X + r.y * CHUNK_X * CHUNK_Z) as usize
}

fn in_extents(pos: IVec3) -> bool {
    pos.cmpge(IVec3::splat(-EXTENT_RADIUS)).all() && pos.cmple(IVec3::splat(EXTENT_RADIUS)).all()
}

fn read_material(materials: &[u8], index: usize, cell: u8) -> u8 {
    if block(cell) == BLOCK_EMPTY {
        MATERIAL_WATER
    } else {
        ((materials[index >> 1] >> (4 * (index & 1))) & 0x0f) + 1
    }
}

fn write_material(materials: &mut [u8], index: usize, material: u8) {
    let shift = 4 * (index & 1);
    let mask = 0x0fu8 << shift;
    let byte = &mut materials[index >> 1];
    *byte &= !mask;
    *byte |= (material.wrapping_sub(1) << shift) & mask;
}

#[derive(Clone)]
pub struct LegacyChunk {
    cells: Box<[u8]>,
    materials: Box<[u8]>,
    non_empty: u32,
}

impl LegacyChunk {
    fn new() -> LegacyChunk {
        LegacyChunk {
            cells: vec![EMPTY_CELL; CHUNK_CELLS].into_boxed_slice(),
            materials: vec![0u8; CHUNK_CELLS / 2].into_boxed_slice(),
            non_empty: 0,
        }
    }

    pub fn cell_at(&self, pos: IVec3) -> u8 {
        self.cells[index_of(pos)]
    }

    pub fn material_at(&self, pos: IVec3) -> u8 {
        let i = index_of(pos);
        read_material(&self.materials, i, self.cells[i])
    }
}

#[derive(Clone, Default)]
pub struct LegacyGrid {
    chunks: HashMap<IVec3, LegacyChunk>,
    non_empty: u32,
    pub changes: ChangeLog,
}

impl LegacyGrid {
    pub fn new() -> LegacyGrid {
        LegacyGrid::default()
    }

    pub fn non_empty_cell_count(&self) -> u32 {
        self.non_empty
    }

    pub fn is_allocated(&self) -> bool {
        self.non_empty > 0
    }

    pub fn chunk(&self, id: IVec3) -> Option<&LegacyChunk> {
        self.chunks.get(&id)
    }

    pub fn get_cell(&self, pos: IVec3) -> u8 {
        match self.chunks.get(&chunk_of(pos)) {
            Some(c) => c.cells[index_of(pos)],
            None => EMPTY_CELL,
        }
    }

    pub fn get_material(&self, pos: IVec3) -> u8 {
        match self.chunks.get(&chunk_of(pos)) {
            Some(c) => c.material_at(pos),
            None => MATERIAL_WATER,
        }
    }

    pub fn set_cell(&mut self, pos: IVec3, cell: u8, material: u8) {
        if !in_extents(pos) {
            return;
        }
        let id = chunk_of(pos);
        let index = index_of(pos);
        let (prev_cell, prev_material) = match self.chunks.get(&id) {
            Some(c) => (
                c.cells[index],
                read_material(&c.materials, index, c.cells[index]),
            ),
            None => (EMPTY_CELL, MATERIAL_WATER),
        };
        let new_material = if material == MATERIAL_UNSPECIFIED {
            prev_material
        } else {
            material
        };
        if prev_cell == cell && prev_material == new_material {
            return;
        }
        let chunk = self.chunks.entry(id).or_insert_with(LegacyChunk::new);
        chunk.cells[index] = cell;
        write_material(&mut chunk.materials, index, new_material);
        let delta = is_empty(prev_cell) as i32 - is_empty(cell) as i32;
        chunk.non_empty = (chunk.non_empty as i32 + delta) as u32;
        self.non_empty = (self.non_empty as i32 + delta) as u32;
        if chunk.non_empty == 0 {
            self.chunks.remove(&id);
        }
        self.changes.push(pos - IVec3::ONE, pos + IVec3::ONE);
    }

    pub fn get_water_cell(&self, pos: IVec3) -> u8 {
        let cell = self.get_cell(pos);
        if self.cell_has_water(cell, pos) {
            if block(cell) == BLOCK_EMPTY {
                cell
            } else {
                WATER_ON_WEDGE_CELL
            }
        } else {
            EMPTY_CELL
        }
    }

    pub fn cell_has_water(&self, cell: u8, pos: IVec3) -> bool {
        !is_empty(cell)
            && block(cell) != BLOCK_SOLID
            && (block(cell) == BLOCK_EMPTY || self.is_water_on_wedge(cell, pos))
    }

    fn is_water_on_wedge(&self, center: u8, pos: IVec3) -> bool {
        let b = block(center);
        if b == BLOCK_EMPTY || b == BLOCK_SOLID {
            return false;
        }
        let o = orientation(center) as usize;
        let above = self.get_cell(pos + IVec3::Y);
        if is_explicit_water(above) {
            return true;
        }
        let primary = self.get_cell(pos + PRIMARY_OFFSET[o]);
        if b == BLOCK_VERTICAL_WEDGE {
            return is_explicit_water(primary);
        }
        let primary_dir = PRIMARY_FACE[o];
        let primary_water = is_explicit_water(primary);
        let primary_partial = !is_empty(primary)
            && is_wedge_side_not_full(primary, OPPOSITE_FACE[primary_dir as usize]);
        let secondary = self.get_cell(pos + SECONDARY_OFFSET[o]);
        let secondary_dir = SECONDARY_FACE[o];
        let secondary_water = is_explicit_water(secondary);
        let secondary_partial = !is_empty(secondary)
            && is_wedge_side_not_full(secondary, OPPOSITE_FACE[secondary_dir as usize]);
        let diagonal_offset = PRIMARY_OFFSET[o] + SECONDARY_OFFSET[o];
        let diagonal = self.get_cell(pos + diagonal_offset);
        let diagonal_water = is_explicit_water(diagonal);
        let inverse_case = b == BLOCK_INVERSE_CORNER_WEDGE
            && !is_empty(primary)
            && !is_empty(secondary)
            && !is_empty(diagonal)
            && is_explicit_water(self.get_cell(pos + diagonal_offset + IVec3::Y));
        let both_water = primary_water && secondary_water;
        let both_support =
            (primary_water || primary_partial) && (secondary_water || secondary_partial);
        inverse_case || (diagonal_water && both_support) || both_water
    }

    pub fn non_empty_chunks(&self) -> Vec<IVec3> {
        let mut ids: Vec<IVec3> = self.chunks.keys().copied().collect();
        ids.sort_by_key(|v| (v.x, v.y, v.z));
        ids
    }

    pub fn non_empty_chunks_in_region(&self, min: IVec3, max: IVec3) -> Vec<IVec3> {
        if min.cmpgt(max).any() {
            return Vec::new();
        }
        let lo = chunk_of(min);
        let hi = chunk_of(max);
        let span = (hi - lo + IVec3::ONE).as_i64vec3();
        let total = span.x * span.y * span.z;
        let mut result = Vec::new();
        if total < self.chunks.len() as i64 * 2 {
            for y in lo.y..=hi.y {
                for z in lo.z..=hi.z {
                    for x in lo.x..=hi.x {
                        let id = IVec3::new(x, y, z);
                        if self.chunks.contains_key(&id) {
                            result.push(id);
                        }
                    }
                }
            }
        } else {
            for id in self.non_empty_chunks() {
                if id.cmpge(lo).all() && id.cmple(hi).all() {
                    result.push(id);
                }
            }
        }
        result
    }

    pub fn clear(&mut self) {
        let ids = self.non_empty_chunks();
        self.chunks.clear();
        self.non_empty = 0;
        for id in ids {
            self.changes
                .push(chunk_min(id) - IVec3::ONE, chunk_max(id) + IVec3::ONE);
        }
    }

    pub fn decode_v1(&mut self, data: &[u8]) -> Result<(), TerrainError> {
        if data.is_empty() {
            return Ok(());
        }
        let mut r = Reader::new(data);
        for i in 0..16 {
            for j in 0..4 {
                for k in 0..16 {
                    let chunk = IVec3::new(i - 8, j, k - 8);
                    self.decode_chunk_v1(chunk, &mut r)?;
                    if r.eof() {
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    }

    fn decode_chunk_v1(&mut self, chunk: IVec3, r: &mut Reader) -> Result<(), TerrainError> {
        let base = IVec3::new(chunk.x * CHUNK_X, chunk.y * CHUNK_Y, chunk.z * CHUNK_Z);
        let mut idx = IVec3::ZERO;
        while idx.y < CHUNK_Y {
            let value = r.u8()?;
            let count = r.count()?;
            for _ in 0..count {
                let pos = base + idx;
                let deprecated_material = value & 7;
                if deprecated_material == MATERIAL_EMPTY {
                    self.set_cell(pos, EMPTY_CELL, MATERIAL_WATER);
                } else {
                    self.set_cell(
                        pos,
                        solid_cell(block(value), orientation(value)),
                        deprecated_material,
                    );
                }
                idx.x += 1;
                if idx.x >= CHUNK_X {
                    idx.x = 0;
                    idx.z += 1;
                }
                if idx.z >= CHUNK_Z {
                    idx.z = 0;
                    idx.y += 1;
                }
                if idx.y >= CHUNK_Y {
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    pub fn decode_v2(&mut self, data: &[u8]) -> Result<(), TerrainError> {
        if data.is_empty() {
            return Ok(());
        }
        let mut r = Reader::new(data);
        for i in 0..16 {
            for j in 0..4 {
                for k in 0..16 {
                    self.decode_chunk(IVec3::new(i - 8, j, k - 8), &mut r)?;
                    if r.eof() {
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    }

    pub fn decode_v3(&mut self, data: &[u8]) -> Result<(), TerrainError> {
        if data.is_empty() {
            return Ok(());
        }
        let mut r = Reader::new(data);
        loop {
            let x = r.i16()?;
            let y = r.i16()?;
            let z = r.i16()?;
            self.decode_chunk(IVec3::new(x as i32, y as i32, z as i32), &mut r)?;
            if r.eof() {
                return Ok(());
            }
        }
    }

    fn read_runs(r: &mut Reader, out: &mut [u8]) -> Result<(), TerrainError> {
        let mut total = 0usize;
        while total < out.len() {
            let value = r.u8()?;
            let count = (r.count()? as usize).min(out.len() - total);
            out[total..total + count].fill(value);
            total += count;
        }
        Ok(())
    }

    fn decode_chunk(&mut self, chunk: IVec3, r: &mut Reader) -> Result<(), TerrainError> {
        let mut cells = vec![0u8; CHUNK_CELLS];
        let mut materials = vec![0u8; CHUNK_CELLS];
        Self::read_runs(r, &mut cells)?;
        Self::read_runs(r, &mut materials)?;
        let start = chunk_min(chunk);
        let was_empty = !self.chunks.contains_key(&chunk);
        let mut offset = 0;
        for y in 0..CHUNK_Y {
            for z in 0..CHUNK_Z {
                for x in 0..CHUNK_X {
                    let cell = cells[offset];
                    if !(was_empty && is_empty(cell)) {
                        self.set_cell(start + IVec3::new(x, y, z), cell, materials[offset]);
                    }
                    offset += 1;
                }
            }
        }
        Ok(())
    }

    pub fn encode_v3(&self) -> Vec<u8> {
        let mut out = Vec::new();
        if self.non_empty == 0 {
            return out;
        }
        for id in self.non_empty_chunks() {
            out.extend_from_slice(&(id.x as i16).to_le_bytes());
            out.extend_from_slice(&(id.y as i16).to_le_bytes());
            out.extend_from_slice(&(id.z as i16).to_le_bytes());
            let chunk = &self.chunks[&id];
            write_runs(&mut out, chunk.cells.iter().copied());
            write_runs(
                &mut out,
                (0..CHUNK_CELLS).map(|i| read_material(&chunk.materials, i, chunk.cells[i])),
            );
        }
        out
    }

    #[allow(clippy::needless_range_loop)]
    pub fn autowedge_cell(&mut self, pos: IVec3) -> bool {
        let current = self.get_cell(pos);
        let material = self.get_material(pos);
        if block(current) == BLOCK_EMPTY {
            return false;
        }
        let solid = |g: &LegacyGrid, p: IVec3| block(g.get_cell(p)) != BLOCK_EMPTY;
        let block_below = solid(self, pos - IVec3::Y);
        let mut s = [[[false; 3]; 2]; 3];
        for (j, layer) in [0, 1].into_iter().enumerate() {
            for k in 0..3 {
                for i in 0..3 {
                    s[i][j][k] = solid(
                        self,
                        IVec3::new(pos.x - 1 + i as i32, pos.y + layer, pos.z - 1 + k as i32),
                    );
                }
            }
        }
        let mut sides = [false; 4];
        let mut corners = [false; 4];
        let mut sides_above = [false; 4];
        let mut corners_above = [false; 4];
        for step in [0usize, 2] {
            sides[step] = s[step][0][1];
            sides[step + 1] = s[1][0][2 - step];
            sides_above[step] = s[step][1][1];
            sides_above[step + 1] = s[1][1][2 - step];
            corners[step] = s[step][0][step];
            corners[step + 1] = s[step][0][2 - step];
            corners_above[step] = s[step][1][step];
            corners_above[step + 1] = s[step][1][2 - step];
        }
        let adjacent_sides = sides.iter().filter(|b| **b).count();
        let adjacent_corners = corners.iter().filter(|b| **b).count();
        let above = s[1][1][1];
        let set = |g: &mut LegacyGrid, b: u8, o: usize| {
            g.set_cell(pos, solid_cell(b, o as u8), material);
        };
        if !above {
            if adjacent_sides == 1 && block_below {
                for n in 0..4 {
                    if sides[n] {
                        set(self, 1, (n + 1) % 4);
                        return true;
                    }
                }
            } else if adjacent_sides == 3 {
                for n in 0..4 {
                    if sides[n]
                        && corners[(n + 1) % 4]
                        && sides[(n + 1) % 4]
                        && corners[(n + 2) % 4]
                        && sides[(n + 2) % 4]
                    {
                        set(self, 1, (n + 2) % 4);
                        return true;
                    }
                }
            }
            for n in 0..4 {
                if sides[n]
                    && corners[(n + 1) % 4]
                    && sides[(n + 1) % 4]
                    && (adjacent_sides == 2
                        || (adjacent_sides == 3
                            && (corners[(n + 3) % 4]
                                || (sides[(n + 2) % 4] && corners[(n + 2) % 4])
                                || (sides[(n + 3) % 4] && corners[n]))))
                {
                    set(self, 2, (n + 2) % 4);
                    return true;
                }
            }
        }
        if adjacent_sides == 3 && above {
            if adjacent_corners > 1 {
                for n in 0..4usize {
                    if (!corners[n] || !corners_above[n])
                        && (!sides[(n + 3) % 4] || !sides[n])
                        && (!sides_above[n]
                            && sides_above[(n + 1) % 4]
                            && sides_above[(n + 2) % 4]
                            && !sides_above[(n + 3) % 4])
                    {
                        set(self, 3, (n + 3) % 4);
                        return true;
                    }
                }
            }
        } else if adjacent_sides == 4 && adjacent_corners == 3 {
            for n in 0..4 {
                if !corners[n]
                    && (!above
                        || (!sides_above[n]
                            && sides_above[(n + 1) % 4]
                            && corners_above[(n + 2) % 4]
                            && sides_above[(n + 2) % 4]
                            && !sides_above[(n + 3) % 4]))
                {
                    set(self, 3, (n + 3) % 4);
                    return true;
                }
            }
        }
        if adjacent_sides == 2 && adjacent_corners < 4 {
            for n in 0..4 {
                if !sides[n]
                    && !sides[(n + 1) % 4]
                    && (!above
                        || (!sides_above[n]
                            && !sides_above[(n + 1) % 4]
                            && sides_above[(n + 2) % 4]
                            && sides_above[(n + 3) % 4]))
                {
                    set(self, 4, n);
                    return true;
                }
            }
        }
        set(self, 0, 0);
        false
    }

    pub fn autowedge_cells(&mut self, min: IVec3, max: IVec3) {
        for id in self.non_empty_chunks_in_region(min, max) {
            let lo = chunk_min(id).max(min);
            let hi = chunk_max(id).min(max);
            for y in lo.y..=hi.y {
                for z in lo.z..=hi.z {
                    for x in lo.x..=hi.x {
                        self.autowedge_cell(IVec3::new(x, y, z));
                    }
                }
            }
        }
    }
}

fn write_runs(out: &mut Vec<u8>, values: impl Iterator<Item = u8>) {
    let mut value = 0u8;
    let mut count = 0u32;
    for v in values {
        if value != v && count != 0 {
            out.push(value);
            write_count(out, count);
            count = 0;
        }
        value = v;
        count += 1;
    }
    out.push(value);
    write_count(out, count);
}

fn write_count(out: &mut Vec<u8>, count: u32) {
    if count < 255 {
        out.push(count as u8);
    } else {
        out.push(255);
        out.push((count >> 8) as u8);
        out.push((count & 0xff) as u8);
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Reader<'a> {
        Reader { data, pos: 0 }
    }

    fn eof(&self) -> bool {
        self.pos >= self.data.len()
    }

    fn u8(&mut self) -> Result<u8, TerrainError> {
        let v = *self
            .data
            .get(self.pos)
            .ok_or_else(|| TerrainError::Decode("Reading past end of string".into()))?;
        self.pos += 1;
        Ok(v)
    }

    fn count(&mut self) -> Result<u32, TerrainError> {
        let c = self.u8()?;
        if c < 255 {
            return Ok(c as u32);
        }
        let hi = self.u8()? as u32;
        let lo = self.u8()? as u32;
        Ok((hi << 8) | lo)
    }

    fn i16(&mut self) -> Result<i16, TerrainError> {
        let a = self.u8()?;
        let b = self.u8()?;
        Ok(i16::from_le_bytes([a, b]))
    }
}

pub fn world_to_cell_floor(p: Vec3) -> IVec3 {
    IVec3::new(
        (p.x / 4.0).floor() as i32,
        (p.y / 4.0).floor() as i32,
        (p.z / 4.0).floor() as i32,
    )
}

pub fn cell_corner(c: IVec3) -> Vec3 {
    c.as_vec3() * 4.0
}

pub fn cell_center(c: IVec3) -> Vec3 {
    cell_corner(c) + Vec3::splat(2.0)
}
