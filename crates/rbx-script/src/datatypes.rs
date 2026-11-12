use crate::types::{
    LCFrame, LColor3, LEnumItem, LUDim2, LVector2, LVector3, cannot_assign, not_member, rt,
};
use mlua::{
    Error, FromLua, Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods,
    Value,
};
use rbx_dom::InstanceId;
use rbx_math::{CFrame, Color3, Mat3, UDim, UDim2, Vec2, Vec3};
use rbx_reflection::FontFace;

pub const FONT_ENUM: &[(u32, &str, u16, u8, f32)] = &[
    (0, "LegacyArial", 400, 0, 1.5),
    (1, "Arial", 400, 0, 1.0),
    (2, "Arial", 700, 0, 1.0),
    (3, "SourceSansPro", 400, 0, 1.0),
    (4, "SourceSansPro", 700, 0, 1.0),
    (5, "SourceSansPro", 300, 0, 1.0),
    (6, "SourceSansPro", 400, 1, 1.0),
    (7, "AccanthisADFStd", 400, 0, 1.0),
    (8, "Guru", 400, 0, 1.0),
    (9, "ComicNeueAngular", 400, 0, 1.0),
    (10, "Inconsolata", 400, 0, 1.0),
    (11, "HighwayGothic", 400, 0, 1.0),
    (12, "Zekton", 400, 0, 1.0),
    (13, "PressStart2P", 400, 0, 1.0),
    (14, "Balthazar", 400, 0, 1.0),
    (15, "RomanAntique", 400, 0, 1.0),
    (16, "SourceSansPro", 600, 0, 1.0),
    (17, "GothamSSm", 400, 0, 1.0),
    (18, "GothamSSm", 500, 0, 1.0),
    (19, "GothamSSm", 700, 0, 1.0),
    (20, "GothamSSm", 900, 0, 1.0),
    (21, "AmaticSC", 400, 0, 1.0),
    (22, "Bangers", 400, 0, 1.0),
    (23, "Creepster", 400, 0, 1.0),
    (24, "DenkOne", 400, 0, 1.0),
    (25, "Fondamento", 400, 0, 1.0),
    (26, "FredokaOne", 400, 0, 1.0),
    (27, "GrenzeGotisch", 400, 0, 1.0),
    (28, "IndieFlower", 400, 0, 1.0),
    (29, "JosefinSans", 400, 0, 1.0),
    (30, "Jura", 400, 0, 1.0),
    (31, "Kalam", 400, 0, 1.0),
    (32, "LuckiestGuy", 400, 0, 1.0),
    (33, "Merriweather", 400, 0, 1.0),
    (34, "Michroma", 400, 0, 1.0),
    (35, "Nunito", 400, 0, 1.0),
    (36, "Oswald", 400, 0, 1.0),
    (37, "PatrickHand", 400, 0, 1.0),
    (38, "PermanentMarker", 400, 0, 1.0),
    (39, "Roboto", 400, 0, 1.0),
    (40, "RobotoCondensed", 400, 0, 1.0),
    (41, "RobotoMono", 400, 0, 1.0),
    (42, "Sarpanch", 400, 0, 1.0),
    (43, "SpecialElite", 400, 0, 1.0),
    (44, "TitilliumWeb", 400, 0, 1.0),
    (45, "Ubuntu", 400, 0, 1.0),
    (46, "BuilderSans", 400, 0, 1.0),
    (47, "BuilderSans", 500, 0, 1.0),
    (48, "BuilderSans", 700, 0, 1.0),
    (49, "BuilderSans", 800, 0, 1.0),
    (50, "Arimo", 400, 0, 1.0),
    (51, "Arimo", 700, 0, 1.0),
];

pub fn font_family_path(name: &str) -> String {
    format!("rbxasset://fonts/families/{name}.json")
}

pub fn font_from_enum(value: u32) -> Option<FontFace> {
    FONT_ENUM
        .iter()
        .find(|f| f.0 == value)
        .map(|&(_, family, weight, style, _)| FontFace {
            family: font_family_path(family),
            weight,
            style,
            cached_face_id: String::new(),
        })
}

