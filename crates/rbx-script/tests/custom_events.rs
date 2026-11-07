mod common;

use common::Fixture;

const SETUP: &str = "_G.calls = 0 _G.last = 0 \
    evt = Instance.new('CustomEvent') receiver = Instance.new('CustomEventReceiver') \
    receiver.SourceValueChanged:connect(function(v) _G.calls = _G.calls + 1 _G.last = v end) \
    _G.evt = evt _G.receiver = receiver";

fn state(f: &Fixture) -> (f64, f64) {
    (f.number("return _G.calls"), f.number("return _G.last"))
}

fn persisted(f: &Fixture, expr: &str) -> f64 {
    let id = f.instance(&format!("return {expr}"));
    f.rt.engine.lock().dm.custom_event_value(id) as f64
}

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn creating_state_sanity_check() {
    let f = Fixture::new();
    f.execute(SETUP).expect("setup");
    assert!(f.bool("return _G.evt.Parent == nil and _G.receiver.Parent == nil"));
    assert_eq!(persisted(&f, "_G.evt"), 0.0);
    assert_eq!(f.number("return _G.receiver:GetCurrentValue()"), 0.0);
}

#[test]
fn attached_list_maintenance() {
    let f = Fixture::new();
    f.execute(SETUP).expect("setup");
    f.execute("_G.receiver.Source = _G.evt").expect("attach");
    assert!(
        f.bool("local r = _G.evt:GetAttachedReceivers() return #r == 1 and r[1] == _G.receiver")
    );
    f.execute("_G.receiver.Source = nil").expect("detach");
    assert_eq!(f.number("return #_G.evt:GetAttachedReceivers()"), 0.0);
    f.execute("_G.receiver.Source = _G.evt").expect("attach");
    assert_eq!(f.number("return #_G.evt:GetAttachedReceivers()"), 1.0);
    f.execute("_G.receiver:Destroy()").expect("destroy");
    assert_eq!(f.number("return #_G.evt:GetAttachedReceivers()"), 0.0);
}

#[test]
fn send_values_only_when_connected() {
    let f = Fixture::new();
    f.execute(SETUP).expect("setup");
    f.execute("_G.receiver.Source = _G.evt").expect("attach");
    assert_eq!(state(&f).0, 0.0);
    f.execute("_G.evt:SetValue(0)").expect("set");
    assert_eq!(state(&f).0, 0.0);
    f.execute("_G.evt:SetValue(.5)").expect("set");
    assert_eq!(persisted(&f, "_G.evt"), 0.5);
    assert_eq!(f.number("return _G.receiver:GetCurrentValue()"), 0.5);
    assert_eq!(state(&f), (1.0, 0.5));
    f.execute(
        "_G.evt2 = Instance.new('CustomEvent') _G.evt2:SetValue(.5) _G.receiver.Source = _G.evt2",
    )
    .expect("switch");
    assert_eq!(state(&f), (1.0, 0.5));
    f.execute("_G.evt:SetValue(.75)").expect("old");
    assert_eq!(f.number("return _G.receiver:GetCurrentValue()"), 0.5);
    assert_eq!(state(&f), (1.0, 0.5));
    f.execute("_G.evt2:SetValue(.25)").expect("new");
    assert_eq!(state(&f), (2.0, 0.25));
    f.execute("_G.receiver.Source = _G.evt").expect("back");
    assert_eq!(f.number("return _G.receiver:GetCurrentValue()"), 0.75);
    assert_eq!(state(&f), (3.0, 0.75));
}

#[test]
fn value_clamping() {
    let f = Fixture::new();
    f.execute(SETUP).expect("setup");
    f.execute("_G.evt:SetValue(-100)").expect("set");
    assert_eq!(persisted(&f, "_G.evt"), 0.0);
    f.execute("_G.evt:SetValue(100)").expect("set");
    assert_eq!(persisted(&f, "_G.evt"), 1.0);
    assert_eq!(state(&f).0, 0.0);
    f.execute("_G.receiver.Source = _G.evt").expect("attach");
    assert_eq!(state(&f), (1.0, 1.0));
    f.execute("_G.evt:SetValue(7)").expect("set");
    assert_eq!(state(&f), (1.0, 1.0));
    f.execute("_G.evt:SetValue(1)").expect("set");
    assert_eq!(state(&f), (1.0, 1.0));
    f.execute(
        "_G.evt2 = Instance.new('CustomEvent') _G.evt2:SetValue(30) _G.receiver.Source = _G.evt2",
    )
    .expect("switch");
    assert_eq!(state(&f), (1.0, 1.0));
    assert_eq!(persisted(&f, "_G.evt2"), 1.0);
}

#[test]
fn setting_parent_to_null_breaks_connection() {
    let f = Fixture::new();
    f.execute(SETUP).expect("setup");
    f.execute("_G.part = Instance.new('Part', workspace) _G.part.Anchored = true _G.evt.Parent = _G.part _G.receiver.Parent = _G.part _G.receiver.Source = _G.evt _G.evt:SetValue(.1)").expect("wire");
    let (c, l) = state(&f);
    assert!(c == 1.0 && approx(l, 0.1));
    f.execute("_G.evt.Parent = nil _G.evt:SetValue(.2)")
        .expect("unparent");
    assert_eq!(state(&f), (2.0, 0.0));
    assert_eq!(f.number("return #_G.evt:GetAttachedReceivers()"), 0.0);
    f.execute("_G.evt.Parent = _G.part").expect("reparent");
    assert_eq!(state(&f), (2.0, 0.0));
    f.execute("_G.receiver.Source = _G.evt").expect("reattach");
    let (c, l) = state(&f);
    assert!(c == 3.0 && approx(l, 0.2));
    f.execute("_G.receiver.Parent = nil _G.evt:SetValue(.3)")
        .expect("unparent receiver");
    assert_eq!(state(&f), (4.0, 0.0));
    assert_eq!(f.number("return #_G.evt:GetAttachedReceivers()"), 0.0);
}

