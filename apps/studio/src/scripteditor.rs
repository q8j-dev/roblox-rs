use crate::app::Studio;
use crate::theme;
use egui::text::{CCursor, CCursorRange};
use egui::{Align2, Color32, FontId, Sense, Stroke, Ui, vec2};
use rbx_dom::InstanceId;
use rbx_studio_ui::panels::luau_layout;

#[derive(Default)]
pub struct EditorState {
    pub find: String,
    pub replace: String,
    pub show_find: bool,
    pub show_replace: bool,
    pub goto: Option<String>,
    pub pending: Option<(usize, usize)>,
}

pub fn edit_id(id: InstanceId) -> egui::Id {
    egui::Id::unique(("studio-script-editor", id.raw()))
}

fn char_to_byte(s: &str, c: usize) -> usize {
    s.char_indices().nth(c).map(|(b, _)| b).unwrap_or(s.len())
}

fn byte_to_char(s: &str, b: usize) -> usize {
    s[..b.min(s.len())].chars().count()
}

fn line_start(s: &str, line: usize) -> usize {
    if line <= 1 {
        return 0;
    }
    s.match_indices('\n')
        .nth(line - 2)
        .map(|(i, _)| i + 1)
        .unwrap_or(s.len())
}

impl Studio {
    fn editor_selection(&self, ctx: &egui::Context, id: InstanceId) -> (usize, usize) {
        egui::TextEdit::load_state(ctx, edit_id(id))
            .and_then(|s| s.cursor.char_range())
            .map(|r| {
                let [a, b] = r.sorted_cursors();
                (a.index.0, b.index.0)
            })
            .unwrap_or((0, 0))
    }

    pub fn active_script(&self) -> Option<InstanceId> {
        self.active_doc
    }

    fn buffer_mut(&mut self, id: InstanceId) -> Option<&mut String> {
        self.scripts
            .iter_mut()
            .find(|(s, _)| *s == id)
            .map(|(_, b)| b)
    }

