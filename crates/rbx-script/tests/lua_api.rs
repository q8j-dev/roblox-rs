mod common;

use common::Fixture;
use rbx_script::Identity;

#[test]
fn vector3_is_close_num_args() {
    let f = Fixture::new();
    assert!(f.execute("return Vector3.new(0,0,0):isClose()").is_err());
    assert!(
        f.execute("return Vector3.new(.00002,0,0):isClose()")
            .is_err()
    );
    assert!(f.bool("return Vector3.new(1,2,3):isClose(Vector3.new(1,2,3))"));
    assert!(f.bool("return Vector3.new(1.00001,2,3):isClose(Vector3.new(1,2,3), .00001)"));
}

#[test]
fn vector3_is_close_precision() {
    let f = Fixture::new();
    assert!(f.bool("return Vector3.new(1.00001,2,3):isClose(Vector3.new(1,2,3))"));
    assert!(!f.bool("return Vector3.new(1.00001,2,3):isClose(Vector3.new(1,2,3), 0)"));
    assert!(!f.bool("return Vector3.new(1.00002,2,3):isClose(Vector3.new(1,2,3), .00001)"));
    assert!(f.bool("return Vector3.new(1.00002,2,3):isClose(Vector3.new(1,2,3), .00002)"));
    assert!(f.bool("return Vector3.new(1,2.00003,3):isClose(Vector3.new(1,2,3), .00002)"));
    assert!(f.bool("return Vector3.new(1,2,3):isClose(Vector3.new(1,2.00003,3), .00002)"));
    assert!(f.bool("return Vector3.new(1,2,3):isClose(Vector3.new(2,3,4), 1)"));
}

#[test]
fn vector3_negative_values() {
    let f = Fixture::new();
    assert!(f.bool("return Vector3.new(-1,-2,-3):isClose(Vector3.new(-1,-2,-3), .00001)"));
    assert!(f.bool("return Vector3.new(-1.00001,-2,-3):isClose(Vector3.new(-1,-2,-3), .00001)"));
    assert!(!f.bool("return Vector3.new(-1,-2.00005,-3):isClose(Vector3.new(-1,-2,-3), .00001)"));
}

#[test]
fn vector3_multiple_axis_diff() {
    let f = Fixture::new();
    assert!(f.bool("return Vector3.new(1.00001,2.00002,3):isClose(Vector3.new(1,2,3), .00001)"));
    assert!(!f.bool("return Vector3.new(1.00005,2.00002,3):isClose(Vector3.new(1,2,3), .00001)"));
    assert!(!f.bool("return Vector3.new(1.00001,2.00005,3):isClose(Vector3.new(1,2,3), .00001)"));
    assert!(!f.bool("return Vector3.new(1.00005,2.00005,3):isClose(Vector3.new(1,2,3), .00001)"));
}

#[test]
fn vector3_negative_epsilon() {
    let f = Fixture::new();
    assert!(f.bool("return Vector3.new(1,2,3):isClose(Vector3.new(1,2,3), -.00001)"));
    assert!(!f.bool("return Vector3.new(1,2.00005,3):isClose(Vector3.new(1,2,3), -.00001)"));
}

fn hack_wait(f: &Fixture, hack: &str) {
    let _ = f.execute_as(hack, Identity::GameScript);
    let r = f
        .execute_as("return type(wait) == 'function'", Identity::GameScript)
        .expect("run");
    assert!(
        matches!(r.first(), Some(mlua::Value::Boolean(true))),
        "{hack} changed wait"
    );
}

fn assert_script_true(f: &Fixture, src: &str) {
    let r = f
        .execute_as(src, Identity::GameScript)
        .unwrap_or_else(|e| panic!("{src}: {e}"));
    assert!(
        matches!(r.first(), Some(mlua::Value::Boolean(true))),
        "{src}"
    );
}

