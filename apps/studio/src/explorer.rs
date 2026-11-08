use crate::icons::Icons;
use crate::metadata::ClassMetadata;
use crate::theme;
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};
use rbx_dom::{DataModel, InstanceId};
use rbx_studio_ui::Document;
use std::collections::HashSet;

pub const ROW: f32 = 19.0;
const INDENT: f32 = 19.0;

#[derive(Default)]
pub struct Explorer {
    pub expanded: HashSet<InstanceId>,
    anchor: Option<InstanceId>,
    renaming: Option<(InstanceId, String)>,
    last_selection: Vec<InstanceId>,
}

pub enum Request {
    OpenScript(InstanceId),
    Context(InstanceId),
}

struct Row {
    id: InstanceId,
    depth: usize,
    has_children: bool,
    expanded: bool,
    name: String,
    class: String,
}

pub fn visible_children(dm: &DataModel, meta: &ClassMetadata, id: InstanceId) -> Vec<InstanceId> {
    let root = dm.root();
    let core = dm.find_service("CoreGui");
    let mut kids: Vec<(i64, String, InstanceId)> = dm
        .children_of(id)
        .iter()
        .copied()
        .filter(|&c| {
            let class = dm.class_of(c).unwrap_or("Instance");
            if id == root {
                meta.order(class) >= 0
            } else {
                Some(c) != core && core.is_none_or(|g| !dm.is_ancestor_of(g, Some(c)))
            }
        })
        .map(|c| {
            let class = dm.class_of(c).unwrap_or("Instance");
            (meta.order(class), dm.name_of(c).to_string(), c)
        })
        .collect();
    kids.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    kids.into_iter().map(|k| k.2).collect()
}

impl Explorer {
    fn rows(&self, dm: &DataModel, meta: &ClassMetadata) -> Vec<Row> {
        let mut out = Vec::new();
        let mut stack: Vec<(InstanceId, usize)> = visible_children(dm, meta, dm.root())
            .into_iter()
            .rev()
            .map(|c| (c, 0))
            .collect();
        while let Some((id, depth)) = stack.pop() {
            let kids = visible_children(dm, meta, id);
            let expanded = self.expanded.contains(&id);
            out.push(Row {
                id,
                depth,
                has_children: !kids.is_empty(),
                expanded,
                name: dm.name_of(id).to_string(),
                class: dm.class_of(id).unwrap_or("Instance").to_string(),
            });
            if expanded {
                for k in kids.into_iter().rev() {
                    stack.push((k, depth + 1));
                }
            }
        }
        out
    }

    fn reveal(&mut self, dm: &DataModel, ids: &[InstanceId]) {
        for &id in ids {
            let mut cur = dm.parent_of(id);
            while let Some(p) = cur {
                if p == dm.root() {
                    break;
                }
                self.expanded.insert(p);
                cur = dm.parent_of(p);
            }
        }
    }

