use crate::DataModel;
use crate::gui::{GuiView, Placement};
use rbx_math::{UDim, UDim2, Vec2};
use rbx_reflection::{InstanceId, Variant};
use rustc_hash::FxHashMap as HashMap;

#[derive(Default)]
pub struct GuiLayout {
    pub nodes: HashMap<InstanceId, Placement>,
    pub canvases: HashMap<InstanceId, Placement>,
    pub content_sizes: HashMap<InstanceId, Vec2>,
    pub text_bounds: HashMap<InstanceId, Vec2>,
}

fn udim2(dm: &DataModel, id: InstanceId, name: &str) -> UDim2 {
    match dm.get(id, name) {
        Ok(Variant::UDim2(u)) => u,
        _ => UDim2::default(),
    }
}

fn udim(dm: &DataModel, id: InstanceId, name: &str) -> UDim {
    match dm.get(id, name) {
        Ok(Variant::UDim(u)) => u,
        _ => UDim::default(),
    }
}

fn vec2(dm: &DataModel, id: InstanceId, name: &str) -> Vec2 {
    match dm.get(id, name) {
        Ok(Variant::Vector2(v)) => v,
        _ => Vec2::ZERO,
    }
}

fn number(dm: &DataModel, id: InstanceId, name: &str) -> f32 {
    match dm.get(id, name) {
        Ok(Variant::Float(v)) => v,
        Ok(Variant::Double(v)) => v as f32,
        Ok(Variant::Int(v)) => v as f32,
        Ok(Variant::Int64(v)) => v as f32,
        _ => 0.0,
    }
}

fn enum_value(dm: &DataModel, id: InstanceId, name: &str) -> u32 {
    match dm.get(id, name) {
        Ok(Variant::Enum(v)) => v,
        _ => 0,
    }
}

fn boolean(dm: &DataModel, id: InstanceId, name: &str, default: bool) -> bool {
    match dm.get(id, name) {
        Ok(Variant::Bool(b)) => b,
        _ => default,
    }
}

fn resolve(u: UDim2, size: Vec2) -> Vec2 {
    Vec2::new(
        u.x.scale * size.x + u.x.offset as f32,
        u.y.scale * size.y + u.y.offset as f32,
    )
}

fn resolve1(u: UDim, size: f32) -> f32 {
    u.scale * size + u.offset as f32
}

fn component_child(dm: &DataModel, id: InstanceId, class: &str) -> Option<InstanceId> {
    dm.children_of(id)
        .iter()
        .copied()
        .find(|c| dm.class_of(*c) == Some(class))
}

fn layout_child(dm: &DataModel, id: InstanceId) -> Option<InstanceId> {
    dm.children_of(id).iter().copied().find(|c| {
        matches!(
            dm.class_of(*c),
            Some("UIListLayout" | "UIGridLayout" | "UIPageLayout")
        )
    })
}

fn gui_children(dm: &DataModel, id: InstanceId, out: &mut Vec<InstanceId>) {
    for &c in dm.children_of(id) {
        if dm.class_of(c) == Some("Folder") {
            gui_children(dm, c, out);
        } else if dm.is_a(c, "GuiObject") {
            out.push(c);
        }
    }
}

fn padding(dm: &DataModel, id: InstanceId, size: Vec2) -> [f32; 4] {
    match component_child(dm, id, "UIPadding") {
        Some(p) => [
            resolve1(udim(dm, p, "PaddingLeft"), size.x),
            resolve1(udim(dm, p, "PaddingTop"), size.y),
            resolve1(udim(dm, p, "PaddingRight"), size.x),
            resolve1(udim(dm, p, "PaddingBottom"), size.y),
        ],
        None => [0.0; 4],
    }
}

fn content_rect(dm: &DataModel, id: InstanceId, p: Placement) -> Placement {
    let pad = padding(dm, id, p.size);
    Placement {
        position: p.position + Vec2::new(pad[0], pad[1]),
        size: Vec2::new(
            (p.size.x - pad[0] - pad[2]).max(0.0),
            (p.size.y - pad[1] - pad[3]).max(0.0),
        ),
        rotation: p.rotation,
    }
}

