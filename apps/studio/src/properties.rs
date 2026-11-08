use crate::theme;
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};
use rbx_dom::{InstanceId, Variant};
use rbx_math::{BrickColor, CFrame, Color3, UDim, UDim2, Vec2, Vec3};
use rbx_reflection::{Security, ValueType, VariantType, database};
use rbx_studio_ui::Document;
use std::collections::{HashMap, HashSet};

const ROW: f32 = 20.0;

#[derive(Default)]
pub struct Properties {
    collapsed: HashSet<String>,
    initialized: bool,
    editing: Option<(InstanceId, String, String)>,
    popup: Option<(InstanceId, String, Rect)>,
    pub split: f32,
    pub filter: String,
}

struct Prop {
    name: String,
    ty: ValueType,
    read_only: bool,
}

fn category_name(raw: &str) -> String {
    match raw {
        "CAT_VIS" => "Appearance".into(),
        "CAT_EMISSION" => "Emission".into(),
        "CAT_MOTION" => "Motion".into(),
        "CAT_PARTICLE_BEHAVIOR" => "Particles".into(),
        r if r.starts_with("category_") => {
            let s = &r["category_".len()..];
            match s {
                "Part" | "BasicPart" | "Pyramid" | "CornerWedge" | "ParallelRamp" | "ExtrudedPart"
                | "RightAngleRamp" | "Prism" => "Part".into(),
                other => other.into(),
            }
        }
        "" => "Data".into(),
        r => r.trim().into(),
    }
}

fn category_rank(name: &str) -> (u8, String) {
    let r = match name {
        "Appearance" => 0,
        "Data" => 1,
        "Behavior" => 2,
        "Part" => 3,
        "Surface Inputs" => 8,
        "Surface" => 9,
        _ => 5,
    };
    (r, name.to_string())
}

fn num(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.3}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn to8(c: f32) -> i32 {
    (c.clamp(0.0, 1.0) * 255.0).round() as i32
}

pub fn display(doc: &Document, v: &Variant) -> String {
    match v {
        Variant::Nil => String::new(),
        Variant::Bool(b) => b.to_string(),
        Variant::Int(i) => i.to_string(),
        Variant::Int64(i) => i.to_string(),
        Variant::Float(f) => num(*f as f64),
        Variant::Double(f) => num(*f),
        Variant::String(s) | Variant::Content(s) => s.clone(),
        Variant::ProtectedString(_) => String::new(),
        Variant::Vector3(p) => format!("{}, {}, {}", num(p.x as f64), num(p.y as f64), num(p.z as f64)),
        Variant::Vector2(p) => format!("{}, {}", num(p.x as f64), num(p.y as f64)),
        Variant::CFrame(cf) => {
            let p = cf.translation;
            format!("{}, {}, {}", num(p.x as f64), num(p.y as f64), num(p.z as f64))
        }
        Variant::Color3(c) => format!("[{}, {}, {}]", to8(c.r), to8(c.g), to8(c.b)),
        Variant::BrickColor(b) => b.name().to_string(),
        Variant::UDim(u) => format!("{}, {}", num(u.scale as f64), u.offset),
        Variant::UDim2(u) => format!(
            "{{{}, {}}}, {{{}, {}}}",
            num(u.x.scale as f64),
            u.x.offset,
            num(u.y.scale as f64),
            u.y.offset
        ),
        Variant::Ref(Some(r)) => doc.runtime.engine.lock().dm.name_of(*r).to_string(),
        Variant::Ref(None) => String::new(),
        Variant::NumberRange(r) => format!("{} {}", num(r.min as f64), num(r.max as f64)),
        other => {
            let s = format!("{other:?}");
            s.split_once('(').map(|(_, rest)| rest.trim_end_matches(')').to_string()).unwrap_or(s)
        }
    }
}

fn floats(s: &str) -> Vec<f32> {
    s.split(|c: char| c == ',' || c.is_whitespace() || c == '[' || c == ']' || c == '{' || c == '}')
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse::<f32>().ok())
        .collect()
}