    pub fn show(&mut self, ui: &mut Ui, doc: &mut Document, meta: &ClassMetadata, icons: &mut Icons) -> Option<Request> {
        let selection = doc.selection();
        if selection != self.last_selection {
            let e = doc.runtime.engine.lock();
            self.reveal(&e.dm, &selection);
            drop(e);
            self.last_selection = selection.clone();
        }
        let rows = {
            let e = doc.runtime.engine.lock();
            self.rows(&e.dm, meta)
        };
        let full = ui.available_rect_before_wrap();
        ui.painter().rect_filled(full, 0.0, Color32::WHITE);
        let mut request = None;
        let mut toggle = None;
        let mut new_selection: Option<Vec<InstanceId>> = None;
        let font = FontId::proportional(theme::FONT_SIZE);
        let (mods, f2) = ui.input(|i| (i.modifiers, i.key_pressed(egui::Key::F2)));
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show_rows(ui, ROW, rows.len(), |ui, range| {
                let width = ui.available_width().max(200.0);
                for row in &rows[range] {
                    let (r, resp) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
                    let x0 = r.left() + 4.0 + row.depth as f32 * INDENT;
                    let arrow = Rect::from_min_size(pos2(x0, r.top()), vec2(14.0, ROW));
                    let icon = Rect::from_min_size(pos2(x0 + 15.0, r.center().y - 8.0), vec2(16.0, 16.0));
                    let tw = ui.fonts_mut(|f| f.layout_no_wrap(row.name.clone(), font.clone(), Color32::BLACK).size().x);
                    let label = Rect::from_min_max(pos2(icon.right() + 2.0, r.top()), pos2(icon.right() + tw + 8.0, r.bottom()));
                    let selected = selection.contains(&row.id);
                    let item = Rect::from_min_max(pos2(icon.left() - 1.0, r.top()), pos2(label.right(), r.bottom()));
                    if selected {
                        theme::selected_row(ui.painter(), item);
                    } else if resp.hovered() {
                        theme::hovered_row(ui.painter(), item);
                    }
                    if row.has_children {
                        let c = arrow.center();
                        let hover = ui.rect_contains_pointer(arrow);
                        if row.expanded {
                            let color = if hover { theme::ACCENT } else { Color32::from_rgb(38, 38, 38) };
                            ui.painter().add(egui::Shape::convex_polygon(
                                vec![c + vec2(2.5, -2.5), c + vec2(2.5, 2.5), c + vec2(-2.5, 2.5)],
                                color,
                                Stroke::NONE,
                            ));
                        } else {
                            let color = if hover { theme::ACCENT } else { Color32::from_rgb(166, 166, 166) };
                            ui.painter().add(egui::Shape::convex_polygon(
                                vec![c + vec2(-2.0, -4.0), c + vec2(2.5, 0.0), c + vec2(-2.0, 4.0)],
                                Color32::TRANSPARENT,
                                Stroke::new(1.0, color),
                            ));
                        }
                    }
                    if let Some((tex, uv)) = icons.class(ui.ctx(), meta.image(&row.class)) {
                        ui.painter().image(tex, icon, uv, Color32::WHITE);
                    }
                    let editing = matches!(&self.renaming, Some((id, _)) if *id == row.id);
                    if editing {
                        let mut done = false;
                        if let Some((_, buf)) = &mut self.renaming {
                            let edit_rect = Rect::from_min_size(label.min, vec2(label.width().max(140.0), ROW));
                            let te = ui.put(edit_rect, egui::TextEdit::singleline(buf).font(font.clone()).margin(vec2(2.0, 1.0)));
                            te.request_focus();
                            if te.lost_focus() {
                                done = true;
                            }
                        }
                        if done && let Some((id, buf)) = self.renaming.take() {
                            let cancelled = ui.input(|i| i.key_pressed(egui::Key::Escape));
                            if !cancelled && !buf.is_empty() {
                                doc.rename(id, &buf);
                            }
                        }
                    } else {
                        let color = if selected { Color32::WHITE } else { Color32::BLACK };
                        ui.painter().text(pos2(label.left() + 2.0, r.center().y), Align2::LEFT_CENTER, &row.name, font.clone(), color);
                    }
                    let resp = resp.on_hover_text(&row.class);
                    if resp.clicked() {
                        if row.has_children && ui.rect_contains_pointer(arrow) {
                            toggle = Some(row.id);
                        } else if mods.command || mods.ctrl {
                            let mut s = selection.clone();
                            if let Some(i) = s.iter().position(|x| *x == row.id) {
                                s.remove(i);
                            } else {
                                s.push(row.id);
                            }
                            new_selection = Some(s);
                            self.anchor = Some(row.id);
                        } else if mods.shift {
                            let ids: Vec<InstanceId> = rows.iter().map(|r| r.id).collect();
                            let a = self.anchor.and_then(|a| ids.iter().position(|x| *x == a));
                            let b = ids.iter().position(|x| *x == row.id);
                            if let (Some(a), Some(b)) = (a, b) {
                                let (lo, hi) = (a.min(b), a.max(b));
                                new_selection = Some(ids[lo..=hi].to_vec());
                            }
                        } else {
                            new_selection = Some(vec![row.id]);
                            self.anchor = Some(row.id);
                        }
                    }
                    if resp.double_clicked() {
                        if doc.script_source(row.id).is_some() {
                            request = Some(Request::OpenScript(row.id));
                        } else if row.has_children {
                            toggle = Some(row.id);
                        }
                    }
                    if resp.secondary_clicked() {
                        if !selected {
                            new_selection = Some(vec![row.id]);
                        }
                        request = Some(Request::Context(row.id));
                    }
                }
            });
        if let Some(t) = toggle
            && !self.expanded.remove(&t)
        {
            self.expanded.insert(t);
        }
        if let Some(s) = new_selection {
            self.last_selection = s.clone();
            doc.select(s);
        }
        if f2 && !ui.ctx().egui_wants_keyboard_input() && let Some(&id) = doc.selection().last() {
            let name = doc.runtime.engine.lock().dm.name_of(id).to_string();
            self.renaming = Some((id, name));
        }
        request
    }

    pub fn start_rename(&mut self, doc: &Document, id: InstanceId) {
        let name = doc.runtime.engine.lock().dm.name_of(id).to_string();
        self.renaming = Some((id, name));
    }
}
