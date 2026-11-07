use rbx_dom::{DataModel, DomEvent, Variant};
use rbx_math::{BrickColor, Color3, Vec3};

#[test]
fn create_parent_and_events() {
    let mut dm = DataModel::new();
    let cursor = dm.subscribe();
    let ws = dm.get_service("Workspace").expect("workspace");
    let part = dm.create("Part").expect("part");
    assert_eq!(dm.name_of(part), "Part");
    dm.set_parent(part, Some(ws)).expect("parent");
    let events = dm.drain_events(cursor);
    assert!(events.contains(&DomEvent::ChildAdded {
        parent: ws,
        child: part
    }));
    assert!(events
        .iter()
        .any(|e| matches!(e, DomEvent::DescendantAdded { ancestor, descendant } if *ancestor == dm.root() && *descendant == part)));
    assert_eq!(dm.full_name(part), "Workspace.Part");
}

#[test]
fn parenting_errors() {
    let mut dm = DataModel::new();
    let a = dm.create("Model").expect("model");
    let b = dm.create("Model").expect("model");
    dm.set_parent(b, Some(a)).expect("parent");
    let err = dm.set_parent(a, Some(b)).expect_err("circular");
    assert!(err.to_string().contains("circular reference"));
    let err = dm.set_parent(a, Some(a)).expect_err("self");
    assert!(err.to_string().contains("as its own parent"));
    let ws = dm.get_service("Workspace").expect("workspace");
    let err = dm.set_parent(ws, Some(a)).expect_err("locked");
    assert!(err.to_string().contains("is locked"));
}

#[test]
fn part_properties_and_aliases() {
    let mut dm = DataModel::new();
    let part = dm.create("Part").expect("part");
    assert_eq!(
        dm.get(part, "Size").ok(),
        Some(Variant::Vector3(Vec3::new(4.0, 1.2, 2.0)))
    );
    dm.set(part, "Position", Variant::Vector3(Vec3::new(1.0, 2.0, 3.0)))
        .expect("position");
    let cf = dm
        .get(part, "CFrame")
        .ok()
        .and_then(|v| v.as_cframe())
        .unwrap_or_default();
    assert_eq!(cf.translation, Vec3::new(1.0, 2.0, 3.0));
    dm.set(part, "BrickColor", Variant::BrickColor(BrickColor::new(21)))
        .expect("color");
    assert_eq!(
        dm.get(part, "brickColor").ok(),
        Some(Variant::BrickColor(BrickColor::new(21)))
    );
    dm.set(part, "Color", Variant::Color3(Color3::new(0.0, 0.0, 1.0)))
        .expect("color3");
    assert_ne!(
        dm.get(part, "BrickColor").ok(),
        Some(Variant::BrickColor(BrickColor::new(21)))
    );
    dm.set(part, "Size", Variant::Vector3(Vec3::new(5.4, 2.0, 0.2)))
        .expect("size");
    assert_eq!(
        dm.get(part, "Size").ok(),
        Some(Variant::Vector3(Vec3::new(5.0, 2.4, 1.0)))
    );
    let err = dm
        .set(part, "NotAProperty", Variant::Bool(true))
        .expect_err("unknown");
    assert!(err.to_string().contains("is not a valid member"));
}

#[test]
fn clone_remaps_references() {
    let mut dm = DataModel::new();
    let model = dm.create("Model").expect("model");
    let part = dm.create("Part").expect("part");
    dm.set_parent(part, Some(model)).expect("parent");
    dm.set(model, "PrimaryPart", Variant::Ref(Some(part)))
        .expect("primary");
    let copy = dm.clone_instance(model).expect("clone").expect("some");
    let copied_part = dm.children_of(copy)[0];
    assert_ne!(copied_part, part);
    assert_eq!(dm.get_ref(copy, "PrimaryPart"), Some(copied_part));
}

#[test]
fn destroy_locks_parent_and_collects() {
    let mut dm = DataModel::new();
    let ws = dm.get_service("Workspace").expect("workspace");
    let part = dm.create("Part").expect("part");
    dm.set_parent(part, Some(ws)).expect("parent");
    dm.destroy(part).expect("destroy");
    assert_eq!(dm.parent_of(part), None);
    assert!(dm.set_parent(part, Some(ws)).is_err());
    let before = dm.len();
    let freed = dm.collect_garbage();
    assert!(freed >= 1);
    assert_eq!(dm.len(), before - freed);
    assert!(!dm.contains(part));
}

#[test]
fn find_first_child_recursive() {
    let mut dm = DataModel::new();
    let a = dm.create("Model").expect("a");
    let b = dm.create("Model").expect("b");
    let c = dm.create("Part").expect("c");
    dm.set_name(c, "Target");
    dm.set_parent(b, Some(a)).expect("p");
    dm.set_parent(c, Some(b)).expect("p");
    assert_eq!(dm.find_first_child(a, "Target", false), None);
    assert_eq!(dm.find_first_child(a, "Target", true), Some(c));
}
