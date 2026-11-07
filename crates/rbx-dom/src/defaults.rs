use rbx_math::{BrickColor, CFrame, Color3, NumberRange, UDim2, Vec2, Vec3};
use rbx_reflection::{PropertyDescriptor, ValueType, Variant, database};
use std::collections::HashMap;
use std::sync::LazyLock;

static GENERATED: LazyLock<HashMap<String, HashMap<String, Variant>>> =
    LazyLock::new(
        || match serde_json::from_str(include_str!("../data/defaults.json")) {
            Ok(v) => v,
            Err(e) => panic!("embedded defaults are invalid: {e}"),
        },
    );

fn brick(n: i64) -> Color3 {
    BrickColor::new(n).color3()
}

fn manual(class: &str, prop: &str) -> Option<Variant> {
    Some(match (class, prop) {
        ("GuiObject", "BorderSizePixel") => Variant::Int(1),
        ("GuiObject", "BorderColor") => Variant::BrickColor(BrickColor::new(26)),
        ("GuiObject", "BackgroundColor") => Variant::BrickColor(BrickColor::new(194)),
        ("GuiObject", "Visible") => Variant::Bool(true),
        ("GuiButton" | "TextBox", "Active") => Variant::Bool(true),
        ("GuiButton", "AutoButtonColor") => Variant::Bool(true),
        ("TextLabel", "Text") => Variant::String("Label".into()),
        ("TextButton", "Text") => Variant::String("Button".into()),
        ("TextBox", "Text") => Variant::String("TextBox".into()),
        ("TextBox", "ClearTextOnFocus") => Variant::Bool(true),
        ("TextLabel" | "TextButton" | "TextBox", "TextColor3") => Variant::Color3(brick(26)),
        ("TextLabel" | "TextButton" | "TextBox", "TextColor") => {
            Variant::BrickColor(BrickColor::new(26))
        }
        ("TextLabel" | "TextButton" | "TextBox", "TextXAlignment") => Variant::Enum(2),
        ("TextLabel" | "TextButton" | "TextBox", "TextYAlignment") => Variant::Enum(1),
        ("TextLabel" | "TextButton" | "TextBox", "TextStrokeTransparency") => Variant::Float(1.0),
        ("ImageLabel" | "ImageButton", "ImageColor3") => {
            Variant::Color3(Color3::new(1.0, 1.0, 1.0))
        }
        ("ScrollingFrame", "ScrollBarThickness") => Variant::Int(12),
        ("ScrollingFrame", "ScrollingEnabled") => Variant::Bool(true),
        ("ScrollingFrame", "TopImage") => {
            Variant::Content("rbxasset://textures/ui/Scroll/scroll-top.png".into())
        }
        ("ScrollingFrame", "MidImage") => {
            Variant::Content("rbxasset://textures/ui/Scroll/scroll-middle.png".into())
        }
        ("ScrollingFrame", "BottomImage") => {
            Variant::Content("rbxasset://textures/ui/Scroll/scroll-bottom.png".into())
        }
        ("BillboardGui" | "SurfaceGui", "Enabled") => Variant::Bool(true),
        ("SurfaceGui", "CanvasSize") => Variant::Vector2(Vec2::new(800.0, 600.0)),
        ("SurfaceGui", "ToolPunchThroughDistance") => Variant::Float(0.0),
        ("SurfaceGui", "Face") => Variant::Enum(5),
        ("SurfaceGui", "Active") => Variant::Bool(true),
        ("UserGameSettings", "MasterVolume" | "MouseSensitivity") => Variant::Float(1.0),
        ("UserGameSettings", "StartMaximized") => Variant::Bool(true),
        ("UserInputService", "KeyboardEnabled" | "MouseEnabled" | "MouseIconEnabled") => {
            Variant::Bool(true)
        }
        ("Players", "ClassicChat") => Variant::Bool(true),
        ("Players", "BubbleChat") => Variant::Bool(false),
        ("Fire" | "Smoke" | "Sparkles" | "ParticleEmitter", "Enabled") => Variant::Bool(true),
        ("ParticleEmitter", "Texture") => {
            Variant::Content("rbxasset://textures/particles/sparkles_main.dds".into())
        }
        ("ParticleEmitter", "Color") => Variant::ColorSequence(rbx_math::ColorSequence::constant(
            Color3::new(1.0, 1.0, 1.0),
        )),
        ("ParticleEmitter", "Transparency") => {
            Variant::NumberSequence(rbx_math::NumberSequence::constant(0.0))
        }
        ("ParticleEmitter", "Size") => {
            Variant::NumberSequence(rbx_math::NumberSequence::constant(1.0))
        }
        ("ParticleEmitter", "Rate") => Variant::Float(20.0),
        ("ParticleEmitter", "Speed") => Variant::NumberRange(NumberRange::new(5.0, 5.0)),
        ("ParticleEmitter", "Lifetime") => Variant::NumberRange(NumberRange::new(5.0, 10.0)),
        ("ParticleEmitter", "Rotation" | "RotSpeed") => {
            Variant::NumberRange(NumberRange::new(0.0, 0.0))
        }
        ("ParticleEmitter", "EmissionDirection") => Variant::Enum(1),
        ("SpotLight" | "SurfaceLight", "Angle") => Variant::Float(90.0),
        ("SpotLight" | "SurfaceLight", "Face") => Variant::Enum(5),
        ("BasePart", "Friction") => Variant::Float(0.3),
        ("BasePart", "Elasticity") => Variant::Float(0.5),
        ("BasePart", "size") | ("BasePart", "Size") => Variant::Vector3(Vec3::new(4.0, 1.2, 2.0)),
        ("BasePart", "Material") => Variant::Enum(256),
        ("BasePart", "TopSurface") => Variant::Enum(3),
        ("BasePart", "BottomSurface") => Variant::Enum(4),
        ("BasePart", "CanCollide") => Variant::Bool(true),
        (
            "BasePart",
            "TopParamA" | "BottomParamA" | "LeftParamA" | "RightParamA" | "FrontParamA"
            | "BackParamA",
        ) => Variant::Float(-0.5),
        (
            "BasePart",
            "TopParamB" | "BottomParamB" | "LeftParamB" | "RightParamB" | "FrontParamB"
            | "BackParamB",
        ) => Variant::Float(0.5),
        ("BasePart", "Anchored") => Variant::Bool(false),
        ("BasePart", "Elasticity_") => Variant::Float(0.5),
        ("FormFactorPart", "formFactorRaw") | ("FormFactorPart", "FormFactor") => Variant::Enum(1),
        ("Part", "shape") | ("Part", "Shape") => Variant::Enum(1),
        ("WedgePart", "size") => Variant::Vector3(Vec3::new(4.0, 1.2, 2.0)),
        ("CornerWedgePart", "size") => Variant::Vector3(Vec3::new(2.0, 2.0, 2.0)),
        ("TrussPart", "size") => Variant::Vector3(Vec3::new(2.0, 2.0, 2.0)),
        ("SpawnLocation", "size") => Variant::Vector3(Vec3::new(4.0, 1.2, 2.0)),
        ("Seat", "size") | ("VehicleSeat", "size") => Variant::Vector3(Vec3::new(4.0, 1.2, 2.0)),
        ("Terrain", "WaterColor") => Variant::Color3(Color3::new(0.05, 0.33, 0.36)),
        ("Workspace", "FallenPartsDestroyHeight") => Variant::Float(-500.0),
        ("Workspace", "PhysicalPropertiesMode") => Variant::Enum(1),
        ("Lighting", "TimeOfDay") => Variant::String("14:00:00".into()),
        ("Lighting", "ClockTime") => Variant::Float(14.0),
        ("Lighting", "GeographicLatitude") => Variant::Float(41.7333),
        ("TestService", "Timeout") => Variant::Double(10.0),
        ("TestService", "IsPhysicsEnvironmentalThrottled")
        | ("TestService", "IsSleepAllowed")
        | ("TestService", "Is30FpsThrottleEnabled")
        | ("TestService", "AutoRuns") => Variant::Bool(true),
        ("TestService", "PhysicsSendRate") => Variant::Float(20.0),
        ("FunctionalTest", "Timeout") => Variant::Double(60.0),
        ("FunctionalTest", "Description") => Variant::String("?".into()),
        ("FunctionalTest", "AllowSleep")
        | ("FunctionalTest", "PhysicsEnvironmentalThrottle")
        | ("FunctionalTest", "Is30FpsThrottleEnabled") => Variant::Bool(true),
        ("Camera", "CFrame") | ("Camera", "CoordinateFrame") => Variant::CFrame(CFrame::look_at(
            Vec3::new(0.0, 20.0, 20.0),
            Vec3::ZERO,
            Vec3::Y,
        )),
        ("Camera", "Focus") | ("Camera", "focus") => {
            Variant::CFrame(CFrame::from_translation(Vec3::new(0.0, 0.0, -5.0)))
        }
        ("Camera", "FieldOfView") => Variant::Float(70.0),
        ("GuiObject", "BackgroundColor3") => Variant::Color3(brick(194)),
        ("GuiObject", "BorderColor3") => Variant::Color3(brick(26)),
        ("GuiObject", "Active") => Variant::Bool(false),
        ("GuiBase3d", "Color3") => Variant::Color3(brick(23)),
        ("SelectionBox", "SurfaceColor3") | ("SelectionSphere", "SurfaceColor3") => {
            Variant::Color3(brick(23))
        }
        ("Humanoid", "HealthDisplayDistance") | ("Humanoid", "NameDisplayDistance") => {
            Variant::Float(100.0)
        }
        ("Humanoid", "JumpPower") => Variant::Float(50.0),
        ("Player", "HealthDisplayDistance") | ("Player", "NameDisplayDistance") => {
            Variant::Float(100.0)
        }
        ("StarterPlayer", "HealthDisplayDistance") | ("StarterPlayer", "NameDisplayDistance") => {
            Variant::Float(100.0)
        }
        ("Player", "CameraMaxZoomDistance") | ("StarterPlayer", "CameraMaxZoomDistance") => {
            Variant::Float(400.0)
        }
        ("Player", "CameraMinZoomDistance") | ("StarterPlayer", "CameraMinZoomDistance") => {
            Variant::Float(0.5)
        }
        ("Player", "MaximumSimulationRadius") => Variant::Float(1000.0),
        ("Player", "SimulationRadius") => Variant::Float(80.0),
        ("Attachment", "Position") => Variant::Vector3(Vec3::ZERO),
        ("DataModelMesh", "Scale") => Variant::Vector3(Vec3::ONE),
        ("DataModelMesh", "Offset") => Variant::Vector3(Vec3::ZERO),
        ("DataModelMesh", "VertexColor") => Variant::Vector3(Vec3::ONE),
        ("BodyAngularVelocity", "MaxTorque") | ("BodyAngularVelocity", "maxTorque") => {
            Variant::Vector3(Vec3::splat(4000.0))
        }
        ("BodyGyro", "MaxTorque") | ("BodyGyro", "maxTorque") => {
            Variant::Vector3(Vec3::new(4e5, 0.0, 4e5))
        }
        ("BodyPosition", "MaxForce") | ("BodyPosition", "maxForce") => {
            Variant::Vector3(Vec3::splat(4000.0))
        }
        ("BodyVelocity", "MaxForce") | ("BodyVelocity", "maxForce") => {
            Variant::Vector3(Vec3::splat(4000.0))
        }
        ("RocketPropulsion", "MaxTorque") => Variant::Vector3(Vec3::new(4e5, 4e5, 0.0)),
        ("RocketPropulsion", "TargetOffset") => Variant::Vector3(Vec3::ZERO),
        ("ArcHandles", "Axes") => Variant::Axes(rbx_math::Axes(7)),
        ("Handles", "Faces") => Variant::Faces(rbx_math::Faces(63)),
        ("FloorWire", "TextureSize") | ("TextureTrail", "TextureSize") => {
            Variant::Vector2(Vec2::ONE)
        }
        ("PartOperation", "InitialSize") => Variant::Vector3(Vec3::ONE),
        ("Scale9Frame", "ScaleEdgeSize") => {
            Variant::Vector2int16(rbx_math::Vector2int16::default())
        }
        ("ScrollingFrame", "CanvasPosition") => Variant::Vector2(Vec2::ZERO),
        ("ScrollingFrame", "CanvasSize") => Variant::UDim2(UDim2::new(0.0, 0, 2.0, 0)),
        ("Sparkles", "SparkleColor") => {
            Variant::Color3(Color3::new(144.0 / 255.0, 25.0 / 255.0, 1.0))
        }
        ("Sky", "MoonTextureId") => Variant::Content("rbxasset://sky/moon.jpg".into()),
        ("Sky", "SunTextureId") => Variant::Content("rbxasset://sky/sun.jpg".into()),
        ("Sky", "SkyboxBk") => Variant::Content("rbxasset://textures/sky/sky512_bk.tex".into()),
        ("Sky", "SkyboxDn") => Variant::Content("rbxasset://textures/sky/sky512_dn.tex".into()),
        ("Sky", "SkyboxFt") => Variant::Content("rbxasset://textures/sky/sky512_ft.tex".into()),
        ("Sky", "SkyboxLf") => Variant::Content("rbxasset://textures/sky/sky512_lf.tex".into()),
        ("Sky", "SkyboxRt") => Variant::Content("rbxasset://textures/sky/sky512_rt.tex".into()),
        ("Sky", "SkyboxUp") => Variant::Content("rbxasset://textures/sky/sky512_up.tex".into()),
        ("UserGameSettings", "StartScreenPosition") => Variant::Vector2(Vec2::new(20.0, 20.0)),
        ("UserGameSettings", "StartScreenSize") => Variant::Vector2(Vec2::new(800.0, 600.0)),
        _ => return None,
    })
}

