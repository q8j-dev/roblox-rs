mod common;

use common::Fixture;
use mlua::Value;

fn strings(v: &[Value]) -> Vec<String> {
    v.iter()
        .map(|x| match x {
            Value::String(s) => s.to_string_lossy().to_string(),
            Value::Number(n) => n.to_string(),
            Value::Integer(n) => n.to_string(),
            Value::Boolean(b) => b.to_string(),
            Value::Nil => "nil".into(),
            other => format!("{other:?}"),
        })
        .collect()
}

#[test]
fn instance_round_trip() {
    let f = Fixture::new();
    let r = f
        .execute("l = Instance.new('Part', workspace) return l")
        .expect("run");
    assert_eq!(r.len(), 1);
    assert!(matches!(r[0], Value::UserData(_)));
}

#[test]
fn ypcall_success() {
    let f = Fixture::new();
    for _ in 0..100 {
        let r = f
            .execute("return ypcall(function(a, b) return a, b, a .. b end, 'a', 'b')")
            .expect("run");
        assert_eq!(strings(&r), vec!["true", "a", "b", "ab"]);
    }
}

#[test]
fn ypcall_failure() {
    let f = Fixture::new();
    for _ in 0..100 {
        let r = f
            .execute("return ypcall(function() error('failure') end)")
            .expect("run");
        assert_eq!(r.len(), 2);
        assert!(matches!(r[0], Value::Boolean(false)));
        assert!(strings(&r)[1].contains("failure"));
    }
}

const DIVIDE: &str = "f = Instance.new('BindableFunction', workspace) f.Name = 'Divide' \
    f.OnInvoke = function(a, b) if b == 0 then error('failure') end return a / b end \
    g = Instance.new('BindableFunction', workspace) g.Name = 'AsyncDivide' \
    g.OnInvoke = function(a, b) wait(0.01) if b == 0 then error('failure') end return a / b end";

#[test]
fn ypcall_async_immediate() {
    let f = Fixture::new();
    f.execute(DIVIDE).expect("setup");
    let ok = f.execute("local f = workspace.Divide return ypcall(function() f:Invoke(2, 3) return f:Invoke(1, 2) end)").expect("run");
    assert_eq!(strings(&ok), vec!["true", "0.5"]);
    let bad = f.execute("local f = workspace.Divide return ypcall(function() f:Invoke(4, 4) return f:Invoke(2, 0) end)").expect("run");
    assert!(matches!(bad[0], Value::Boolean(false)));
    assert!(strings(&bad)[1].contains("failure"));
}

#[test]
fn ypcall_async_success_and_failure() {
    let f = Fixture::new();
    f.execute(DIVIDE).expect("setup");
    f.run();
    for _ in 0..20 {
        f.execute("_G.r = nil local g = workspace.AsyncDivide _G.r = {ypcall(function() g:Invoke(2, 3) return g:Invoke(1, 2) end)}").expect("run");
        f.step_seconds(0.2);
        assert_eq!(
            strings(&f.execute("return unpack(_G.r)").expect("read")),
            vec!["true", "0.5"]
        );
        f.execute("_G.r = nil local g = workspace.AsyncDivide _G.r = {ypcall(function() g:Invoke(2, 3) return g:Invoke(2, 0) end)}").expect("run");
        f.step_seconds(0.2);
        let r = f.execute("return unpack(_G.r)").expect("read");
        assert!(matches!(r[0], Value::Boolean(false)));
        assert!(strings(&r)[1].contains("failure"));
    }
}

#[test]
fn ypcall_async_success_nested() {
    let f = Fixture::new();
    f.execute(DIVIDE).expect("setup");
    f.run();
    f.execute(
        "local g = workspace.AsyncDivide \
         _G.r = {ypcall(function() local s, two = ypcall(function() wait(0.01) return g:Invoke(2, 1) end) return g:Invoke(1, two) end)}",
    )
    .expect("run");
    f.step_seconds(0.3);
    assert_eq!(
        strings(&f.execute("return unpack(_G.r)").expect("read")),
        vec!["true", "0.5"]
    );
}