pub fn font_enum_for(face: &FontFace) -> Option<u32> {
    FONT_ENUM
        .iter()
        .find(|&&(_, family, weight, style, _)| {
            face.family.eq_ignore_ascii_case(&font_family_path(family))
                && face.weight == weight
                && face.style == style
        })
        .map(|f| f.0)
}

#[derive(Clone, Debug, PartialEq)]
pub struct LFont(pub FontFace);

impl FromLua for LFont {
    fn from_lua(value: Value, _: &Lua) -> Result<Self> {
        match &value {
            Value::UserData(ud) => match ud.borrow::<Self>() {
                Ok(v) => Ok((*v).clone()),
                Err(_) => Err(Error::FromLuaConversionError {
                    from: "userdata",
                    to: "Font".into(),
                    message: None,
                }),
            },
            _ => Err(Error::FromLuaConversionError {
                from: value.type_name(),
                to: "Font".into(),
                message: None,
            }),
        }
    }
}

impl UserData for LFont {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_meta_field(MetaMethod::Type, "Font");
        f.add_field_method_get("Family", |_, t| Ok(t.0.family.clone()));
        f.add_field_method_get("Weight", |lua, t| {
            LEnumItem::lookup("FontWeight", t.0.weight as u32)
                .map(|i| crate::types::enum_item_value(lua, i))
                .unwrap_or(Ok(Value::Number(t.0.weight as f64)))
        });
        f.add_field_method_get("Style", |lua, t| {
            LEnumItem::lookup("FontStyle", t.0.style as u32)
                .map(|i| crate::types::enum_item_value(lua, i))
                .unwrap_or(Ok(Value::Number(t.0.style as f64)))
        });
        f.add_field_method_get("Bold", |_, t| Ok(t.0.weight >= 600));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| {
            Ok(format!(
                "Font {{ Family = {}, Weight = {}, Style = {} }}",
                t.0.family,
                LEnumItem::lookup("FontWeight", t.0.weight as u32)
                    .map(|i| i.name)
                    .unwrap_or("Regular"),
                LEnumItem::lookup("FontStyle", t.0.style as u32)
                    .map(|i| i.name)
                    .unwrap_or("Normal"),
            ))
        });
        m.add_meta_method(MetaMethod::Eq, |_, t, o: Value| {
            Ok(matches!(&o, Value::UserData(u) if u.borrow::<LFont>().map(|v| v.0 == t.0).unwrap_or(false)))
        });
        m.add_meta_method(MetaMethod::Index, |_, _, k: String| -> Result<Value> {
            Err(not_member(&k))
        });
        m.add_meta_method(
            MetaMethod::NewIndex,
            |_, _, (k, _v): (String, Value)| -> Result<()> { Err(cannot_assign(&k)) },
        );
    }
}

fn enum_arg(lua: &Lua, v: Option<&Value>, name: &str) -> Option<u32> {
    let item = LEnumItem::from_lua(v?.clone(), lua).ok()?;
    (item.enum_name == name).then_some(item.value)
}

fn vec3_arg(lua: &Lua, v: Option<&Value>) -> Option<Vec3> {
    LVector3::from_lua(v?.clone(), lua).ok().map(|x| x.0)
}

fn num(v: Option<&Value>) -> f32 {
    crate::convert::arg_number(v).unwrap_or(0.0) as f32
}

fn euler_yxz(rx: f32, ry: f32, rz: f32) -> CFrame {
    CFrame::new(
        Mat3::from_rotation_y(ry) * Mat3::from_rotation_x(rx) * Mat3::from_rotation_z(rz),
        Vec3::ZERO,
    )
}

fn parse_hex(s: &str) -> Option<Color3> {
    let h = s.trim().trim_start_matches('#');
    let expand: String = match h.len() {
        3 => h.chars().flat_map(|c| [c, c]).collect(),
        6 => h.to_string(),
        _ => return None,
    };
    let v = u32::from_str_radix(&expand, 16).ok()?;
    Some(Color3::new(
        ((v >> 16) & 255) as f32 / 255.0,
        ((v >> 8) & 255) as f32 / 255.0,
        (v & 255) as f32 / 255.0,
    ))
}

