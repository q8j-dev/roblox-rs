use rbx_dom::gui::{Placement, canvas, place_child, placement};
use rbx_dom::styles::Component;
use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{Rect2D, Vec2};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Rect {
    pub fn xywh(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect {
            x0: x,
            y0: y,
            x1: x + w,
            y1: y + h,
        }
    }

    pub fn from_placement(p: &Placement) -> Rect {
        let pos = p.rounded_position();
        let size = p.rounded_size();
        Rect::xywh(pos.x, pos.y, size.x, size.y)
    }

    pub fn intersect(&self, o: &Rect) -> Rect {
        Rect {
            x0: self.x0.max(o.x0),
            y0: self.y0.max(o.y0),
            x1: self.x1.min(o.x1).max(self.x0.max(o.x0)),
            y1: self.y1.min(o.y1).max(self.y0.max(o.y0)),
        }
    }

    pub fn width(&self) -> f32 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f32 {
        self.y1 - self.y0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ImageUv {
    Full,
    Pixels { offset: Vec2, size: Vec2 },
}

#[derive(Clone, Debug)]
pub struct TextCmd {
    pub text: String,
    pub font: rbx_text::Font,
    pub size: f32,
    pub scaled: bool,
    pub wrapped: bool,
    pub xalign: rbx_text::XAlign,
    pub yalign: rbx_text::YAlign,
    pub rect: Rect,
    pub color: [f32; 4],
    pub tints: Vec<rbx_dom::rich_text::Tint>,
    pub stroke: Option<[f32; 4]>,
}

#[derive(Clone, Debug)]
pub enum GuiCmd {
    Rect {
        rect: Rect,
        color: [f32; 4],
    },
    Border {
        rect: Rect,
        size: f32,
        color: [f32; 4],
    },
    Image {
        rect: Rect,
        url: String,
        uv: ImageUv,
        color: [f32; 4],
    },
    Thirds {
        rect: Rect,
        url: String,
        edge: Vec2,
        min: Vec2,
        color: [f32; 4],
    },
    Slice {
        rect: Rect,
        url: String,
        center: Rect2D,
        uv: ImageUv,
        color: [f32; 4],
    },
    Text(TextCmd),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub rect: Rect,
    pub radii: [f32; 4],
    pub stroke: f32,
}

#[derive(Clone, Debug)]
pub struct GuiItem {
    pub shape: Option<Shape>,
    pub cmd: GuiCmd,
    pub clip: Option<Rect>,
    pub rotation: f32,
    pub pivot: Vec2,
}

pub(crate) fn get_bool(dm: &DataModel, id: InstanceId, name: &str, default: bool) -> bool {
    match dm.effective(id, name) {
        Ok(Variant::Bool(b)) => b,
        _ => default,
    }
}

pub(crate) fn get_f32(dm: &DataModel, id: InstanceId, name: &str) -> f32 {
    match dm.effective(id, name) {
        Ok(Variant::Float(v)) => v,
        Ok(Variant::Double(v)) => v as f32,
        Ok(Variant::Int(v)) => v as f32,
        _ => 0.0,
    }
}

pub(crate) fn get_i64(dm: &DataModel, id: InstanceId, name: &str) -> i64 {
    match dm.effective(id, name) {
        Ok(Variant::Int(v)) => v as i64,
        Ok(Variant::Int64(v)) => v,
        Ok(Variant::Enum(v)) => v as i64,
        Ok(Variant::Float(v)) => v as i64,
        _ => 0,
    }
}

fn get_color(dm: &DataModel, id: InstanceId, name: &str) -> [f32; 3] {
    match dm.effective(id, name) {
        Ok(Variant::Color3(c)) => [c.r, c.g, c.b],
        Ok(Variant::BrickColor(b)) => {
            let c = b.color3();
            [c.r, c.g, c.b]
        }
        _ => [1.0, 1.0, 1.0],
    }
}

pub(crate) fn get_string(dm: &DataModel, id: InstanceId, name: &str) -> String {
    match dm.effective(id, name) {
        Ok(Variant::String(s)) | Ok(Variant::Content(s)) => s,
        _ => String::new(),
    }
}

pub(crate) fn get_vec2(dm: &DataModel, id: InstanceId, name: &str) -> Vec2 {
    match dm.effective(id, name) {
        Ok(Variant::Vector2(v)) => v,
        _ => Vec2::ZERO,
    }
}

fn rgba(c: [f32; 3], a: f32) -> [f32; 4] {
    [c[0], c[1], c[2], a.clamp(0.0, 1.0)]
}

fn is_text(class: &str) -> bool {
    matches!(class, "TextLabel" | "TextButton" | "TextBox")
}

struct Entry {
    id: InstanceId,
    placement: Placement,
    clip: Option<Rect>,
}

struct Collector<'a> {
    dm: &'a DataModel,
    layers: Vec<(Vec<Entry>, Vec<Entry>)>,
    global: std::collections::BTreeMap<i64, (Vec<Entry>, Vec<Entry>)>,
    layout: std::sync::Arc<rbx_dom::gui_layout::GuiLayout>,
    sibling: bool,
    order: usize,
}

impl Collector<'_> {
    fn load(&mut self, id: InstanceId, parent: Placement, clip: Option<Rect>) {
        let dm = self.dm;
        let class = dm.class_of(id).unwrap_or("");
        if class == "Folder" {
            for &c in dm.children_of(id) {
                self.load(c, parent, clip);
            }
            return;
        }
        if !dm.is_a(id, "GuiObject") || !get_bool(dm, id, "Visible", true) {
            return;
        }
        let p = match self.layout.nodes.get(&id) {
            Some(p) => *p,
            None => place_child(dm, id, parent),
        };
        let entry = Entry {
            id,
            placement: p,
            clip,
        };
        let layer = if self.sibling {
            self.order += 1;
            let z = self.order.min(self.layers.len() - 1);
            &mut self.layers[z]
        } else {
            self.global.entry(get_i64(dm, id, "ZIndex")).or_default()
        };
        if is_text(class) {
            layer.1.push(entry);
        } else {
            layer.0.push(entry);
        }
        let own = Rect::from_placement(&p);
        let child_clip = if get_bool(dm, id, "ClipsDescendants", false) || class == "ScrollingFrame"
        {
            Some(match clip {
                Some(c) => c.intersect(&own),
                None => own,
            })
        } else {
            clip
        };
        let child_parent = canvas(dm, id, p);
        if self.sibling {
            let mut kids: Vec<InstanceId> = dm.children_of(id).to_vec();
            kids.sort_by_key(|c| get_i64(dm, *c, "ZIndex"));
            for c in kids {
                self.load(c, child_parent, child_clip);
            }
        } else {
            for &c in dm.children_of(id) {
                self.load(c, child_parent, child_clip);
            }
        }
    }
}