#[test]
fn ypcall_bindable_function() {
    let f = Fixture::new();
    f.execute("f = Instance.new('BindableFunction') f.Name = 'F' f.Parent = Workspace")
        .expect("setup");
    f.run();
    for _ in 0..10 {
        f.execute(
            "local f = workspace.F _G.r = nil \
             Delay(0.3, function() f.OnInvoke = function() end end) \
             _G.r = {ypcall(function(a, b) f:Invoke() f.OnInvoke = nil return a / b end, 1, 2)}",
        )
        .expect("run");
        f.step_seconds(0.6);
        assert_eq!(
            strings(&f.execute("return unpack(_G.r)").expect("read")),
            vec!["true", "0.5"]
        );
    }
}

#[test]
fn async_failure_halts_thread() {
    let f = Fixture::new();
    f.execute(DIVIDE).expect("setup");
    f.run();
    f.execute("_G.called = false local g = workspace.AsyncDivide g:Invoke(2, 0) _G.called = true")
        .expect("run");
    f.step_seconds(0.5);
    assert!(!f.bool("return _G.called"));
}

#[test]
fn yield_functions() {
    let f = Fixture::new();
    f.execute(
        "v = Instance.new('BindableFunction', workspace) v.Name = 'YieldVoid' v.OnInvoke = function(x) wait(0.01) end \
         i = Instance.new('BindableFunction', workspace) i.Name = 'YieldInt' i.OnInvoke = function(x) wait(0.01) return 2 * x end",
    )
    .expect("setup");
    f.run();
    f.execute("local a = workspace.YieldVoid:Invoke(7) local b = workspace.YieldInt:Invoke(3) _G.r = {a == nil, b}").expect("run");
    f.step_seconds(0.3);
    assert_eq!(
        strings(&f.execute("return unpack(_G.r)").expect("read")),
        vec!["true", "6"]
    );
    f.execute("_G.n = select('#', workspace.YieldVoid:Invoke(7))")
        .expect("run");
    f.step_seconds(0.3);
    assert_eq!(f.number("return _G.n"), 0.0);
}

#[test]
fn tuple_round_trip() {
    let f = Fixture::new();
    let r = f
        .execute("local t = Instance.new('BindableFunction') t.OnInvoke = function(...) return ... end return t:Invoke('a','b','c',1,2,3)")
        .expect("run");
    assert_eq!(strings(&r), vec!["a", "b", "c", "1", "2", "3"]);
}

#[test]
fn empty_data_model_runs() {
    let f = Fixture::new();
    f.run();
    f.step_seconds(0.1);
    assert!(f.errors().is_empty());
}

#[test]
fn callbacks() {
    let f = Fixture::new();
    let r = f
        .execute(
            "l = Instance.new('BindableFunction') l.Name = 'Hypot' l.Parent = workspace \
             l.OnInvoke = function(x, y) return math.sqrt(x*x + y*y) end \
             a = l:Invoke(3, 4) \
             l.OnInvoke = function(x, y) return x*x + y*y end \
             b = l:Invoke(3, 4) \
             return a, b",
        )
        .expect("run");
    assert_eq!(strings(&r), vec!["5", "25"]);
    assert_eq!(f.number("return workspace.Hypot:Invoke(3, 4)"), 25.0);
}

#[test]
fn events() {
    let f = Fixture::new();
    f.execute("_G.v = 0 l = Instance.new('BindableEvent', workspace) l.Name = 'Sig' l.Event:connect(function(x) _G.v = x end)").expect("setup");
    f.execute("workspace.Sig:Fire(1)").expect("fire");
    assert_eq!(f.number("return _G.v"), 1.0);
}

#[test]
fn events_survive_errors_and_die_with_script() {
    let f = Fixture::new();
    f.execute("_G.v = 0 l = Instance.new('BindableEvent', workspace) l.Name = 'FooBar'")
        .expect("setup");
    f.run();
    let ws = f.workspace();
    let script = f.add_script("Script", "local l = workspace.FooBar _G.v = 2 l.Event:connect(function(x) _G.v = x error('ha!') end)", ws);
    f.step_seconds(0.05);
    assert_eq!(f.number("return _G.v"), 2.0);
    f.execute("workspace.FooBar:Fire(1)").expect("fire");
    f.step_seconds(0.05);
    assert_eq!(f.number("return _G.v"), 1.0);
    f.execute("workspace.FooBar:Fire(8)").expect("fire");
    f.step_seconds(0.05);
    assert_eq!(f.number("return _G.v"), 8.0);
    f.rt.engine
        .lock()
        .dm
        .set_parent(script, None)
        .expect("unparent");
    f.step_seconds(0.05);
    f.execute("workspace.FooBar:Fire(3)").expect("fire");
    f.step_seconds(0.05);
    assert_ne!(f.number("return _G.v"), 3.0);
}

