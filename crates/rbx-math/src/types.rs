use crate::cframe::NormalId;
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct UDim {
    pub scale: f32,
    pub offset: i32,
}

impl UDim {
    pub fn new(scale: f32, offset: i32) -> UDim {
        UDim { scale, offset }
    }

    pub fn resolve(&self, size: f32) -> f32 {
        self.scale * size + self.offset as f32
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct UDim2 {
    pub x: UDim,
    pub y: UDim,
}

impl UDim2 {
    pub fn new(xs: f32, xo: i32, ys: f32, yo: i32) -> UDim2 {
        UDim2 {
            x: UDim::new(xs, xo),
            y: UDim::new(ys, yo),
        }
    }

    pub fn resolve(&self, size: Vec2) -> Vec2 {
        Vec2::new(self.x.resolve(size.x), self.y.resolve(size.y))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Ray {
    pub origin: Vec3,
    pub direction: Vec3,
}

impl Ray {
    pub fn new(origin: Vec3, direction: Vec3) -> Ray {
        Ray { origin, direction }
    }

    pub fn unit(&self) -> Ray {
        Ray::new(self.origin, self.direction.normalize_or_zero())
    }

    pub fn closest_point(&self, p: Vec3) -> Vec3 {
        let d = self.direction.normalize_or_zero();
        let t = (p - self.origin).dot(d).max(0.0);
        self.origin + d * t
    }

    pub fn distance(&self, p: Vec3) -> f32 {
        (self.closest_point(p) - p).length()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Rect2D {
    pub min: Vec2,
    pub max: Vec2,
}

impl Rect2D {
    pub fn xyxy(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect2D {
        Rect2D {
            min: Vec2::new(x0, y0),
            max: Vec2::new(x1, y1),
        }
    }

    pub fn width(&self) -> f32 {
        self.max.x - self.min.x
    }

    pub fn height(&self) -> f32 {
        self.max.y - self.min.y
    }

    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.min.x && p.y >= self.min.y && p.x <= self.max.x && p.y <= self.max.y
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Vector2int16 {
    pub x: i16,
    pub y: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Vector3int16 {
    pub x: i16,
    pub y: i16,
    pub z: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Vector3int32 {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl Vector3int16 {
    pub fn new(x: i16, y: i16, z: i16) -> Vector3int16 {
        Vector3int16 { x, y, z }
    }
}

impl Vector3int32 {
    pub fn new(x: i32, y: i32, z: i32) -> Vector3int32 {
        Vector3int32 { x, y, z }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Region3int16 {
    pub min: Vector3int16,
    pub max: Vector3int16,
}

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Region3 {
    pub min: Vec3,
    pub max: Vec3,
}

impl Region3 {
    pub fn new(a: Vec3, b: Vec3) -> Region3 {
        Region3 {
            min: a.min(b),
            max: a.max(b),
        }
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn size(&self) -> Vec3 {
        self.max - self.min
    }

    pub fn expand_to_grid(&self, resolution: f32) -> Region3 {
        let r = resolution.max(f32::EPSILON);
        Region3 {
            min: (self.min / r).floor() * r,
            max: (self.max / r).ceil() * r,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Faces(pub u8);

impl Faces {
    pub fn has(&self, id: NormalId) -> bool {
        self.0 & (1 << id as u8) != 0
    }

    pub fn with(self, id: NormalId, on: bool) -> Faces {
        if on {
            Faces(self.0 | (1 << id as u8))
        } else {
            Faces(self.0 & !(1 << id as u8))
        }
    }

    pub fn all() -> Faces {
        Faces(0x3f)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Axes(pub u8);

impl Axes {
    pub fn x(&self) -> bool {
        self.0 & 1 != 0
    }

    pub fn y(&self) -> bool {
        self.0 & 2 != 0
    }

    pub fn z(&self) -> bool {
        self.0 & 4 != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct PhysicalProperties {
    pub custom: Option<CustomPhysicalProperties>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CustomPhysicalProperties {
    pub density: f32,
    pub friction: f32,
    pub elasticity: f32,
    pub friction_weight: f32,
    pub elasticity_weight: f32,
}

impl PhysicalProperties {
    pub fn custom(
        density: f32,
        friction: f32,
        elasticity: f32,
        friction_weight: f32,
        elasticity_weight: f32,
    ) -> PhysicalProperties {
        PhysicalProperties {
            custom: Some(CustomPhysicalProperties {
                density,
                friction,
                elasticity,
                friction_weight,
                elasticity_weight,
            }),
        }
    }
}
