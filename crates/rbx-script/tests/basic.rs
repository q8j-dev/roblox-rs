use parking_lot::Mutex;
use rbx_dom::DataModel;
use rbx_script::{Engine, Identity, MessageType, ScriptRuntime};
use std::sync::Arc;

fn runtime() -> ScriptRuntime {
    let engine = Arc::new(Mutex::new(Engine::new(DataModel::new())));
    match ScriptRuntime::new(engine) {
        Ok(r) => r,
        Err(e) => panic!("runtime: {e}"),
    }
}

fn outputs(rt: &ScriptRuntime) -> Vec<(MessageType, String)> {
    rt.engine
        .lock()
        .output
        .iter()
        .map(|m| (m.kind, m.text.clone()))
        .collect()
}

fn run(rt: &ScriptRuntime, src: &str) {
    if let Err(e) = rt.execute(src, "Test", Identity::GameScript) {
        panic!("execute: {e}");
    }
}

#[test]
fn values_and_printing() {
    let rt = runtime();
    run(
        &rt,
        r#"
print(Vector3.new(1, 2, 3))
print(Vector3.new(0.1, 0, 0))
local cf = CFrame.new(1, 2, 3) * CFrame.Angles(0, math.pi / 2, 0)
print(cf.p, cf.lookVector.X < -0.99)
print(BrickColor.new("Bright red").Number, BrickColor.new(21).Name)
print(UDim2.new(0.5, 10, 1, -5))
print(Enum.Material.Plastic, Enum.Material.Plastic.Value)
print(Color3.new(1, 0.5, 0))
"#,
    );
    let out = outputs(&rt);
    let texts: Vec<&str> = out.iter().map(|(_, t)| t.as_str()).collect();
    assert_eq!(texts[0], "1, 2, 3");
    assert_eq!(texts[1], "0.100000001, 0, 0");
    assert_eq!(texts[2], "1, 2, 3 true");
    assert_eq!(texts[3], "21 Bright red");
    assert_eq!(texts[4], "{0.5, 10}, {1, -5}");
    assert_eq!(texts[5], "Enum.Material.Plastic 256");
    assert_eq!(texts[6], "1, 0.5, 0");
}

#[test]
fn instances_properties_and_events() {
    let rt = runtime();
    run(
        &rt,
        r#"
local p = Instance.new("Part")
p.Name = "Brick"
p.Size = Vector3.new(4, 2, 4)
local changes = {}
p.Changed:connect(function(prop) table.insert(changes, prop) end)
p.Anchored = true
p.Parent = workspace
print(workspace.Brick == p, p:GetFullName(), p.Size, #changes > 0)
print(game:GetService("Workspace") == workspace, p:IsA("BasePart"), p:IsA("Model"))
local ok, err = pcall(function() return p.NotThere end)
print(ok, err)
local m = Instance.new("Model", workspace)
p.Parent = m
print(m:FindFirstChild("Brick") == p, #m:GetChildren(), workspace:FindFirstChild("Brick", true) == p)
local c = m:Clone()
print(c.Brick ~= p, c.Brick.Size)
p:Destroy()
print(p.Parent, pcall(function() p.Parent = workspace end))
"#,
    );
    let out = outputs(&rt);
    let texts: Vec<&str> = out.iter().map(|(_, t)| t.as_str()).collect();
    assert_eq!(
        texts[0], "true Workspace.Brick 4, 2.4000001, 4 true",
        "{texts:?}"
    );
    assert_eq!(texts[1], "true true false");
    assert!(
        texts[2].starts_with("false")
            && texts[2].contains("NotThere is not a valid member of Part"),
        "{}",
        texts[2]
    );
    assert_eq!(texts[3], "true 1 true");
    assert_eq!(texts[4], "true 4, 2.4000001, 4");
    assert!(texts[5].starts_with("nil false"), "{}", texts[5]);
}

#[test]
fn waits_and_signals() {
    let rt = runtime();
    run(
        &rt,
        r#"
local e = Instance.new("BindableEvent")
spawn(function()
    local v = e.Event:wait()
    print("got", v)
end)
delay(0.1, function() e:Fire(42) end)
local t, g = wait(0.5)
print("waited", t >= 0.5, t, g)
"#,
    );
    for _ in 0..60 {
        rt.step(1.0 / 30.0);
    }
    let out = outputs(&rt);
    let texts: Vec<&str> = out.iter().map(|(_, t)| t.as_str()).collect();
    assert!(texts.contains(&"got 42"), "{texts:?}");
    assert!(
        texts.iter().any(|t| t.starts_with("waited true")),
        "{texts:?}"
    );
}

#[test]
fn modules_and_errors() {
    let rt = runtime();
    {
        let mut e = rt.engine.lock();
        let ws = e.dm.get_service("Workspace").expect("ws");
        let m = e.dm.create("ModuleScript").expect("module");
        e.dm.set_name(m, "Mod");
        e.dm.set(
            m,
            "Source",
            rbx_dom::Variant::ProtectedString("return {value = 7}".into()),
        )
        .expect("source");
        e.dm.set_parent(m, Some(ws)).expect("parent");
    }
    run(
        &rt,
        r#"
local a = require(workspace.Mod)
local b = require(workspace.Mod)
print(a.value, a == b)
error("boom")
"#,
    );
    let out = outputs(&rt);
    assert_eq!(out[0].1, "7 true");
    assert_eq!(out[1].0, MessageType::Error);
    assert!(out[1].1.contains("Test:5: boom"), "{}", out[1].1);
}

#[test]
fn script_source_is_never_loaded_as_bytecode() {
    let rt = runtime();
    let bytecode = "\u{0}print('bytecode ran')";
    let err = rt
        .execute(bytecode, "Test", Identity::GameScript)
        .expect_err("bytecode source must be rejected");
    assert!(!err.to_string().contains("bytecode ran"));
    assert!(!outputs(&rt).iter().any(|(_, t)| t.contains("bytecode ran")));
}
