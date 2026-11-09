use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
};
use std::path::Path;
use std::sync::Arc;

pub const TEXT: Color32 = Color32::from_rgb(59, 59, 59);
pub const ACCENT: Color32 = Color32::from_rgb(102, 146, 220);
pub const ACCENT_HILIGHT: Color32 = Color32::from_rgb(147, 181, 240);
pub const HIGHLIGHT: Color32 = Color32::from_rgb(229, 238, 254);
pub const HIGHLIGHT_DN: Color32 = Color32::from_rgb(219, 219, 219);
pub const CHECKED_BORDER: Color32 = Color32::from_rgb(184, 184, 184);
pub const RIBBON_BACK: Color32 = Color32::from_rgb(243, 243, 243);
pub const RIBBON_FACE: Color32 = Color32::WHITE;
pub const RIBBON_SEPARATOR: Color32 = Color32::from_rgb(212, 212, 212);
pub const DOCK_TITLE: Color32 = Color32::from_rgb(208, 208, 208);
pub const DOC_TAB_BAR: Color32 = Color32::from_rgb(196, 196, 196);
pub const DOC_TAB: Color32 = Color32::from_rgb(242, 242, 242);
pub const VIEW_BORDER: Color32 = Color32::from_rgb(196, 196, 196);
pub const GRID_LINE: Color32 = Color32::from_rgb(238, 238, 238);
pub const CATEGORY: Color32 = Color32::from_rgb(233, 233, 233);
pub const DISABLED_TEXT: Color32 = Color32::from_rgb(128, 128, 128);
pub const WINDOW: Color32 = Color32::from_rgb(243, 243, 243);
pub const EDIT_BORDER: Color32 = Color32::from_rgb(171, 171, 171);
pub const SELECT_TOP: Color32 = Color32::from_rgb(110, 161, 241);
pub const SELECT_BOTTOM: Color32 = Color32::from_rgb(86, 125, 188);
pub const HOVER_TOP: Color32 = Color32::from_rgb(231, 239, 253);
pub const HOVER_BOTTOM: Color32 = Color32::from_rgb(203, 218, 241);
pub const HOVER_BORDER: Color32 = Color32::from_rgb(191, 205, 228);

pub const FONT_SIZE: f32 = 12.0;

pub fn install(ctx: &egui::Context, fonts_dir: &Path) {
    let mut defs = FontDefinitions::default();
    for (name, file) in [
        ("SourceSansPro", "SourceSansPro-Regular.ttf"),
        ("SourceSansProBold", "SourceSansPro-Bold.ttf"),
    ] {
        if let Ok(bytes) = std::fs::read(fonts_dir.join(file)) {
            defs.font_data
                .insert(name.to_string(), Arc::new(FontData::from_owned(bytes)));
        }
    }
    if defs.font_data.contains_key("SourceSansPro") {
        defs.families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "SourceSansPro".into());
    }
    if defs.font_data.contains_key("SourceSansProBold") {
        defs.families.insert(
            FontFamily::Name("Bold".into()),
            vec!["SourceSansProBold".into(), "SourceSansPro".into()],
        );
    }
    ctx.set_fonts(defs);
    ctx.set_theme(egui::Theme::Light);
    ctx.style_mut_of(egui::Theme::Light, |style| {
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(11.0)),
            (TextStyle::Body, FontId::proportional(FONT_SIZE)),
            (TextStyle::Button, FontId::proportional(FONT_SIZE)),
            (TextStyle::Heading, FontId::proportional(15.0)),
            (TextStyle::Monospace, FontId::monospace(12.0)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(4.0, 2.0);
        style.spacing.button_padding = egui::vec2(6.0, 2.0);
        style.spacing.interact_size = egui::vec2(20.0, 18.0);
        style.spacing.indent = 16.0;
        let v = &mut style.visuals;
        v.panel_fill = WINDOW;
        v.window_fill = Color32::WHITE;
        v.extreme_bg_color = Color32::WHITE;
        v.faint_bg_color = Color32::from_rgb(249, 249, 249);
        v.override_text_color = Some(TEXT);
        v.selection.bg_fill = SELECT_BOTTOM;
        v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        v.hyperlink_color = ACCENT;
        v.window_corner_radius = CornerRadius::ZERO;
        v.menu_corner_radius = CornerRadius::ZERO;
        v.window_stroke = Stroke::new(1.0, RIBBON_SEPARATOR);
        v.popup_shadow = egui::epaint::Shadow {
            offset: [2, 2],
            blur: 4,
            spread: 0,
            color: Color32::from_black_alpha(40),
        };
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = CornerRadius::ZERO;
            w.fg_stroke = Stroke::new(1.0, TEXT);
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, RIBBON_SEPARATOR);
        v.widgets.noninteractive.bg_fill = Color32::WHITE;
        v.widgets.inactive.bg_fill = Color32::WHITE;
        v.widgets.inactive.weak_bg_fill = Color32::WHITE;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, EDIT_BORDER);
        v.widgets.hovered.bg_fill = HIGHLIGHT;
        v.widgets.hovered.weak_bg_fill = HIGHLIGHT;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT_HILIGHT);
        v.widgets.hovered.expansion = 0.0;
        v.widgets.active.bg_fill = HIGHLIGHT_DN;
        v.widgets.active.weak_bg_fill = HIGHLIGHT_DN;
        v.widgets.active.bg_stroke = Stroke::new(1.0, CHECKED_BORDER);
        v.widgets.active.expansion = 0.0;
        v.widgets.open.bg_fill = HIGHLIGHT_DN;
        v.widgets.open.weak_bg_fill = HIGHLIGHT_DN;
        v.text_cursor.stroke = Stroke::new(1.0, Color32::BLACK);
    });
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("Bold".into()))
}

pub fn vertical_gradient(painter: &egui::Painter, rect: egui::Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    let i = mesh.vertices.len() as u32;
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(i, i + 1, i + 2);
    mesh.add_triangle(i, i + 2, i + 3);
    painter.add(egui::Shape::mesh(mesh));
}

pub fn selected_row(painter: &egui::Painter, rect: egui::Rect) {
    vertical_gradient(painter, rect, SELECT_TOP, SELECT_BOTTOM);
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0, SELECT_BOTTOM),
        egui::StrokeKind::Inside,
    );
}

pub fn hovered_row(painter: &egui::Painter, rect: egui::Rect) {
    vertical_gradient(painter, rect, HOVER_TOP, HOVER_BOTTOM);
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0, HOVER_BORDER),
        egui::StrokeKind::Inside,
    );
}
