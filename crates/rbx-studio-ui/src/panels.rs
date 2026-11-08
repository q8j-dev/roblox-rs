use crate::document::Document;
use egui::{Color32, RichText, Ui};
use rbx_dom::{InstanceId, Variant};
use rbx_math::{BrickColor, CFrame, Color3};
use rbx_reflection::{Security, ValueType, VariantType, database};
use std::collections::BTreeMap;

pub fn explorer(ui: &mut Ui, doc: &mut Document) {
    let (root, children) = {
        let e = doc.runtime.engine.lock();
        let root = e.dm.root();
        (root, e.dm.children_of(root).to_vec())
    };
    let _ = root;
    let selection = doc.selection();
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for id in children {
                explorer_node(ui, doc, id, &selection);
            }
        });
}

fn explorer_node(ui: &mut Ui, doc: &mut Document, id: InstanceId, selection: &[InstanceId]) {
    let (name, class, kids) = {
        let e = doc.runtime.engine.lock();
        if !e.dm.contains(id) {
            return;
        }
        (
            e.dm.name_of(id).to_string(),
            e.dm.class_of(id).unwrap_or("Instance").to_string(),
            e.dm.children_of(id).to_vec(),
        )
    };
    let selected = selection.contains(&id);
    let label = |ui: &mut Ui| {
        let text = RichText::new(&name);
        let r = ui.selectable_label(selected, text).on_hover_text(&class);
        if r.clicked() {
            let additive =
                ui.input(|i| i.modifiers.ctrl || i.modifiers.command || i.modifiers.shift);
            let mut items = if additive {
                selection.to_vec()
            } else {
                Vec::new()
            };
            if additive && selected {
                items.retain(|x| *x != id);
            } else {
                items.push(id);
            }
            doc.select(items);
        }
    };
    if kids.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(18.0);
            label(ui);
        });
    } else {
        egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            ui.make_persistent_id(("explorer", id.raw())),
            false,
        )
        .show_header(ui, label)
        .body(|ui| {
            for k in kids {
                explorer_node(ui, doc, k, selection);
            }
        });
    }
}

fn editable(ty: &ValueType) -> bool {
    matches!(
        ty,
        ValueType::Enum(_)
            | ValueType::Value(
                VariantType::Bool
                    | VariantType::Int
                    | VariantType::Int64
                    | VariantType::Float
                    | VariantType::Double
                    | VariantType::String
                    | VariantType::Content
                    | VariantType::Vector3
                    | VariantType::Color3
                    | VariantType::BrickColor
                    | VariantType::CFrame
            )
    )
}

pub fn properties(ui: &mut Ui, doc: &mut Document) {
    let selection = doc.selection();
    let Some(&id) = selection.last() else {
        ui.label("Select an object to see its properties.");
        return;
    };
    let class = {
        let e = doc.runtime.engine.lock();
        if !e.dm.contains(id) {
            return;
        }
        e.dm.class_of(id).unwrap_or("Instance").to_string()
    };
    let db = database();
    let mut groups: BTreeMap<String, Vec<(String, ValueType, bool)>> = BTreeMap::new();
    let mut seen = std::collections::HashSet::new();
    for c in db.superclasses(&class) {
        for p in &c.properties {
            if p.deprecated_by.is_some()
                || !matches!(p.security, Security::None | Security::Plugin)
                || !p.is_public()
                || !seen.insert(p.name.clone())
            {
                continue;
            }
            groups
                .entry(if p.category.is_empty() {
                    "Data".into()
                } else {
                    p.category.clone()
                })
                .or_default()
                .push((p.name.clone(), p.ty.clone(), p.read_only));
        }
    }
    ui.label(RichText::new(class.as_str()).strong());
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let current = doc.runtime.engine.lock().dm.name_of(id).to_string();
            let renamed = ui
                .horizontal(|ui| {
                    ui.label("Name");
                    text_field(ui, ("name", id.raw()), &current)
                })
                .inner;
            if let Some(n) = renamed.filter(|n| !n.is_empty() && *n != current) {
                doc.rename(id, &n);
            }
            for (group, props) in groups {
                egui::CollapsingHeader::new(group)
                    .default_open(true)
                    .show(ui, |ui| {
                        egui::Grid::new(("props", id.raw()))
                            .num_columns(2)
                            .striped(true)
                            .show(ui, |ui| {
                                for (name, ty, read_only) in props {
                                    let Ok(value) = doc.runtime.engine.lock().dm.get(id, &name)
                                    else {
                                        continue;
                                    };
                                    ui.label(&name);
                                    if read_only || !editable(&ty) {
                                        ui.label(
                                            RichText::new(display(&value)).color(Color32::GRAY),
                                        );
                                    } else if let Some(v) =
                                        edit_value(ui, &ty, &value, &name, id.raw())
                                    {
                                        let _ = doc.set_property(id, &name, v);
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            }
        });
}

fn display(v: &Variant) -> String {
    match v {
        Variant::Ref(Some(r)) => format!("#{}", r.raw()),
        Variant::Ref(None) => "nil".into(),
        Variant::String(s) | Variant::Content(s) => s.clone(),
        Variant::ProtectedString(s) => format!("{} chars", s.len()),
        other => format!("{other:?}"),
    }
}

fn drag_f32(ui: &mut Ui, v: &mut f32) -> bool {
    ui.add(egui::DragValue::new(v).speed(0.1).max_decimals(3))
        .changed()
}

fn text_field(
    ui: &mut Ui,
    key: impl std::hash::Hash + std::fmt::Debug,
    current: &str,
) -> Option<String> {
    let id = egui::Id::unique(key);
    let mut buffer: String = ui
        .data(|d| d.get_temp::<String>(id))
        .unwrap_or_else(|| current.to_string());
    let response = ui.text_edit_singleline(&mut buffer);
    if response.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buffer));
        None
    } else if response.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(id));
        (buffer != current).then_some(buffer)
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
        None
    }
}

