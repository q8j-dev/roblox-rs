use rbx_dom::gui::{Placement, canvas, place_child, placement};
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

#[derive(Clone, Debug)]
pub struct GuiItem {
    pub cmd: GuiCmd,
    pub clip: Option<Rect>,
    pub rotation: f32,
    pub pivot: Vec2,
}

fn get_bool(dm: &DataModel, id: InstanceId, name: &str, default: bool) -> bool {
    match dm.get(id, name) {
        Ok(Variant::Bool(b)) => b,
        _ => default,
    }
}

fn get_f32(dm: &DataModel, id: InstanceId, name: &str) -> f32 {
    match dm.get(id, name) {
        Ok(Variant::Float(v)) => v,
        Ok(Variant::Double(v)) => v as f32,
        Ok(Variant::Int(v)) => v as f32,
        _ => 0.0,
    }
}

fn get_i64(dm: &DataModel, id: InstanceId, name: &str) -> i64 {
    match dm.get(id, name) {
        Ok(Variant::Int(v)) => v as i64,
        Ok(Variant::Int64(v)) => v,
        Ok(Variant::Enum(v)) => v as i64,
        Ok(Variant::Float(v)) => v as i64,
        _ => 0,
    }
}

fn get_color(dm: &DataModel, id: InstanceId, name: &str) -> [f32; 3] {
    match dm.get(id, name) {
        Ok(Variant::Color3(c)) => [c.r, c.g, c.b],
        Ok(Variant::BrickColor(b)) => {
            let c = b.color3();
            [c.r, c.g, c.b]
        }
        _ => [1.0, 1.0, 1.0],
    }
}

fn get_string(dm: &DataModel, id: InstanceId, name: &str) -> String {
    match dm.get(id, name) {
        Ok(Variant::String(s)) | Ok(Variant::Content(s)) => s,
        _ => String::new(),
    }
}

fn get_vec2(dm: &DataModel, id: InstanceId, name: &str) -> Vec2 {
    match dm.get(id, name) {
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
        let p = place_child(dm, id, parent);
        let z = get_i64(dm, id, "ZIndex").clamp(0, 10) as usize;
        let entry = Entry {
            id,
            placement: p,
            clip,
        };
        if is_text(class) {
            self.layers[z].1.push(entry);
        } else {
            self.layers[z].0.push(entry);
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
        for &c in dm.children_of(id) {
            self.load(c, child_parent, child_clip);
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
    let url = get_string(dm, id, "Image");
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
        let center = match dm.get(id, "SliceCenter") {
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
    let s = get_string(dm, id, "Text");
    if alpha <= 0.0 || s.is_empty() {
        return;
    }
    let stroke_alpha = (1.0 - get_f32(dm, id, "TextStrokeTransparency")) * alpha;
    out.push(GuiCmd::Text(TextCmd {
        text: s,
        font: rbx_text::Font::from_enum(get_i64(dm, id, "Font")).unwrap_or(rbx_text::Font::Legacy),
        size: rbx_text::font_size(get_i64(dm, id, "FontSize")),
        scaled: get_bool(dm, id, "TextScaled", false),
        wrapped: get_bool(dm, id, "TextWrapped", false) || get_bool(dm, id, "TextWrap", false),
        xalign: rbx_text::XAlign::from_enum(get_i64(dm, id, "TextXAlignment")),
        yalign: rbx_text::YAlign::from_enum(get_i64(dm, id, "TextYAlignment")),
        rect,
        color: rgba(get_color(dm, id, "TextColor3"), alpha),
        stroke: (stroke_alpha > 0.05)
            .then(|| rgba(get_color(dm, id, "TextStrokeColor3"), stroke_alpha)),
    }));
}

fn screen_guis(dm: &DataModel) -> Vec<InstanceId> {
    let mut roots = Vec::new();
    let push_from = |parent: InstanceId, roots: &mut Vec<InstanceId>| {
        for &c in dm.children_of(parent) {
            if matches!(dm.class_of(c), Some("ScreenGui" | "GuiMain")) {
                roots.push(c);
            }
        }
    };
    let player_gui = dm
        .local_player
        .and_then(|p| dm.find_first_child_of_class(p, "PlayerGui"));
    match player_gui {
        Some(pg) => push_from(pg, &mut roots),
        None => {
            if let Some(sg) = dm.find_service("StarterGui")
                && get_bool(dm, sg, "ShowDevelopmentGui", true)
            {
                push_from(sg, &mut roots);
            }
        }
    }
    if let Some(core) = dm.find_service("CoreGui") {
        push_from(core, &mut roots);
    }
    roots
}

pub fn extract(dm: &DataModel) -> Vec<GuiItem> {
    let mut items = Vec::new();
    for root in screen_guis(dm) {
        let base = placement(dm, root);
        let mut c = Collector {
            dm,
            layers: (0..=10).map(|_| (Vec::new(), Vec::new())).collect(),
        };
        for &child in dm.children_of(root) {
            c.load(child, base, None);
        }
        for (general, texts) in &c.layers {
            let mut push = |e: &Entry, cmds: Vec<GuiCmd>| {
                for cmd in cmds {
                    items.push(GuiItem {
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
        }
    }
    messages(dm, &mut items);
    items
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
                    cmd: GuiCmd::Rect {
                        rect: inner,
                        color: [0.5, 0.5, 0.5, 0.5],
                    },
                    clip: None,
                    rotation: 0.0,
                    pivot: Vec2::ZERO,
                });
                items.push(GuiItem {
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
    t.draw(
        &cmd.text,
        pos,
        cmd.size,
        cmd.scaled,
        cmd.xalign,
        cmd.yalign,
        space,
        cmd.stroke.is_some(),
    )
}