fn background(dm: &DataModel, id: InstanceId, rect: Rect, out: &mut Vec<GuiCmd>) {
    let alpha = 1.0 - get_f32(dm, id, "BackgroundTransparency");
    if alpha <= 0.0 {
        return;
    }
    out.push(GuiCmd::Rect {
        rect,
        color: rgba(get_color(dm, id, "BackgroundColor3"), alpha),
    });
    let border = get_i64(dm, id, "BorderSizePixel");
    if border > 0 {
        out.push(GuiCmd::Border {
            rect,
            size: border as f32,
            color: rgba(get_color(dm, id, "BorderColor3"), alpha),
        });
    }
}

fn styled(dm: &DataModel, id: InstanceId, class: &str, rect: Rect, out: &mut Vec<GuiCmd>) -> bool {
    let style = get_i64(dm, id, "Style");
    if style == 0 {
        return false;
    }
    let white = [1.0, 1.0, 1.0, 1.0];
    if class == "Frame" {
        let (url, edge, min, slice) = match style {
            1 => ("rbxasset://textures/ui/dialog_blue.png", 0.0, 0.0, true),
            4 => ("rbxasset://textures/ui/dialog_green.png", 0.0, 0.0, true),
            5 => ("rbxasset://textures/ui/dialog_red.png", 0.0, 0.0, true),
            2 => ("rbxasset://textures/blackBkg_square.png", 7.0, 14.0, false),
            3 => ("rbxasset://textures/blackBkg_round.png", 7.0, 14.0, false),
            6 => (
                "rbxasset://textures/ui/newBkg_square.png",
                10.0,
                40.0,
                false,
            ),
            _ => return false,
        };
        if slice {
            out.push(GuiCmd::Slice {
                rect,
                url: url.into(),
                center: Rect2D::xyxy(7.0, 7.0, 33.0, 33.0),
                uv: ImageUv::Full,
                color: white,
            });
        } else {
            out.push(GuiCmd::Thirds {
                rect,
                url: url.into(),
                edge: Vec2::splat(edge),
                min: Vec2::splat(min),
                color: white,
            });
        }
        return true;
    }
    let (url, edge, min) = match style {
        1 => ("rbxasset://textures/ui/btn_red.png", 8.0, 36.0),
        2 => ("rbxasset://textures/ui/btn_grey.png", 8.0, 36.0),
        3 => ("rbxasset://textures/ui/btn_newGrey.png", 6.0, 20.0),
        4 => ("rbxasset://textures/ui/btn_newBlue.png", 6.0, 20.0),
        5 => ("rbxasset://textures/ui/btn_newWhite.png", 6.0, 20.0),
        _ => return false,
    };
    out.push(GuiCmd::Thirds {
        rect,
        url: url.into(),
        edge: Vec2::splat(edge),
        min: Vec2::splat(min),
        color: white,
    });
    true
}