fn enum_default(name: &str) -> Variant {
    let value = database()
        .enum_desc(name)
        .map(|e| if e.is_valid(0) { 0 } else { e.default_value() })
        .unwrap_or(0);
    Variant::Enum(value)
}

pub fn default_value(class: &str, prop: &PropertyDescriptor) -> Variant {
    let db = database();
    for c in db.superclasses(class) {
        if let Some(v) = manual(&c.name, &prop.name) {
            return v;
        }
        if let Some(v) = GENERATED.get(&c.name).and_then(|m| m.get(&prop.name))
            && let Some(t) = prop.ty.variant_type()
            && let Some(v) = v.clone().coerce(t)
        {
            return v;
        }
    }
    match &prop.ty {
        ValueType::Enum(name) => enum_default(name),
        ValueType::Ref(_) => Variant::Ref(None),
        ValueType::Value(t) => Variant::default_for(*t),
        _ => Variant::Nil,
    }
}

pub fn default_name(class: &str) -> String {
    let db = database();
    for c in db.superclasses(class) {
        if let Some(Variant::String(s)) = GENERATED.get(&c.name).and_then(|m| m.get("Name")) {
            return s.clone();
        }
        if c.name == "Instance" {
            break;
        }
    }
    if db.is_a(class, "BasePart") && !matches!(class, "Terrain") {
        return "Part".to_string();
    }
    class.to_string()
}