pub fn register(lua: &Lua, g: &Table) -> Result<()> {
    let font = lua.create_table()?;
    font.set(
        "new",
        lua.create_function(|lua, args: mlua::MultiValue| {
            let family = crate::convert::arg_string(args.get(0))
                .ok_or_else(|| rt("Font.new expects a family string"))?;
            Ok(LFont(FontFace {
                family,
                weight: enum_arg(lua, args.get(1), "FontWeight").unwrap_or(400) as u16,
                style: enum_arg(lua, args.get(2), "FontStyle").unwrap_or(0) as u8,
                cached_face_id: String::new(),
            }))
        })?,
    )?;
    font.set(
        "fromEnum",
        lua.create_function(|lua, v: Value| {
            let value = enum_arg(lua, Some(&v), "Font")
                .ok_or_else(|| rt("Font.fromEnum expects an Enum.Font"))?;
            font_from_enum(value)
                .map(LFont)
                .ok_or_else(|| rt("This font cannot be converted to a Font"))
        })?,
    )?;
    g.set("Font", font)?;

    let cf: Table = g.get("CFrame")?;
    let yxz = lua.create_function(|_, args: mlua::MultiValue| {
        Ok(LCFrame(euler_yxz(
            num(args.get(0)),
            num(args.get(1)),
            num(args.get(2)),
        )))
    })?;
    cf.set("fromEulerAnglesYXZ", yxz.clone())?;
    cf.set("fromOrientation", yxz)?;
    cf.set(
        "fromEulerAngles",
        lua.create_function(|lua, args: mlua::MultiValue| {
            let (x, y, z) = (num(args.get(0)), num(args.get(1)), num(args.get(2)));
            let order = enum_arg(lua, args.get(3), "RotationOrder").unwrap_or(0);
            let (mx, my, mz) = (
                Mat3::from_rotation_x(x),
                Mat3::from_rotation_y(y),
                Mat3::from_rotation_z(z),
            );
            let m = match order {
                1 => mx * mz * my,
                2 => my * mz * mx,
                3 => my * mx * mz,
                4 => mz * mx * my,
                5 => mz * my * mx,
                _ => mx * my * mz,
            };
            Ok(LCFrame(CFrame::new(m, Vec3::ZERO)))
        })?,
    )?;
    cf.set(
        "fromMatrix",
        lua.create_function(|lua, args: mlua::MultiValue| {
            let pos = vec3_arg(lua, args.get(0)).ok_or_else(|| rt("Vector3 expected"))?;
            let x = vec3_arg(lua, args.get(1)).ok_or_else(|| rt("Vector3 expected"))?;
            let y = vec3_arg(lua, args.get(2)).ok_or_else(|| rt("Vector3 expected"))?;
            let z = vec3_arg(lua, args.get(3)).unwrap_or_else(|| x.cross(y).normalize_or_zero());
            Ok(LCFrame(CFrame::new(Mat3::from_cols(x, y, z), pos)))
        })?,
    )?;
    cf.set(
        "lookAt",
        lua.create_function(|lua, args: mlua::MultiValue| {
            let at = vec3_arg(lua, args.get(0)).ok_or_else(|| rt("Vector3 expected"))?;
            let target = vec3_arg(lua, args.get(1)).ok_or_else(|| rt("Vector3 expected"))?;
            let up = vec3_arg(lua, args.get(2)).unwrap_or(Vec3::Y);
            Ok(LCFrame(CFrame::look_at(at, target, up)))
        })?,
    )?;
    cf.set(
        "lookAlong",
        lua.create_function(|lua, args: mlua::MultiValue| {
            let at = vec3_arg(lua, args.get(0)).ok_or_else(|| rt("Vector3 expected"))?;
            let dir = vec3_arg(lua, args.get(1)).ok_or_else(|| rt("Vector3 expected"))?;
            let up = vec3_arg(lua, args.get(2)).unwrap_or(Vec3::Y);
            Ok(LCFrame(CFrame::look_at(at, at + dir, up)))
        })?,
    )?;
    cf.set("identity", LCFrame(CFrame::IDENTITY))?;

    let c3: Table = g.get("Color3")?;
    c3.set(
        "fromHSV",
        lua.create_function(|_, (h, s, v): (f64, f64, f64)| {
            Ok(LColor3(Color3::from_hsv(h as f32, s as f32, v as f32)))
        })?,
    )?;
    c3.set(
        "fromHex",
        lua.create_function(|_, s: String| {
            parse_hex(&s)
                .map(LColor3)
                .ok_or_else(|| rt(format!("Unable to convert characters to hex value: {s}")))
        })?,
    )?;

    let u2: Table = g.get("UDim2")?;
    u2.set(
        "fromOffset",
        lua.create_function(|_, args: mlua::MultiValue| {
            Ok(LUDim2(UDim2 {
                x: UDim::new(0.0, num(args.get(0)) as i32),
                y: UDim::new(0.0, num(args.get(1)) as i32),
            }))
        })?,
    )?;
    u2.set(
        "fromScale",
        lua.create_function(|_, args: mlua::MultiValue| {
            Ok(LUDim2(UDim2 {
                x: UDim::new(num(args.get(0)), 0),
                y: UDim::new(num(args.get(1)), 0),
            }))
        })?,
    )?;

    let v2: Table = g.get("Vector2")?;
    v2.set("zero", LVector2(Vec2::ZERO))?;
    v2.set("one", LVector2(Vec2::new(1.0, 1.0)))?;
    v2.set("xAxis", LVector2(Vec2::new(1.0, 0.0)))?;
    v2.set("yAxis", LVector2(Vec2::new(0.0, 1.0)))?;
    register_more(lua, g)
}

