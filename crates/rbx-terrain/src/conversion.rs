use crate::legacy::{self, LegacyGrid};
use crate::smooth::{self, Cell, CellBox, Region, SmoothGrid};
use rbx_math::IVec3;

pub const PLASTIC: u32 = 0x0100;
pub const WOODPLANKS: u32 = 0x0210;
pub const SLATE: u32 = 0x0320;
pub const CONCRETE: u32 = 0x0330;
pub const BRICK: u32 = 0x0350;
pub const ROCK: u32 = 0x0380;
pub const SANDSTONE: u32 = 0x0390;
pub const BASALT: u32 = 0x0314;
pub const CRACKED_LAVA: u32 = 0x0324;
pub const GRASS: u32 = 0x0500;
pub const SAND: u32 = 0x0510;
pub const SNOW: u32 = 0x0530;
pub const MUD: u32 = 0x0540;
pub const GROUND: u32 = 0x0550;
pub const GLACIER: u32 = 0x0610;
pub const AIR: u32 = 0x0700;
pub const WATER: u32 = 0x0800;

pub const OCCUPANCY_SOLID: u8 = 255;
pub const OCCUPANCY_WEDGE: u8 = 127;
pub const OCCUPANCY_CORNER: u8 = 85;
pub const OCCUPANCY_INVERSE_CORNER: u8 = 170;

pub const MATERIAL_TABLE: [u32; 16] = [
    AIR,
    WATER,
    GRASS,
    SLATE,
    CONCRETE,
    BRICK,
    SAND,
    WOODPLANKS,
    ROCK,
    GLACIER,
    SNOW,
    SANDSTONE,
    MUD,
    BASALT,
    GROUND,
    CRACKED_LAVA,
];
pub const MATERIAL_DEFAULT: u8 = 2;

pub fn occupancy_from_block(block: u8) -> u8 {
    match block {
        legacy::BLOCK_SOLID => OCCUPANCY_SOLID,
        legacy::BLOCK_VERTICAL_WEDGE | legacy::BLOCK_HORIZONTAL_WEDGE => OCCUPANCY_WEDGE,
        legacy::BLOCK_CORNER_WEDGE => OCCUPANCY_CORNER,
        legacy::BLOCK_INVERSE_CORNER_WEDGE => OCCUPANCY_INVERSE_CORNER,
        _ => 0,
    }
}

pub fn block_from_cell(cell: Cell) -> u8 {
    let rounder = 255 / 6;
    let o = cell.occupancy() as i32;
    if cell.material() == smooth::MATERIAL_AIR || o < OCCUPANCY_CORNER as i32 - rounder {
        legacy::BLOCK_EMPTY
    } else if o < OCCUPANCY_WEDGE as i32 - rounder {
        legacy::BLOCK_CORNER_WEDGE
    } else if o < OCCUPANCY_INVERSE_CORNER as i32 - rounder {
        legacy::BLOCK_VERTICAL_WEDGE
    } else if o < OCCUPANCY_SOLID as i32 - rounder {
        legacy::BLOCK_INVERSE_CORNER_WEDGE
    } else {
        legacy::BLOCK_SOLID
    }
}

pub fn material_from_voxel(material: u8) -> u32 {
    MATERIAL_TABLE
        .get(material as usize)
        .copied()
        .unwrap_or(MATERIAL_TABLE[MATERIAL_DEFAULT as usize])
}

pub fn voxel_from_material(material: u32) -> u8 {
    MATERIAL_TABLE
        .iter()
        .position(|m| *m == material)
        .map_or(MATERIAL_DEFAULT, |i| i as u8)
}

pub fn material_from_cell_material(material: u8) -> u32 {
    match material {
        0 => AIR,
        1 => GRASS,
        2 => SAND,
        3 => BRICK,
        4 => SLATE,
        5 => CONCRETE,
        9 | 10 => WOODPLANKS,
        11 => SLATE,
        12 => CONCRETE,
        13 => SLATE,
        14 => CONCRETE,
        17 => WATER,
        _ => MATERIAL_TABLE[MATERIAL_DEFAULT as usize],
    }
}

pub fn cell_material_from_material(material: u32) -> u8 {
    match material {
        AIR => 0,
        WATER => 17,
        GRASS => 1,
        SLATE => 13,
        CONCRETE => 14,
        BRICK => 3,
        SAND => 2,
        WOODPLANKS => 9,
        _ => 1,
    }
}

pub fn convert_to_smooth(old: &LegacyGrid, grid: &mut SmoothGrid) {
    for id in old.non_empty_chunks() {
        let min = legacy::chunk_min(id);
        let Some(chunk) = old.chunk(id) else { continue };
        let mut b = CellBox::new(legacy::CHUNK_X, legacy::CHUNK_Y, legacy::CHUNK_Z);
        for y in 0..legacy::CHUNK_Y {
            for z in 0..legacy::CHUNK_Z {
                for x in 0..legacy::CHUNK_X {
                    let p = min + IVec3::new(x, y, z);
                    let c = chunk.cell_at(p);
                    if legacy::is_empty(c) {
                        continue;
                    }
                    let cell = if legacy::is_explicit_water(c) {
                        Cell::new(smooth::MATERIAL_WATER, smooth::OCCUPANCY_MAX)
                    } else {
                        Cell::new(
                            voxel_from_material(material_from_cell_material(chunk.material_at(p))),
                            occupancy_from_block(legacy::block(c)),
                        )
                    };
                    b.set(x, y, z, cell);
                }
            }
        }
        grid.write(&Region::new(min, legacy::chunk_max(id) + IVec3::ONE), &b);
    }
}