fn edit_value(
    ui: &mut Ui,
    ty: &ValueType,
    value: &Variant,
    name: &str,
    owner: u64,
) -> Option<Variant> {
    match (ty, value) {
        (ValueType::Enum(e), Variant::Enum(cur)) => {
            let desc = database().enum_desc(e)?;
            let current = desc
                .items
                .iter()
                .find(|i| i.value == *cur)
                .map(|i| i.name.clone())
                .unwrap_or_else(|| cur.to_string());
            let mut out = None;
            egui::ComboBox::from_id_salt(("enum", owner, name))
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for item in &desc.items {
                        if ui
                            .selectable_label(item.value == *cur, &item.name)
                            .clicked()
                        {
                            out = Some(Variant::Enum(item.value));
                        }
                    }
                });
            out
        }
        (_, Variant::Bool(b)) => {
            let mut b = *b;
            ui.checkbox(&mut b, "")
                .changed()
                .then_some(Variant::Bool(b))
        }
        (_, Variant::Int(i)) => {
            let mut i = *i;
            ui.add(egui::DragValue::new(&mut i))
                .changed()
                .then_some(Variant::Int(i))
        }
        (_, Variant::Int64(i)) => {
            let mut i = *i;
            ui.add(egui::DragValue::new(&mut i))
                .changed()
                .then_some(Variant::Int64(i))
        }
        (_, Variant::Float(f)) => {
            let mut f = *f;
            drag_f32(ui, &mut f).then_some(Variant::Float(f))
        }
        (_, Variant::Double(f)) => {
            let mut f = *f;
            ui.add(egui::DragValue::new(&mut f).speed(0.1).max_decimals(3))
                .changed()
                .then_some(Variant::Double(f))
        }
        (_, Variant::String(s)) => text_field(ui, ("prop", owner, name), s).map(Variant::String),
        (_, Variant::Content(s)) => text_field(ui, ("prop", owner, name), s).map(Variant::Content),
        (_, Variant::Vector3(v)) => {
            let mut v = *v;
            let changed = ui
                .horizontal(|ui| {
                    drag_f32(ui, &mut v.x) | drag_f32(ui, &mut v.y) | drag_f32(ui, &mut v.z)
                })
                .inner;
            changed.then_some(Variant::Vector3(v))
        }
        (_, Variant::CFrame(cf)) => {
            let mut p = cf.translation;
            let changed = ui
                .horizontal(|ui| {
                    drag_f32(ui, &mut p.x) | drag_f32(ui, &mut p.y) | drag_f32(ui, &mut p.z)
                })
                .inner;
            changed.then_some(Variant::CFrame(CFrame::new(cf.rotation, p)))
        }
        (_, Variant::Color3(c)) => {
            let mut rgb = [c.r, c.g, c.b];
            ui.color_edit_button_rgb(&mut rgb)
                .changed()
                .then_some(Variant::Color3(Color3 {
                    r: rgb[0],
                    g: rgb[1],
                    b: rgb[2],
                }))
        }
        (_, Variant::BrickColor(b)) => {
            let mut out = None;
            let c = b.color3uint8();
            egui::ComboBox::from_id_salt(("brick", owner, name))
                .selected_text(RichText::new(b.name()).color(Color32::from_rgb(c.r, c.g, c.b)))
                .show_ui(ui, |ui| {
                    for p in BrickColor::palette() {
                        let pc = p.color3uint8();
                        let text =
                            RichText::new(p.name()).color(Color32::from_rgb(pc.r, pc.g, pc.b));
                        if ui.selectable_label(p == *b, text).clicked() {
                            out = Some(Variant::BrickColor(p));
                        }
                    }
                });
            out
        }
        _ => {
            ui.label(display(value));
            None
        }
    }
}

pub fn script_editor(ui: &mut Ui, doc: &mut Document, id: InstanceId, buffer: &mut String) -> bool {
    let mut keep_open = true;
    ui.horizontal(|ui| {
        if ui.button("Apply").clicked() {
            doc.set_script_source(id, buffer);
        }
        if ui.button("Close").clicked() {
            doc.set_script_source(id, buffer);
            keep_open = false;
        }
    });
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add(
            egui::TextEdit::multiline(buffer)
                .font(egui::TextStyle::Monospace)
                .code_editor()
                .desired_width(f32::INFINITY)
                .desired_rows(30),
        );
    });
    keep_open
}
