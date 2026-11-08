use crate::icons::Icons;
use crate::theme;
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct ActionInfo {
    pub text: Option<String>,
    pub tooltip: Option<String>,
    pub icon: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Event {
    Action(String),
    Combo,
    Check,
    Color(u16),
}

pub struct Ribbon {
    layout: Value,
    actions: HashMap<String, ActionInfo>,
    pub page: usize,
    open_menu: Option<(String, Rect)>,
    pub plugin_groups: Vec<(String, Vec<PluginItem>)>,
    combos: HashMap<String, usize>,
}

#[derive(Clone, Debug)]
pub struct PluginItem {
    pub id: u64,
    pub text: String,
    pub tooltip: String,
    pub icon: Option<std::path::PathBuf>,
    pub active: bool,
}

pub trait State {
    fn checked(&self, action: &str) -> bool;
    fn enabled(&self, action: &str) -> bool;
}

const TAB_ROW: f32 = 24.0;
const QAT_ROW: f32 = 24.0;
const BODY: f32 = 94.0;
const CAPTION: f32 = 18.0;
const LARGE_H: f32 = 70.0;
const SMALL_H: f32 = 22.0;

fn attr<'a>(n: &'a Value, k: &str) -> Option<&'a str> {
    n.get(k).and_then(Value::as_str)
}