    pub fn script_command(&mut self, ctx: &egui::Context, action: &str) -> bool {
        let Some(id) = self
            .active_script()
            .or_else(|| self.scripts.first().map(|s| s.0))
        else {
            return matches!(action, "findAction" | "replaceAction" | "goToLineAction");
        };
        let (sel_a, sel_b) = self.editor_selection(ctx, id);
        let state = self.editors.entry(id).or_default();
        match action {
            "findAction" | "actionFindMenu" => {
                state.show_find = true;
                state.show_replace = false;
            }
            "replaceAction" => {
                state.show_find = true;
                state.show_replace = true;
            }
            "goToLineAction" => state.goto = Some(String::new()),
            "findNextAction" | "findPreviousAction" => {
                let needle = state.find.clone();
                let forward = action == "findNextAction";
                if let Some(buf) = self.buffer_mut(id)
                    && !needle.is_empty()
                {
                    let lower = buf.to_lowercase();
                    let needle = needle.to_lowercase();
                    let from = char_to_byte(buf, if forward { sel_b } else { sel_a });
                    let found = if forward {
                        lower[from..]
                            .find(&needle)
                            .map(|i| i + from)
                            .or_else(|| lower.find(&needle))
                    } else {
                        lower[..from]
                            .rfind(&needle)
                            .or_else(|| lower.rfind(&needle))
                    };
                    if let Some(b) = found {
                        let a = byte_to_char(buf, b);
                        let n = needle.chars().count();
                        self.editors.entry(id).or_default().pending = Some((a, a + n));
                    }
                }
            }
            "findInScriptsAction" => {
                let q = state.find.clone();
                self.find_in_scripts(&q);
                if self.dock.find_tab(&crate::app::Tab::FindResults).is_none() {
                    self.run_action("viewFindResultsWindowAction");
                }
            }
            "selectAllAction" => {
                let n = self.buffer_mut(id).map(|b| b.chars().count()).unwrap_or(0);
                self.editors.entry(id).or_default().pending = Some((0, n));
            }
            "selectChildrenAction" => {
                let sel = self.doc.selection();
                let kids: Vec<InstanceId> = {
                    let e = self.doc.runtime.engine.lock();
                    sel.iter()
                        .flat_map(|s| e.dm.children_of(*s).to_vec())
                        .collect()
                };
                self.doc.select(kids);
            }
            "commentSelectionAction" | "uncommentSelectionAction" | "toggleCommentAction" => {
                let Some(buf) = self.buffer_mut(id) else {
                    return true;
                };
                let a = char_to_byte(buf, sel_a);
                let b = char_to_byte(buf, sel_b);
                let start = buf[..a].rfind('\n').map(|i| i + 1).unwrap_or(0);
                let end = buf[b..].find('\n').map(|i| i + b).unwrap_or(buf.len());
                let lines: Vec<&str> = buf[start..end].split('\n').collect();
                let all_commented = lines
                    .iter()
                    .all(|l| l.trim_start().starts_with("--") || l.trim().is_empty());
                let comment = match action {
                    "commentSelectionAction" => true,
                    "uncommentSelectionAction" => false,
                    _ => !all_commented,
                };
                let changed: Vec<String> = lines
                    .iter()
                    .map(|l| {
                        if comment {
                            format!("--{l}")
                        } else if let Some(i) = l.find("--").filter(|i| l[..*i].trim().is_empty()) {
                            format!("{}{}", &l[..i], &l[i + 2..])
                        } else {
                            l.to_string()
                        }
                    })
                    .collect();
                let new_text = format!("{}{}{}", &buf[..start], changed.join("\n"), &buf[end..]);
                *buf = new_text.clone();
                self.doc.checkpoint();
                self.doc.write_script_source(id, &new_text);
            }
            "reloadScriptAction" => {
                let src = self.doc.script_source(id).unwrap_or_default();
                if let Some(buf) = self.buffer_mut(id) {
                    *buf = src;
                }
            }
            "goToScriptErrorAction" => {
                self.refresh_analysis();
                let line = self
                    .views
                    .analysis
                    .iter()
                    .find(|(s, _, _)| *s == id)
                    .and_then(|(_, _, m)| m.first().map(|(l, _)| *l));
                if let (Some(line), Some(buf)) = (line, self.buffer_mut(id)) {
                    let c = byte_to_char(buf, line_start(buf, line));
                    self.editors.entry(id).or_default().pending = Some((c, c));
                }
            }
            "expandAllFoldsAction" | "collapseAllFoldsAction" => {}
            _ => return false,
        }
        true
    }