fn apply_constraints(dm: &DataModel, id: InstanceId, mut size: Vec2, parent: Vec2) -> Vec2 {
    if let Some(c) = component_child(dm, id, "UISizeConstraint") {
        let min = vec2(dm, c, "MinSize");
        let max = vec2(dm, c, "MaxSize");
        let max = Vec2::new(
            if max.x <= 0.0 { f32::INFINITY } else { max.x },
            if max.y <= 0.0 { f32::INFINITY } else { max.y },
        );
        size = size.max(min).min(max.max(min));
    }
    if let Some(c) = component_child(dm, id, "UIAspectRatioConstraint") {
        let ratio = number(dm, c, "AspectRatio").max(1e-4);
        let scale_with_parent = enum_value(dm, c, "AspectType") == 1;
        let height_dominant = enum_value(dm, c, "DominantAxis") == 1;
        if scale_with_parent {
            if height_dominant {
                size = Vec2::new(size.y * ratio, size.y);
            } else {
                size = Vec2::new(size.x, size.x / ratio);
            }
        } else {
            let fit = if size.x / size.y.max(1e-4) > ratio {
                Vec2::new(size.y * ratio, size.y)
            } else {
                Vec2::new(size.x, size.x / ratio)
            };
            size = fit;
        }
        let _ = parent;
    }
    size
}

fn ui_scale(dm: &DataModel, id: InstanceId) -> f32 {
    component_child(dm, id, "UIScale")
        .map(|s| number(dm, s, "Scale"))
        .unwrap_or(1.0)
}

fn is_text(class: &str) -> bool {
    matches!(class, "TextLabel" | "TextButton" | "TextBox")
}

pub fn font_for(dm: &DataModel, id: InstanceId) -> rbx_text::Font {
    if let Ok(Variant::Font(face)) = dm.get(id, "FontFace") {
        let bold = face.weight >= 600;
        let light = face.weight <= 300;
        let italic = face.style == 1;
        let family = face.family.rsplit('/').next().unwrap_or("");
        return match family {
            "Arial.json" | "LegacyArial.json" if bold => rbx_text::Font::ArialBold,
            "Arial.json" => rbx_text::Font::Arial,
            "LegacyArial.json" => rbx_text::Font::Legacy,
            _ if italic => rbx_text::Font::SourceSansItalic,
            _ if bold => rbx_text::Font::SourceSansBold,
            _ if light => rbx_text::Font::SourceSansLight,
            _ => rbx_text::Font::SourceSans,
        };
    }
    let value = match dm.get(id, "Font") {
        Ok(Variant::Enum(v)) => v as i64,
        _ => 0,
    };
    rbx_text::Font::from_enum(value).unwrap_or(rbx_text::Font::SourceSans)
}

pub fn text_size(dm: &DataModel, id: InstanceId) -> f32 {
    if dm.property_descriptor(id, "TextSize").is_some() {
        let s = number(dm, id, "TextSize");
        if s > 0.0 {
            return s;
        }
    }
    rbx_text::font_size(match dm.get(id, "FontSize") {
        Ok(Variant::Enum(v)) => v as i64,
        _ => 0,
    })
}

fn measure_text(dm: &DataModel, id: InstanceId, width: f32) -> Vec2 {
    let text = match dm.get(id, "Text") {
        Ok(v) => v.as_str().unwrap_or("").to_string(),
        Err(_) => String::new(),
    };
    if text.is_empty() {
        return Vec2::ZERO;
    }
    let wrapped = boolean(dm, id, "TextWrapped", false);
    let avail = if wrapped {
        width.max(1.0)
    } else {
        f32::INFINITY
    };
    let (out, _) = rbx_text::typesetter(font_for(dm, id)).measure(
        &text,
        text_size(dm, id),
        [avail, f32::INFINITY],
    );
    Vec2::new(out[0], out[1])
}

