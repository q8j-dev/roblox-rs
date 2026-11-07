mod common;

use common::Fixture;
use rbx_dom::{InstanceId, Variant};

fn add(f: &Fixture, class: &str, name: &str, source: &str) -> InstanceId {
    let ws = f.workspace();
    let mut e = f.rt.engine.lock();
    let s = e.dm.create(class).expect("create");
    e.dm.set_name(s, name);
    let _ =
        e.dm.set(s, "Source", Variant::ProtectedString(source.to_string()));
    e.dm.set_parent(s, Some(ws)).expect("parent");
    s
}

fn start(f: &Fixture) {
    f.rt.scan_scripts();
    f.run();
    f.step_seconds(0.05);
}

fn child(f: &Fixture, name: &str) -> Option<InstanceId> {
    let ws = f.workspace();
    let e = f.rt.engine.lock();
    e.dm.find_first_child(ws, name, false)
}

fn has_part(f: &Fixture) -> bool {
    let ws = f.workspace();
    let e = f.rt.engine.lock();
    e.dm.find_first_descendant_which_is_a(ws, "BasePart")
        .is_some()
}

fn rename(f: &Fixture, id: InstanceId, suffix: &str) {
    let mut e = f.rt.engine.lock();
    let n = format!("{}{suffix}", e.dm.name_of(id));
    e.dm.set_name(id, &n);
}

#[test]
fn modules_do_not_run_on_their_own() {
    let f = Fixture::new();
    add(
        &f,
        "ModuleScript",
        "Module",
        "Instance.new(\"BUG\", Workspace) return 1",
    );
    start(&f);
    assert!(child(&f, "BUG").is_none());
}

#[test]
fn scope_of_script_in_returned_functions() {
    let f = Fixture::new();
    add(
        &f,
        "ModuleScript",
        "Module",
        "return function () return script.Name .. \"2\" end",
    );
    add(
        &f,
        "Script",
        "Caller",
        "local v = Instance.new(\"StringValue\", Workspace) v.Name = require(Workspace.Module)()",
    );
    start(&f);
    assert!(child(&f, "Module2").is_some());
    assert!(child(&f, "Caller2").is_none());
}

const COUNTER: &str = "local count = 0 return function (amt) count = count + amt if count > 10 then local val = Instance.new(\"IntValue\", Workspace) val.Value = count val.Name = \"\" .. count end end";

#[test]
fn return_stateful_closure() {
    let f = Fixture::new();
    add(&f, "ModuleScript", "Module", COUNTER);
    add(
        &f,
        "Script",
        "Loader",
        "local fun = require(script.Parent.Module) fun(9) fun(7) fun(-5)",
    );
    start(&f);
    assert!(child(&f, "9").is_none());
    let v16 = child(&f, "16").expect("16");
    let v11 = child(&f, "11").expect("11");
    let e = f.rt.engine.lock();
    assert_eq!(
        e.dm.get(v16, "Value").ok().and_then(|v| v.as_i32()),
        Some(16)
    );
    assert_eq!(
        e.dm.get(v11, "Value").ok().and_then(|v| v.as_i32()),
        Some(11)
    );
}

#[test]
fn shared_load_result() {
    let f = Fixture::new();
    add(&f, "ModuleScript", "Module", COUNTER);
    add(
        &f,
        "Script",
        "Loader1",
        "local fun = require(script.Parent.Module) fun(9)",
    );
    add(
        &f,
        "Script",
        "Loader2",
        "local fun = require(script.Parent.Module) fun(5)",
    );
    start(&f);
    let v14 = child(&f, "14").expect("14");
    assert_eq!(
        f.rt.engine
            .lock()
            .dm
            .get(v14, "Value")
            .ok()
            .and_then(|v| v.as_i32()),
        Some(14)
    );
}

#[test]
fn clone_does_not_share_result() {
    let f = Fixture::new();
    add(&f, "ModuleScript", "Module", COUNTER);
    add(
        &f,
        "Script",
        "Loader1",
        "local fun = require(script.Parent.Module) fun(9)",
    );
    add(
        &f,
        "Script",
        "Loader2",
        "local fun = require(script.Parent.Module:Clone()) fun(5)",
    );
    start(&f);
    assert!(child(&f, "14").is_none());
}

#[test]
fn load_module_from_module() {
    let f = Fixture::new();
    add(
        &f,
        "ModuleScript",
        "DeepModule",
        "return function (v1) return function (v2) return \"Deep\" .. v2 .. \"Orig\" .. v1 end end",
    );
    add(
        &f,
        "ModuleScript",
        "Module",
        "return require(script.Parent.DeepModule)(\"L1String\")",
    );
    add(
        &f,
        "Script",
        "Loader",
        "local v = Instance.new(\"Part\", Workspace) v.Name = require(script.Parent.Module)(\"L2String\")",
    );
    start(&f);
    assert!(child(&f, "DeepL2StringOrigL1String").is_some());
}

#[test]
fn module_script_does_not_leak_globals() {
    let f = Fixture::new();
    let caller = add(
        &f,
        "ModuleScript",
        "CallerModule",
        "shallowVariable = 'FailureDepth1' require(script.Parent.CalleeModule) success = pcall(function() script.Name = deepVariable end) if not success then script.Name = 'RanCorrectly' end return 1",
    );
    let callee = add(
        &f,
        "ModuleScript",
        "CalleeModule",
        "deepVariable = 'FailureDepth2' return deepVariable",
    );
    let loader = add(
        &f,
        "Script",
        "Loader",
        "require(script.Parent.CallerModule) success2 = pcall(function() script.Name = shallowVariable end) success3 = pcall(function() script.Name = deepVariable end) if not success2 and not success3 then script.Name = 'TopLevelCorrect' end",
    );
    start(&f);
    assert_eq!(f.name_of(caller), "RanCorrectly");
    assert_eq!(f.name_of(callee), "CalleeModule");
    assert_eq!(f.name_of(loader), "TopLevelCorrect");
}

