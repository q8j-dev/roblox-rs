pub mod changes;
pub mod conversion;
pub mod legacy;
pub mod materials;
pub mod mesher;
pub mod region;
pub mod smooth;

pub use changes::ChangeLog;
pub use legacy::LegacyGrid;
pub use materials::{MaterialTable, material_table};
pub use smooth::SmoothGrid;

#[derive(Debug, thiserror::Error)]
pub enum TerrainError {
    #[error("Error while decoding data: {0}")]
    Decode(String),
}

pub const CELL_SIZE: f32 = 4.0;
