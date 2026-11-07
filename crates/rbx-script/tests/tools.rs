mod common;

use common::Fixture;

const CHARACTER: &str = "local m = Instance.new('Model') m.Name = 'Dummy' \
    local torso = Instance.new('Part') torso.Name = 'Torso' torso.Size = Vector3.new(2, 2, 1) torso.Anchored = true torso.CFrame = CFrame.new(0, 3, 0) torso.Parent = m \
    local arm = Instance.new('Part') arm.Name = 'Right Arm' arm.Size = Vector3.new(1, 2, 1) arm.Anchored = true arm.CFrame = CFrame.new(1.5, 3, 0) arm.Parent = m \
    local shoulder = Instance.new('Motor6D') shoulder.Name = 'Right Shoulder' shoulder.Part0 = torso shoulder.Part1 = arm shoulder.Parent = torso \
    Instance.new('Humanoid', m) \
    m.Parent = workspace _G.character = m _G.arm = arm \
    local tool = Instance.new('Tool') local handle = Instance.new('Part') handle.Name = 'Handle' handle.Size = Vector3.new(1, 1, 1) handle.Parent = tool \
    _G.tool = tool _G.handle = handle";

fn fixture() -> Fixture {
    let f = Fixture::new();
    f.execute(CHARACTER).expect("character");
    f
}

#[test]
fn equip_fires_for_connections_before_and_after_equip() {
    let f = fixture();
    f.execute(
        "_G.before, _G.after, _G.afterUnequip = {}, {}, {} \
         local function counter(t) return function(mouse) table.insert(t, mouse) end end \
         _G.tool.Equipped:connect(counter(_G.before)) \
         _G.tool.Parent = _G.character",
    )
    .expect("equip");
    assert_eq!(f.number("return #_G.before"), 1.0);
    assert!(f.bool("return _G.before[1]:IsA('Mouse')"));
    f.execute("_G.tool.Equipped:connect(function(m) table.insert(_G.after, m) end)")
        .expect("late connect");
    assert_eq!(f.number("return #_G.before"), 1.0);
    assert_eq!(f.number("return #_G.after"), 1.0);
    assert!(f.bool("return _G.after[1] == _G.before[1]"));
    f.execute("_G.tool.Parent = workspace").expect("unequip");
    assert_eq!(f.number("return #_G.before"), 1.0);
    assert_eq!(f.number("return #_G.after"), 1.0);
    f.execute("_G.tool.Equipped:connect(function(m) table.insert(_G.afterUnequip, m) end)")
        .expect("connect after unequip");
    assert_eq!(f.number("return #_G.afterUnequip"), 0.0);
    f.execute("_G.tool.Parent = _G.character")
        .expect("equip again");
    assert_eq!(f.number("return #_G.before"), 2.0);
    assert_eq!(f.number("return #_G.after"), 2.0);
    assert_eq!(f.number("return #_G.afterUnequip"), 1.0);
    assert!(f.bool(
        "return _G.before[2] == _G.after[2] and _G.after[2] == _G.afterUnequip[1] and _G.before[2] ~= _G.before[1]"
    ));
}

#[test]
fn equipping_builds_right_grip_weld() {
    let f = fixture();
    f.execute(
        "_G.tool.Grip = CFrame.new(0, 0, -1.5) _G.unequipped = 0 \
         _G.tool.Unequipped:connect(function() _G.unequipped = _G.unequipped + 1 end) \
         _G.tool.Parent = _G.character",
    )
    .expect("equip");
    assert!(f.bool(
        "local w = _G.arm:FindFirstChild('RightGrip') return w ~= nil and w:IsA('Weld') and w.Part0 == _G.arm and w.Part1 == _G.handle"
    ));
    assert!(f.bool(
        "local w = _G.arm.RightGrip return w.C0 == CFrame.new(0, -1, 0, 1, 0, 0, 0, 0, 1, 0, -1, 0) and w.C1 == _G.tool.Grip"
    ));
    assert!(f.bool("return _G.handle.CanCollide == false"));
    f.execute("_G.tool.GripPos = Vector3.new(0, 0, -2)")
        .expect("grip pos");
    assert!(f.bool("return _G.arm.RightGrip.C1.p == Vector3.new(0, 0, -2)"));
    f.execute("_G.tool.Parent = workspace").expect("drop");
    assert!(f.bool("return _G.arm:FindFirstChild('RightGrip') == nil"));
    assert!(f.bool("return _G.handle.CanCollide == true"));
    assert_eq!(f.number("return _G.unequipped"), 1.0);
}

