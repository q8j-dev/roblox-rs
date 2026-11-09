use crate::app::Studio;
use crate::theme;
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use rbx_dom::InstanceId;
use rbx_reflection::{ParamDescriptor, Security, ValueType, database};
use std::collections::HashMap;

const ROW: f32 = 19.0;

pub type ScriptMessages = (InstanceId, String, Vec<(usize, String)>);

#[derive(Default)]
pub struct Views {
    pub analysis: Vec<ScriptMessages>,
    pub analysis_age: f64,
    pub find_results: Vec<(InstanceId, usize, String)>,
    pub find_query: String,
    pub browser_class: Option<String>,
    pub browser_member: Option<(String, String)>,
    pub browser_filter: String,
    pub perf_last: HashMap<InstanceId, (f64, u64)>,
    pub perf_rows: Vec<(String, usize, f64, f64)>,
    pub perf_age: f64,
    pub frame_ms: f64,
    pub step_ms: f64,
    pub render_ms: f64,
}

pub fn header(ui: &mut Ui, columns: &[(&str, f32)]) -> Vec<f32> {
    let width = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, 0.0, Color32::WHITE);
    p.line_segment(
        [
            r.left_bottom() - vec2(0.0, 0.5),
            r.right_bottom() - vec2(0.0, 0.5),
        ],
        Stroke::new(1.0, theme::RIBBON_SEPARATOR),
    );
    let total: f32 = columns.iter().map(|c| c.1).sum();
    let mut x = r.left();
    let mut xs = Vec::new();
    for (name, frac) in columns {
        let w = width * frac / total;
        xs.push(x);
        p.text(
            pos2(x + 6.0, r.center().y),
            Align2::LEFT_CENTER,
            *name,
            FontId::proportional(theme::FONT_SIZE),
            Color32::BLACK,
        );
        p.line_segment(
            [
                pos2(x + w - 0.5, r.top() + 3.0),
                pos2(x + w - 0.5, r.bottom() - 3.0),
            ],
            Stroke::new(1.0, theme::RIBBON_SEPARATOR),
        );
        x += w;
    }
    xs.push(r.right());
    xs
}

pub fn row(ui: &mut Ui, xs: &[f32], cells: &[String], color: Color32) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
    if resp.hovered() {
        theme::hovered_row(ui.painter(), r);
    }
    for (i, c) in cells.iter().enumerate() {
        let (x0, x1) = (xs[i], xs[i + 1]);
        ui.painter()
            .with_clip_rect(Rect::from_x_y_ranges(x0..=x1 - 2.0, r.y_range()))
            .text(
                pos2(x0 + 6.0, r.center().y),
                Align2::LEFT_CENTER,
                c,
                FontId::proportional(theme::FONT_SIZE),
                color,
            );
    }
    resp
}

fn white(ui: &mut Ui) {
    let full = ui.available_rect_before_wrap();
    ui.painter().rect_filled(full, 0.0, Color32::WHITE);
    ui.painter().rect_stroke(
        full,
        0.0,
        Stroke::new(1.0, theme::VIEW_BORDER),
        egui::StrokeKind::Inside,
    );
}

fn type_name(t: &ValueType) -> String {
    match t {
        ValueType::Value(v) => {
            let s = format!("{v:?}");
            match s.as_str() {
                "Bool" => "bool".into(),
                "Int" => "int".into(),
                "Int64" => "int64".into(),
                "Float" => "float".into(),
                "Double" => "double".into(),
                "String" => "string".into(),
                _ => s,
            }
        }
        ValueType::Enum(e) => e.clone(),
        ValueType::Ref(c) => c.clone(),
        ValueType::Void => "void".into(),
        other => format!("{other:?}"),
    }
}