macro_rules! lua_from_userdata {
    ($t:ty, $name:expr) => {
        impl FromLua for $t {
            fn from_lua(value: Value, _: &Lua) -> Result<Self> {
                match &value {
                    Value::UserData(ud) => match ud.borrow::<Self>() {
                        Ok(v) => Ok((*v).clone()),
                        Err(_) => Err(Error::FromLuaConversionError {
                            from: "userdata",
                            to: $name.into(),
                            message: None,
                        }),
                    },
                    _ => Err(Error::FromLuaConversionError {
                        from: value.type_name(),
                        to: $name.into(),
                        message: None,
                    }),
                }
            }
        }
    };
}

fn enum_value_or(lua: &Lua, v: Option<&Value>, name: &str, default: u32) -> u32 {
    enum_arg(lua, v, name).unwrap_or(default)
}

fn enum_lua(lua: &Lua, name: &str, value: u32) -> Result<Value> {
    match LEnumItem::lookup(name, value) {
        Some(i) => crate::types::enum_item_value(lua, i),
        None => Ok(Value::Number(value as f64)),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LPath2DControlPoint {
    pub position: UDim2,
    pub left: UDim2,
    pub right: UDim2,
}
lua_from_userdata!(LPath2DControlPoint, "Path2DControlPoint");

impl UserData for LPath2DControlPoint {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_meta_field(MetaMethod::Type, "Path2DControlPoint");
        f.add_field_method_get("Position", |_, t| Ok(LUDim2(t.position)));
        f.add_field_method_get("LeftTangent", |_, t| Ok(LUDim2(t.left)));
        f.add_field_method_get("RightTangent", |_, t| Ok(LUDim2(t.right)));
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LDateTime(pub i64);
lua_from_userdata!(LDateTime, "DateTime");

fn time_table(lua: &Lua, millis: i64) -> Result<Table> {
    let p = crate::datetime::parts(millis);
    let t = lua.create_table()?;
    t.set("Year", p.year)?;
    t.set("Month", p.month)?;
    t.set("Day", p.day)?;
    t.set("Hour", p.hour)?;
    t.set("Minute", p.minute)?;
    t.set("Second", p.second)?;
    t.set("Millisecond", p.millisecond)?;
    Ok(t)
}

impl UserData for LDateTime {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_meta_field(MetaMethod::Type, "DateTime");
        f.add_field_method_get("UnixTimestamp", |_, t| Ok(t.0.div_euclid(1000)));
        f.add_field_method_get("UnixTimestampMillis", |_, t| Ok(t.0));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("ToIsoDate", |_, t, ()| Ok(crate::datetime::iso(t.0)));
        m.add_method("ToUniversalTime", |lua, t, ()| time_table(lua, t.0));
        m.add_method("ToLocalTime", |lua, t, ()| time_table(lua, t.0));
        m.add_method(
            "FormatUniversalTime",
            |_, t, (fmt, _locale): (String, Option<String>)| Ok(crate::datetime::format(t.0, &fmt)),
        );
        m.add_method(
            "FormatLocalTime",
            |_, t, (fmt, _locale): (String, Option<String>)| Ok(crate::datetime::format(t.0, &fmt)),
        );
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| {
            Ok(crate::datetime::iso(t.0))
        });
        m.add_meta_method(MetaMethod::Eq, |_, t, o: Value| {
            Ok(matches!(&o, Value::UserData(u) if u.borrow::<LDateTime>().map(|v| v.0 == t.0).unwrap_or(false)))
        });
        m.add_meta_method(MetaMethod::Lt, |_, t, o: LDateTime| Ok(t.0 < o.0));
        m.add_meta_method(MetaMethod::Le, |_, t, o: LDateTime| Ok(t.0 <= o.0));
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LTweenInfo {
    pub time: f64,
    pub style: u32,
    pub direction: u32,
    pub repeat_count: i32,
    pub reverses: bool,
    pub delay: f64,
}
lua_from_userdata!(LTweenInfo, "TweenInfo");

impl UserData for LTweenInfo {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_meta_field(MetaMethod::Type, "TweenInfo");
        f.add_field_method_get("Time", |_, t| Ok(t.time));
        f.add_field_method_get("EasingStyle", |lua, t| {
            enum_lua(lua, "EasingStyle", t.style)
        });
        f.add_field_method_get("EasingDirection", |lua, t| {
            enum_lua(lua, "EasingDirection", t.direction)
        });
        f.add_field_method_get("RepeatCount", |_, t| Ok(t.repeat_count));
        f.add_field_method_get("Reverses", |_, t| Ok(t.reverses));
        f.add_field_method_get("DelayTime", |_, t| Ok(t.delay));
    }
}

#[derive(Clone, Debug, Default)]
pub struct LRaycastParams {
    pub filter: Vec<InstanceId>,
    pub filter_type: u32,
    pub ignore_water: bool,
    pub collision_group: String,
    pub respect_can_collide: bool,
    pub brute_force: bool,
}
lua_from_userdata!(LRaycastParams, "RaycastParams");

fn instance_list(lua: &Lua, v: &Value) -> Vec<InstanceId> {
    match v {
        Value::Table(t) => t
            .sequence_values::<Value>()
            .filter_map(|x| x.ok())
            .filter_map(|x| crate::runtime::instance_from_value(lua, &x))
            .collect(),
        _ => Vec::new(),
    }
}

impl UserData for LRaycastParams {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_meta_field(MetaMethod::Type, "RaycastParams");
        f.add_field_method_get("FilterDescendantsInstances", |lua, t| {
            lua.create_sequence_from(crate::instance::push_instances(lua, &t.filter)?)
        });
        f.add_field_method_set("FilterDescendantsInstances", |lua, t, v: Value| {
            t.filter = instance_list(lua, &v);
            Ok(())
        });
        f.add_field_method_get("FilterType", |lua, t| {
            enum_lua(lua, "RaycastFilterType", t.filter_type)
        });
        f.add_field_method_set("FilterType", |lua, t, v: Value| {
            t.filter_type = enum_value_or(lua, Some(&v), "RaycastFilterType", t.filter_type);
            Ok(())
        });
        f.add_field_method_get("IgnoreWater", |_, t| Ok(t.ignore_water));
        f.add_field_method_set("IgnoreWater", |_, t, v: bool| {
            t.ignore_water = v;
            Ok(())
        });
        f.add_field_method_get("CollisionGroup", |_, t| Ok(t.collision_group.clone()));
        f.add_field_method_set("CollisionGroup", |_, t, v: String| {
            t.collision_group = v;
            Ok(())
        });
        f.add_field_method_get("RespectCanCollide", |_, t| Ok(t.respect_can_collide));
        f.add_field_method_set("RespectCanCollide", |_, t, v: bool| {
            t.respect_can_collide = v;
            Ok(())
        });
        f.add_field_method_get("BruteForceAllSlow", |_, t| Ok(t.brute_force));
        f.add_field_method_set("BruteForceAllSlow", |_, t, v: bool| {
            t.brute_force = v;
            Ok(())
        });
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum LContent {
    None,
    Uri(String),
    Object(InstanceId),
}
lua_from_userdata!(LContent, "Content");

impl LContent {
    pub fn uri(&self) -> String {
        match self {
            LContent::Uri(u) => u.clone(),
            _ => String::new(),
        }
    }
}

impl UserData for LContent {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_meta_field(MetaMethod::Type, "Content");
        f.add_field_method_get("SourceType", |lua, t| {
            let v = match t {
                LContent::None => 0,
                LContent::Uri(_) => 1,
                LContent::Object(_) => 2,
            };
            enum_lua(lua, "ContentSourceType", v)
        });
        f.add_field_method_get("Uri", |_, t| {
            Ok(match t {
                LContent::Uri(u) => Some(u.clone()),
                _ => None,
            })
        });
        f.add_field_method_get("Object", |lua, t| match t {
            LContent::Object(id) => crate::instance::push_instance(lua, *id),
            _ => Ok(Value::Nil),
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::Eq, |_, t, o: Value| {
            Ok(matches!(&o, Value::UserData(u) if u.borrow::<LContent>().map(|v| *v == *t).unwrap_or(false)))
        });
    }
}

#[derive(Clone, Debug)]
pub struct LRandom {
    state: u64,
    inc: u64,
}
lua_from_userdata!(LRandom, "Random");

impl LRandom {
    pub fn new(seed: u64) -> LRandom {
        let mut r = LRandom {
            state: 0,
            inc: (0xda3e39cb94b95bdb_u64 << 1) | 1,
        };
        r.next_u32();
        r.state = r.state.wrapping_add(seed);
        r.next_u32();
        r
    }

    fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(6364136223846793005).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    fn next_f64(&mut self) -> f64 {
        let hi = (self.next_u32() as u64) << 21;
        let lo = (self.next_u32() >> 11) as u64;
        ((hi | lo) as f64) / ((1u64 << 53) as f64)
    }
}

impl UserData for LRandom {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_meta_field(MetaMethod::Type, "Random");
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method_mut("NextNumber", |_, t, (a, b): (Option<f64>, Option<f64>)| {
            let (lo, hi) = (a.unwrap_or(0.0), b.unwrap_or(1.0));
            Ok(lo + (hi - lo) * t.next_f64())
        });
        m.add_method_mut("NextInteger", |_, t, (a, b): (f64, f64)| {
            let (lo, hi) = (a.floor() as i64, b.floor() as i64);
            if hi < lo {
                return Err(rt(
                    "invalid argument #2 to 'NextInteger' (interval is empty)",
                ));
            }
            let span = (hi - lo + 1) as f64;
            Ok(lo + (t.next_f64() * span).floor() as i64)
        });
        m.add_method_mut("NextUnitVector", |_, t, ()| {
            let z = t.next_f64() * 2.0 - 1.0;
            let a = t.next_f64() * std::f64::consts::TAU;
            let r = (1.0 - z * z).sqrt();
            Ok(LVector3(Vec3::new(
                (r * a.cos()) as f32,
                (r * a.sin()) as f32,
                z as f32,
            )))
        });
        m.add_method_mut("Shuffle", |_, t, list: Table| {
            let n = list.raw_len();
            for i in (2..=n).rev() {
                let j = 1 + (t.next_f64() * i as f64).floor() as usize;
                let a: Value = list.raw_get(i)?;
                let b: Value = list.raw_get(j)?;
                list.raw_set(i, b)?;
                list.raw_set(j, a)?;
            }
            Ok(())
        });
        m.add_method("Clone", |_, t, ()| Ok(t.clone()));
    }
}

fn now_millis() -> i64 {
    rbx_base::time::SystemTime::now()
        .duration_since(rbx_base::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn udim2_arg(lua: &Lua, v: Option<&Value>) -> UDim2 {
    v.and_then(|v| LUDim2::from_lua(v.clone(), lua).ok())
        .map(|u| u.0)
        .unwrap_or_default()
}

fn int(v: Option<&Value>, default: i64) -> i64 {
    crate::convert::arg_number(v)
        .map(|n| n as i64)
        .unwrap_or(default)
}

fn register_more(lua: &Lua, g: &Table) -> Result<()> {
    let path = lua.create_table()?;
    path.set(
        "new",
        lua.create_function(|lua, args: mlua::MultiValue| {
            Ok(LPath2DControlPoint {
                position: udim2_arg(lua, args.get(0)),
                left: udim2_arg(lua, args.get(1)),
                right: udim2_arg(lua, args.get(2)),
            })
        })?,
    )?;
    g.set("Path2DControlPoint", path)?;

    let dt = lua.create_table()?;
    dt.set(
        "now",
        lua.create_function(|_, ()| Ok(LDateTime(now_millis())))?,
    )?;
    dt.set(
        "fromUnixTimestamp",
        lua.create_function(|_, t: f64| Ok(LDateTime((t * 1000.0).floor() as i64)))?,
    )?;
    dt.set(
        "fromUnixTimestampMillis",
        lua.create_function(|_, t: f64| Ok(LDateTime(t.floor() as i64)))?,
    )?;
    dt.set(
        "fromIsoDate",
        lua.create_function(|_, s: String| Ok(crate::datetime::parse_iso(&s).map(LDateTime)))?,
    )?;
    let from_parts = lua.create_function(|_, args: mlua::MultiValue| {
        Ok(LDateTime(crate::datetime::millis_from_parts(
            int(args.get(0), 1970),
            int(args.get(1), 1),
            int(args.get(2), 1),
            int(args.get(3), 0),
            int(args.get(4), 0),
            int(args.get(5), 0),
            int(args.get(6), 0),
        )))
    })?;
    dt.set("fromUniversalTime", from_parts.clone())?;
    dt.set("fromLocalTime", from_parts)?;
    g.set("DateTime", dt)?;

    let ti = lua.create_table()?;
    ti.set(
        "new",
        lua.create_function(|lua, args: mlua::MultiValue| {
            Ok(LTweenInfo {
                time: crate::convert::arg_number(args.get(0)).unwrap_or(1.0),
                style: enum_value_or(lua, args.get(1), "EasingStyle", 1),
                direction: enum_value_or(lua, args.get(2), "EasingDirection", 1),
                repeat_count: int(args.get(3), 0) as i32,
                reverses: matches!(args.get(4), Some(Value::Boolean(true))),
                delay: crate::convert::arg_number(args.get(5)).unwrap_or(0.0),
            })
        })?,
    )?;
    g.set("TweenInfo", ti)?;

    let rp = lua.create_table()?;
    rp.set(
        "new",
        lua.create_function(|_, ()| {
            Ok(LRaycastParams {
                collision_group: "Default".into(),
                ..Default::default()
            })
        })?,
    )?;
    g.set("RaycastParams", rp)?;

    let content = lua.create_table()?;
    content.set(
        "fromUri",
        lua.create_function(|_, s: String| Ok(LContent::Uri(s)))?,
    )?;
    content.set(
        "fromObject",
        lua.create_function(|lua, v: Value| {
            crate::runtime::instance_from_value(lua, &v)
                .map(LContent::Object)
                .ok_or_else(|| rt("Content.fromObject expects an Object"))
        })?,
    )?;
    content.set("none", LContent::None)?;
    g.set("Content", content)?;

    let random = lua.create_table()?;
    random.set(
        "new",
        lua.create_function(|_, seed: Option<f64>| {
            Ok(LRandom::new(
                seed.map(|s| s as i64 as u64)
                    .unwrap_or_else(|| now_millis() as u64),
            ))
        })?,
    )?;
    g.set("Random", random)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_colors_parse_short_and_long_forms() {
        assert_eq!(parse_hex("#fff"), Some(Color3::new(1.0, 1.0, 1.0)));
        assert_eq!(parse_hex("000000"), Some(Color3::new(0.0, 0.0, 0.0)));
        assert_eq!(parse_hex("zz"), None);
    }

    #[test]
    fn font_enum_table_matches_client() {
        let legacy = font_from_enum(0).expect("legacy");
        assert_eq!(legacy.family, "rbxasset://fonts/families/LegacyArial.json");
        let bold = font_from_enum(48).expect("builder sans bold");
        assert_eq!((bold.weight, bold.style), (700, 0));
    }
}