#[test]
fn activate_requires_equipped_tool() {
    let f = fixture();
    f.execute(
        "_G.activated = 0 _G.tool.Activated:connect(function() _G.activated = _G.activated + 1 end) \
         _G.tool.Parent = workspace _G.tool:Activate()",
    )
    .expect("activate unequipped");
    assert_eq!(f.number("return _G.activated"), 0.0);
    assert!(f.output().iter().any(|m| m
        == "Tool:Activate() called from script when tool is not equipped. Tool will not be activated."));
    f.execute("_G.tool.Parent = _G.character _G.tool:Activate()")
        .expect("activate equipped");
    assert_eq!(f.number("return _G.activated"), 1.0);
}

#[test]
fn grip_vectors_round_trip() {
    let f = fixture();
    f.execute("_G.tool.GripForward = Vector3.new(1, 0, 0)")
        .expect("forward");
    assert!(f.bool("return (_G.tool.GripForward - Vector3.new(1, 0, 0)).magnitude < 1e-5"));
    f.execute("_G.tool.Grip = CFrame.new() _G.tool.GripUp = Vector3.new(0, 0, 1)")
        .expect("up");
    assert!(f.bool("return (_G.tool.GripUp - Vector3.new(0, 0, 1)).magnitude < 1e-5"));
    f.execute("_G.tool.Grip = CFrame.new() _G.tool.GripRight = Vector3.new(0, 0, 1)")
        .expect("right");
    assert!(f.bool("return (_G.tool.GripRight - Vector3.new(0, 0, 1)).magnitude < 1e-5"));
}

#[test]
fn humanoid_equip_and_unequip_with_player() {
    let f = fixture();
    f.execute(
        "local p = Instance.new('Player') p.Parent = game.Players p.Character = _G.character \
         local bp = Instance.new('Backpack') bp.Parent = p _G.backpack = bp \
         _G.tool.Parent = bp \
         _G.character.Humanoid:EquipTool(_G.tool)",
    )
    .expect("equip tool");
    assert!(f.bool("return _G.tool.Parent == _G.character"));
    assert!(f.bool("return _G.arm:FindFirstChild('RightGrip') ~= nil"));
    f.execute("_G.character.Humanoid:UnequipTools()")
        .expect("unequip");
    assert!(f.bool("return _G.tool.Parent == _G.backpack"));
    assert!(f.bool("return _G.arm:FindFirstChild('RightGrip') == nil"));
}

#[test]
fn unequip_tools_warns_without_player() {
    let f = fixture();
    f.execute("_G.tool.Parent = _G.character _G.character.Humanoid:UnequipTools()")
        .expect("unequip");
    assert!(f.bool("return _G.tool.Parent == _G.character"));
    assert!(f.output().iter().any(|m| m
        == "UnequipTools can only be called on a Humanoid with a corresponding Player object."));
}

#[test]
fn touching_a_dropped_tool_picks_it_up() {
    let f = fixture();
    f.execute(
        "local p = Instance.new('Player') p.Parent = game.Players p.Character = _G.character \
         local bp = Instance.new('Backpack') bp.Parent = p _G.backpack = bp \
         local other = Instance.new('Tool') local h = Instance.new('Part') h.Name = 'Handle' h.Parent = other \
         other.Parent = _G.character _G.other = other \
         _G.handle.CFrame = CFrame.new(0, 6, 0) _G.tool.Parent = workspace",
    )
    .expect("setup");
    f.run();
    f.step_seconds(1.0);
    assert!(f.bool("return _G.tool.Parent == _G.character"));
    assert!(f.bool("return _G.other.Parent == _G.backpack"));
}

#[test]
fn set_walk_direction_before_humanoid_is_in_world() {
    let f = Fixture::new();
    f.execute(
        "local h = Instance.new('Humanoid') _G.dir = Vector3.new(1, 2, 3).unit h:Move(_G.dir) _G.h = h",
    )
    .expect("move");
    assert!(f.bool("return (_G.h.MoveDirection - _G.dir).magnitude < 1e-5"));
}
