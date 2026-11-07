mod common;

use common::Fixture;

const CHARACTER: &str = "local function part(name, size, cf, parent) \
        local p = Instance.new('Part') p.Name = name p.FormFactor = Enum.FormFactor.Custom p.Size = size p.CFrame = cf p.Parent = parent return p end \
    local function motor(name, p0, p1, c0, c1) local m = Instance.new('Motor6D') m.Name = name m.Part0 = p0 m.Part1 = p1 m.C0 = c0 m.C1 = c1 m.Parent = p0 return m end \
    function _G.makeCharacter(at) \
        local m = Instance.new('Model') m.Name = 'Dummy' \
        local root = part('HumanoidRootPart', Vector3.new(2, 2, 1), at, m) root.Transparency = 1 \
        local torso = part('Torso', Vector3.new(2, 2, 1), at, m) \
        local head = part('Head', Vector3.new(2, 1, 1), at * CFrame.new(0, 1.5, 0), m) \
        local ll = part('Left Leg', Vector3.new(1, 2, 1), at * CFrame.new(-0.5, -2, 0), m) \
        local rl = part('Right Leg', Vector3.new(1, 2, 1), at * CFrame.new(0.5, -2, 0), m) \
        local la = part('Left Arm', Vector3.new(1, 2, 1), at * CFrame.new(-1.5, 0, 0), m) \
        local ra = part('Right Arm', Vector3.new(1, 2, 1), at * CFrame.new(1.5, 0, 0), m) \
        motor('RootJoint', root, torso, CFrame.new(), CFrame.new()) \
        motor('Neck', torso, head, CFrame.new(0, 1, 0), CFrame.new(0, -0.5, 0)) \
        motor('Left Hip', torso, ll, CFrame.new(-1, -1, 0), CFrame.new(-0.5, 1, 0)) \
        motor('Right Hip', torso, rl, CFrame.new(1, -1, 0), CFrame.new(0.5, 1, 0)) \
        motor('Left Shoulder', torso, la, CFrame.new(-1, 0.5, 0), CFrame.new(0.5, 0.5, 0)) \
        motor('Right Shoulder', torso, ra, CFrame.new(1, 0.5, 0), CFrame.new(-0.5, 0.5, 0)) \
        local h = Instance.new('Humanoid') h.Parent = m \
        m.PrimaryPart = head \
        m.Parent = workspace \
        return m, h \
    end \
    local base = Instance.new('Part') base.Name = 'Base' base.Anchored = true base.Size = Vector3.new(200, 2, 200) base.CFrame = CFrame.new(0, -1, 0) base.Parent = workspace";

fn fixture() -> Fixture {
    let f = Fixture::new();
    f.execute(CHARACTER).expect("setup");
    f
}

fn seat_and_drop(f: &Fixture, class: &str) {
    f.execute(&format!(
        "local s = Instance.new('{class}') s.Anchored = true s.CFrame = CFrame.new(0, 0.6, 0) s.Parent = workspace _G.seat = s \
         _G.character, _G.humanoid = _G.makeCharacter(CFrame.new(0, 5, 0))"
    ))
    .expect("seat");
    f.run();
    f.step_seconds(2.0);
}

#[test]
fn touching_a_seat_sits_the_humanoid() {
    let f = fixture();
    seat_and_drop(&f, "Seat");
    assert!(f.bool("return _G.seat:FindFirstChild('SeatWeld') ~= nil"));
    assert!(f.bool(
        "local w = _G.seat.SeatWeld return w.Part0 == _G.seat and w.Part1 == _G.character.HumanoidRootPart and w.Archivable == false"
    ));
    assert!(f.bool(
        "return _G.seat.SeatWeld.C0 == CFrame.new(0, 0.6, 0, 1, 0, 0, 0, 0, 1, 0, -1, 0) and _G.seat.SeatWeld.C1 == CFrame.new(0, -1.5, 0, 1, 0, 0, 0, 0, 1, 0, -1, 0)"
    ));
    assert!(f.bool("return _G.seat.Occupant == _G.humanoid"));
    assert!(f.bool("return _G.humanoid.Sit == true and _G.humanoid.SeatPart == _G.seat"));
}

