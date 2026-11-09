use crate::icons::Icons;
use crate::theme;
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};

pub enum Entry {
    Item {
        id: String,
        text: String,
        shortcut: String,
        icon: Option<String>,
        class_icon: Option<usize>,
        enabled: bool,
        checked: bool,
    },
    Sub {
        text: String,
        items: Vec<Entry>,
    },
    Separator,
}

pub fn item(id: &str, text: &str, shortcut: &str, icon: Option<&str>, enabled: bool) -> Entry {
    Entry::Item {
        id: id.into(),
        text: text.into(),
        shortcut: shortcut.into(),
        icon: icon.map(str::to_string),
        class_icon: None,
        enabled,
        checked: false,
    }
}

pub fn show(
    ctx: &egui::Context,
    id: egui::Id,
    at: Pos2,
    entries: &[Entry],
    icons: &mut Icons,
) -> (Option<String>, bool) {
    let mut chosen = None;
    let area = egui::Area::new(id)
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .show(ctx, |ui| frame(ui, entries, icons, &mut chosen, id));
    let outside =
        ctx.input(|i| i.pointer.any_pressed()) && !area.response.contains_pointer() && !area.inner;
    (chosen, outside)
}

fn frame(
    ui: &mut Ui,
    entries: &[Entry],
    icons: &mut Icons,
    chosen: &mut Option<String>,
    id: egui::Id,
) -> bool {
    let mut sub_hovered = false;
    egui::Frame::new()
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1.0, theme::RIBBON_SEPARATOR))
        .inner_margin(2.0)
        .shadow(egui::epaint::Shadow {
            offset: [2, 2],
            blur: 4,
            spread: 0,
            color: Color32::from_black_alpha(40),
        })
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            let font = FontId::proportional(theme::FONT_SIZE);
            let width = entries
                .iter()
                .map(|e| match e {
                    Entry::Item { text, shortcut, .. } => {
                        ui.fonts_mut(|f| {
                            f.layout_no_wrap(text.clone(), font.clone(), Color32::BLACK)
                                .size()
                                .x
                        }) + ui.fonts_mut(|f| {
                            f.layout_no_wrap(shortcut.clone(), font.clone(), Color32::BLACK)
                                .size()
                                .x
                        }) + 80.0
                    }
                    Entry::Sub { text, .. } => {
                        ui.fonts_mut(|f| {
                            f.layout_no_wrap(text.clone(), font.clone(), Color32::BLACK)
                                .size()
                                .x
                        }) + 70.0
                    }
                    Entry::Separator => 0.0,
                })
                .fold(170.0f32, f32::max);
            for (n, e) in entries.iter().enumerate() {
                match e {
                    Entry::Separator => {
                        let (r, _) = ui.allocate_exact_size(vec2(width, 7.0), Sense::hover());
                        ui.painter().line_segment(
                            [
                                pos2(r.left() + 28.0, r.center().y),
                                pos2(r.right() - 2.0, r.center().y),
                            ],
                            Stroke::new(1.0, theme::RIBBON_SEPARATOR),
                        );
                    }
                    Entry::Item {
                        id: aid,
                        text,
                        shortcut,
                        icon,
                        class_icon,
                        enabled,
                        checked,
                    } => {
                        let (r, resp) = ui.allocate_exact_size(vec2(width, 22.0), Sense::click());
                        if resp.hovered() && *enabled {
                            ui.painter().rect_filled(r, 0.0, theme::HIGHLIGHT);
                            ui.painter().rect_stroke(
                                r,
                                0.0,
                                Stroke::new(1.0, theme::ACCENT_HILIGHT),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if *checked {
                            ui.painter().rect_filled(
                                Rect::from_min_size(r.min + vec2(2.0, 2.0), vec2(18.0, 18.0)),
                                0.0,
                                theme::HIGHLIGHT_DN,
                            );
                        }
                        let ir = Rect::from_min_size(
                            pos2(r.left() + 4.0, r.center().y - 8.0),
                            vec2(16.0, 16.0),
                        );
                        if let Some(i) = icon
                            && let Some(tex) = if *enabled {
                                icons.studio(ui.ctx(), i)
                            } else {
                                icons.studio_disabled(ui.ctx(), i)
                            }
                        {
                            ui.painter().image(
                                tex.id(),
                                ir,
                                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                                Color32::WHITE,
                            );
                        }
                        if let Some(ci) = class_icon
                            && let Some((tex, uv)) = icons.class(ui.ctx(), *ci)
                        {
                            ui.painter().image(tex, ir, uv, Color32::WHITE);
                        }
                        let color = if *enabled {
                            Color32::BLACK
                        } else {
                            theme::DISABLED_TEXT
                        };
                        ui.painter().text(
                            pos2(r.left() + 28.0, r.center().y),
                            Align2::LEFT_CENTER,
                            text,
                            font.clone(),
                            color,
                        );
                        ui.painter().text(
                            pos2(r.right() - 10.0, r.center().y),
                            Align2::RIGHT_CENTER,
                            shortcut,
                            font.clone(),
                            color,
                        );
                        if resp.clicked() && *enabled {
                            *chosen = Some(aid.clone());
                        }
                    }
                    Entry::Sub { text, items } => {
                        let (r, resp) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
                        let open_id = id.with(("sub", n));
                        let open = ui.data(|d| d.get_temp::<bool>(open_id).unwrap_or(false));
                        if resp.hovered() {
                            ui.data_mut(|d| d.insert_temp(open_id, true));
                        }
                        if resp.hovered() || open {
                            ui.painter().rect_filled(r, 0.0, theme::HIGHLIGHT);
                            ui.painter().rect_stroke(
                                r,
                                0.0,
                                Stroke::new(1.0, theme::ACCENT_HILIGHT),
                                egui::StrokeKind::Inside,
                            );
                        }
                        ui.painter().text(
                            pos2(r.left() + 28.0, r.center().y),
                            Align2::LEFT_CENTER,
                            text,
                            font.clone(),
                            Color32::BLACK,
                        );
                        let c = pos2(r.right() - 10.0, r.center().y);
                        ui.painter().add(egui::Shape::convex_polygon(
                            vec![
                                c + vec2(-2.0, -4.0),
                                c + vec2(2.0, 0.0),
                                c + vec2(-2.0, 4.0),
                            ],
                            Color32::BLACK,
                            Stroke::NONE,
                        ));
                        if resp.hovered() || open {
                            let inner = egui::Area::new(open_id.with("area"))
                                .order(egui::Order::Tooltip)
                                .fixed_pos(r.right_top())
                                .show(ui.ctx(), |ui| {
                                    egui::ScrollArea::vertical()
                                        .max_height(500.0)
                                        .show(ui, |ui| frame(ui, items, icons, chosen, open_id))
                                });
                            let keep = resp.hovered() || inner.response.contains_pointer();
                            sub_hovered |= inner.response.contains_pointer();
                            if !keep {
                                ui.data_mut(|d| d.insert_temp(open_id, false));
                            }
                        }
                    }
                }
            }
        });
    sub_hovered
}