#[test]
fn breaking_connection_sets_receiver_to_zero() {
    let f = Fixture::new();
    f.execute(SETUP).expect("setup");
    f.execute("_G.part = Instance.new('Part', workspace) _G.part.Anchored = true _G.evt.Parent = _G.part _G.receiver.Parent = _G.part _G.receiver.Source = _G.evt _G.receiver.Source = _G.evt _G.evt:SetValue(.1)").expect("wire");
    assert_eq!(state(&f).0, 1.0);
    f.execute("_G.receiver.Source = nil").expect("break");
    assert_eq!(state(&f), (2.0, 0.0));
    f.execute("_G.evt:SetValue(0) _G.receiver.Source = _G.evt _G.receiver.Source = nil")
        .expect("zero");
    assert_eq!(state(&f), (2.0, 0.0));
    f.execute("_G.evt:SetValue(.2) _G.receiver.Source = _G.evt")
        .expect("again");
    assert_eq!(state(&f).0, 3.0);
    f.execute("_G.evt.Parent = nil").expect("remove sender");
    assert_eq!(state(&f), (4.0, 0.0));
    f.execute("_G.evt = Instance.new('CustomEvent') _G.evt.Parent = _G.part _G.receiver.Source = _G.evt _G.evt.Parent = nil").expect("zero sender");
    assert_eq!(state(&f), (4.0, 0.0));
    f.execute("_G.evt.Parent = _G.part _G.evt:SetValue(.3) _G.receiver.Source = _G.evt")
        .expect("wire");
    assert_eq!(state(&f).0, 5.0);
    f.execute("_G.receiver.Parent = nil")
        .expect("remove receiver");
    assert_eq!(state(&f), (6.0, 0.0));
}

#[test]
fn clone_event() {
    let f = Fixture::new();
    f.execute(SETUP).expect("setup");
    f.execute("local p = Instance.new('Part') _G.evt.Parent = p _G.receiver.Parent = p _G.receiver.Source = _G.evt _G.cloned = _G.evt:Clone()").expect("clone");
    assert_eq!(f.number("return #_G.cloned:GetAttachedReceivers()"), 0.0);
    assert!(f.bool("return _G.receiver.Source == _G.evt"));
    assert_eq!(f.number("return #_G.evt:GetAttachedReceivers()"), 1.0);
    f.execute("_G.cloned:SetValue(1)").expect("set");
    assert_eq!(state(&f).0, 0.0);
    f.execute("_G.evt:SetValue(1)").expect("set");
    assert_eq!(state(&f).0, 1.0);
}

#[test]
fn clone_receiver() {
    let f = Fixture::new();
    f.execute(SETUP).expect("setup");
    f.execute("local p = Instance.new('Part') _G.evt.Parent = p _G.receiver.Parent = p _G.receiver.Source = _G.evt _G.cloned = _G.receiver:Clone()").expect("clone");
    assert!(f.bool("return _G.cloned.Source == _G.evt"));
    assert_eq!(f.number("return #_G.evt:GetAttachedReceivers()"), 2.0);
    f.execute("_G.evt:SetValue(1)").expect("set");
    assert_eq!(f.number("return _G.cloned:GetCurrentValue()"), 1.0);
}

#[test]
fn clone_event_and_receiver() {
    let f = Fixture::new();
    f.execute(SETUP).expect("setup");
    f.execute(
        "local p = Instance.new('Part') _G.evt.Parent = p _G.receiver.Parent = p _G.receiver.Source = _G.evt \
         _G.copy = p:Clone() _G.cevt = _G.copy:FindFirstChild('CustomEvent') _G.crec = _G.copy:FindFirstChild('CustomEventReceiver')",
    )
    .expect("clone");
    assert!(f.bool("return _G.crec.Source == _G.cevt"));
    assert_eq!(f.number("return #_G.cevt:GetAttachedReceivers()"), 1.0);
    assert_eq!(f.number("return #_G.evt:GetAttachedReceivers()"), 1.0);
    f.execute("_G.cevt:SetValue(1)").expect("set");
    assert_eq!(f.number("return _G.crec:GetCurrentValue()"), 1.0);
    assert_eq!(f.number("return _G.receiver:GetCurrentValue()"), 0.0);
}

#[test]
fn connect_and_disconnect_signals() {
    let f = Fixture::new();
    f.execute(SETUP).expect("setup");
    f.execute(
        "_G.log = {} \
         _G.evt.ReceiverConnected:connect(function(r) table.insert(_G.log, 'rc:' .. tostring(r == _G.receiver) .. ':' .. #_G.evt:GetAttachedReceivers()) end) \
         _G.evt.ReceiverDisconnected:connect(function(r) table.insert(_G.log, 'rd:' .. tostring(r == _G.receiver) .. ':' .. #_G.evt:GetAttachedReceivers()) end) \
         _G.receiver.EventConnected:connect(function(e) table.insert(_G.log, 'ec:' .. tostring(e == _G.evt) .. ':' .. tostring(_G.receiver.Source == _G.evt)) end) \
         _G.receiver.EventDisconnected:connect(function(e) table.insert(_G.log, 'ed:' .. tostring(e == _G.evt) .. ':' .. tostring(_G.receiver.Source == nil)) end) \
         _G.receiver.Source = _G.evt _G.receiver.Source = nil",
    )
    .expect("signals");
    assert_eq!(
        f.string("return table.concat(_G.log, ',')"),
        "rc:true:1,ec:true:true,rd:true:0,ed:true:true"
    );
}