struct Item {
    id: InstanceId,
    size: Vec2,
}

struct Ctx<'a> {
    dm: &'a DataModel,
    out: GuiLayout,
}

impl Ctx<'_> {
    fn sized(&mut self, id: InstanceId, parent: Vec2, scale: f32) -> Vec2 {
        let dm = self.dm;
        let constrained = match enum_value(dm, id, "SizeConstraint") {
            1 => Vec2::new(parent.x, parent.x),
            2 => Vec2::new(parent.y, parent.y),
            _ => parent,
        };
        let raw = udim2(dm, id, "Size");
        let mut size = Vec2::new(
            raw.x.scale * constrained.x + raw.x.offset as f32 * scale,
            raw.y.scale * constrained.y + raw.y.offset as f32 * scale,
        );
        let auto = enum_value(dm, id, "AutomaticSize");
        if auto != 0 {
            let content = self.measure_content(id, size, scale);
            if auto & 1 != 0 {
                size.x = size.x.max(content.x);
            }
            if auto & 2 != 0 {
                size.y = size.y.max(content.y);
            }
        }
        let own_scale = ui_scale(dm, id);
        apply_constraints(dm, id, size * own_scale, parent)
    }

    fn measure_content(&mut self, id: InstanceId, size: Vec2, scale: f32) -> Vec2 {
        let dm = self.dm;
        let pad = padding(dm, id, size);
        let inner = Vec2::new(
            (size.x - pad[0] - pad[2]).max(0.0),
            (size.y - pad[1] - pad[3]).max(0.0),
        );
        let mut extent = Vec2::ZERO;
        let class = dm.class_of(id).unwrap_or("");
        if is_text(class) {
            let auto = enum_value(dm, id, "AutomaticSize");
            let width = if auto & 1 != 0 {
                f32::INFINITY
            } else {
                inner.x
            };
            extent = extent.max(measure_text(dm, id, width));
        }
        let mut kids = Vec::new();
        gui_children(dm, id, &mut kids);
        let child_scale = scale * ui_scale(dm, id);
        match layout_child(dm, id) {
            Some(layout) if dm.class_of(layout) != Some("UIPageLayout") => {
                let items: Vec<Item> = self.items(&kids, inner, child_scale);
                let rect = Placement {
                    position: Vec2::ZERO,
                    size: inner,
                    rotation: 0.0,
                };
                let (_, content) = self.arrange(layout, &items, rect, true);
                extent = extent.max(content);
            }
            _ => {
                for k in kids {
                    if !boolean(dm, k, "Visible", true) {
                        continue;
                    }
                    let s = self.sized(k, inner, child_scale);
                    let pos = udim2(dm, k, "Position");
                    let anchor = vec2(dm, k, "AnchorPoint");
                    let p = Vec2::new(
                        pos.x.offset as f32 * child_scale,
                        pos.y.offset as f32 * child_scale,
                    ) - anchor * s;
                    extent = extent.max(p + s);
                }
            }
        }
        Vec2::new(extent.x + pad[0] + pad[2], extent.y + pad[1] + pad[3])
    }

    fn items(&mut self, kids: &[InstanceId], inner: Vec2, scale: f32) -> Vec<Item> {
        kids.iter()
            .copied()
            .filter(|k| boolean(self.dm, *k, "Visible", true))
            .map(|k| Item {
                id: k,
                size: self.sized(k, inner, scale),
            })
            .collect()
    }

    fn sort(&self, layout: InstanceId, items: &mut [Item]) {
        let dm = self.dm;
        match enum_value(dm, layout, "SortOrder") {
            0 => items.sort_by(|a, b| dm.name_of(a.id).cmp(dm.name_of(b.id))),
            2 => items.sort_by_key(|i| number(dm, i.id, "LayoutOrder") as i64),
            _ => {}
        }
    }

    fn arrange(
        &mut self,
        layout: InstanceId,
        items: &[Item],
        rect: Placement,
        measuring: bool,
    ) -> (Vec<(InstanceId, Vec2, Vec2)>, Vec2) {
        let dm = self.dm;
        let mut items: Vec<Item> = items
            .iter()
            .map(|i| Item {
                id: i.id,
                size: i.size,
            })
            .collect();
        self.sort(layout, &mut items);
        let class = dm.class_of(layout).unwrap_or("");
        let vertical = enum_value(dm, layout, "FillDirection") == 1;
        let halign = enum_value(dm, layout, "HorizontalAlignment");
        let valign = enum_value(dm, layout, "VerticalAlignment");
        let along = |v: Vec2| if vertical { v.y } else { v.x };
        let across = |v: Vec2| if vertical { v.x } else { v.y };
        let make = |a: f32, c: f32| {
            if vertical {
                Vec2::new(c, a)
            } else {
                Vec2::new(a, c)
            }
        };
        let main_align = if vertical { valign } else { halign };
        let cross_align = if vertical { halign } else { valign };
        let align_offset = |align: u32, free: f32| match align {
            0 => free * 0.5,
            2 => free,
            _ => 0.0,
        };
        let mut placed = Vec::new();
        if class == "UIGridLayout" {
            let cell = resolve(udim2(dm, layout, "CellSize"), rect.size);
            let gap = resolve(udim2(dm, layout, "CellPadding"), rect.size);
            let main_avail = along(rect.size);
            let per_line = {
                let max_cells = number(dm, layout, "FillDirectionMaxCells") as usize;
                let fit = ((main_avail + along(gap)) / (along(cell) + along(gap)).max(1.0))
                    .floor()
                    .max(1.0) as usize;
                if max_cells > 0 {
                    fit.min(max_cells)
                } else {
                    fit
                }
            };
            let lines = items.len().div_ceil(per_line.max(1));
            let used_main =
                per_line.min(items.len()) as f32 * (along(cell) + along(gap)) - along(gap);
            let used_cross = lines as f32 * (across(cell) + across(gap)) - across(gap);
            let content = make(used_main.max(0.0), used_cross.max(0.0));
            let origin = rect.position
                + make(
                    align_offset(main_align, along(rect.size) - used_main),
                    align_offset(cross_align, across(rect.size) - used_cross),
                );
            for (i, item) in items.iter().enumerate() {
                let line = i / per_line.max(1);
                let slot = i % per_line.max(1);
                let pos = origin
                    + make(
                        slot as f32 * (along(cell) + along(gap)),
                        line as f32 * (across(cell) + across(gap)),
                    );
                placed.push((item.id, pos, cell));
            }
            if !measuring {
                self.out.content_sizes.insert(layout, content);
            }
            return (placed, content);
        }
        let gap = resolve1(udim(dm, layout, "Padding"), along(rect.size));
        let flex = enum_value(
            dm,
            layout,
            if vertical {
                "VerticalFlex"
            } else {
                "HorizontalFlex"
            },
        );
        let wraps = boolean(dm, layout, "Wraps", false);
        let mut lines: Vec<Vec<usize>> = vec![Vec::new()];
        let mut run = 0.0;
        for (i, item) in items.iter().enumerate() {
            let len = along(item.size);
            let line = lines.last_mut().unwrap();
            if wraps && !line.is_empty() && run + gap + len > along(rect.size) + 0.5 {
                lines.push(vec![i]);
                run = len;
            } else {
                if !line.is_empty() {
                    run += gap;
                }
                run += len;
                line.push(i);
            }
        }
        let mut cross_cursor = 0.0;
        let mut content = Vec2::ZERO;
        let line_gap = if wraps {
            resolve1(
                udim(
                    dm,
                    layout,
                    if vertical {
                        "HorizontalPadding"
                    } else {
                        "VerticalPadding"
                    },
                ),
                across(rect.size),
            )
        } else {
            0.0
        };
        for line in &lines {
            let mut sizes: Vec<Vec2> = line.iter().map(|&i| items[i].size).collect();
            let total: f32 = sizes.iter().map(|s| along(*s)).sum::<f32>()
                + gap * (sizes.len().saturating_sub(1)) as f32;
            let free = along(rect.size) - total;
            let mut spacing = gap;
            let mut lead = 0.0;
            let flex_items: Vec<(usize, u32, f32)> = line
                .iter()
                .enumerate()
                .filter_map(|(n, &i)| {
                    component_child(dm, items[i].id, "UIFlexItem")
                        .map(|f| (n, enum_value(dm, f, "FlexMode"), number(dm, f, "GrowRatio")))
                })
                .collect();
            if !measuring && free > 0.0 && !flex_items.is_empty() {
                let growers: Vec<(usize, f32)> = flex_items
                    .iter()
                    .filter(|(_, mode, _)| matches!(mode, 1 | 3 | 4))
                    .map(|(n, mode, ratio)| (*n, if *mode == 4 { *ratio } else { 1.0 }))
                    .collect();
                let weight: f32 = growers.iter().map(|g| g.1).sum();
                if weight > 0.0 {
                    for (n, w) in growers {
                        let extra = free * w / weight;
                        if vertical {
                            sizes[n].y += extra;
                        } else {
                            sizes[n].x += extra;
                        }
                    }
                }
            } else if !measuring && free > 0.0 && flex != 0 {
                let count = sizes.len() as f32;
                match flex {
                    1 => {
                        for s in sizes.iter_mut() {
                            if vertical {
                                s.y += free / count;
                            } else {
                                s.x += free / count;
                            }
                        }
                    }
                    2 => {
                        spacing = gap + free / count;
                        lead = free / count * 0.5;
                    }
                    3 if count > 1.0 => spacing = gap + free / (count - 1.0),
                    4 => {
                        spacing = gap + free / (count + 1.0);
                        lead = free / (count + 1.0);
                    }
                    _ => {}
                }
            } else if !measuring && free < 0.0 && !flex_items.is_empty() {
                let shrinkers: Vec<usize> = flex_items
                    .iter()
                    .filter(|(_, mode, _)| matches!(mode, 2 | 3 | 4))
                    .map(|(n, _, _)| *n)
                    .collect();
                if !shrinkers.is_empty() {
                    let cut = -free / shrinkers.len() as f32;
                    for n in shrinkers {
                        if vertical {
                            sizes[n].y = (sizes[n].y - cut).max(0.0);
                        } else {
                            sizes[n].x = (sizes[n].x - cut).max(0.0);
                        }
                    }
                }
            }
            let used: f32 = sizes.iter().map(|s| along(*s)).sum::<f32>()
                + spacing * (sizes.len().saturating_sub(1)) as f32
                + lead * 2.0;
            let line_cross = sizes.iter().map(|s| across(*s)).fold(0.0, f32::max);
            let start = if flex == 0 || measuring {
                align_offset(main_align, along(rect.size) - used)
            } else {
                0.0
            } + lead;
            let mut cursor = start;
            for (n, &i) in line.iter().enumerate() {
                let s = sizes[n];
                let cross_space = if wraps { line_cross } else { across(rect.size) };
                let c = cross_cursor + align_offset(cross_align, cross_space - across(s));
                placed.push((items[i].id, rect.position + make(cursor, c), s));
                cursor += along(s) + spacing;
            }
            content = make(
                along(content).max(used),
                across(content) + line_cross + if cross_cursor > 0.0 { line_gap } else { 0.0 },
            );
            cross_cursor += line_cross + line_gap;
        }
        if !measuring {
            self.out.content_sizes.insert(layout, content);
        }
        (placed, content)
    }

    fn container(&mut self, id: InstanceId, own: Placement, scale: f32) {
        let dm = self.dm;
        self.out.nodes.insert(id, own);
        let mut base = own;
        if dm.class_of(id) == Some("ScrollingFrame") {
            let mut canvas_size = resolve(udim2(dm, id, "CanvasSize"), own.size);
            let auto = enum_value(dm, id, "AutomaticCanvasSize");
            if auto != 0 {
                let content = self.measure_content(id, own.size, scale);
                if auto & 1 != 0 {
                    canvas_size.x = canvas_size.x.max(content.x);
                }
                if auto & 2 != 0 {
                    canvas_size.y = canvas_size.y.max(content.y);
                }
            }
            let offset = vec2(dm, id, "CanvasPosition");
            base = Placement {
                position: own.position - offset,
                size: canvas_size.max(own.size),
                rotation: own.rotation,
            };
            self.out.canvases.insert(id, base);
        }
        let inner = content_rect(dm, id, base);
        let child_scale = scale * ui_scale(dm, id);
        let mut kids = Vec::new();
        gui_children(dm, id, &mut kids);
        match layout_child(dm, id) {
            Some(layout) if dm.class_of(layout) != Some("UIPageLayout") => {
                let items = self.items(&kids, inner.size, child_scale);
                let (placed, _) = self.arrange(layout, &items, inner, false);
                for (child, pos, size) in placed {
                    let p = Placement {
                        position: pos,
                        size,
                        rotation: own.rotation + number(dm, child, "Rotation"),
                    };
                    self.node(child, p, child_scale);
                }
                for k in kids {
                    if !boolean(dm, k, "Visible", true) {
                        let size = self.sized(k, inner.size, child_scale);
                        self.node(
                            k,
                            Placement {
                                position: inner.position,
                                size,
                                rotation: own.rotation,
                            },
                            child_scale,
                        );
                    }
                }
            }
            _ => {
                for k in kids {
                    let size = self.sized(k, inner.size, child_scale);
                    let pos = udim2(dm, k, "Position");
                    let anchor = vec2(dm, k, "AnchorPoint");
                    let local = inner.position
                        + Vec2::new(
                            pos.x.scale * inner.size.x + pos.x.offset as f32 * child_scale,
                            pos.y.scale * inner.size.y + pos.y.offset as f32 * child_scale,
                        )
                        - anchor * size;
                    let p = Placement {
                        position: local,
                        size,
                        rotation: own.rotation + number(dm, k, "Rotation"),
                    };
                    self.node(k, p, child_scale);
                }
            }
        }
    }

    fn node(&mut self, id: InstanceId, p: Placement, scale: f32) {
        let dm = self.dm;
        if is_text(dm.class_of(id).unwrap_or("")) {
            let bounds = measure_text(dm, id, p.size.x);
            self.out.text_bounds.insert(id, bounds);
        }
        self.container(id, p, scale);
    }
}