fn image(dm: &DataModel, id: InstanceId, rect: Rect, out: &mut Vec<GuiCmd>) {
    let mut url = get_string(dm, id, "Image");
    if url.trim().is_empty() {
        url = get_string(dm, id, "ImageContent");
    }
    let alpha = 1.0 - get_f32(dm, id, "ImageTransparency");
    if url.trim().is_empty() || alpha <= 0.0 {
        return;
    }
    let color = rgba(get_color(dm, id, "ImageColor3"), alpha);
    let size = get_vec2(dm, id, "ImageRectSize");
    let uv = if size.x != 0.0 && size.y != 0.0 {
        ImageUv::Pixels {
            offset: get_vec2(dm, id, "ImageRectOffset"),
            size,
        }
    } else {
        ImageUv::Full
    };
    if get_i64(dm, id, "ScaleType") == 1 {
        let center = match dm.effective(id, "SliceCenter") {
            Ok(Variant::Rect2D(r)) => r,
            _ => Rect2D::default(),
        };
        out.push(GuiCmd::Slice {
            rect,
            url,
            center,
            uv,
            color,
        });
    } else {
        out.push(GuiCmd::Image {
            rect,
            url,
            uv,
            color,
        });
    }
}

fn text(dm: &DataModel, id: InstanceId, rect: Rect, out: &mut Vec<GuiCmd>) {
    let alpha = 1.0 - get_f32(dm, id, "TextTransparency");
    let mut rich = rbx_dom::gui_layout::display_text(dm, id);
    let mut color = get_color(dm, id, "TextColor3");
    if rich.text.is_empty() && dm.class_of(id) == Some("TextBox") {
        rich = rbx_dom::rich_text::RichText {
            text: get_string(dm, id, "PlaceholderText"),
            ..Default::default()
        };
        color = get_color(dm, id, "PlaceholderColor3");
    }
    if alpha <= 0.0 || rich.text.is_empty() {
        return;
    }
    let mut stroke_alpha = (1.0 - get_f32(dm, id, "TextStrokeTransparency")) * alpha;
    let mut stroke_color = get_color(dm, id, "TextStrokeColor3");
    if let Some(stroke) = component(dm, id, "UIStroke")
        && stroke.enum_value(dm, "ApplyStrokeMode") == 0
    {
        stroke_alpha = (1.0 - stroke.f32(dm, "Transparency")) * alpha;
        stroke_color = comp_color(dm, &stroke, "Color");
    }
    out.push(GuiCmd::Text(TextCmd {
        font: rbx_dom::gui_layout::display_font(dm, id, &rich),
        tints: rich.tints,
        text: rich.text,
        size: rbx_dom::gui_layout::text_size(dm, id) * rbx_dom::gui::node_scale(dm, id),
        scaled: get_bool(dm, id, "TextScaled", false),
        wrapped: get_bool(dm, id, "TextWrapped", false) || get_bool(dm, id, "TextWrap", false),
        xalign: rbx_text::XAlign::from_enum(get_i64(dm, id, "TextXAlignment")),
        yalign: rbx_text::YAlign::from_enum(get_i64(dm, id, "TextYAlignment")),
        rect,
        color: rgba(color, alpha),
        stroke: (stroke_alpha > 0.05).then(|| rgba(stroke_color, stroke_alpha)),
    }));
}