#[test]
fn jumping_leaves_the_seat() {
    let f = fixture();
    seat_and_drop(&f, "Seat");
    assert!(f.bool("return _G.seat.Occupant == _G.humanoid"));
    f.execute("_G.humanoid.Jump = true").expect("jump");
    f.step_seconds(0.5);
    assert!(f.bool("return _G.seat:FindFirstChild('SeatWeld') == nil"));
    assert!(f.bool("return _G.seat.Occupant == nil"));
    assert!(f.bool("return _G.humanoid.SeatPart == nil"));
}

#[test]
fn disabling_a_seat_removes_the_weld() {
    let f = fixture();
    seat_and_drop(&f, "Seat");
    f.execute("_G.seat.Disabled = true").expect("disable");
    assert!(f.bool("return _G.seat:FindFirstChild('SeatWeld') == nil and _G.seat.Occupant == nil"));
    f.step_seconds(4.0);
    assert!(f.bool("return _G.seat:FindFirstChild('SeatWeld') == nil"));
}

#[test]
fn removed_weld_puts_the_seat_to_sleep() {
    let f = fixture();
    seat_and_drop(&f, "Seat");
    f.execute("_G.seat.SeatWeld:Destroy()").expect("destroy");
    f.step_seconds(1.0);
    assert!(f.bool("return _G.seat:FindFirstChild('SeatWeld') == nil"));
}

#[test]
fn vehicle_seat_clamps_throttle_and_steer() {
    let f = fixture();
    f.execute("local s = Instance.new('VehicleSeat') s.Throttle = 5 s.Steer = -3 _G.s = s")
        .expect("seat");
    assert_eq!(f.number("return _G.s.Throttle"), 1.0);
    assert_eq!(f.number("return _G.s.Steer"), -1.0);
}

#[test]
fn vehicle_seat_drives_hinged_wheels() {
    let f = fixture();
    f.execute(
        "local car = Instance.new('Model') car.Parent = workspace \
         local chassis = Instance.new('Part') chassis.Size = Vector3.new(6, 1, 10) chassis.CFrame = CFrame.new(0, 3, 0) chassis.Parent = car \
         local seat = Instance.new('VehicleSeat') seat.CFrame = CFrame.new(0, 4.1, 0) seat.Parent = car \
         local w = Instance.new('Weld') w.Part0 = chassis w.Part1 = seat w.C0 = CFrame.new(0, 1.1, 0) w.Parent = chassis \
         for _, x in ipairs({-3.5, 3.5}) do for _, z in ipairs({-4, 4}) do \
             local wheel = Instance.new('Part') wheel.Shape = Enum.PartType.Ball wheel.Size = Vector3.new(2, 2, 2) wheel.CFrame = CFrame.new(x, 2, z) wheel.Parent = car \
             local h = Instance.new('Rotate') h.Part0 = chassis h.Part1 = wheel \
             h.C0 = CFrame.new(x > 0 and 3 or -3, -1, z) * CFrame.Angles(0, x > 0 and math.pi / 2 or -math.pi / 2, 0) \
             h.C1 = CFrame.new(x > 0 and -0.5 or 0.5, 0, 0) * CFrame.Angles(0, x > 0 and math.pi / 2 or -math.pi / 2, 0) \
             h.Parent = chassis \
         end end \
         _G.seat = seat _G.chassis = chassis",
    )
    .expect("car");
    f.run();
    f.step_seconds(1.0);
    assert!(f.bool("return _G.seat.AreHingesDetected == 4"));
    let start = f.number("return _G.chassis.Position.Z");
    f.execute("_G.seat.Throttle = 1").expect("throttle");
    f.step_seconds(3.0);
    let end = f.number("return _G.chassis.Position.Z");
    assert!((end - start).abs() > 5.0, "car moved {}", end - start);
}