#[test]
fn sandbox_hack_getfenv() {
    let f = Fixture::new();
    hack_wait(&f, "getfenv(0).wait = 'hacko!'");
    hack_wait(
        &f,
        "_G = getmetatable(getfenv(1)).__index _G._G = _G _G.wait = 'Hacko'",
    );
}

#[test]
fn sandbox_hack_global() {
    let f = Fixture::new();
    hack_wait(&f, "_G.wait = 'hacko!'");
}

#[test]
fn sandbox_hack_math_library() {
    let f = Fixture::new();
    assert_script_true(
        &f,
        "b = pcall(function() rawset(math, 'sin', 'hacko!') end) return not b",
    );
    assert_script_true(&f, "return getmetatable(math) == nil");
}

#[test]
fn sandbox_hack_string_metatable() {
    let f = Fixture::new();
    assert_script_true(&f, "return getmetatable('') == 'The metatable is locked'");
}

#[test]
fn sandbox_lua_timeout_setting() {
    let f = Fixture::new();
    f.execute("game:GetService('ScriptContext'):SetTimeout(2)")
        .expect("timeout");
    let ws = f.workspace();
    let start = std::time::Instant::now();
    let script = f.add_script("Script", "script.Name = 'foo' while true do end", ws);
    f.run();
    f.step_seconds(0.1);
    let elapsed = start.elapsed().as_secs_f64();
    assert_eq!(f.name_of(script), "foo");
    assert!(
        (2.0..=4.0).contains(&elapsed),
        "interrupted after {elapsed}s"
    );
    assert!(f.errors().iter().any(|e| e.contains("Game script timeout")));
}

#[test]
fn yielding_wrong_resumer_when_yielding_in_pcall() {
    let f = Fixture::new();
    let ws = f.workspace();
    let script = f.add_script(
        "Script",
        "pcall(function () return Workspace:WaitForChild('PartA') end) local p = Workspace:WaitForChild('PartB') script.Name = p.Name",
        ws,
    );
    f.run();
    f.execute("local p = Instance.new('Part') p.Name = 'PartA' p.Parent = Workspace")
        .expect("part");
    f.step_seconds(0.5);
    assert_eq!(f.name_of(script), "Script");
    f.execute("local p = Workspace.PartA p.Parent = nil p.Name = 'PartB' p.Parent = Workspace")
        .expect("move");
    f.step_seconds(0.5);
    assert_eq!(f.name_of(script), "PartB");
}

#[test]
fn yielding_ypcall_c_function_with_yield() {
    let f = Fixture::new();
    let ws = f.workspace();
    let script = f.add_script(
        "Script",
        "succ, p = ypcall(Workspace.WaitForChild, Workspace, 'PartA') script.Name = p.Name",
        ws,
    );
    f.run();
    f.step_seconds(0.15);
    assert_eq!(f.name_of(script), "Script");
    f.execute("local p = Instance.new('Part') p.Name = 'PartA' p.Parent = Workspace")
        .expect("part");
    f.step_seconds(0.5);
    assert_eq!(f.name_of(script), "PartA");
}

#[test]
fn yielding_perf_pcall_print() {
    let f = Fixture::new();
    let ws = f.workspace();
    let start = std::time::Instant::now();
    f.add_script(
        "Script",
        "i = 8000000 while i > 0 do pcall(print) i = i - 1 end",
        ws,
    );
    f.run();
    assert!(f.errors().is_empty(), "{:?}", f.errors());
    assert!(start.elapsed().as_secs_f64() < 60.0);
}

#[test]
fn tokenizer_parse() {
    use rbx_script::api::test::args_in_bracket;
    assert_eq!(args_in_bracket("( a() )"), vec!["a()"]);
    assert_eq!(
        args_in_bracket("( a , 34+s(r, 6), 'd\\'fg'\t, \"sd'fs\")"),
        vec!["a", "34+s(r, 6)", "'d\\'fg'", "\"sd'fs\""]
    );
    assert_eq!(
        args_in_bracket("( function() return 1 end )"),
        vec!["function() return 1 end"]
    );
}