fn component(dm: &DataModel, id: InstanceId, class: &str) -> Option<Component> {
    rbx_dom::styles::component(dm, id, class)
}

fn comp_color(dm: &DataModel, c: &Component, name: &str) -> [f32; 3] {
    match c.get(dm, name) {
        Variant::Color3(c) => [c.r, c.g, c.b],
        _ => [1.0, 1.0, 1.0],
    }
}

fn comp_udim(dm: &DataModel, c: &Component, name: &str, basis: f32, k: f32) -> f32 {
    match c.get(dm, name) {
        Variant::UDim(u) => u.scale * basis + u.offset as f32 * k,
        _ => 0.0,
    }
}

fn corner_shape(dm: &DataModel, id: InstanceId, rect: Rect) -> Option<Shape> {
    let corner = component(dm, id, "UICorner")?;
    let basis = rect.width().min(rect.height());
    let k = rbx_dom::gui::node_scale(dm, id);
    let whole = corner.get(dm, "CornerRadius");
    let synced = whole == corner.get(dm, "TopLeftRadius");
    let names = [
        "TopLeftRadius",
        "TopRightRadius",
        "BottomRightRadius",
        "BottomLeftRadius",
    ];
    let radii = names.map(|n| {
        let r = if synced {
            comp_udim(dm, &corner, n, basis, k)
        } else {
            comp_udim(dm, &corner, "CornerRadius", basis, k)
        };
        r.max(0.0).min(basis * 0.5)
    });
    Some(Shape {
        rect,
        radii,
        stroke: 0.0,
    })
}

fn stroke_items(dm: &DataModel, e: &Entry, items: &mut Vec<GuiItem>) {
    let Some(stroke) = component(dm, e.id, "UIStroke") else {
        return;
    };
    let class = dm.class_of(e.id).unwrap_or("");
    if stroke.enum_value(dm, "ApplyStrokeMode") == 0 && is_text(class) {
        return;
    }
    let alpha = 1.0 - stroke.f32(dm, "Transparency");
    let t = stroke.f32(dm, "Thickness");
    if alpha <= 0.0 || t <= 0.0 {
        return;
    }
    let rect = Rect::from_placement(&e.placement);
    let grow = match stroke.enum_value(dm, "BorderStrokePosition") {
        1 => t * 0.5,
        2 => 0.0,
        _ => t,
    };
    let outer = Rect {
        x0: rect.x0 - grow,
        y0: rect.y0 - grow,
        x1: rect.x1 + grow,
        y1: rect.y1 + grow,
    };
    let radii = corner_shape(dm, e.id, rect)
        .map(|s| s.radii.map(|r| r + grow))
        .unwrap_or([0.0; 4]);
    items.push(GuiItem {
        shape: Some(Shape {
            rect: outer,
            radii,
            stroke: t,
        }),
        cmd: GuiCmd::Rect {
            rect: outer,
            color: rgba(comp_color(dm, &stroke, "Color"), alpha),
        },
        clip: e.clip,
        rotation: e.placement.rotation,
        pivot: e.placement.position + e.placement.size * 0.5,
    });
}

