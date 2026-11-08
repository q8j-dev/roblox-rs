use rbx_dom::{InstanceId, Variant};
use rbx_math::{BrickColor, Vec3};
use rbx_studio_ui::Document;
use std::path::PathBuf;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn find(doc: &Document, name: &str) -> InstanceId {
    let e = doc.runtime.engine.lock();
    let ws = e.dm.find_service("Workspace").expect("workspace");
    e.dm.descendants(ws)
        .into_iter()
        .find(|d| e.dm.name_of(*d) == name)
        .unwrap_or_else(|| panic!("{name} not found"))
}

fn edit(doc: &mut Document) {
    let ws = doc
        .runtime
        .engine
        .lock()
        .dm
        .find_service("Workspace")
        .expect("ws");
    let part = doc.insert("Part", ws).expect("insert part");
    doc.rename(part, "EditedPart");
    doc.set_property(part, "BrickColor", Variant::BrickColor(BrickColor::new(21)))
        .expect("color");
    doc.set_property(part, "Transparency", Variant::Float(0.25))
        .expect("transparency");
    doc.translate(&[part], Vec3::new(4.0, 2.0, -3.0));
    doc.rotate(&[part], Vec3::Y, std::f32::consts::FRAC_PI_2);
    doc.resize(part, Vec3::X, 3.0);
    let script = doc.insert("Script", part).expect("insert script");
    doc.set_script_source(script, "print('hello from the editor')");
    let doomed = doc.insert("Part", ws).expect("insert doomed");
    doc.rename(doomed, "Doomed");
    doc.select(vec![doomed]);
    doc.delete_selection();
}

fn round_trip(ext: &str) {
    let root = repo();
    let content = root.join("assets").join("content");
    let place = root.join("tests/places/RobloxStudio/content/FirstPlaceBasePlate.rbxl");
    let mut doc = Document::open(&place, &content).expect("open");
    edit(&mut doc);
    let dir = std::env::temp_dir().join(format!("rbx-studio-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let out = dir.join(format!("edited.{ext}"));
    doc.save_to(&out).expect("save");
    assert!(!doc.dirty);
    let before = doc.runtime.engine.lock().dm.save_place_tree();
    let reopened = Document::open(&out, &content).expect("reopen");
    let after = reopened.runtime.engine.lock().dm.save_place_tree();
    assert_eq!(
        before, after,
        "the reopened place differs from the saved one"
    );
    let part = find(&reopened, "EditedPart");
    {
        let e = reopened.runtime.engine.lock();
        assert_eq!(
            e.dm.get(part, "BrickColor").ok(),
            Some(Variant::BrickColor(BrickColor::new(21)))
        );
        assert_eq!(
            e.dm.get(part, "Transparency").ok(),
            Some(Variant::Float(0.25))
        );
        let size =
            e.dm.get(part, "Size")
                .ok()
                .and_then(|v| v.as_vec3())
                .expect("size");
        assert!((size.x - 7.0).abs() < 1e-4, "size {size:?}");
        let script =
            e.dm.find_first_child_of_class(part, "Script")
                .expect("script");
        assert_eq!(
            e.dm.get(script, "Source").ok(),
            Some(Variant::ProtectedString(
                "print('hello from the editor')".into()
            ))
        );
        let ws = e.dm.find_service("Workspace").expect("ws");
        assert!(
            e.dm.descendants(ws)
                .into_iter()
                .all(|d| e.dm.name_of(d) != "Doomed")
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn edits_survive_binary_save_and_reopen() {
    round_trip("rbxl");
}

#[test]
fn edits_survive_xml_save_and_reopen() {
    round_trip("rbxlx");
}

#[test]
fn undo_and_redo_restore_the_place() {
    let root = repo();
    let content = root.join("assets").join("content");
    let mut doc = Document::new_baseplate(&content).expect("new");
    let ws = doc
        .runtime
        .engine
        .lock()
        .dm
        .find_service("Workspace")
        .expect("ws");
    let part = doc.insert("Part", ws).expect("insert");
    doc.rename(part, "Undoable");
    assert!(doc.undo().expect("undo"));
    assert!(doc.undo().expect("undo"));
    let count = |doc: &Document, name: &str| {
        let e = doc.runtime.engine.lock();
        let ws = e.dm.find_service("Workspace").expect("ws");
        e.dm.descendants(ws)
            .into_iter()
            .filter(|d| e.dm.name_of(*d) == name)
            .count()
    };
    assert_eq!(count(&doc, "Part") + count(&doc, "Undoable"), 0);
    assert!(doc.redo().expect("redo"));
    assert!(doc.redo().expect("redo"));
    assert_eq!(count(&doc, "Undoable"), 1);
}
