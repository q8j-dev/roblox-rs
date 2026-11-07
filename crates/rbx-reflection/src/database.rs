use crate::variant::VariantType;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::LazyLock;

pub const XML_PUBLIC: u32 = 1;
pub const XML_REPLICATE: u32 = 2;
pub const XML_READ: u32 = 4;
pub const XML_WRITE: u32 = 8;
pub const XML_SCRIPTABLE: u32 = 16;
pub const XML_ALWAYS_CLONE: u32 = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Security {
    None,
    Plugin,
    LocalUser,
    WritePlayer,
    RobloxScript,
    Roblox,
    RobloxPlace,
    TestLocalUser,
    NotAccessible,
}

impl Security {
    fn parse(s: Option<&str>) -> Security {
        match s.unwrap_or("None") {
            "Plugin" => Security::Plugin,
            "LocalUser" => Security::LocalUser,
            "WritePlayer" => Security::WritePlayer,
            "RobloxScript" => Security::RobloxScript,
            "Roblox" => Security::Roblox,
            "RobloxPlace" => Security::RobloxPlace,
            "TestLocalUser" => Security::TestLocalUser,
            "NotAccessible" => Security::NotAccessible,
            _ => Security::None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ValueType {
    Value(VariantType),
    Enum(String),
    Ref(String),
    Array,
    Map,
    Dictionary,
    Tuple,
    Objects,
    Function,
    Object,
    Variant,
    Void,
    NumberSequenceKeypoint,
    ColorSequenceKeypoint,
}

impl ValueType {
    pub fn parse(s: &str) -> ValueType {
        if let Some(e) = s.strip_prefix("Enum:") {
            return ValueType::Enum(e.to_string());
        }
        if let Some(c) = s.strip_prefix("Instance:") {
            return ValueType::Ref(c.to_string());
        }
        let v = match s {
            "bool" => VariantType::Bool,
            "int" => VariantType::Int,
            "int64" => VariantType::Int64,
            "float" => VariantType::Float,
            "double" => VariantType::Double,
            "string" => VariantType::String,
            "ProtectedString" => VariantType::ProtectedString,
            "BinaryString" => VariantType::BinaryString,
            "Content" => VariantType::Content,
            "Vector2" => VariantType::Vector2,
            "Vector3" => VariantType::Vector3,
            "Vector2int16" => VariantType::Vector2int16,
            "Vector3int16" => VariantType::Vector3int16,
            "Vector3int32" => VariantType::Vector3int32,
            "CFrame" => VariantType::CFrame,
            "Color3" => VariantType::Color3,
            "BrickColor" => VariantType::BrickColor,
            "UDim" => VariantType::UDim,
            "UDim2" => VariantType::UDim2,
            "Ray" => VariantType::Ray,
            "Rect2D" => VariantType::Rect2D,
            "Region3" => VariantType::Region3,
            "Region3int16" => VariantType::Region3int16,
            "Faces" => VariantType::Faces,
            "Axes" => VariantType::Axes,
            "PhysicalProperties" => VariantType::PhysicalProperties,
            "NumberSequence" => VariantType::NumberSequence,
            "ColorSequence" => VariantType::ColorSequence,
            "NumberRange" => VariantType::NumberRange,
            "SystemAddress" => VariantType::SystemAddress,
            "Array" => return ValueType::Array,
            "Map" => return ValueType::Map,
            "Dictionary" => return ValueType::Dictionary,
            "Tuple" => return ValueType::Tuple,
            "Objects" => return ValueType::Objects,
            "Function" => return ValueType::Function,
            "Object" => return ValueType::Object,
            "Variant" => return ValueType::Variant,
            "void" => return ValueType::Void,
            "NumberSequenceKeypoint" => return ValueType::NumberSequenceKeypoint,
            "ColorSequenceKeypoint" => return ValueType::ColorSequenceKeypoint,
            _ => return ValueType::Variant,
        };
        ValueType::Value(v)
    }

    pub fn variant_type(&self) -> Option<VariantType> {
        match self {
            ValueType::Value(v) => Some(*v),
            ValueType::Enum(_) => Some(VariantType::Enum),
            ValueType::Ref(_) => Some(VariantType::Ref),
            _ => None,
        }
    }
}

#[derive(Deserialize)]
struct RawProperty {
    name: String,
    #[serde(rename = "type")]
    ty: String,
    category: Option<String>,
    flags: u32,
    security: Option<String>,
    read_only: bool,
    deprecated_by: Option<String>,
}

#[derive(Deserialize)]
struct RawParam {
    name: String,
    #[serde(rename = "type")]
    ty: String,
    default: Option<String>,
}

#[derive(Deserialize)]
struct RawFunction {
    name: String,
    #[serde(rename = "return")]
    ret: Option<String>,
    params: Vec<RawParam>,
    security: Option<String>,
    deprecated_by: Option<String>,
}

#[derive(Deserialize)]
struct RawEvent {
    name: String,
    params: Vec<RawParam>,
    security: Option<String>,
    deprecated_by: Option<String>,
    kind: String,
}

#[derive(Deserialize)]
struct RawClass {
    name: String,
    superclass: Option<String>,
    creatable: bool,
    flags: Vec<String>,
    security: Option<String>,
    properties: Vec<RawProperty>,
    functions: Vec<RawFunction>,
    yield_functions: Vec<RawFunction>,
    callbacks: Vec<RawFunction>,
    events: Vec<RawEvent>,
}

#[derive(Deserialize)]
struct RawEnumItem {
    name: String,
    value: Option<i64>,
}

#[derive(Deserialize)]
struct RawEnum {
    name: String,
    items: Vec<RawEnumItem>,
    legacy: Vec<RawEnumItem>,
}

#[derive(Deserialize)]
struct RawDatabase {
    classes: Vec<RawClass>,
    enums: Vec<RawEnum>,
}

#[derive(Clone, Debug)]
pub struct PropertyDescriptor {
    pub name: String,
    pub class: String,
    pub ty: ValueType,
    pub category: String,
    pub flags: u32,
    pub security: Security,
    pub read_only: bool,
    pub deprecated_by: Option<String>,
}

impl PropertyDescriptor {
    pub fn is_public(&self) -> bool {
        self.flags & XML_PUBLIC != 0
    }

    pub fn can_replicate(&self) -> bool {
        self.flags & XML_REPLICATE != 0
    }

    pub fn can_xml_read(&self) -> bool {
        self.flags & XML_READ != 0
    }

    pub fn can_xml_write(&self) -> bool {
        self.flags & XML_WRITE != 0
    }

    pub fn is_scriptable(&self) -> bool {
        self.flags & XML_SCRIPTABLE != 0
    }

    pub fn always_clone(&self) -> bool {
        self.flags & XML_ALWAYS_CLONE != 0
    }

    pub fn is_deprecated(&self) -> bool {
        self.deprecated_by.is_some()
    }

    pub fn serialized(&self) -> bool {
        !self.read_only && self.can_xml_write()
    }
}

#[derive(Clone, Debug)]
pub struct ParamDescriptor {
    pub name: String,
    pub ty: ValueType,
    pub default: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionKind {
    Function,
    YieldFunction,
    Callback,
}

#[derive(Clone, Debug)]
pub struct FunctionDescriptor {
    pub name: String,
    pub class: String,
    pub kind: FunctionKind,
    pub ret: ValueType,
    pub params: Vec<ParamDescriptor>,
    pub security: Security,
    pub deprecated_by: Option<String>,
}

#[derive(Clone, Debug)]
pub struct EventDescriptor {
    pub name: String,
    pub class: String,
    pub params: Vec<ParamDescriptor>,
    pub security: Security,
    pub deprecated_by: Option<String>,
    pub remote: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClassFunctionality(pub u32);

impl ClassFunctionality {
    pub fn is_public(&self) -> bool {
        self.0 & 0x1 != 0
    }

    pub fn replicates(&self) -> bool {
        self.0 & 0x2 != 0
    }

    pub fn replicates_to_player(&self) -> bool {
        self.0 & 0x4 != 0
    }

    pub fn is_serializable(&self) -> bool {
        self.0 & 0x8 != 0
    }

    pub fn is_script_creatable(&self) -> bool {
        self.0 & 0x10 != 0
    }
}

#[derive(Clone, Debug)]
pub struct ClassDescriptor {
    pub name: String,
    pub superclass: Option<String>,
    pub creatable: bool,
    pub functionality: ClassFunctionality,
    pub security: Security,
    pub properties: Vec<PropertyDescriptor>,
    pub functions: Vec<FunctionDescriptor>,
    pub events: Vec<EventDescriptor>,
    pub depth: usize,
}

#[derive(Clone, Debug)]
pub struct EnumItem {
    pub name: String,
    pub value: u32,
}

#[derive(Clone, Debug)]
pub struct EnumDescriptor {
    pub name: String,
    pub items: Vec<EnumItem>,
    pub legacy: Vec<EnumItem>,
}

impl EnumDescriptor {
    pub fn from_name(&self, name: &str) -> Option<u32> {
        self.items
            .iter()
            .chain(self.legacy.iter())
            .find(|i| i.name == name)
            .map(|i| i.value)
    }

    pub fn name_of(&self, value: u32) -> Option<&str> {
        self.items
            .iter()
            .find(|i| i.value == value)
            .map(|i| i.name.as_str())
    }

    pub fn is_valid(&self, value: u32) -> bool {
        self.items.iter().any(|i| i.value == value)
    }

    pub fn default_value(&self) -> u32 {
        self.items.first().map(|i| i.value).unwrap_or(0)
    }
}

fn class_functionality(flags: &[String]) -> ClassFunctionality {
    let mut value = 0x1 + 0x2 + 0x8 + 0x10;
    for f in flags {
        value = match f.as_str() {
            "PERSISTENT" => 0x1 + 0x2 + 0x8 + 0x10,
            "PERSISTENT_PLAYER" => 0x1 + 0x4 + 0x8 + 0x10,
            "PERSISTENT_LOCAL" => 0x1 + 0x8 + 0x10,
            "RUNTIME" => 0x1 + 0x2 + 0x10,
            "RUNTIME_PLAYER" => 0x1 + 0x4 + 0x10,
            "RUNTIME_LOCAL" => 0x1 + 0x10,
            "INTERNAL" => 0x1 + 0x2,
            "INTERNAL_PLAYER" => 0x1 + 0x4,
            "INTERNAL_LOCAL" => 0x1,
            "PERSISTENT_HIDDEN" => 0x1 + 0x2 + 0x8,
            "PERSISTENT_LOCAL_INTERNAL" => 0x1 + 0x8,
            _ => value,
        };
    }
    ClassFunctionality(value)
}

pub struct ReflectionDatabase {
    classes: HashMap<String, ClassDescriptor>,
    enums: HashMap<String, EnumDescriptor>,
    class_order: Vec<String>,
}

fn convert_params(params: &[RawParam]) -> Vec<ParamDescriptor> {
    params
        .iter()
        .map(|p| ParamDescriptor {
            name: p.name.clone(),
            ty: ValueType::parse(&p.ty),
            default: p.default.clone(),
        })
        .collect()
}

impl ReflectionDatabase {
    fn from_json(json: &str) -> ReflectionDatabase {
        let raw: RawDatabase = match serde_json::from_str(json) {
            Ok(raw) => raw,
            Err(e) => panic!("embedded reflection database is invalid: {e}"),
        };
        let mut classes = HashMap::new();
        let mut class_order = Vec::new();
        for c in raw.classes {
            let name = c.name.clone();
            let properties = c
                .properties
                .into_iter()
                .map(|p| PropertyDescriptor {
                    name: p.name,
                    class: name.clone(),
                    ty: ValueType::parse(&p.ty),
                    category: p.category.unwrap_or_else(|| "Data".to_string()),
                    flags: p.flags,
                    security: Security::parse(p.security.as_deref()),
                    read_only: p.read_only,
                    deprecated_by: p.deprecated_by,
                })
                .collect();
            let mut functions = Vec::new();
            for (list, kind) in [
                (c.functions, FunctionKind::Function),
                (c.yield_functions, FunctionKind::YieldFunction),
                (c.callbacks, FunctionKind::Callback),
            ] {
                for f in list {
                    functions.push(FunctionDescriptor {
                        name: f.name,
                        class: name.clone(),
                        kind,
                        ret: ValueType::parse(f.ret.as_deref().unwrap_or("void")),
                        params: convert_params(&f.params),
                        security: Security::parse(f.security.as_deref()),
                        deprecated_by: f.deprecated_by,
                    });
                }
            }
            let events = c
                .events
                .into_iter()
                .map(|e| EventDescriptor {
                    name: e.name,
                    class: name.clone(),
                    params: convert_params(&e.params),
                    security: Security::parse(e.security.as_deref()),
                    deprecated_by: e.deprecated_by,
                    remote: e.kind == "RemoteEventDesc",
                })
                .collect();
            class_order.push(name.clone());
            classes.insert(
                name.clone(),
                ClassDescriptor {
                    name,
                    superclass: c.superclass,
                    creatable: c.creatable,
                    functionality: class_functionality(&c.flags),
                    security: Security::parse(c.security.as_deref()),
                    properties,
                    functions,
                    events,
                    depth: 0,
                },
            );
        }
        let names: Vec<String> = classes.keys().cloned().collect();
        for n in names {
            let mut depth = 0;
            let mut cur = classes.get(&n).and_then(|c| c.superclass.clone());
            while let Some(s) = cur {
                depth += 1;
                if depth > 64 {
                    break;
                }
                cur = classes.get(&s).and_then(|c| c.superclass.clone());
            }
            if let Some(c) = classes.get_mut(&n) {
                c.depth = depth;
            }
        }
        let enums = raw
            .enums
            .into_iter()
            .map(|e| {
                let conv = |items: Vec<RawEnumItem>| {
                    items
                        .into_iter()
                        .filter_map(|i| {
                            i.value.map(|v| EnumItem {
                                name: i.name,
                                value: v as u32,
                            })
                        })
                        .collect::<Vec<_>>()
                };
                (
                    e.name.clone(),
                    EnumDescriptor {
                        name: e.name,
                        items: conv(e.items),
                        legacy: conv(e.legacy),
                    },
                )
            })
            .collect();
        ReflectionDatabase {
            classes,
            enums,
            class_order,
        }
    }

    pub fn class(&self, name: &str) -> Option<&ClassDescriptor> {
        self.classes.get(name)
    }

    pub fn classes(&self) -> impl Iterator<Item = &ClassDescriptor> {
        self.class_order.iter().filter_map(|n| self.classes.get(n))
    }

    pub fn enum_desc(&self, name: &str) -> Option<&EnumDescriptor> {
        self.enums.get(name)
    }

    pub fn enums(&self) -> impl Iterator<Item = &EnumDescriptor> {
        self.enums.values()
    }

    pub fn superclasses<'a>(
        &'a self,
        name: &str,
    ) -> impl Iterator<Item = &'a ClassDescriptor> + 'a {
        let mut cur = self.classes.get(name);
        std::iter::from_fn(move || {
            let c = cur?;
            cur = c.superclass.as_deref().and_then(|s| self.classes.get(s));
            Some(c)
        })
    }

    pub fn is_a(&self, class: &str, ancestor: &str) -> bool {
        if class == ancestor || ancestor == "Instance" {
            return self.classes.contains_key(class) || class == ancestor;
        }
        self.superclasses(class).any(|c| c.name == ancestor)
    }

    pub fn find_property(&self, class: &str, name: &str) -> Option<&PropertyDescriptor> {
        self.superclasses(class)
            .find_map(|c| c.properties.iter().find(|p| p.name == name))
    }

    pub fn find_function(&self, class: &str, name: &str) -> Option<&FunctionDescriptor> {
        self.superclasses(class)
            .find_map(|c| c.functions.iter().find(|f| f.name == name))
    }

    pub fn find_event(&self, class: &str, name: &str) -> Option<&EventDescriptor> {
        self.superclasses(class)
            .find_map(|c| c.events.iter().find(|e| e.name == name))
    }

    pub fn properties<'a>(&'a self, class: &str) -> Vec<&'a PropertyDescriptor> {
        let chain: Vec<&ClassDescriptor> = self.superclasses(class).collect();
        let mut out = Vec::new();
        for c in chain.iter().rev() {
            for p in &c.properties {
                if !out.iter().any(|q: &&PropertyDescriptor| q.name == p.name) {
                    out.push(p);
                }
            }
        }
        out
    }

    pub fn serialized_properties<'a>(&'a self, class: &str) -> Vec<&'a PropertyDescriptor> {
        let mut props: Vec<&PropertyDescriptor> = self
            .properties(class)
            .into_iter()
            .filter(|p| p.serialized())
            .collect();
        props.sort_by(|a, b| a.name.cmp(&b.name));
        props
    }