pub fn screen_rect(dm: &DataModel, id: InstanceId, view: GuiView) -> Placement {
    if boolean(dm, id, "IgnoreGuiInset", false) {
        return Placement {
            position: Vec2::ZERO,
            size: view.screen,
            rotation: 0.0,
        };
    }
    view.user_rect()
}

pub fn compute(dm: &DataModel) -> GuiLayout {
    let mut ctx = Ctx {
        dm,
        out: GuiLayout::default(),
    };
    let view = dm.gui_view;
    let mut roots = Vec::new();
    fn add(dm: &DataModel, parent: InstanceId, roots: &mut Vec<InstanceId>) {
        for &c in dm.children_of(parent) {
            if matches!(dm.class_of(c), Some("ScreenGui" | "GuiMain")) {
                roots.push(c);
            } else if !dm.is_a(c, "GuiBase2d") && !dm.is_a(c, "BasePart") {
                add(dm, c, roots);
            }
        }
    }
    if let Some(core) = dm.find_service("CoreGui") {
        add(dm, core, &mut roots);
    }
    if let Some(sg) = dm.find_service("StarterGui") {
        add(dm, sg, &mut roots);
    }
    if let Some(players) = dm.find_service("Players") {
        for &p in dm.children_of(players) {
            if let Some(pg) = dm.find_first_child_of_class(p, "PlayerGui") {
                add(dm, pg, &mut roots);
            }
        }
    }
    for root in roots {
        let rect = screen_rect(dm, root, view);
        ctx.container(root, rect, 1.0);
    }
    ctx.out
}