fn children(n: &Value) -> &[Value] {
    n.get("children")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

enum Item<'a> {
    Large(&'a Value),
    Small(&'a Value),
    Break,
    Plugin(&'a PluginItem),
}

impl Ribbon {
    pub fn load(dir: &Path) -> Ribbon {
        let layout = std::fs::read_to_string(dir.join("ribbon.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        let mut actions = HashMap::new();
        if let Some(Value::Object(map)) = std::fs::read_to_string(dir.join("actions.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        {
            for (k, v) in map {
                actions.insert(
                    k,
                    ActionInfo {
                        text: attr(&v, "text").map(str::to_string),
                        tooltip: attr(&v, "tooltip").map(str::to_string),
                        icon: attr(&v, "icon").map(str::to_string),
                    },
                );
            }
        }
        Ribbon {
            layout,
            actions,
            page: 0,
            open_menu: None,
            plugin_groups: Vec::new(),
            combos: HashMap::new(),
        }
    }

    pub fn empty() -> Ribbon {
        Ribbon {
            layout: Value::Null,
            actions: HashMap::new(),
            page: 0,
            open_menu: None,
            plugin_groups: Vec::new(),
            combos: HashMap::new(),
        }
    }

    pub fn height() -> f32 {
        QAT_ROW + TAB_ROW + BODY
    }

    fn section(&self, tag: &str) -> Option<&Value> {
        children(&self.layout).iter().find(|c| attr(c, "tag") == Some(tag))
    }

    fn pages(&self) -> Vec<&Value> {
        self.section("RibbonBar")
            .map(|r| {
                children(r)
                    .iter()
                    .filter(|p| attr(p, "tag") == Some("TabPage") && attr(p, "name") != Some("scriptMenu"))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn text_of(&self, n: &Value) -> String {
        if let Some(t) = attr(n, "text") {
            return t.trim().to_string();
        }
        let name = attr(n, "name").unwrap_or("");
        if let Some(t) = self.actions.get(name).and_then(|a| a.text.clone()) {
            return t;
        }
        if attr(n, "isProxy").is_some() {
            return name.split('.').next().unwrap_or(name).to_string();
        }
        String::new()
    }

    fn tooltip_of(&self, n: &Value) -> Option<String> {
        attr(n, "tooltip")
            .map(str::to_string)
            .or_else(|| self.actions.get(attr(n, "name")?).and_then(|a| a.tooltip.clone()))
            .filter(|s| !s.is_empty())
    }

    fn icon_of(&self, n: &Value) -> Option<String> {
        attr(n, "icon")
            .map(str::to_string)
            .or_else(|| self.actions.get(attr(n, "name")?).and_then(|a| a.icon.clone()))
    }

    pub fn show(&mut self, ui: &mut Ui, icons: &mut Icons, state: &dyn State) -> Vec<Event> {
        let mut events = Vec::new();
        let full = ui.available_rect_before_wrap();
        let rect = Rect::from_min_size(full.min, vec2(full.width(), Ribbon::height()));
        ui.allocate_rect(rect, Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme::RIBBON_BACK);

        let qat = Rect::from_min_size(rect.min, vec2(rect.width(), QAT_ROW));
        self.quick_access(ui, icons, state, qat, &mut events);

        let tabs = Rect::from_min_size(pos2(rect.left(), qat.bottom()), vec2(rect.width(), TAB_ROW));
        let body = Rect::from_min_max(pos2(rect.left(), tabs.bottom()), rect.max);
        let active_tab = self.tabs(ui, tabs, &mut events);

        painter.rect_filled(body, 0.0, theme::RIBBON_FACE);
        let sep = Stroke::new(1.0, theme::RIBBON_SEPARATOR);
        painter.line_segment([pos2(body.left(), body.top()), pos2(active_tab.left(), body.top())], sep);
        painter.line_segment([pos2(active_tab.right(), body.top()), pos2(body.right(), body.top())], sep);
        painter.line_segment([pos2(body.left(), body.bottom() - 0.5), pos2(body.right(), body.bottom() - 0.5)], sep);

        let pages = self.pages();
        let page = self.page.min(pages.len().saturating_sub(1));
        let groups: Vec<Value> = pages
            .get(page)
            .map(|p| children(p).to_vec())
            .unwrap_or_default();
        let is_plugins = pages.get(page).and_then(|p| attr(p, "name")) == Some("Plugins");
        let mut x = body.left() + 2.0;
        for g in &groups {
            if attr(g, "tag") != Some("Group") {
                continue;
            }
            let items = self.flatten(g);
            x = self.group(ui, icons, state, g, items, x, body, &mut events);
        }
        if is_plugins {
            let groups = self.plugin_groups.clone();
            for (name, items) in &groups {
                let flat: Vec<Item> = items.iter().map(Item::Plugin).collect();
                let header = serde_json::json!({ "name": name });
                x = self.group(ui, icons, state, &header, flat, x, body, &mut events);
            }
        }
        self.menu_popup(ui, icons, state, &mut events);
        events
    }

    fn quick_access(&mut self, ui: &mut Ui, icons: &mut Icons, state: &dyn State, row: Rect, events: &mut Vec<Event>) {
        let Some(qab) = self.section("QuickAccessBar").cloned() else {
            return;
        };
        let mut x = row.left() + 6.0;
        for a in children(&qab) {
            if attr(a, "visible") != Some("true") {
                continue;
            }
            let r = Rect::from_min_size(pos2(x, row.top() + 2.0), vec2(22.0, 20.0));
            let name = attr(a, "name").unwrap_or("").to_string();
            let resp = ui.interact(r, ui.scope_id().with(("qat", &name)), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r, 0.0, theme::HIGHLIGHT);
            }
            if let Some(tex) = self.icon_of(a).and_then(|i| icons.studio(ui.ctx(), &i)) {
                let tint = if state.enabled(&name) { Color32::WHITE } else { Color32::from_white_alpha(90) };
                ui.painter().image(
                    tex.id(),
                    Rect::from_center_size(r.center(), vec2(16.0, 16.0)),
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    tint,
                );
            }
            let resp = match self.tooltip_of(a).or_else(|| Some(self.text_of(a))) {
                Some(t) if !t.is_empty() => resp.on_hover_text(t),
                _ => resp,
            };
            if resp.clicked() {
                events.push(Event::Action(name));
            }
            x += 23.0;
        }
    }

    fn tabs(&mut self, ui: &mut Ui, row: Rect, events: &mut Vec<Event>) -> Rect {
        let font = FontId::proportional(theme::FONT_SIZE);
        let file = Rect::from_min_size(pos2(row.left(), row.top()), vec2(58.0, row.height()));
        let fr = ui.interact(file, ui.scope_id().with("file-button"), Sense::click());
        ui.painter().rect_filled(
            file,
            0.0,
            if fr.hovered() { theme::ACCENT_HILIGHT } else { theme::ACCENT },
        );
        ui.painter().text(file.center(), Align2::CENTER_CENTER, "FILE", font.clone(), Color32::WHITE);
        if fr.clicked() {
            let open = matches!(&self.open_menu, Some((n, _)) if n == "#file");
            self.open_menu = if open { None } else { Some(("#file".into(), file)) };
        }
        let mut x = file.right() + 2.0;
        let mut active = Rect::NOTHING;
        let names: Vec<String> = self
            .pages()
            .iter()
            .map(|p| attr(p, "text").or(attr(p, "name")).unwrap_or("").to_uppercase())
            .collect();
        for (i, name) in names.iter().enumerate() {
            let w = ui.fonts_mut(|f| f.layout_no_wrap(name.clone(), font.clone(), Color32::BLACK).size().x) + 22.0;
            let r = Rect::from_min_size(pos2(x, row.top() + 1.0), vec2(w, row.height() - 1.0));
            let resp = ui.interact(r, ui.scope_id().with(("ribbon-tab", i)), Sense::click());
            let selected = i == self.page;
            if selected {
                ui.painter().rect_filled(r.expand2(vec2(0.0, 0.5)), 0.0, theme::RIBBON_FACE);
                let s = Stroke::new(1.0, theme::RIBBON_SEPARATOR);
                ui.painter().line_segment([r.left_bottom(), r.left_top()], s);
                ui.painter().line_segment([r.left_top(), r.right_top()], s);
                ui.painter().line_segment([r.right_top(), r.right_bottom()], s);
                active = r;
            }
            let color = if selected || resp.hovered() { theme::ACCENT } else { theme::TEXT };
            ui.painter().text(r.center(), Align2::CENTER_CENTER, name, font.clone(), color);
            if resp.clicked() {
                self.page = i;
                self.open_menu = None;
            }
            x += w;
        }
        let _ = events;
        active
    }

    fn flatten<'a>(&self, g: &'a Value) -> Vec<Item<'a>> {
        let mut out = Vec::new();
        for c in children(g) {
            match attr(c, "tag") {
                Some("addseparator") => out.push(Item::Break),
                Some("groupoptionsmenu") => {}
                Some("ActionGroup") => {
                    let large = attr(c, "style") == Some("textundericon");
                    for a in children(c) {
                        out.push(if large { Item::Large(a) } else { Item::Small(a) });
                    }
                }
                Some("addcheckbox") | Some("addcombobox") => out.push(Item::Small(c)),
                Some(_) => {
                    let large = attr(c, "style") == Some("textundericon");
                    out.push(if large { Item::Large(c) } else { Item::Small(c) });
                }
                None => {}
            }
        }
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn group(
        &mut self,
        ui: &mut Ui,
        icons: &mut Icons,
        state: &dyn State,
        g: &Value,
        items: Vec<Item>,
        start: f32,
        body: Rect,
        events: &mut Vec<Event>,
    ) -> f32 {
        let font = FontId::proportional(theme::FONT_SIZE);
        let top = body.top() + 3.0;
        let mut x = start + 3.0;
        let mut col: Vec<&Value> = Vec::new();
        let flush = |this: &mut Ribbon, ui: &mut Ui, icons: &mut Icons, col: &mut Vec<&Value>, x: &mut f32, events: &mut Vec<Event>| {
            if col.is_empty() {
                return;
            }
            let width = col
                .iter()
                .map(|n| this.small_width(ui, n))
                .fold(0.0f32, f32::max);
            for (row, n) in col.iter().enumerate() {
                let r = Rect::from_min_size(pos2(*x, top + 1.0 + row as f32 * SMALL_H), vec2(width, SMALL_H));
                this.small(ui, icons, state, n, r, events);
            }
            *x += width + 2.0;
            col.clear();
        };
        for item in &items {
            match item {
                Item::Break => flush(self, ui, icons, &mut col, &mut x, events),
                Item::Small(n) => {
                    col.push(n);
                    if col.len() == 3 {
                        flush(self, ui, icons, &mut col, &mut x, events);
                    }
                }
                Item::Large(n) => {
                    flush(self, ui, icons, &mut col, &mut x, events);
                    let text = self.text_of(n);
                    let tw = ui.fonts_mut(|f| f.layout_no_wrap(text.clone(), font.clone(), Color32::BLACK).size().x);
                    let w = (tw + 10.0).max(42.0);
                    let r = Rect::from_min_size(pos2(x, top), vec2(w, LARGE_H));
                    self.large(ui, icons, state, n, &text, r, events);
                    x += w + 1.0;
                }
                Item::Plugin(p) => {
                    flush(self, ui, icons, &mut col, &mut x, events);
                    let has_icon = p.icon.as_ref().is_some_and(|i| i.is_file());
                    let text = if p.text.is_empty() && !has_icon { p.tooltip.clone() } else { p.text.clone() };
                    let tw = ui.fonts_mut(|f| f.layout_no_wrap(text.clone(), font.clone(), Color32::BLACK).size().x);
                    let w = (tw + 10.0).max(42.0);
                    let r = Rect::from_min_size(pos2(x, top), vec2(w, LARGE_H));
                    let resp = ui.interact(r, ui.scope_id().with(("plugin-btn", p.id)), Sense::click());
                    paint_button(ui, r, resp.hovered(), resp.is_pointer_button_down_on(), p.active);
                    if let Some(tex) = p.icon.as_ref().and_then(|i| icons.get(ui.ctx(), i)) {
                        let ir = Rect::from_min_size(pos2(r.center().x - 16.0, r.top() + 4.0), vec2(32.0, 32.0));
                        ui.painter().image(tex.id(), ir, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    ui.painter().text(
                        pos2(r.center().x, r.top() + 40.0),
                        Align2::CENTER_TOP,
                        &text,
                        font.clone(),
                        theme::TEXT,
                    );
                    let resp = if p.tooltip.is_empty() { resp } else { resp.on_hover_text(&p.tooltip) };
                    if resp.clicked() {
                        events.push(Event::Action(format!("#plugin:{}", p.id)));
                    }
                    x += w + 1.0;
                }
            }
        }
        flush(self, ui, icons, &mut col, &mut x, events);
        let name = attr(g, "name").unwrap_or("");
        let cw = ui.fonts_mut(|f| f.layout_no_wrap(name.to_string(), font.clone(), Color32::BLACK).size().x);
        let end = x.max(start + cw + 16.0) + 3.0;
        let caption = Rect::from_min_max(pos2(start, body.bottom() - CAPTION - 1.0), pos2(end, body.bottom() - 1.0));
        ui.painter().text(caption.center(), Align2::CENTER_CENTER, name, font, theme::DISABLED_TEXT);
        ui.painter().line_segment(
            [pos2(end, body.top() + 4.0), pos2(end, body.bottom() - 4.0)],
            Stroke::new(1.0, theme::RIBBON_SEPARATOR),
        );
        end + 1.0
    }

    fn small_width(&self, ui: &mut Ui, n: &Value) -> f32 {
        let font = FontId::proportional(theme::FONT_SIZE);
        let text = match attr(n, "tag") {
            Some("addcombobox") => "8 Players".to_string(),
            _ => self.text_of(n),
        };
        let tw = ui.fonts_mut(|f| f.layout_no_wrap(text, font, Color32::BLACK).size().x);
        let arrow = if matches!(attr(n, "tag"), Some("MenuButton" | "MenuPopupButton")) { 12.0 } else { 0.0 };
        tw + 30.0 + arrow
    }

    fn is_menu(n: &Value) -> bool {
        matches!(attr(n, "tag"), Some("MenuButton" | "MenuPopupButton" | "ColorPicker"))
    }

    #[allow(clippy::too_many_arguments)]
    fn large(&mut self, ui: &mut Ui, icons: &mut Icons, state: &dyn State, n: &Value, text: &str, r: Rect, events: &mut Vec<Event>) {
        let name = attr(n, "name").unwrap_or("").to_string();
        let resp = ui.interact(r, ui.scope_id().with(("large", &name, r.left() as i32)), Sense::click());
        let enabled = state.enabled(&name);
        let open = matches!(&self.open_menu, Some((m, _)) if *m == name);
        paint_button(ui, r, resp.hovered() && enabled, resp.is_pointer_button_down_on() || open, state.checked(&name));
        if let Some(tex) = self.icon_of(n).and_then(|i| icons.studio(ui.ctx(), &i)) {
            let ir = Rect::from_min_size(pos2(r.center().x - 16.0, r.top() + 4.0), vec2(32.0, 32.0));
            let tint = if enabled { Color32::WHITE } else { Color32::from_white_alpha(90) };
            ui.painter().image(tex.id(), ir, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
        }
        let color = if enabled { theme::TEXT } else { theme::DISABLED_TEXT };
        let font = FontId::proportional(theme::FONT_SIZE);
        ui.painter().text(pos2(r.center().x, r.top() + 39.0), Align2::CENTER_TOP, text, font, color);
        if Ribbon::is_menu(n) {
            arrow(ui, pos2(r.center().x, r.top() + 60.0), color);
        }
        let resp = match self.tooltip_of(n) {
            Some(t) => resp.on_hover_text(t),
            None => resp,
        };
        if resp.clicked() && enabled {
            self.activate(n, r, events);
        }
    }

    fn small(&mut self, ui: &mut Ui, icons: &mut Icons, state: &dyn State, n: &Value, r: Rect, events: &mut Vec<Event>) {
        let name = attr(n, "name").unwrap_or("").to_string();
        let font = FontId::proportional(theme::FONT_SIZE);
        match attr(n, "tag") {
            Some("addcheckbox") => {
                let resp = ui.interact(r, ui.scope_id().with(("cb", &name)), Sense::click());
                let on = state.checked(&name);
                let b = Rect::from_min_size(pos2(r.left() + 3.0, r.center().y - 6.0), vec2(12.0, 12.0));
                ui.painter().rect_filled(b, 0.0, Color32::WHITE);
                ui.painter().rect_stroke(b, 0.0, Stroke::new(1.0, theme::EDIT_BORDER), egui::StrokeKind::Inside);
                if on {
                    ui.painter().line_segment([b.left_center() + vec2(2.0, 0.0), b.center_bottom() - vec2(1.0, 3.0)], Stroke::new(1.5, theme::TEXT));
                    ui.painter().line_segment([b.center_bottom() - vec2(1.0, 3.0), b.right_top() + vec2(-2.0, 3.0)], Stroke::new(1.5, theme::TEXT));
                }
                ui.painter().text(pos2(b.right() + 5.0, r.center().y), Align2::LEFT_CENTER, self.text_of(n), font, theme::TEXT);
                if resp.clicked() {
                    let _ = name;
                    events.push(Event::Check);
                }
                return;
            }
            Some("addcombobox") => {
                let items: Vec<String> = children(n).iter().map(|c| self.text_of(c)).collect();
                let cur = *self.combos.get(&name).unwrap_or(&0);
                let mut sel = cur;
                let b = r.shrink2(vec2(2.0, 2.0));
                ui.scope_builder(egui::UiBuilder::new().max_rect(b), |ui| {
                    egui::ComboBox::from_id_salt(("ribbon-combo", &name))
                        .width(b.width() - 6.0)
                        .selected_text(items.get(cur).cloned().unwrap_or_default())
                        .show_ui(ui, |ui| {
                            for (i, t) in items.iter().enumerate() {
                                ui.selectable_value(&mut sel, i, t);
                            }
                        });
                });
                if sel != cur {
                    self.combos.insert(name.clone(), sel);
                    events.push(Event::Combo);
                }
                return;
            }
            _ => {}
        }
        let resp = ui.interact(r, ui.scope_id().with(("small", &name, r.left() as i32, r.top() as i32)), Sense::click());
        let enabled = state.enabled(&name);
        let open = matches!(&self.open_menu, Some((m, _)) if *m == name);
        paint_button(ui, r, resp.hovered() && enabled, resp.is_pointer_button_down_on() || open, state.checked(&name));
        let color = if enabled { theme::TEXT } else { theme::DISABLED_TEXT };
        if let Some(tex) = self.icon_of(n).and_then(|i| icons.studio(ui.ctx(), &i)) {
            let ir = Rect::from_min_size(pos2(r.left() + 3.0, r.center().y - 8.0), vec2(16.0, 16.0));
            let tint = if enabled { Color32::WHITE } else { Color32::from_white_alpha(90) };
            ui.painter().image(tex.id(), ir, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
        }
        let label = self.text_of(n);
        ui.painter().text(pos2(r.left() + 23.0, r.center().y), Align2::LEFT_CENTER, &label, font, color);
        if Ribbon::is_menu(n) {
            arrow(ui, pos2(r.right() - 7.0, r.center().y), color);
        }
        let resp = match self.tooltip_of(n) {
            Some(t) => resp.on_hover_text(t),
            None => resp,
        };
        if resp.clicked() && enabled {
            self.activate(n, r, events);
        }
    }

    fn activate(&mut self, n: &Value, r: Rect, events: &mut Vec<Event>) {
        let name = attr(n, "name").unwrap_or("").to_string();
        if Ribbon::is_menu(n) {
            let open = matches!(&self.open_menu, Some((m, _)) if *m == name);
            self.open_menu = if open { None } else { Some((name, r)) };
        } else {
            self.open_menu = None;
            events.push(Event::Action(name));
        }
    }

    fn find_menu(&self, name: &str) -> Option<Value> {
        fn walk(n: &Value, name: &str) -> Option<Value> {
            if matches!(attr(n, "tag"), Some("MenuButton" | "MenuPopupButton" | "ColorPicker")) && attr(n, "name") == Some(name) {
                return Some(n.clone());
            }
            children(n).iter().find_map(|c| walk(c, name))
        }
        walk(&self.layout, name)
    }

    fn menu_popup(&mut self, ui: &mut Ui, icons: &mut Icons, state: &dyn State, events: &mut Vec<Event>) {
        let Some((name, anchor)) = self.open_menu.clone() else {
            return;
        };
        let at: Pos2 = anchor.left_bottom() + vec2(0.0, 1.0);
        let mut close = false;
        let area = egui::Area::new(ui.scope_id().with(("ribbon-menu", &name)))
            .order(egui::Order::Foreground)
            .fixed_pos(at)
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .stroke(Stroke::new(1.0, theme::RIBBON_SEPARATOR))
                    .inner_margin(2.0)
                    .show(ui, |ui| {
                        if name == "#file" {
                            if let Some(sb) = self.section("SystemButton").cloned() {
                                let popup = children(&sb).first().cloned().unwrap_or(Value::Null);
                                close |= self.menu_items(ui, icons, state, children(&popup), events);
                            }
                            return;
                        }
                        let Some(menu) = self.find_menu(&name) else {
                            close = true;
                            return;
                        };
                        if attr(&menu, "tag") == Some("ColorPicker") {
                            close |= color_grid(ui, events);
                            return;
                        }
                        let mut items: Vec<Value> = Vec::new();
                        for c in children(&menu) {
                            if attr(c, "tag") == Some("ActionGroup") {
                                items.extend(children(c).iter().cloned());
                            } else {
                                items.push(c.clone());
                            }
                        }
                        close |= self.menu_items(ui, icons, state, &items, events);
                    });
            });
        let clicked_outside = ui.input(|i| i.pointer.any_pressed())
            && !area.response.contains_pointer()
            && !ui.rect_contains_pointer(anchor);
        if close || clicked_outside {
            self.open_menu = None;
        }
    }

    fn menu_items(&mut self, ui: &mut Ui, icons: &mut Icons, state: &dyn State, items: &[Value], events: &mut Vec<Event>) -> bool {
        let font = FontId::proportional(theme::FONT_SIZE);
        let mut close = false;
        ui.set_min_width(180.0);
        for c in items {
            match attr(c, "tag") {
                Some("addseparator") => {
                    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 5.0), Sense::hover());
                    ui.painter().line_segment(
                        [pos2(r.left() + 26.0, r.center().y), pos2(r.right(), r.center().y)],
                        Stroke::new(1.0, theme::RIBBON_SEPARATOR),
                    );
                }
                Some("submenu") => {
                    let label = attr(c, "name").unwrap_or("").to_string();
                    ui.menu_button(label, |ui| {
                        close |= self.menu_items(ui, icons, state, children(c), events);
                    });
                }
                _ => {
                    let name = attr(c, "name").unwrap_or("").to_string();
                    let text = self.text_of(c);
                    let text = if text.is_empty() { name.clone() } else { text };
                    let big = attr(c, "width") == Some("32");
                    let h = if big { 36.0 } else { 22.0 };
                    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width().max(180.0), h), Sense::click());
                    let enabled = state.enabled(&name);
                    if resp.hovered() && enabled {
                        ui.painter().rect_filled(r, 0.0, theme::HIGHLIGHT);
                        ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, theme::ACCENT_HILIGHT), egui::StrokeKind::Inside);
                    }
                    if state.checked(&name) {
                        ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(h, h)).shrink(1.0), 0.0, theme::HIGHLIGHT_DN);
                    }
                    let isz = if big { 32.0 } else { 16.0 };
                    if let Some(tex) = self.icon_of(c).and_then(|i| icons.studio(ui.ctx(), &i)) {
                        let ir = Rect::from_min_size(pos2(r.left() + (h - isz) / 2.0, r.center().y - isz / 2.0), vec2(isz, isz));
                        ui.painter().image(tex.id(), ir, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    let color = if enabled { theme::TEXT } else { theme::DISABLED_TEXT };
                    ui.painter().text(pos2(r.left() + h + 6.0, r.center().y), Align2::LEFT_CENTER, &text, font.clone(), color);
                    if resp.clicked() && enabled {
                        events.push(Event::Action(name));
                        close = true;
                    }
                }
            }
        }
        close
    }
}

fn paint_button(ui: &Ui, r: Rect, hovered: bool, pressed: bool, checked: bool) {
    let color = if pressed || (checked && !hovered) {
        Some(theme::HIGHLIGHT_DN)
    } else if hovered {
        Some(theme::HIGHLIGHT)
    } else {
        None
    };
    if let Some(c) = color {
        ui.painter().rect_filled(r, 0.0, c);
        if checked {
            ui.painter().rect_stroke(r, 2.0, Stroke::new(1.0, theme::CHECKED_BORDER), egui::StrokeKind::Inside);
        }
    }
}

fn arrow(ui: &Ui, c: Pos2, color: Color32) {
    ui.painter().add(egui::Shape::convex_polygon(
        vec![c + vec2(-3.0, -1.5), c + vec2(3.0, -1.5), c + vec2(0.0, 1.5)],
        color,
        Stroke::NONE,
    ));
}

fn color_grid(ui: &mut Ui, events: &mut Vec<Event>) -> bool {
    let mut close = false;
    let colors: Vec<rbx_math::BrickColor> = rbx_math::BrickColor::palette().collect();
    let per_row = 8;
    for row in colors.chunks(per_row) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
            for c in row {
                let (r, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
                let col = c.color3();
                let fill = Color32::from_rgb((col.r * 255.0) as u8, (col.g * 255.0) as u8, (col.b * 255.0) as u8);
                ui.painter().rect_filled(r, 0.0, fill);
                let stroke = if resp.hovered() { Stroke::new(2.0, theme::ACCENT) } else { Stroke::new(1.0, theme::EDIT_BORDER) };
                ui.painter().rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
                let resp = resp.on_hover_text(c.name());
                if resp.clicked() {
                    events.push(Event::Color(c.number()));
                    close = true;
                }
            }
        });
    }
    close
}