fn parse(text: &str, current: &Variant) -> Option<Variant> {
    let t = text.trim();
    Some(match current {
        Variant::Int(_) => Variant::Int(t.parse::<f64>().ok()? as i32),
        Variant::Int64(_) => Variant::Int64(t.parse::<f64>().ok()? as i64),
        Variant::Float(_) => Variant::Float(t.parse().ok()?),
        Variant::Double(_) => Variant::Double(t.parse().ok()?),
        Variant::String(_) => Variant::String(text.to_string()),
        Variant::Content(_) => Variant::Content(t.to_string()),
        Variant::Vector3(_) => {
            let f = floats(t);
            Variant::Vector3(Vec3::new(*f.first()?, *f.get(1)?, *f.get(2)?))
        }
        Variant::Vector2(_) => {
            let f = floats(t);
            Variant::Vector2(Vec2::new(*f.first()?, *f.get(1)?))
        }
        Variant::CFrame(cf) => {
            let f = floats(t);
            Variant::CFrame(CFrame::new(cf.rotation, Vec3::new(*f.first()?, *f.get(1)?, *f.get(2)?)))
        }
        Variant::Color3(_) => {
            let f = floats(t);
            Variant::Color3(Color3::new(*f.first()? / 255.0, *f.get(1)? / 255.0, *f.get(2)? / 255.0))
        }
        Variant::UDim(_) => {
            let f = floats(t);
            Variant::UDim(UDim::new(*f.first()?, *f.get(1)? as i32))
        }
        Variant::UDim2(_) => {
            let f = floats(t);
            Variant::UDim2(UDim2::new(*f.first()?, *f.get(1)? as i32, *f.get(2)?, *f.get(3)? as i32))
        }
        Variant::BrickColor(_) => {
            let b = BrickColor::all().find(|b| b.name().eq_ignore_ascii_case(t))?;
            Variant::BrickColor(b)
        }
        _ => return None,
    })
}

fn editable(ty: &ValueType, value: &Variant) -> bool {
    match ty {
        ValueType::Enum(_) => true,
        ValueType::Value(VariantType::Ref) => false,
        ValueType::Value(VariantType::ProtectedString) => false,
        ValueType::Value(_) => !matches!(value, Variant::Nil | Variant::BinaryString(_)),
        _ => false,
    }
}

fn swatch(c: Color3) -> Color32 {
    Color32::from_rgb(to8(c.r) as u8, to8(c.g) as u8, to8(c.b) as u8)
}

impl Properties {
    fn gather(class: &str) -> Vec<(String, Vec<Prop>)> {
        let db = database();
        let mut groups: HashMap<String, Vec<Prop>> = HashMap::new();
        let mut seen = HashSet::new();
        for c in db.superclasses(class) {
            for p in &c.properties {
                let shown_type = !matches!(
                    p.ty,
                    ValueType::Value(
                        VariantType::CFrame
                            | VariantType::Ray
                            | VariantType::Region3
                            | VariantType::Region3int16
                            | VariantType::ProtectedString
                            | VariantType::BinaryString
                            | VariantType::SystemAddress
                    )
                );
                if p.deprecated_by.is_some()
                    || p.security != Security::None
                    || !p.is_public()
                    || !shown_type
                    || !seen.insert(p.name.clone())
                {
                    continue;
                }
                groups.entry(category_name(&p.category)).or_default().push(Prop {
                    name: p.name.clone(),
                    ty: p.ty.clone(),
                    read_only: p.read_only,
                });
            }
        }
        let mut out: Vec<(String, Vec<Prop>)> = groups.into_iter().collect();
        out.sort_by_key(|(n, _)| category_rank(n));
        for (_, props) in &mut out {
            props.sort_by(|a, b| a.name.cmp(&b.name));
        }
        out
    }

