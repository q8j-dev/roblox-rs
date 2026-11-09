use rbx_math::{
    Axes, BrickColor, CFrame, Color3, ColorSequence, Faces, NumberRange, NumberSequence,
    PhysicalProperties, Ray, Rect2D, Region3, Region3int16, UDim, UDim2, Vec2, Vec3, Vector2int16,
    Vector3int16, Vector3int32,
};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct InstanceId(pub NonZeroU64);

impl InstanceId {
    pub fn from_raw(raw: u64) -> Option<InstanceId> {
        NonZeroU64::new(raw).map(InstanceId)
    }

    pub fn raw(&self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FontFace {
    pub family: String,
    pub weight: u16,
    pub style: u8,
    pub cached_face_id: String,
}

impl Default for FontFace {
    fn default() -> FontFace {
        FontFace {
            family: "rbxasset://fonts/families/SourceSansPro.json".to_string(),
            weight: 400,
            style: 0,
            cached_face_id: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UniqueId {
    pub index: u32,
    pub time: u32,
    pub random: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VariantType {
    Nil,
    Bool,
    Int,
    Int64,
    Float,
    Double,
    String,
    ProtectedString,
    BinaryString,
    Content,
    Vector2,
    Vector3,
    Vector2int16,
    Vector3int16,
    Vector3int32,
    CFrame,
    Color3,
    BrickColor,
    UDim,
    UDim2,
    Ray,
    Rect2D,
    Region3,
    Region3int16,
    Faces,
    Axes,
    PhysicalProperties,
    NumberSequence,
    ColorSequence,
    NumberRange,
    Enum,
    Ref,
    SystemAddress,
    Font,
    UniqueId,
    SecurityCapabilities,
    OptionalCFrame,
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub enum Variant {
    #[default]
    Nil,
    Bool(bool),
    Int(i32),
    Int64(i64),
    Float(f32),
    Double(f64),
    String(String),
    ProtectedString(String),
    BinaryString(Vec<u8>),
    Content(String),
    Vector2(Vec2),
    Vector3(Vec3),
    Vector2int16(Vector2int16),
    Vector3int16(Vector3int16),
    Vector3int32(Vector3int32),
    CFrame(CFrame),
    Color3(Color3),
    BrickColor(BrickColor),
    UDim(UDim),
    UDim2(UDim2),
    Ray(Ray),
    Rect2D(Rect2D),
    Region3(Region3),
    Region3int16(Region3int16),
    Faces(Faces),
    Axes(Axes),
    PhysicalProperties(PhysicalProperties),
    NumberSequence(NumberSequence),
    ColorSequence(ColorSequence),
    NumberRange(NumberRange),
    Enum(u32),
    Ref(Option<InstanceId>),
    SystemAddress(String),
    Font(FontFace),
    UniqueId(UniqueId),
    SecurityCapabilities(u64),
    OptionalCFrame(Option<CFrame>),
}

impl Variant {
    pub fn ty(&self) -> VariantType {
        match self {
            Variant::Nil => VariantType::Nil,
            Variant::Bool(_) => VariantType::Bool,
            Variant::Int(_) => VariantType::Int,
            Variant::Int64(_) => VariantType::Int64,
            Variant::Float(_) => VariantType::Float,
            Variant::Double(_) => VariantType::Double,
            Variant::String(_) => VariantType::String,
            Variant::ProtectedString(_) => VariantType::ProtectedString,
            Variant::BinaryString(_) => VariantType::BinaryString,
            Variant::Content(_) => VariantType::Content,
            Variant::Vector2(_) => VariantType::Vector2,
            Variant::Vector3(_) => VariantType::Vector3,
            Variant::Vector2int16(_) => VariantType::Vector2int16,
            Variant::Vector3int16(_) => VariantType::Vector3int16,
            Variant::Vector3int32(_) => VariantType::Vector3int32,
            Variant::CFrame(_) => VariantType::CFrame,
            Variant::Color3(_) => VariantType::Color3,
            Variant::BrickColor(_) => VariantType::BrickColor,
            Variant::UDim(_) => VariantType::UDim,
            Variant::UDim2(_) => VariantType::UDim2,
            Variant::Ray(_) => VariantType::Ray,
            Variant::Rect2D(_) => VariantType::Rect2D,
            Variant::Region3(_) => VariantType::Region3,
            Variant::Region3int16(_) => VariantType::Region3int16,
            Variant::Faces(_) => VariantType::Faces,
            Variant::Axes(_) => VariantType::Axes,
            Variant::PhysicalProperties(_) => VariantType::PhysicalProperties,
            Variant::NumberSequence(_) => VariantType::NumberSequence,
            Variant::ColorSequence(_) => VariantType::ColorSequence,
            Variant::NumberRange(_) => VariantType::NumberRange,
            Variant::Enum(_) => VariantType::Enum,
            Variant::Ref(_) => VariantType::Ref,
            Variant::SystemAddress(_) => VariantType::SystemAddress,
            Variant::Font(_) => VariantType::Font,
            Variant::UniqueId(_) => VariantType::UniqueId,
            Variant::SecurityCapabilities(_) => VariantType::SecurityCapabilities,
            Variant::OptionalCFrame(_) => VariantType::OptionalCFrame,
        }
    }

    pub fn default_for(ty: VariantType) -> Variant {
        match ty {
            VariantType::Nil => Variant::Nil,
            VariantType::Bool => Variant::Bool(false),
            VariantType::Int => Variant::Int(0),
            VariantType::Int64 => Variant::Int64(0),
            VariantType::Float => Variant::Float(0.0),
            VariantType::Double => Variant::Double(0.0),
            VariantType::String => Variant::String(String::new()),
            VariantType::ProtectedString => Variant::ProtectedString(String::new()),
            VariantType::BinaryString => Variant::BinaryString(Vec::new()),
            VariantType::Content => Variant::Content(String::new()),
            VariantType::Vector2 => Variant::Vector2(Vec2::ZERO),
            VariantType::Vector3 => Variant::Vector3(Vec3::ZERO),
            VariantType::Vector2int16 => Variant::Vector2int16(Vector2int16::default()),
            VariantType::Vector3int16 => Variant::Vector3int16(Vector3int16::default()),
            VariantType::Vector3int32 => Variant::Vector3int32(Vector3int32::default()),
            VariantType::CFrame => Variant::CFrame(CFrame::IDENTITY),
            VariantType::Color3 => Variant::Color3(Color3::default()),
            VariantType::BrickColor => Variant::BrickColor(BrickColor::default()),
            VariantType::UDim => Variant::UDim(UDim::default()),
            VariantType::UDim2 => Variant::UDim2(UDim2::default()),
            VariantType::Ray => Variant::Ray(Ray::default()),
            VariantType::Rect2D => Variant::Rect2D(Rect2D::default()),
            VariantType::Region3 => Variant::Region3(Region3::default()),
            VariantType::Region3int16 => Variant::Region3int16(Region3int16::default()),
            VariantType::Faces => Variant::Faces(Faces::default()),
            VariantType::Axes => Variant::Axes(Axes::default()),
            VariantType::PhysicalProperties => {
                Variant::PhysicalProperties(PhysicalProperties::default())
            }
            VariantType::NumberSequence => Variant::NumberSequence(NumberSequence::default()),
            VariantType::ColorSequence => Variant::ColorSequence(ColorSequence::default()),
            VariantType::NumberRange => Variant::NumberRange(NumberRange::default()),
            VariantType::Enum => Variant::Enum(0),
            VariantType::Ref => Variant::Ref(None),
            VariantType::SystemAddress => Variant::SystemAddress(String::new()),
            VariantType::Font => Variant::Font(FontFace::default()),
            VariantType::UniqueId => Variant::UniqueId(UniqueId::default()),
            VariantType::SecurityCapabilities => Variant::SecurityCapabilities(0),
            VariantType::OptionalCFrame => Variant::OptionalCFrame(None),
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Variant::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Variant::Float(v) => Some(*v),
            Variant::Double(v) => Some(*v as f32),
            Variant::Int(v) => Some(*v as f32),
            Variant::Int64(v) => Some(*v as f32),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Variant::Float(v) => Some(*v as f64),
            Variant::Double(v) => Some(*v),
            Variant::Int(v) => Some(*v as f64),
            Variant::Int64(v) => Some(*v as f64),
            _ => None,
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Variant::Int(v) => Some(*v),
            Variant::Int64(v) => Some(*v as i32),
            Variant::Float(v) => Some(*v as i32),
            Variant::Double(v) => Some(*v as i32),
            Variant::Enum(v) => Some(*v as i32),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Variant::String(s)
            | Variant::ProtectedString(s)
            | Variant::Content(s)
            | Variant::SystemAddress(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_vec3(&self) -> Option<Vec3> {
        match self {
            Variant::Vector3(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_cframe(&self) -> Option<CFrame> {
        match self {
            Variant::CFrame(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_color3(&self) -> Option<Color3> {
        match self {
            Variant::Color3(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_brick_color(&self) -> Option<BrickColor> {
        match self {
            Variant::BrickColor(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_enum(&self) -> Option<u32> {
        match self {
            Variant::Enum(v) => Some(*v),
            Variant::Int(v) if *v >= 0 => Some(*v as u32),
            _ => None,
        }
    }

    pub fn as_ref(&self) -> Option<Option<InstanceId>> {
        match self {
            Variant::Ref(v) => Some(*v),
            Variant::Nil => Some(None),
            _ => None,
        }
    }

    pub fn coerce(self, ty: VariantType) -> Option<Variant> {
        if self.ty() == ty {
            return Some(self);
        }
        Some(match (self, ty) {
            (Variant::Int(v), VariantType::Float) => Variant::Float(v as f32),
            (Variant::Int(v), VariantType::Double) => Variant::Double(v as f64),
            (Variant::Int(v), VariantType::Int64) => Variant::Int64(v as i64),
            (Variant::Int(v), VariantType::BrickColor) => {
                Variant::BrickColor(BrickColor::new(v as i64))
            }
            (Variant::Int(v), VariantType::Enum) if v >= 0 => Variant::Enum(v as u32),
            (Variant::Int64(v), VariantType::Int) => Variant::Int(v as i32),
            (Variant::Int64(v), VariantType::Double) => Variant::Double(v as f64),
            (Variant::Int64(v), VariantType::Float) => Variant::Float(v as f32),
            (Variant::Float(v), VariantType::Double) => Variant::Double(v as f64),
            (Variant::Float(v), VariantType::Int) => Variant::Int(v as i32),
            (Variant::Double(v), VariantType::Float) => Variant::Float(v as f32),
            (Variant::Double(v), VariantType::Int) => Variant::Int(v as i32),
            (Variant::Double(v), VariantType::Int64) => Variant::Int64(v as i64),
            (Variant::Enum(v), VariantType::Int) => Variant::Int(v as i32),
            (Variant::String(s), VariantType::ProtectedString) => Variant::ProtectedString(s),
            (Variant::String(s), VariantType::Content) => Variant::Content(s),
            (Variant::String(s), VariantType::BinaryString) => {
                Variant::BinaryString(s.into_bytes())
            }
            (Variant::ProtectedString(s), VariantType::String) => Variant::String(s),
            (Variant::Content(s), VariantType::String) => Variant::String(s),
            (Variant::BinaryString(b), VariantType::String) => match String::from_utf8(b) {
                Ok(s) => Variant::String(s),
                Err(e) => Variant::BinaryString(e.into_bytes()),
            },
            (Variant::Nil, VariantType::Ref) => Variant::Ref(None),
            (Variant::BrickColor(b), VariantType::Int) => Variant::Int(b.number() as i32),
            (Variant::Color3(c), VariantType::BrickColor) => {
                Variant::BrickColor(BrickColor::closest(c))
            }
            _ => return None,
        })
    }
}