fn args_text(params: &[ParamDescriptor]) -> String {
    params
        .iter()
        .map(|p| match &p.default {
            Some(d) => format!("{} {} = {d}", type_name(&p.ty), p.name),
            None => format!("{} {}", type_name(&p.ty), p.name),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn security_icon(base: usize, security: Security) -> usize {
    match security {
        Security::None => base,
        _ => base + 1,
    }
}

impl Studio {
    pub fn refresh_analysis(&mut self) {
        let scripts: Vec<(InstanceId, String, String)> = {
            let e = self.doc.runtime.engine.lock();
            e.dm.descendants(e.dm.root())
                .into_iter()
                .filter(|d| e.dm.is_a(*d, "LuaSourceContainer"))
                .filter_map(|d| {
                    let src = e.dm.get(d, "Source").ok()?.as_str().map(str::to_string)?;
                    Some((d, e.dm.full_name(d), src))
                })
                .collect()
        };
        let lua = &self.doc.runtime.lua;
        self.views.analysis = scripts
            .into_iter()
            .map(|(id, name, src)| {
                let mut messages = Vec::new();
                if let Err(err) = lua.load(&src).set_name(format!("={name}")).into_function() {
                    let text = err.to_string();
                    let line = text
                        .split(':')
                        .nth(1)
                        .and_then(|l| l.trim().parse::<usize>().ok())
                        .unwrap_or(1);
                    let msg = text
                        .splitn(3, ':')
                        .nth(2)
                        .unwrap_or(&text)
                        .trim()
                        .to_string();
                    messages.push((line, msg));
                }
                (id, name, messages)
            })
            .filter(|(_, _, m)| !m.is_empty())
            .collect();
    }

    pub fn script_analysis_ui(&mut self, ui: &mut Ui) {
        let now = ui.input(|i| i.time);
        if now - self.views.analysis_age > 1.0 {
            self.views.analysis_age = now;
            self.refresh_analysis();
        }
        white(ui);
        let mut open = None;
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.views.analysis.is_empty() {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("  No errors or warnings").color(theme::DISABLED_TEXT),
                    );
                }
                for (id, name, messages) in &self.views.analysis {
                    let (r, _) =
                        ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
                    ui.painter().text(
                        pos2(r.left() + 6.0, r.center().y),
                        Align2::LEFT_CENTER,
                        format!("{name} ({})", messages.len()),
                        theme::bold(theme::FONT_SIZE),
                        Color32::BLACK,
                    );
                    for (line, msg) in messages {
                        let (r, resp) =
                            ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                        if resp.hovered() {
                            theme::hovered_row(ui.painter(), r);
                        }
                        ui.painter().text(
                            pos2(r.left() + 26.0, r.center().y),
                            Align2::LEFT_CENTER,
                            format!("E {line}: {msg}"),
                            FontId::proportional(theme::FONT_SIZE),
                            Color32::from_rgb(200, 0, 0),
                        );
                        if resp.double_clicked() {
                            open = Some(*id);
                        }
                    }
                }
            });
        if let Some(id) = open {
            self.open_script(id);
        }
    }

    pub fn diagnostics_ui(&mut self, ui: &mut Ui) {
        white(ui);
        let mut counts: HashMap<String, usize> = HashMap::new();
        let (total, parts, game_time, physics_fps) = {
            let e = self.doc.runtime.engine.lock();
            let all = e.dm.descendants(e.dm.root());
            let mut parts = 0;
            for d in &all {
                let c = e.dm.class_of(*d).unwrap_or("Instance").to_string();
                if e.dm.is_a(*d, "BasePart") {
                    parts += 1;
                }
                *counts.entry(c).or_default() += 1;
            }
            (all.len(), parts, e.game_time, e.fps)
        };
        let threads = self.doc.runtime.running_thread_count();
        let xs = header(ui, &[("Name", 1.0), ("Value", 1.0)]);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (k, v) in [
                    ("Instances", total.to_string()),
                    ("Parts", parts.to_string()),
                    ("Lua threads", threads.to_string()),
                    ("Game time", format!("{game_time:.2}")),
                    ("Physics fps", format!("{physics_fps:.1}")),
                    ("Frame time", format!("{:.2} ms", self.views.frame_ms)),
                ] {
                    row(ui, &xs, &[k.to_string(), v], Color32::BLACK);
                }
                let mut classes: Vec<(String, usize)> = counts.into_iter().collect();
                classes.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
                for (c, n) in classes {
                    row(
                        ui,
                        &xs,
                        &[format!("    {c}"), n.to_string()],
                        Color32::BLACK,
                    );
                }
            });
    }

    pub fn task_scheduler_ui(&mut self, ui: &mut Ui) {
        white(ui);
        let xs = header(
            ui,
            &[
                ("Name", 2.0),
                ("Error", 0.7),
                ("Priority", 0.8),
                ("Activity", 0.8),
                ("Rate", 0.7),
                ("CV", 0.6),
                ("Time", 0.8),
            ],
        );
        let frame = self.views.frame_ms.max(0.001);
        let fps = 1000.0 / frame;
        let playing = self.play.is_some();
        let rows = [
            ("Render", self.views.render_ms, fps),
            ("Heartbeat", 0.05, fps),
            (
                "Physics",
                if playing { self.views.step_ms } else { 0.0 },
                if playing { 60.0 } else { 0.0 },
            ),
            ("Lua Wait", self.views.step_ms * 0.1, fps),
        ];
        for (name, ms, rate) in rows {
            row(
                ui,
                &xs,
                &[
                    name.to_string(),
                    "0".into(),
                    "Normal".into(),
                    format!("{:.1}%", ms / frame * 100.0),
                    format!("{rate:.1}"),
                    "0".into(),
                    format!("{ms:.2}ms"),
                ],
                Color32::BLACK,
            );
        }
    }

    pub fn script_performance_ui(&mut self, ui: &mut Ui) {
        let now = ui.input(|i| i.time);
        let dt = now - self.views.perf_age;
        if dt > 1.0 {
            self.views.perf_age = now;
            let stats = self.doc.runtime.script_activity();
            let e = self.doc.runtime.engine.lock();
            let mut grouped: HashMap<String, (usize, f64, f64)> = HashMap::new();
            for (id, t, n) in stats {
                let (lt, ln) = self.views.perf_last.insert(id, (t, n)).unwrap_or((t, n));
                let name = e.dm.full_name(id);
                let g = grouped.entry(name).or_insert((0, 0.0, 0.0));
                g.0 += 1;
                g.1 += (t - lt) / dt * 100.0;
                g.2 += (n - ln) as f64 / dt;
            }
            let mut rows: Vec<(String, usize, f64, f64)> = grouped
                .into_iter()
                .map(|(k, v)| (k, v.0, v.1, v.2))
                .collect();
            rows.sort_by(|a, b| {
                b.2.partial_cmp(&a.2)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.0.cmp(&b.0))
            });
            self.views.perf_rows = rows;
        }
        white(ui);
        let xs = header(
            ui,
            &[
                ("Name", 3.0),
                ("Count", 0.6),
                ("Activity", 0.8),
                ("Rate", 0.8),
            ],
        );
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (name, count, activity, rate) in &self.views.perf_rows {
                    row(
                        ui,
                        &xs,
                        &[
                            name.clone(),
                            count.to_string(),
                            format!("{activity:.3}%"),
                            format!("{rate:.1}/s"),
                        ],
                        Color32::BLACK,
                    );
                }
            });
    }

    pub fn find_in_scripts(&mut self, query: &str) {
        self.views.find_query = query.to_string();
        let needle = query.to_lowercase();
        let e = self.doc.runtime.engine.lock();
        let mut out = Vec::new();
        if !needle.is_empty() {
            for d in e.dm.descendants(e.dm.root()) {
                if !e.dm.is_a(d, "LuaSourceContainer") {
                    continue;
                }
                let Some(src) =
                    e.dm.get(d, "Source")
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_string))
                else {
                    continue;
                };
                for (n, line) in src.lines().enumerate() {
                    if line.to_lowercase().contains(&needle) {
                        out.push((d, n + 1, line.trim().to_string()));
                    }
                }
            }
        }
        self.views.find_results = out;
    }

    pub fn find_results_ui(&mut self, ui: &mut Ui) {
        white(ui);
        let mut open = None;
        let names: Vec<String> = {
            let e = self.doc.runtime.engine.lock();
            self.views
                .find_results
                .iter()
                .map(|(id, _, _)| e.dm.full_name(*id))
                .collect()
        };
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(2.0);
                ui.label(format!(
                    "  Find all \"{}\" — {} matches",
                    self.views.find_query,
                    self.views.find_results.len()
                ));
                for (i, (id, line, text)) in self.views.find_results.iter().enumerate() {
                    let (r, resp) =
                        ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                    if resp.hovered() {
                        theme::hovered_row(ui.painter(), r);
                    }
                    ui.painter().text(
                        pos2(r.left() + 8.0, r.center().y),
                        Align2::LEFT_CENTER,
                        format!("{}({line}): {text}", names[i]),
                        FontId::proportional(theme::FONT_SIZE),
                        Color32::BLACK,
                    );
                    if resp.double_clicked() {
                        open = Some(*id);
                    }
                }
            });
        if let Some(id) = open {
            self.open_script(id);
        }
    }

    pub fn object_browser_ui(&mut self, ui: &mut Ui) {
        white(ui);
        let full = ui.available_rect_before_wrap();
        let left = Rect::from_min_max(
            full.min,
            pos2(full.left() + full.width() * 0.28, full.bottom()),
        );
        let right = Rect::from_min_max(pos2(left.right() + 4.0, full.top()), full.max);
        let members_rect = Rect::from_min_max(
            right.min,
            pos2(right.right(), right.top() + right.height() * 0.65),
        );
        let decl_rect =
            Rect::from_min_max(pos2(right.left(), members_rect.bottom() + 4.0), right.max);
        let db = database();
        let mut classes: Vec<&str> = db.classes().map(|c| c.name.as_str()).collect();
        classes.sort_unstable();
        let mut picked_class = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
            ui.add_sized(
                vec2(left.width(), 22.0),
                egui::TextEdit::singleline(&mut self.views.browser_filter)
                    .hint_text("Search")
                    .margin(vec2(4.0, 3.0)),
            );
            let filter = self.views.browser_filter.to_lowercase();
            egui::ScrollArea::vertical()
                .id_salt("ob-classes")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for c in classes
                        .iter()
                        .filter(|c| filter.is_empty() || c.to_lowercase().contains(&filter))
                    {
                        let (r, resp) =
                            ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                        let selected = self.views.browser_class.as_deref() == Some(*c);
                        if selected {
                            theme::selected_row(ui.painter(), r);
                        } else if resp.hovered() {
                            theme::hovered_row(ui.painter(), r);
                        }
                        if let Some((tex, uv)) =
                            self.icons
                                .strip(ui.ctx(), "images/img_classtree.bmp", 3, 16)
                        {
                            ui.painter().image(
                                tex,
                                Rect::from_min_size(
                                    pos2(r.left() + 4.0, r.center().y - 8.0),
                                    vec2(16.0, 16.0),
                                ),
                                uv,
                                Color32::WHITE,
                            );
                        }
                        ui.painter().text(
                            pos2(r.left() + 24.0, r.center().y),
                            Align2::LEFT_CENTER,
                            *c,
                            FontId::proportional(theme::FONT_SIZE),
                            if selected {
                                Color32::WHITE
                            } else {
                                Color32::BLACK
                            },
                        );
                        if resp.clicked() {
                            picked_class = Some(c.to_string());
                        }
                    }
                });
        });
        if let Some(c) = picked_class {
            self.views.browser_class = Some(c);
            self.views.browser_member = None;
        }
        let Some(class) = self.views.browser_class.clone() else {
            return;
        };
        let Some(desc) = db.class(&class) else { return };
        let mut members: Vec<(String, usize, String, String)> = Vec::new();
        for p in &desc.properties {
            members.push((
                p.name.clone(),
                security_icon(6, p.security),
                format!("{} {}", type_name(&p.ty), p.name),
                "Property".into(),
            ));
        }
        for f in &desc.functions {
            members.push((
                format!("{}()", f.name),
                security_icon(4, f.security),
                format!("{} {}({})", type_name(&f.ret), f.name, args_text(&f.params)),
                "Function".into(),
            ));
        }
        for ev in &desc.events {
            members.push((
                ev.name.clone(),
                security_icon(11, ev.security),
                format!("{}({})", ev.name, args_text(&ev.params)),
                "Event".into(),
            ));
        }
        members.sort_by(|a, b| a.0.cmp(&b.0));
        let mut picked = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(members_rect), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("ob-members")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (label, icon, decl, _) in &members {
                        let (r, resp) =
                            ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                        let selected = self
                            .views
                            .browser_member
                            .as_ref()
                            .is_some_and(|m| m.0 == *label);
                        if selected {
                            theme::selected_row(ui.painter(), r);
                        } else if resp.hovered() {
                            theme::hovered_row(ui.painter(), r);
                        }
                        if let Some((tex, uv)) =
                            self.icons
                                .strip(ui.ctx(), "images/img_classtree.bmp", *icon, 16)
                        {
                            ui.painter().image(
                                tex,
                                Rect::from_min_size(
                                    pos2(r.left() + 4.0, r.center().y - 8.0),
                                    vec2(16.0, 16.0),
                                ),
                                uv,
                                Color32::WHITE,
                            );
                        }
                        ui.painter().text(
                            pos2(r.left() + 24.0, r.center().y),
                            Align2::LEFT_CENTER,
                            label,
                            FontId::proportional(theme::FONT_SIZE),
                            if selected {
                                Color32::WHITE
                            } else {
                                Color32::BLACK
                            },
                        );
                        if resp.clicked() {
                            picked = Some((label.clone(), decl.clone()));
                        }
                    }
                });
        });
        if picked.is_some() {
            self.views.browser_member = picked;
        }
        let p = ui.painter();
        p.rect_stroke(
            decl_rect,
            0.0,
            Stroke::new(1.0, theme::VIEW_BORDER),
            egui::StrokeKind::Inside,
        );
        let (decl, owner) = match &self.views.browser_member {
            Some((_, d)) => (d.clone(), Some(class.clone())),
            None => (format!("class {class}"), desc.superclass.clone()),
        };
        p.text(
            decl_rect.min + vec2(8.0, 8.0),
            Align2::LEFT_TOP,
            decl,
            theme::bold(theme::FONT_SIZE),
            Color32::BLACK,
        );
        if let Some(o) = owner {
            let label = if self.views.browser_member.is_some() {
                format!("Member of {o}")
            } else {
                format!("Inherits from {o}")
            };
            p.text(
                decl_rect.min + vec2(8.0, 28.0),
                Align2::LEFT_TOP,
                label,
                FontId::proportional(theme::FONT_SIZE),
                Color32::BLACK,
            );
        }
        let _ = Pos2::ZERO;
    }
}