    pub fn show(&mut self, ui: &mut Ui, doc: &mut Document) {
        if !self.initialized {
            self.collapsed.insert("Surface".into());
            self.collapsed.insert("Surface Inputs".into());
            self.split = 0.45;
            self.initialized = true;
        }
        let selection = doc.selection();
        let label = selection_label(doc, &selection);
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
        ui.painter().rect_filled(bar, 0.0, theme::WINDOW);
        ui.painter().rect_stroke(bar, 0.0, Stroke::new(1.0, theme::EDIT_BORDER), egui::StrokeKind::Inside);
        ui.painter().with_clip_rect(bar.shrink(2.0)).text(
            pos2(bar.left() + 9.0, bar.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(theme::FONT_SIZE),
            Color32::BLACK,
        );
        ui.add_space(4.0);
        ui.add_sized(
            vec2(ui.available_width(), 22.0),
            egui::TextEdit::singleline(&mut self.filter).hint_text("Search Properties").margin(vec2(4.0, 3.0)),
        );
        ui.add_space(4.0);
        let full = ui.available_rect_before_wrap();
        ui.painter().rect_filled(full, 0.0, Color32::WHITE);
        ui.painter().rect_stroke(full, 0.0, Stroke::new(1.0, theme::VIEW_BORDER), egui::StrokeKind::Inside);
        let Some(&id) = selection.last() else {
            return;
        };
        let class = {
            let e = doc.runtime.engine.lock();
            if !e.dm.contains(id) {
                return;
            }
            e.dm.class_of(id).unwrap_or("Instance").to_string()
        };
        let needle = self.filter.trim().to_lowercase();
        let mut groups = Properties::gather(&class);
        if !needle.is_empty() {
            for (_, props) in &mut groups {
                props.retain(|p| p.name.to_lowercase().contains(&needle));
            }
            groups.retain(|(_, props)| !props.is_empty());
        }
        let font = FontId::proportional(theme::FONT_SIZE);
        let bold = theme::bold(theme::FONT_SIZE);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let width = ui.available_width();
                let split_x = (width * self.split).clamp(60.0, width - 60.0);
                for (cat, props) in &groups {
                    let (r, resp) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
                    ui.painter().rect_filled(r, 0.0, theme::CATEGORY);
                    let open = !self.collapsed.contains(cat);
                    let c = pos2(r.left() + 9.0, r.center().y);
                    if open {
                        ui.painter().add(egui::Shape::convex_polygon(
                            vec![c + vec2(2.5, -2.5), c + vec2(2.5, 2.5), c + vec2(-2.5, 2.5)],
                            Color32::from_rgb(38, 38, 38),
                            Stroke::NONE,
                        ));
                    } else {
                        ui.painter().add(egui::Shape::convex_polygon(
                            vec![c + vec2(-2.0, -4.0), c + vec2(2.5, 0.0), c + vec2(-2.0, 4.0)],
                            Color32::TRANSPARENT,
                            Stroke::new(1.0, Color32::from_rgb(110, 110, 110)),
                        ));
                    }
                    ui.painter().text(pos2(r.left() + 18.0, r.center().y), Align2::LEFT_CENTER, cat, bold.clone(), Color32::BLACK);
                    ui.painter().line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, theme::GRID_LINE));
                    if resp.clicked() && !self.collapsed.remove(cat) {
                        self.collapsed.insert(cat.clone());
                    }
                    if !open {
                        continue;
                    }
                    for p in props {
                        let value = {
                            let e = doc.runtime.engine.lock();
                            match p.name.as_str() {
                                "Name" => Ok(Variant::String(e.dm.name_of(id).to_string())),
                                n => e.dm.get(id, n),
                            }
                        };
                        let Ok(value) = value else { continue };
                        let (r, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
                        let name_rect = Rect::from_min_max(r.min, pos2(r.left() + split_x, r.bottom()));
                        let value_rect = Rect::from_min_max(pos2(r.left() + split_x + 1.0, r.top()), r.max);
                        let can_edit = !p.read_only && editable(&p.ty, &value);
                        let text_color = if can_edit { Color32::BLACK } else { Color32::GRAY };
                        ui.painter().text(
                            pos2(name_rect.left() + 18.0, r.center().y),
                            Align2::LEFT_CENTER,
                            &p.name,
                            font.clone(),
                            text_color,
                        );
                        ui.painter().line_segment([name_rect.right_top(), name_rect.right_bottom()], Stroke::new(1.0, theme::GRID_LINE));
                        ui.painter().line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, theme::GRID_LINE));
                        self.value_cell(ui, doc, id, p, &value, value_rect, can_edit, text_color);
                    }
                }
            });
        self.popups(ui, doc);
    }

    #[allow(clippy::too_many_arguments)]
    fn value_cell(&mut self, ui: &mut Ui, doc: &mut Document, id: InstanceId, p: &Prop, value: &Variant, r: Rect, can_edit: bool, color: Color32) {
        let font = FontId::proportional(theme::FONT_SIZE);
        let key = (id, p.name.clone());
        if let Some((eid, ename, buf)) = &mut self.editing
            && (*eid, ename.clone()) == key
        {
            let te = ui.put(
                r.shrink2(vec2(1.0, 1.0)),
                egui::TextEdit::singleline(buf).font(font.clone()).margin(vec2(4.0, 1.0)).frame(egui::Frame::new()),
            );
            te.request_focus();
            if te.lost_focus() {
                let cancelled = ui.input(|i| i.key_pressed(egui::Key::Escape));
                let text = buf.clone();
                self.editing = None;
                if !cancelled {
                    if p.name == "Name" {
                        if !text.is_empty() {
                            doc.rename(id, &text);
                        }
                    } else if let Some(v) = parse(&text, value) {
                        let _ = doc.set_property(id, &p.name, v);
                    }
                }
            }
            return;
        }
        let resp = ui.interact(r, ui.scope_id().with(("prop-value", id.raw(), &p.name)), Sense::click());
        let mut x = r.left() + 4.0;
        match value {
            Variant::Bool(b) => {
                let bx = Rect::from_min_size(pos2(x, r.center().y - 6.5), vec2(13.0, 13.0));
                ui.painter().rect_filled(bx, 0.0, Color32::WHITE);
                ui.painter().rect_stroke(bx, 0.0, Stroke::new(1.0, theme::EDIT_BORDER), egui::StrokeKind::Inside);
                if *b {
                    let s = Stroke::new(1.6, if can_edit { Color32::from_rgb(38, 38, 38) } else { Color32::GRAY });
                    ui.painter().line_segment([bx.left_center() + vec2(2.5, 0.0), bx.center_bottom() + vec2(-1.0, -3.0)], s);
                    ui.painter().line_segment([bx.center_bottom() + vec2(-1.0, -3.0), bx.right_top() + vec2(-2.5, 3.0)], s);
                }
                if resp.clicked() && can_edit {
                    let _ = doc.set_property(id, &p.name, Variant::Bool(!b));
                }
                return;
            }
            Variant::BrickColor(b) => {
                let sw = Rect::from_min_size(pos2(x, r.center().y - 6.0), vec2(12.0, 12.0));
                ui.painter().rect_filled(sw, 0.0, swatch(b.color3()));
                ui.painter().rect_stroke(sw, 0.0, Stroke::new(1.0, Color32::BLACK), egui::StrokeKind::Inside);
                x = sw.right() + 4.0;
            }
            Variant::Color3(c) => {
                let sw = Rect::from_min_size(pos2(x, r.center().y - 6.0), vec2(12.0, 12.0));
                ui.painter().rect_filled(sw, 0.0, swatch(*c));
                ui.painter().rect_stroke(sw, 0.0, Stroke::new(1.0, Color32::BLACK), egui::StrokeKind::Inside);
                x = sw.right() + 4.0;
            }
            _ => {}
        }
        let text = match (&p.ty, value) {
            (ValueType::Enum(e), Variant::Enum(v)) => database()
                .enum_desc(e)
                .and_then(|d| d.items.iter().find(|i| i.value == *v).map(|i| i.name.clone()))
                .unwrap_or_else(|| v.to_string()),
            _ => display(doc, value),
        };
        let galley = ui.painter().layout_no_wrap(text, font.clone(), color);
        ui.painter().with_clip_rect(r.shrink2(vec2(1.0, 0.0))).galley(pos2(x, r.center().y - galley.size().y / 2.0), galley, color);
        if matches!(p.ty, ValueType::Enum(_)) && can_edit {
            let c = pos2(r.right() - 9.0, r.center().y);
            ui.painter().add(egui::Shape::convex_polygon(
                vec![c + vec2(-3.5, -1.5), c + vec2(3.5, -1.5), c + vec2(0.0, 2.0)],
                theme::TEXT,
                Stroke::NONE,
            ));
        }
        if resp.clicked() && can_edit {
            match (&p.ty, value) {
                (ValueType::Enum(_), _) | (_, Variant::BrickColor(_)) => {
                    self.popup = Some((id, p.name.clone(), r));
                }
                _ => {
                    let initial = if p.name == "Name" {
                        doc.runtime.engine.lock().dm.name_of(id).to_string()
                    } else {
                        display(doc, value)
                    };
                    self.editing = Some((id, p.name.clone(), initial));
                }
            }
        }
    }

    fn popups(&mut self, ui: &mut Ui, doc: &mut Document) {
        let Some((id, name, anchor)) = self.popup.clone() else {
            return;
        };
        let (ty, value) = {
            let e = doc.runtime.engine.lock();
            let ty = e.dm.property_descriptor(id, &name).map(|d| d.ty.clone());
            (ty, e.dm.get(id, &name).ok())
        };
        let (Some(ty), Some(value)) = (ty, value) else {
            self.popup = None;
            return;
        };
        let mut chosen = None;
        let area = egui::Area::new(ui.scope_id().with(("prop-popup", id.raw(), &name)))
            .order(egui::Order::Foreground)
            .fixed_pos(anchor.left_bottom())
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .stroke(Stroke::new(1.0, theme::EDIT_BORDER))
                    .inner_margin(1.0)
                    .show(ui, |ui| match (&ty, &value) {
                        (ValueType::Enum(e), Variant::Enum(cur)) => {
                            let Some(desc) = database().enum_desc(e) else { return };
                            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                                ui.set_min_width(anchor.width() - 2.0);
                                for item in &desc.items {
                                    let (r, resp) = ui.allocate_exact_size(vec2(anchor.width() - 2.0, 18.0), Sense::click());
                                    if item.value == *cur {
                                        theme::selected_row(ui.painter(), r);
                                    } else if resp.hovered() {
                                        theme::hovered_row(ui.painter(), r);
                                    }
                                    let color = if item.value == *cur { Color32::WHITE } else { Color32::BLACK };
                                    ui.painter().text(pos2(r.left() + 4.0, r.center().y), Align2::LEFT_CENTER, &item.name, FontId::proportional(theme::FONT_SIZE), color);
                                    if resp.clicked() {
                                        chosen = Some(Variant::Enum(item.value));
                                    }
                                }
                            });
                        }
                        (_, Variant::BrickColor(cur)) => {
                            let colors: Vec<BrickColor> = BrickColor::palette().collect();
                            for row in colors.chunks(8) {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
                                    for b in row {
                                        let (r, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                                        ui.painter().rect_filled(r, 0.0, swatch(b.color3()));
                                        let stroke = if b == cur || resp.hovered() {
                                            Stroke::new(2.0, Color32::BLACK)
                                        } else {
                                            Stroke::new(1.0, Color32::from_gray(120))
                                        };
                                        ui.painter().rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
                                        if resp.on_hover_text(b.name()).clicked() {
                                            chosen = Some(Variant::BrickColor(*b));
                                        }
                                    }
                                });
                            }
                        }
                        _ => {}
                    });
            });
        if let Some(v) = chosen {
            let _ = doc.set_property(id, &name, v);
            self.popup = None;
        } else if ui.input(|i| i.pointer.any_pressed()) && !area.response.contains_pointer() && !ui.rect_contains_pointer(anchor) {
            self.popup = None;
        }
    }
}

fn selection_label(doc: &Document, selection: &[InstanceId]) -> String {
    let e = doc.runtime.engine.lock();
    let items: Vec<InstanceId> = selection.iter().copied().filter(|i| e.dm.contains(*i)).collect();
    let Some(&first) = items.first() else {
        return String::new();
    };
    let name = e.dm.name_of(first).to_string();
    let class = e.dm.class_of(first).unwrap_or("").to_string();
    if items.len() == 1 {
        return format!("{class} \"{name}\"");
    }
    let same_name = items.iter().all(|i| e.dm.name_of(*i) == name);
    let same_class = items.iter().all(|i| e.dm.class_of(*i) == Some(class.as_str()));
    match (same_class, same_name) {
        (true, true) => format!("{class} \"{name}\" - {} items", items.len()),
        (true, false) => format!("{class} - {} items", items.len()),
        _ => format!("{} items", items.len()),
    }
}