    pub fn script_editor_ui(&mut self, ui: &mut Ui, id: InstanceId) {
        let Some(i) = self.scripts.iter().position(|(sid, _)| *sid == id) else {
            return;
        };
        let mut buffer = std::mem::take(&mut self.scripts[i].1);
        let state = self.editors.entry(id).or_default();
        let full = ui.available_rect_before_wrap();
        ui.painter().rect_filled(full, 0.0, Color32::WHITE);
        let mut find_next = false;
        let mut find_prev = false;
        let mut replace_one = false;
        let mut replace_all = false;
        if state.show_find {
            let bar = egui::Rect::from_min_size(
                full.min,
                vec2(full.width(), if state.show_replace { 52.0 } else { 28.0 }),
            );
            ui.painter()
                .rect_filled(bar, 0.0, Color32::from_rgb(243, 243, 243));
            ui.painter().line_segment(
                [bar.left_bottom(), bar.right_bottom()],
                Stroke::new(1.0, Color32::from_rgb(196, 196, 196)),
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(bar.shrink(3.0)), |ui| {
                ui.horizontal(|ui| {
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut state.find)
                            .hint_text("Find...")
                            .desired_width(220.0),
                    );
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        find_next = true;
                    }
                    find_prev |= ui.button("Previous").clicked();
                    find_next |= ui.button("Next").clicked();
                    if ui.button("x").clicked() {
                        state.show_find = false;
                    }
                });
                if state.show_replace {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut state.replace)
                                .hint_text("Replace...")
                                .desired_width(220.0),
                        );
                        replace_one = ui.button("Replace").clicked();
                        replace_all = ui.button("Replace All").clicked();
                    });
                }
            });
            ui.add_space(bar.height());
        }
        if let Some(text) = &mut state.goto {
            let mut done = None;
            egui::Window::new("Go To Line")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
                .show(ui.ctx(), |ui| {
                    ui.label(format!(
                        "Line number (1 - {}):",
                        buffer.lines().count().max(1)
                    ));
                    let r = ui.add(egui::TextEdit::singleline(text).desired_width(200.0));
                    r.request_focus();
                    ui.horizontal(|ui| {
                        if ui.button("OK").clicked()
                            || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        {
                            done = Some(text.trim().parse::<usize>().ok());
                        }
                        if ui.button("Cancel").clicked() {
                            done = Some(None);
                        }
                    });
                });
            if let Some(result) = done {
                if let Some(line) = result {
                    let c = byte_to_char(&buffer, line_start(&buffer, line));
                    state.pending = Some((c, c));
                }
                state.goto = None;
            }
        }
        let pending = state.pending.take();
        let font = FontId::monospace(13.0);
        let row = ui.fonts_mut(|f| f.row_height(&font));
        let lines = buffer.lines().count().max(1) + usize::from(buffer.ends_with('\n'));
        let digits = lines.to_string().len().max(2) as f32;
        let char_w = ui.fonts_mut(|f| f.glyph_width(&font, '0'));
        let margin = digits * char_w + 26.0;
        let mut changed = false;
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    let height = (lines as f32 * row + 8.0).max(ui.available_height());
                    let (gutter, _) = ui.allocate_exact_size(vec2(margin, height), Sense::hover());
                    ui.painter()
                        .rect_filled(gutter, 0.0, Color32::from_rgb(221, 221, 221));
                    ui.painter().rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(gutter.right() - 12.0, gutter.top()),
                            gutter.right_bottom(),
                        ),
                        0.0,
                        Color32::from_rgb(249, 249, 249),
                    );
                    for n in 0..lines {
                        ui.painter().text(
                            egui::pos2(gutter.right() - 16.0, gutter.top() + 2.0 + n as f32 * row),
                            Align2::RIGHT_TOP,
                            (n + 1).to_string(),
                            font.clone(),
                            Color32::BLACK,
                        );
                    }
                    let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, wrap: f32| {
                        let mut job = luau_layout(text.as_str());
                        job.wrap.max_width = wrap;
                        ui.fonts_mut(|f| f.layout_job(job))
                    };
                    let output = egui::TextEdit::multiline(&mut buffer)
                        .id(edit_id(id))
                        .font(egui::TextStyle::Monospace)
                        .code_editor()
                        .frame(egui::Frame::new())
                        .margin(vec2(4.0, 2.0))
                        .desired_width(f32::INFINITY)
                        .min_size(vec2(0.0, height))
                        .layouter(&mut layouter)
                        .show(ui);
                    if output.response.gained_focus() {
                        self.doc.checkpoint();
                    }
                    changed = output.response.changed();
                    if let Some((a, b)) = pending {
                        let mut st = output.state.clone();
                        st.cursor.set_char_range(Some(CCursorRange::two(
                            CCursor::new(a),
                            CCursor::new(b),
                        )));
                        st.store(ui.ctx(), edit_id(id));
                        output.response.request_focus();
                        let y = output.galley_pos.y
                            + output.galley.pos_from_cursor(CCursor::new(a)).min.y;
                        ui.scroll_to_rect(
                            egui::Rect::from_min_size(egui::pos2(full.left(), y), vec2(1.0, row)),
                            Some(egui::Align::Center),
                        );
                    }
                });
            });
        if changed {
            self.doc.write_script_source(id, &buffer);
        }
        self.scripts[i].1 = buffer;
        if find_next {
            self.script_command(ui.ctx(), "findNextAction");
        }
        if find_prev {
            self.script_command(ui.ctx(), "findPreviousAction");
        }
        if replace_one || replace_all {
            let (find, replace) = {
                let st = self.editors.entry(id).or_default();
                (st.find.clone(), st.replace.clone())
            };
            if !find.is_empty()
                && let Some(i) = self.scripts.iter().position(|(sid, _)| *sid == id)
            {
                let buf = &mut self.scripts[i].1;
                let new_text = if replace_all {
                    buf.replace(&find, &replace)
                } else {
                    buf.replacen(&find, &replace, 1)
                };
                *buf = new_text.clone();
                self.doc.checkpoint();
                self.doc.write_script_source(id, &new_text);
            }
        }
        let _ = theme::TEXT;
    }
}