#[test]
fn arg_types_round_trip() {
    let f = Fixture::new();
    let r = f
        .execute(
            "local t = Instance.new('BindableFunction') t.OnInvoke = function(...) return ... end \
             local inst = Instance.new('Part') \
             local out = {t:Invoke('Hello world!', true, 2, 2.5, inst, {1}, Vector3int16.new(1,2,3), Vector3.new(1,2,3), Vector2.new(1,2), \
                Ray.new(Vector3.new(), Vector3.new(0,1,0)), CFrame.new(1,2,3), Color3.new(1,0,0), BrickColor.new(21), \
                Region3.new(Vector3.new(), Vector3.new(1,1,1)), Region3int16.new(Vector3int16.new(), Vector3int16.new(1,1,1)), \
                UDim.new(1, 2), UDim2.new(1, 2, 3, 4), Faces.new(Enum.NormalId.Top), Axes.new(Enum.Axis.X), Vector2int16.new(1, 2), \
                {hello = 1}, Rect.new(0, 0, 1, 1))} \
             local types = {} for i, v in ipairs(out) do types[i] = typeof(v) end return unpack(types)",
        )
        .expect("run");
    assert_eq!(
        strings(&r),
        vec![
            "string",
            "boolean",
            "number",
            "number",
            "Instance",
            "table",
            "Vector3int16",
            "Vector3",
            "Vector2",
            "Ray",
            "CFrame",
            "Color3",
            "BrickColor",
            "Region3",
            "Region3int16",
            "UDim",
            "UDim2",
            "Faces",
            "Axes",
            "Vector2int16",
            "table",
            "Rect"
        ]
    );
}

#[test]
fn arg_variant() {
    let f = Fixture::new();
    assert_eq!(f.string("t = Instance.new('BindableFunction') t.OnInvoke = function(v) return v end return t:Invoke('hello')"), "hello");
}

#[test]
fn cyclic_tables_rejected() {
    let f = Fixture::new();
    let e = f
        .execute("local t = {} t[1] = t Instance.new('BindableFunction'):Invoke(t)")
        .expect_err("cyclic");
    assert!(e.contains("tables cannot be cyclic"), "{e}");
    let e = f
        .execute("t1 = {} t2 = {} t1['t2'] = t2 t2['t1'] = t1 Instance.new('BindableFunction'):Invoke(t1)")
        .expect_err("cyclic");
    assert!(e.contains("tables cannot be cyclic"), "{e}");
}

#[test]
fn duplicate_table_becomes_two_tables() {
    let f = Fixture::new();
    let r = f
        .execute(
            "local b = Instance.new('BindableFunction') b.OnInvoke = function() local t = {} return t, t end \
             local x, y = b:Invoke() return type(x), type(y), rawequal(x, y)",
        )
        .expect("run");
    assert_eq!(strings(&r), vec!["table", "table", "false"]);
}

#[test]
fn perf_pcall_and_calls() {
    let f = Fixture::new();
    let start = std::time::Instant::now();
    f.execute(
        "for i = 1, 1000000 do local _, j = pcall(function(x) hello = 'hello' return x end, i) end",
    )
    .expect("pcall");
    f.execute("for i = 1, 1000000 do local _, j = ypcall(function(x) return x end, i) end")
        .expect("ypcall");
    f.execute("local p = Instance.new('Part') for i = 1, 100000 do local n = p.Name end")
        .expect("property");
    f.execute("local p = Instance.new('Part') for i = 1, 100000 do p:GetChildren() end")
        .expect("method");
    f.execute("local p = Instance.new('Part') for i = 1, 2000 do p:Clone() end")
        .expect("clone");
    assert!(
        start.elapsed().as_secs_f64() < 30.0,
        "{:?}",
        start.elapsed()
    );
}