#[test]
fn load_module_from_module_with_yields() {
    let f = Fixture::new();
    add(
        &f,
        "ModuleScript",
        "DeepModule",
        "wait() return function (v1) wait() return function (v2) wait() return \"Deep\" .. v2 .. \"Orig\" .. v1 end end",
    );
    add(
        &f,
        "ModuleScript",
        "Module",
        "wait() local fun = require(script.Parent.DeepModule) wait() return fun(\"L1String\")",
    );
    add(
        &f,
        "Script",
        "Loader",
        "local v = Instance.new(\"Part\") v.Name = require(script.Parent.Module)(\"L2String\") v.Parent = Workspace",
    );
    start(&f);
    f.step_seconds(0.5);
    assert!(child(&f, "DeepL2StringOrigL1String").is_some());
}

#[test]
fn module_deletes_script_that_required_it() {
    let f = Fixture::new();
    add(
        &f,
        "ModuleScript",
        "Module",
        "wait() Workspace.Caller.Parent = nil return 1",
    );
    add(
        &f,
        "Script",
        "Caller",
        "require(Workspace.Module) local v = Instance.new(\"Part\", Workspace)",
    );
    start(&f);
    f.step_seconds(0.5);
    assert!(child(&f, "Caller").is_none());
    assert!(!has_part(&f));
}

fn caller_stopped(module_source: &str, wait: f64) {
    let f = Fixture::new();
    add(&f, "ModuleScript", "Module", module_source);
    add(
        &f,
        "Script",
        "Caller",
        "require(Workspace.Module) local v = Instance.new(\"Part\", Workspace)",
    );
    start(&f);
    f.step_seconds(wait);
    assert!(!has_part(&f), "caller kept running after: {module_source}");
}

#[test]
fn parse_error_in_module_script() {
    caller_stopped("*ThisIS 12312 invalid syntax!", 0.0);
}

#[test]
fn immediate_error_in_module_script() {
    caller_stopped("local foo = nil foo.Parent = Workspace", 0.0);
}

#[test]
fn error_in_module_script_after_yield() {
    caller_stopped("local foo = nil wait() foo.Parent = Workspace", 0.5);
}

#[test]
fn module_does_not_return_result() {
    caller_stopped("local foo = 2", 0.0);
}

#[test]
fn error_in_nested_module() {
    let f = Fixture::new();
    add(
        &f,
        "ModuleScript",
        "DeepModule",
        "local f = nil f.Parent = Workspace",
    );
    add(
        &f,
        "ModuleScript",
        "Module",
        "local fun = require(script.Parent.DeepModule) return fun(\"L1String\")",
    );
    add(
        &f,
        "Script",
        "Loader",
        "local v = Instance.new(\"Part\") v.Name = require(script.Parent.Module)(\"L2String\") v.Parent = Workspace",
    );
    start(&f);
    f.step_seconds(0.5);
    assert!(!has_part(&f));
}

#[test]
fn connections_in_modules_survive_when_includer_stops() {
    let f = Fixture::new();
    let ws = f.workspace();
    let part = {
        let mut e = f.rt.engine.lock();
        let p = e.dm.create("Part").expect("part");
        let _ = e.dm.set(p, "Anchored", Variant::Bool(true));
        e.dm.set_parent(p, Some(ws)).expect("parent");
        p
    };
    add(
        &f,
        "ModuleScript",
        "Module",
        "local p = Workspace.Part local s = Workspace.Script p.Changed:connect(function() s.Name = s.Name .. 'x' end) return 1",
    );
    let loader = add(&f, "Script", "Script", "require(Workspace.Module)");
    start(&f);
    rename(&f, part, "x");
    f.step_seconds(0.05);
    assert_eq!(f.name_of(loader), "Scriptx");
    f.rt.engine
        .lock()
        .dm
        .set(loader, "Disabled", Variant::Bool(true))
        .expect("disable");
    f.step_seconds(0.05);
    rename(&f, part, "x");
    f.step_seconds(0.05);
    assert_eq!(f.name_of(loader), "Scriptxx");
}

#[test]
fn connections_in_modules_break_on_module_error() {
    let f = Fixture::new();
    let ws = f.workspace();
    let part = {
        let mut e = f.rt.engine.lock();
        let p = e.dm.create("Part").expect("part");
        let _ = e.dm.set(p, "Anchored", Variant::Bool(true));
        e.dm.set_parent(p, Some(ws)).expect("parent");
        p
    };
    add(
        &f,
        "ModuleScript",
        "Module",
        "local p = Workspace.Part local s = Workspace.Script p.Changed:connect(function() s.Name = s.Name .. 'x' end) p = nil p.Name = 'foo' return 1",
    );
    let loader = add(&f, "Script", "Script", "require(Workspace.Module)");
    start(&f);
    rename(&f, part, "x");
    f.step_seconds(0.05);
    assert_eq!(f.name_of(loader), "Script");
}

#[test]
fn require_many_modules() {
    let f = Fixture::new();
    add(&f, "ModuleScript", "Module", "print('Hio!') return 1");
    add(
        &f,
        "Script",
        "Loader",
        "for i=1,100 do local m = Workspace.Module:Clone() m.Parent = Workspace m.Name = m.Name .. i require(m) end",
    );
    start(&f);
    for i in 1..=100 {
        assert!(child(&f, &format!("Module{i}")).is_some());
    }
    assert_eq!(
        f.output().iter().filter(|o| o.as_str() == "Hio!").count(),
        100
    );
    assert!(f.errors().is_empty(), "{:?}", f.errors());
}