fn screen_guis(dm: &DataModel) -> Vec<InstanceId> {
    rbx_dom::gui::screen_gui_roots(dm)
}

pub fn extract(dm: &DataModel) -> Vec<GuiItem> {
    let mut items = Vec::new();
    for root in screen_guis(dm) {
        collect_root(dm, root, placement(dm, root), None, &mut items);
    }
    messages(dm, &mut items);
    items
}

pub(crate) fn collect_root(
    dm: &DataModel,
    root: InstanceId,
    base: Placement,
    clip: Option<Rect>,
    items: &mut Vec<GuiItem>,
) {
    {
        let sibling = get_i64(dm, root, "ZIndexBehavior") == 1;
        let mut c = Collector {
            dm,
            layers: (0..if sibling { 4096 } else { 0 })
                .map(|_| (Vec::new(), Vec::new()))
                .collect(),
            global: Default::default(),
            layout: dm.gui_layout(),
            sibling,
            order: 0,
        };
        if sibling {
            let mut kids: Vec<InstanceId> = dm.children_of(root).to_vec();
            kids.sort_by_key(|k| get_i64(dm, *k, "ZIndex"));
            for child in kids {
                c.load(child, base, clip);
            }
        } else {
            for &child in dm.children_of(root) {
                c.load(child, base, clip);
            }
            c.layers = std::mem::take(&mut c.global).into_values().collect();
        }
        for (general, texts) in &c.layers {
            let mut push = |e: &Entry, cmds: Vec<GuiCmd>| {
                let rect = Rect::from_placement(&e.placement);
                let shape = corner_shape(dm, e.id, rect);
                for cmd in cmds {
                    let shape = if matches!(cmd, GuiCmd::Text(_)) {
                        None
                    } else {
                        shape
                    };
                    items.push(GuiItem {
                        shape,
                        cmd,
                        clip: e.clip,
                        rotation: e.placement.rotation,
                        pivot: e.placement.position + e.placement.size * 0.5,
                    });
                }
            };
            for e in general {
                let class = dm.class_of(e.id).unwrap_or("");
                let rect = Rect::from_placement(&e.placement);
                let mut cmds = Vec::new();
                if !styled(dm, e.id, class, rect, &mut cmds) {
                    background(dm, e.id, rect, &mut cmds);
                }
                push(e, cmds);
            }
            for e in general {
                let class = dm.class_of(e.id).unwrap_or("");
                if matches!(class, "ImageLabel" | "ImageButton") {
                    let mut cmds = Vec::new();
                    image(dm, e.id, Rect::from_placement(&e.placement), &mut cmds);
                    push(e, cmds);
                }
            }
            for e in texts {
                let class = dm.class_of(e.id).unwrap_or("");
                let rect = Rect::from_placement(&e.placement);
                let mut cmds = Vec::new();
                if !styled(dm, e.id, class, rect, &mut cmds) {
                    background(dm, e.id, rect, &mut cmds);
                }
                text(dm, e.id, rect, &mut cmds);
                push(e, cmds);
            }
            for e in general.iter().chain(texts.iter()) {
                stroke_items(dm, e, items);
            }
        }
    }
}

