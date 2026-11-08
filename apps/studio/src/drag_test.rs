use crate::app::Studio;
use crate::viewport::Tool;
use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, vec2};
use rbx_dom::{InstanceId, Variant};
use rbx_math::{CFrame, Vec3};
use rbx_render::adorn::{self, OverlayHandle};
use rbx_studio_ui::Document;
use std::path::PathBuf;

struct Harness {
    ctx: egui::Context,
    studio: Studio,
    time: f64,
}

impl Harness {
    fn new(tool: Tool) -> (Harness, InstanceId) {
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let doc = Document::new_baseplate(&assets.join("content")).expect("baseplate");
        let part = {
            let mut e = doc.runtime.engine.lock();
            let ws = e.dm.find_service("Workspace").expect("workspace");
            let p = e.dm.create("Part").expect("part");
            let _ = e.dm.set(p, "Anchored", Variant::Bool(true));
            let _ = e.dm.set(p, "Size", Variant::Vector3(Vec3::new(4.0, 2.0, 4.0)));
            let _ = e.dm.set(p, "CFrame", Variant::CFrame(CFrame::from_translation(Vec3::new(0.0, 5.0, 0.0))));
            let _ = e.dm.set_parent(p, Some(ws));
            p
        };
        doc.select(vec![part]);
        let mut studio = Studio::new(doc, assets);
        studio.tool = tool;
        studio.view.camera = crate::viewport::FlyCamera::looking_at(Vec3::new(14.0, 14.0, 14.0), Vec3::new(0.0, 5.0, 0.0));
        let ctx = egui::Context::default();
        let mut h = Harness { ctx, studio, time: 0.0 };
        for _ in 0..3 {
            h.frame(vec![]);
        }
        (h, part)
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.time += 1.0 / 60.0;
        let raw = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        let studio = &mut self.studio;
        let _ = self.ctx.run_ui(raw, |ui| studio.ui(ui));
        self.studio.doc.runtime.step(1.0 / 60.0);
    }

    fn handle_screen(&self, part: InstanceId, kind: OverlayHandle, id: usize) -> Pos2 {
        let cam = &self.studio.view.camera;
        let (cf, size) = {
            let e = self.studio.doc.runtime.engine.lock();
            crate::viewport::part_box(&e.dm, part).expect("box")
        };
        let local = adorn::handle_position(cam.position, size, kind, id);
        let world = cf.point_to_world_space(local);
        let tip = if kind == OverlayHandle::Move {
            world + adorn::normal_vector_of(id) * adorn::handle_radius(cam.position, kind, world) * 0.5
        } else {
            world
        };
        cam.project(self.studio.view.rect, tip).expect("visible")
    }