    pub fn canonical_property<'a>(
        &'a self,
        class: &str,
        name: &str,
    ) -> Option<&'a PropertyDescriptor> {
        let p = self.find_property(class, name)?;
        match &p.deprecated_by {
            Some(target) if !target.is_empty() => self.find_property(class, target).or(Some(p)),
            _ => Some(p),
        }
    }
}

static DATABASE: LazyLock<ReflectionDatabase> =
    LazyLock::new(|| ReflectionDatabase::from_json(include_str!("../data/api.json")));

pub fn database() -> &'static ReflectionDatabase {
    &DATABASE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_core_classes() {
        let db = database();
        assert!(db.class("Instance").is_some());
        assert!(db.is_a("Part", "BasePart"));
        assert!(db.is_a("Part", "PVInstance"));
        assert!(db.is_a("Workspace", "Model"));
        let size = db.find_property("Part", "size").map(|p| p.serialized());
        assert_eq!(size, Some(true));
        let name = db.find_property("Part", "Name").map(|p| p.ty.clone());
        assert_eq!(name, Some(ValueType::Value(VariantType::String)));
        let mat = db.enum_desc("Material").map(|e| e.from_name("Plastic"));
        assert!(matches!(mat, Some(Some(_))));
        assert!(db.find_function("Part", "FindFirstChild").is_some());
        assert!(db.find_event("Part", "Touched").is_some());
    }
}