fn messages(dm: &DataModel, items: &mut Vec<GuiItem>) {
    let Some(ws) = dm.find_service("Workspace") else {
        return;
    };
    let view = dm.gui_view.user_rect();
    let area = Rect::xywh(view.position.x, view.position.y, view.size.x, view.size.y);
    for &c in dm.children_of(ws) {
        let s = get_string(dm, c, "Text");
        match dm.class_of(c) {
            Some("Hint") => {
                let bar = Rect {
                    x0: area.x0,
                    y0: area.y0,
                    x1: area.x1,
                    y1: area.y0 + 20.0,
                };
                items.push(GuiItem {
                    shape: None,
                    cmd: GuiCmd::Rect {
                        rect: bar,
                        color: [0.0, 0.0, 0.0, 1.0],
                    },
                    clip: None,
                    rotation: 0.0,
                    pivot: Vec2::ZERO,
                });
                if !s.is_empty() {
                    items.push(GuiItem {
                        shape: None,
                        cmd: GuiCmd::Text(TextCmd {
                            text: s,
                            font: rbx_text::Font::Legacy,
                            size: 14.0,
                            scaled: false,
                            wrapped: false,
                            xalign: rbx_text::XAlign::Center,
                            yalign: rbx_text::YAlign::Center,
                            rect: bar,
                            color: [1.0, 1.0, 1.0, 1.0],
                            tints: Vec::new(),
                            stroke: None,
                        }),
                        clip: None,
                        rotation: 0.0,
                        pivot: Vec2::ZERO,
                    });
                }
            }
            Some("Message") if !s.is_empty() => {
                let inner = Rect {
                    x0: area.x0 + 100.0,
                    y0: area.y0 + 100.0,
                    x1: area.x1 - 100.0,
                    y1: area.y1 - 100.0,
                };
                items.push(GuiItem {
                    shape: None,
                    cmd: GuiCmd::Rect {
                        rect: inner,
                        color: [0.5, 0.5, 0.5, 0.5],
                    },
                    clip: None,
                    rotation: 0.0,
                    pivot: Vec2::ZERO,
                });
                items.push(GuiItem {
                    shape: None,
                    cmd: GuiCmd::Text(TextCmd {
                        text: s,
                        font: rbx_text::Font::Legacy,
                        size: 18.0,
                        scaled: false,
                        wrapped: true,
                        xalign: rbx_text::XAlign::Center,
                        yalign: rbx_text::YAlign::Center,
                        rect: inner,
                        color: [1.0, 1.0, 1.0, 1.0],
                        tints: Vec::new(),
                        stroke: Some([0.0, 0.0, 0.0, 1.0]),
                    }),
                    clip: None,
                    rotation: 0.0,
                    pivot: Vec2::ZERO,
                });
            }
            _ => {}
        }
    }
}

pub fn text_quads(cmd: &TextCmd) -> rbx_text::TextDraw {
    let t = rbx_text::typesetter(cmd.font);
    let r = cmd.rect;
    let pos = [
        match cmd.xalign {
            rbx_text::XAlign::Left => r.x0,
            rbx_text::XAlign::Right => r.x1,
            rbx_text::XAlign::Center => (r.x0 + r.x1) * 0.5,
        },
        match cmd.yalign {
            rbx_text::YAlign::Top => r.y0,
            rbx_text::YAlign::Bottom => r.y1,
            rbx_text::YAlign::Center => (r.y0 + r.y1) * 0.5,
        },
    ];
    let space = if cmd.wrapped {
        [r.width(), r.height()]
    } else {
        [0.0, 0.0]
    };
    let (size, scaled) = if cmd.scaled && !cmd.wrapped {
        let reference = 100.0;
        let (m, _) = t.measure(&cmd.text, reference, [0.0, 0.0]);
        let fit = (r.width() / m[0].max(1.0)).min(r.height() / m[1].max(1.0)) * reference;
        let fit = fit.clamp(1.0, 100.0);
        (if t.is_legacy() { fit.floor() } else { fit }, false)
    } else {
        (cmd.size, cmd.scaled)
    };
    let fractional = !t.is_legacy() && !scaled && size.fract() != 0.0;
    let base = if fractional {
        size.ceil().max(1.0)
    } else {
        size
    };
    let mut draw = t.draw(
        &cmd.text,
        pos,
        base,
        scaled,
        cmd.xalign,
        cmd.yalign,
        space,
        cmd.stroke.is_some(),
    );
    if fractional {
        let k = size / base;
        let anchor = pos;
        let at = |v: f32, a: f32| a + (v - a) * k;
        for q in &mut draw.quads {
            q.x = at(q.x, anchor[0]);
            q.y = at(q.y, anchor[1]);
            q.w *= k;
            q.h *= k;
            q.pen = [at(q.pen[0], anchor[0]), at(q.pen[1], anchor[1])];
            q.em *= k;
        }
        draw.size = [draw.size[0] * k, draw.size[1] * k];
    }
    draw
}