    fn drag(&mut self, from: Pos2, to: Pos2) {
        self.frame(vec![Event::PointerMoved(from)]);
        self.frame(vec![Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
        for i in 1..=10 {
            let t = i as f32 / 10.0;
            self.frame(vec![Event::PointerMoved(from + (to - from) * t)]);
        }
        self.frame(vec![Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
        self.frame(vec![]);
    }

    fn part(&self, part: InstanceId) -> (CFrame, Vec3) {
        let e = self.studio.doc.runtime.engine.lock();
        crate::viewport::part_box(&e.dm, part).expect("box")
    }
}

#[test]
fn move_handle_drags_part() {
    let (mut h, part) = Harness::new(Tool::Move);
    let from = h.handle_screen(part, OverlayHandle::Move, 1);
    h.drag(from, from - vec2(0.0, 80.0));
    let (cf, _) = h.part(part);
    assert!(cf.translation.y > 6.0, "part did not move up: {:?}", cf.translation);
    assert!(cf.translation.x.abs() < 1e-3 && cf.translation.z.abs() < 1e-3);
}

#[test]
fn scale_handle_resizes_part() {
    let (mut h, part) = Harness::new(Tool::Scale);
    let from = h.handle_screen(part, OverlayHandle::Resize, 1);
    h.drag(from, from - vec2(0.0, 80.0));
    let (_, size) = h.part(part);
    assert!(size.y > 3.0, "part did not grow: {size:?}");
}

#[test]
fn rotate_handle_turns_part() {
    let (mut h, part) = Harness::new(Tool::Rotate);
    let (cf, size) = h.part(part);
    let cam = h.studio.view.camera.position;
    let normal = adorn::normal_vector_of(1);
    let local = adorn::handle_position(cam, size, OverlayHandle::Rotate, 1);
    let start = cf.point_to_world_space(local);
    let v = start - cf.translation;
    let turned = cf.translation + normal.cross(v);
    let rect = h.studio.view.rect;
    let from = h.studio.view.camera.project(rect, start).expect("visible");
    let to = h.studio.view.camera.project(rect, turned).expect("visible");
    h.drag(from, to);
    let (cf, _) = h.part(part);
    assert!((cf.rotation.x_axis - Vec3::X).length() > 0.1 || (cf.rotation.z_axis - Vec3::Z).length() > 0.1, "part did not rotate");
}

#[test]
fn select_tool_drags_part_on_surface() {
    let (mut h, part) = Harness::new(Tool::Select);
    let (cf, _) = h.part(part);
    let from = h.studio.view.camera.project(h.studio.view.rect, cf.translation + Vec3::new(0.0, 1.0, 0.0)).expect("visible");
    h.drag(from, from + vec2(60.0, 0.0));
    let (moved, _) = h.part(part);
    assert!((moved.translation - cf.translation).length() > 0.5, "part did not move: {:?}", moved.translation);
}

#[test]
fn inserted_part_moves_with_handles_in_real_place() {
    let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let place = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/places/RobloxStudio/content/FirstPlaceBasePlate.rbxl");
    let doc = Document::open(&place, &assets.join("content")).expect("open");
    let mut studio = Studio::new(doc, assets);
    studio.view.camera = crate::app::camera_for(&studio.doc);
    let mut h = Harness { ctx: egui::Context::default(), studio, time: 0.0 };
    for _ in 0..3 {
        h.frame(vec![]);
    }
    h.studio.run_action("placePartAction");
    h.studio.run_action("advTranslateAction");
    h.frame(vec![]);
    let part = h.studio.doc.selection()[0];
    let before = h.part(part).0.translation;
    let from = h.handle_screen(part, OverlayHandle::Move, 1);
    h.drag(from, from - vec2(0.0, 60.0));
    let after = h.part(part).0.translation;
    assert!(after.y > before.y + 0.5, "part did not move up: {before:?} -> {after:?}");
}


#[test]
#[ignore]
fn ui_frame_cost() {
    let place = std::env::var("STUDIO_BENCH_PLACE").expect("place");
    let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let doc = Document::open(&PathBuf::from(place), &assets.join("content")).expect("open");
    let mut studio = Studio::new(doc, assets);
    studio.view.camera = crate::app::camera_for(&studio.doc);
    let mut h = Harness { ctx: egui::Context::default(), studio, time: 0.0 };
    let mut ui = Vec::new();
    let mut step = Vec::new();
    for i in 0..330 {
        h.time += 1.0 / 60.0;
        let raw = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0))),
            time: Some(h.time),
            events: vec![Event::PointerMoved(Pos2::new(800.0 + (i as f32).sin() * 50.0, 500.0))],
            ..Default::default()
        };
        let t = std::time::Instant::now();
        let studio = &mut h.studio;
        let out = h.ctx.run_ui(raw, |ui| studio.ui(ui));
        let _ = h.ctx.tessellate(out.shapes, out.pixels_per_point);
        let a = t.elapsed().as_secs_f64() * 1000.0;
        let t = std::time::Instant::now();
        h.studio.doc.runtime.step(1.0 / 60.0);
        let b = t.elapsed().as_secs_f64() * 1000.0;
        if i >= 30 {
            ui.push(a);
            step.push(b);
        }
    }
    for (name, mut v) in [("ui", ui), ("step", step)] {
        v.sort_by(|a, b| a.total_cmp(b));
        let avg = v.iter().sum::<f64>() / v.len() as f64;
        eprintln!("{name} avg {avg:.2} median {:.2} p99 {:.2} max {:.2}", v[v.len() / 2], v[v.len() * 99 / 100], v[v.len() - 1]);
    }
}
