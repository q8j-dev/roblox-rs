mod brick_color;
mod brick_color_table;
mod cframe;
mod color;
mod sequence;
mod types;

pub use brick_color::BrickColor;
pub use brick_color_table::BrickColorInfo;
pub use cframe::{CFrame, NormalId, normal_id_to_vector, vector_to_normal_id};
pub use color::{Color3, Color3uint8};
pub use glam::{IVec3, Mat3, Quat, Vec2, Vec3, Vec4};
pub use sequence::{
    ColorSequence, ColorSequenceKeypoint, NumberRange, NumberSequence, NumberSequenceKeypoint,
};
pub use types::{
    Axes, CustomPhysicalProperties, Faces, PhysicalProperties, Ray, Rect2D, Region3, Region3int16,
    UDim, UDim2, Vector2int16, Vector3int16, Vector3int32,
};

pub const EPSILON: f32 = 1e-5;
