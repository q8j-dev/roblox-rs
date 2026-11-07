mod common;

use common::Fixture;

#[test]
fn clone_does_not_leak() {
    let f = Fixture::new();
    let _ = f.rt.lua.gc_collect();
    let _ = f.rt.lua.gc_collect();
    f.rt.collect();
    f.rt.engine.lock().dm.collect_garbage();
    let before = f.rt.engine.lock().dm.len();
    f.execute("do local i = Instance.new('Configuration') i:Clone() end")
        .expect("clone");
    let _ = f.rt.lua.gc_collect();
    let _ = f.rt.lua.gc_collect();
    f.rt.collect();
    f.rt.engine.lock().dm.collect_garbage();
    let after = f.rt.engine.lock().dm.len();
    assert_eq!(before, after);
}

#[test]
fn is_a_and_casts() {
    let f = Fixture::new();
    assert!(f.bool(
        "local p = Instance.new('Part') return p:IsA('Instance') and p:IsA('BasePart') and p:IsA('PVInstance') and not p:IsA('Tool')"
    ));
    assert!(f.bool(
        "local c = Instance.new('Configuration') return c:IsA('Instance') and not c:IsA('BasePart')"
    ));
    let start = std::time::Instant::now();
    assert!(f.bool(
        "local p = Instance.new('Part') local n = 0 for i = 1, 100000 do if p:IsA('Tool') then n = n + 1 end end return n == 0"
    ));
    assert!(start.elapsed().as_secs() < 30);
}
