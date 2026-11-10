use ab_glyph::{Font as _, FontRef, GlyphId, PxScale, ScaleFont};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

const FONT_SIZE_PADDING: f32 = 1.0;
pub const FONT_PADDING: f32 = 1.0;

static ARIAL: &[u8] = include_bytes!("../../../assets/content/fonts/arial.ttf");
static ARIAL_BOLD: &[u8] = include_bytes!("../../../assets/content/fonts/arialbd.ttf");
static SOURCE_SANS: &[u8] =
    include_bytes!("../../../assets/content/fonts/SourceSansPro-Regular.ttf");
static SOURCE_SANS_BOLD: &[u8] =
    include_bytes!("../../../assets/content/fonts/SourceSansPro-Bold.ttf");
static SOURCE_SANS_LIGHT: &[u8] =
    include_bytes!("../../../assets/content/fonts/SourceSansPro-Light.ttf");
static SOURCE_SANS_ITALIC: &[u8] =
    include_bytes!("../../../assets/content/fonts/SourceSansPro-It.ttf");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Font {
    Legacy,
    Arial,
    ArialBold,
    SourceSans,
    SourceSansBold,
    SourceSansLight,
    SourceSansItalic,
    Face(u16),
}

impl Font {
    pub const ALL: [Font; 7] = [
        Font::Legacy,
        Font::Arial,
        Font::ArialBold,
        Font::SourceSans,
        Font::SourceSansBold,
        Font::SourceSansLight,
        Font::SourceSansItalic,
    ];

    pub fn from_enum(value: i64) -> Option<Font> {
        Font::ALL.get(usize::try_from(value).ok()?).copied()
    }

    pub fn index(self) -> usize {
        match self {
            Font::Legacy => 0,
            Font::Arial => 1,
            Font::ArialBold => 2,
            Font::SourceSans => 3,
            Font::SourceSansBold => 4,
            Font::SourceSansLight => 5,
            Font::SourceSansItalic => 6,
            Font::Face(n) => 7 + n as usize,
        }
    }
}

pub fn font_size(value: i64) -> f32 {
    match value {
        0 => 8.0,
        1 => 9.0,
        2 => 10.0,
        3 => 11.0,
        4 => 12.0,
        5 => 14.0,
        6 => 18.0,
        7 => 24.0,
        8 => 36.0,
        9 => 48.0,
        10 => 28.0,
        11 => 32.0,
        12 => 42.0,
        13 => 60.0,
        14 => 96.0,
        _ => 12.0,
    }
}

pub const SMALLEST_SIZE: f32 = 8.0;
pub const LARGEST_SIZE: f32 = 96.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XAlign {
    Left,
    Center,
    Right,
}

impl XAlign {
    pub fn from_enum(value: i64) -> XAlign {
        match value {
            1 => XAlign::Right,
            2 => XAlign::Center,
            _ => XAlign::Left,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YAlign {
    Top,
    Center,
    Bottom,
}

impl YAlign {
    pub fn from_enum(value: i64) -> YAlign {
        match value {
            1 => YAlign::Center,
            2 => YAlign::Bottom,
            _ => YAlign::Top,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Glyph {
    pub xoffset: i32,
    pub yoffset: i32,
    pub xadvance: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Debug)]
pub struct Line {
    pub start: usize,
    pub end: usize,
    pub yoffset: i32,
    pub width: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct GlyphQuad {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub ch: char,
    pub size: u32,
    pub stroke: bool,
    pub pen: [f32; 2],
    pub em: f32,
}

#[derive(Clone, Debug, Default)]
pub struct TextDraw {
    pub quads: Vec<GlyphQuad>,
    pub alpha: f32,
    pub size: [f32; 2],
}

pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

pub struct Typesetter {
    face: FontRef<'static>,
    legacy_height_scale: f32,
    legacy_metrics: bool,
    glyphs: Mutex<HashMap<(char, u32), Glyph>>,
    ligatures: HashMap<u16, Vec<(Vec<u16>, u16)>>,
    private: HashMap<char, GlyphId>,
    private_by_glyph: HashMap<u16, char>,
}

const PRIVATE_BASE: u32 = 0xF0000;

fn read_ligatures(data: &[u8]) -> HashMap<u16, Vec<(Vec<u16>, u16)>> {
    let mut out: HashMap<u16, Vec<(Vec<u16>, u16)>> = HashMap::new();
    let Ok(face) = ttf_parser::Face::parse(data, 0) else {
        return out;
    };
    let Some(gsub) = face.tables().gsub else {
        return out;
    };
    let glyphs = face.number_of_glyphs();
    for lookup in gsub.lookups {
        for i in 0..lookup.subtables.len() {
            let Some(ttf_parser::gsub::SubstitutionSubtable::Ligature(lig)) =
                lookup
                    .subtables
                    .get::<ttf_parser::gsub::SubstitutionSubtable>(i)
            else {
                continue;
            };
            for g in 0..glyphs {
                let Some(index) = lig.coverage.get(ttf_parser::GlyphId(g)) else {
                    continue;
                };
                let Some(set) = lig.ligature_sets.get(index) else {
                    continue;
                };
                for l in set {
                    let comps: Vec<u16> = l.components.into_iter().map(|c| c.0).collect();
                    out.entry(g).or_default().push((comps, l.glyph.0));
                }
            }
        }
    }
    for list in out.values_mut() {
        list.sort_by_key(|(c, _)| std::cmp::Reverse(c.len()));
    }
    out
}

fn is_whitespace(c: char) -> bool {
    c == ' ' || c == '\t'
}

fn is_zero_width(c: char) -> bool {
    c == '\u{1}'
}

const BORDER_OFFSETS: [(f32, f32); 5] = [
    (-1.0, -1.0),
    (1.0, -1.0),
    (-1.0, 1.0),
    (1.0, 1.0),
    (0.0, 0.0),
];

fn adjust_x(x: f32, width: i32, scale: f32, align: XAlign) -> f32 {
    match align {
        XAlign::Center => x - (width / 2) as f32 * scale,
        XAlign::Right => x - width as f32 * scale,
        XAlign::Left => x,
    }
}

fn adjust_y(y: f32, height: i32, scale: f32, align: YAlign) -> f32 {
    match align {
        YAlign::Center => y - (height / 2) as f32 * scale,
        YAlign::Bottom => y - height as f32 * scale,
        YAlign::Top => y,
    }
}

impl Typesetter {
    fn new(data: &'static [u8], legacy_height_scale: f32) -> Typesetter {
        Typesetter::from_bytes(data, legacy_height_scale, true).expect("bundled font")
    }

    fn from_bytes(
        data: &'static [u8],
        legacy_height_scale: f32,
        legacy_metrics: bool,
    ) -> Option<Typesetter> {
        let face = FontRef::try_from_slice(data).ok()?;
        let ligatures = read_ligatures(data);
        let mut private = HashMap::new();
        let mut private_by_glyph = HashMap::new();
        let mut next = PRIVATE_BASE;
        for list in ligatures.values() {
            for (_, glyph) in list {
                if let std::collections::hash_map::Entry::Vacant(e) = private_by_glyph.entry(*glyph)
                    && let Some(c) = char::from_u32(next)
                {
                    e.insert(c);
                    private.insert(c, GlyphId(*glyph));
                    next += 1;
                }
            }
        }
        Some(Typesetter {
            face,
            legacy_height_scale,
            legacy_metrics,
            glyphs: Mutex::new(HashMap::new()),
            ligatures,
            private,
            private_by_glyph,
        })
    }

    fn gid(&self, ch: char) -> GlyphId {
        match self.private.get(&ch) {
            Some(g) => *g,
            None => self.face.glyph_id(ch),
        }
    }

    pub fn shape(&self, s: &str) -> Vec<char> {
        let chars: Vec<char> = s.chars().collect();
        if self.ligatures.is_empty() {
            return chars;
        }
        let ids: Vec<u16> = chars.iter().map(|c| self.face.glyph_id(*c).0).collect();
        let mut out = Vec::with_capacity(chars.len());
        let mut i = 0;
        while i < chars.len() {
            let mut matched = None;
            if let Some(list) = self.ligatures.get(&ids[i]) {
                for (comps, glyph) in list {
                    let end = i + 1 + comps.len();
                    if end <= ids.len() && ids[i + 1..end] == comps[..] {
                        matched = Some((end, *glyph));
                        break;
                    }
                }
            }
            match matched.and_then(|(end, g)| self.private_by_glyph.get(&g).map(|c| (end, *c))) {
                Some((end, c)) => {
                    out.push(c);
                    i = end;
                }
                None => {
                    out.push(chars[i]);
                    i += 1;
                }
            }
        }
        out
    }

    fn face_height(&self) -> f32 {
        if self.legacy_metrics {
            self.face.height_unscaled() + self.face.line_gap_unscaled()
        } else {
            self.face.height_unscaled()
        }
    }

    fn size_padding(&self) -> f32 {
        if self.legacy_metrics {
            FONT_SIZE_PADDING
        } else {
            0.0
        }
    }

    pub fn em(&self, size: u32) -> f32 {
        size as f32 + self.size_padding()
    }

    pub fn ascender(&self, size: u32) -> i32 {
        let mult = self.em(size) / self.face_height();
        (self.face.ascent_unscaled() * mult).round() as i32
    }

    fn scale(&self, size: u32) -> PxScale {
        PxScale::from(self.em(size))
    }

    pub fn glyph(&self, ch: char, size: u32) -> Glyph {
        let mut cache = self.glyphs.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(g) = cache.get(&(ch, size)) {
            return *g;
        }
        let scaled = self.face.as_scaled(self.scale(size));
        let id = self.gid(ch);
        let glyph = id.with_scale_and_position(self.scale(size), ab_glyph::point(0.0, 0.0));
        let mut g = Glyph {
            xadvance: scaled.h_advance(id).round() as i32,
            ..Default::default()
        };
        if let Some(outline) = self.face.outline_glyph(glyph) {
            let b = outline.px_bounds();
            g.xoffset = b.min.x as i32;
            g.yoffset = -(b.min.y as i32);
            g.width = b.width() as i32;
            g.height = b.height() as i32;
        }
        cache.insert((ch, size), g);
        g
    }

    pub fn kerning(&self, previous: char, current: char, size: u32) -> i32 {
        if previous == '\0' {
            return 0;
        }
        let mult = self.em(size) / self.face_height();
        let a: GlyphId = self.gid(previous);
        let b: GlyphId = self.gid(current);
        (self.face.kern_unscaled(a, b) * mult).round() as i32
    }

    pub fn rasterize(&self, ch: char, size: u32) -> Option<Bitmap> {
        let glyph = self
            .gid(ch)
            .with_scale_and_position(self.scale(size), ab_glyph::point(0.0, 0.0));
        let outline = self.face.outline_glyph(glyph)?;
        let b = outline.px_bounds();
        let pad = FONT_PADDING as u32;
        let width = b.width() as u32 + pad * 2;
        let height = b.height() as u32 + pad * 2;
        let mut data = vec![0u8; (width * height) as usize];
        outline.draw(|x, y, c| {
            let i = ((y + pad) * width + x + pad) as usize;
            if let Some(p) = data.get_mut(i) {
                *p = (c.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        });
        Some(Bitmap {
            width,
            height,
            data,
        })
    }

    pub fn layout(
        &self,
        text: &[char],
        mut lines: Option<&mut Vec<Line>>,
        size: u32,
        available: Option<(i32, i32)>,
        only_proper_fit: bool,
        fits: &mut bool,
    ) -> (i32, i32) {
        let height = size as i32;
        let use_available = available.is_some();
        let available = available.unwrap_or((0, 0));
        let (mut x, mut y) = (0i32, 0i32);
        let mut line_start = 0usize;
        let mut line_max = 0i32;
        let mut word_start = 0usize;
        let mut word_width = 0i32;
        let mut whitespace_width = 0i32;
        let mut previous = '\0';
        let push = |lines: &mut Option<&mut Vec<Line>>, start, end, yoffset, width| {
            if let Some(l) = lines.as_deref_mut() {
                l.push(Line {
                    start,
                    end,
                    yoffset,
                    width,
                });
            }
        };
        for (i, &c) in text.iter().enumerate() {
            if is_zero_width(c) {
                continue;
            }
            if c == '\n' {
                push(&mut lines, line_start, i, y, x);
                line_start = i + 1;
                line_max = line_max.max(x);
                word_start = i + 1;
                word_width = 0;
                whitespace_width = 0;
                x = 0;
                y += height;
                continue;
            }
            let glyph = self.glyph(c, size);
            let xoffset = self.kerning(previous, c, size) + glyph.xadvance;
            if is_whitespace(c) {
                if word_width != 0 {
                    whitespace_width = 0;
                }
                word_start = i;
                word_width = 0;
                whitespace_width += xoffset;
            } else {
                if word_width == 0 {
                    word_start = i;
                }
                word_width += xoffset;
            }
            x += xoffset;
            if use_available && x > available.0 {
                if word_width <= available.0 {
                    push(
                        &mut lines,
                        line_start,
                        word_start,
                        y,
                        x - word_width - whitespace_width,
                    );
                    line_start = word_start;
                    line_max = line_max.max(x - word_width - whitespace_width);
                    x = word_width;
                } else {
                    if only_proper_fit {
                        *fits = false;
                        return (line_max, y);
                    }
                    push(&mut lines, line_start, i, y, x - xoffset);
                    line_start = i;
                    line_max = line_max.max(x - xoffset);
                    x = xoffset;
                }
                word_width = x;
                whitespace_width = 0;
                y += height;
            }
            previous = c;
        }
        push(&mut lines, line_start, text.len(), y, x);
        line_max = line_max.max(x);
        y += height;
        if use_available && y > available.1 {
            let count = (available.1 / height.max(1)).max(1);
            if let Some(l) = lines {
                l.resize(
                    count as usize,
                    Line {
                        start: text.len(),
                        end: text.len(),
                        yoffset: 0,
                        width: 0,
                    },
                );
            }
            *fits = false;
            (line_max, count * height)
        } else {
            *fits = true;
            (line_max, y)
        }
    }

    fn height_for(&self, size: f32) -> u32 {
        (size * self.legacy_height_scale).max(0.0) as u32
    }

    pub fn measure(&self, s: &str, size: f32, available: [f32; 2]) -> ([f32; 2], bool) {
        let text: Vec<char> = self.shape(s);
        let use_available = available[0] != 0.0;
        let height = self.height_for(size);
        let avail = use_available.then_some((available[0] as i32, available[1] as i32));
        let mut fits = false;
        let (w, h) = self.layout(&text, None, height, avail, true, &mut fits);
        let (w, h) = if !fits {
            self.layout(&text, None, height, avail, false, &mut fits)
        } else {
            (w, h)
        };
        ([w as f32, h as f32], fits)
    }

    fn line_width(&self, text: &[char], line: &Line, size: u32) -> f32 {
        let mut x = 0;
        let mut previous = '\0';
        for &c in &text[line.start..line.end] {
            x += self.kerning(previous, c, size) + self.glyph(c, size).xadvance;
            previous = c;
        }
        x as f32
    }

    fn layout_ratio(
        &self,
        text: &[char],
        lines: &mut Vec<Line>,
        size: u32,
        available: [f32; 2],
    ) -> (i32, i32) {
        let ratio = available[0] / available[1].max(1e-3);
        let height = size as f32;
        let mut min = 10.0f32;
        let mut max = height * text.len() as f32;
        max = max.max(height * ratio * 1.01);
        let mut x = max;
        let mut best_ratio = f32::MAX;
        let mut best_lines: Vec<Line> = lines.clone();
        let mut best_size = (0, 0);
        for _ in 0..10 {
            let y = x / ratio;
            let space = (x as i32, y as i32);
            lines.clear();
            let mut fits = false;
            let (w, h) = self.layout(text, Some(lines), size, Some(space), true, &mut fits);
            let actual = w as f32 / h.max(1) as f32;
            if fits {
                if (actual - ratio).abs() < (best_ratio - ratio).abs() {
                    best_lines = lines.clone();
                    best_ratio = actual;
                    best_size = (w, h);
                }
                max = x;
                x = min + (x - min) / 2.0;
                if actual >= ratio - 0.05 && actual <= ratio + 0.05 {
                    break;
                }
            } else {
                min = x;
                x += (max - x) / 2.0;
            }
            if max - x <= 0.0 || x - min <= 0.0 {
                break;
            }
        }
        *lines = best_lines;
        best_size
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &self,
        text: &[char],
        lines: &[Line],
        xalign: XAlign,
        scale: f32,
        measure_size: u32,
        render_size: u32,
        ascender: i32,
        start: (f32, f32),
        stroke: bool,
        out: &mut Vec<GlyphQuad>,
    ) {
        let first = if stroke { 0 } else { 4 };
        for (index, (dx, dy)) in BORDER_OFFSETS.iter().enumerate().skip(first) {
            let is_stroke = index != 4;
            let mut previous = '\0';
            for line in lines {
                let line_x = adjust_x(start.0, line.width, scale, xalign);
                let line_y = start.1 + line.yoffset as f32 * scale;
                let mut x = 0i32;
                for &c in &text[line.start.min(text.len())..line.end.min(text.len())] {
                    if is_zero_width(c) {
                        continue;
                    }
                    let glyph = self.glyph(c, measure_size);
                    x += self.kerning(previous, c, measure_size);
                    if !is_whitespace(c) && glyph.width > 0 && glyph.height > 0 {
                        out.push(GlyphQuad {
                            x: line_x + dx + (x + glyph.xoffset) as f32 * scale - FONT_PADDING,
                            y: line_y + dy + (ascender - glyph.yoffset) as f32 * scale
                                - FONT_PADDING,
                            w: glyph.width as f32 * scale + FONT_PADDING * 2.0,
                            h: glyph.height as f32 * scale + FONT_PADDING * 2.0,
                            ch: c,
                            size: render_size,
                            stroke: is_stroke,
                            pen: [
                                line_x + dx + x as f32 * scale,
                                line_y + dy + ascender as f32 * scale,
                            ],
                            em: self.em(measure_size) * scale,
                        });
                    }
                    x += glyph.xadvance;
                    previous = c;
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        s: &str,
        position: [f32; 2],
        size: f32,
        auto_scale: bool,
        xalign: XAlign,
        yalign: YAlign,
        available: [f32; 2],
        stroke: bool,
    ) -> TextDraw {
        let text: Vec<char> = self.shape(s);
        let use_available = available[0] != 0.0;
        let mut lines = Vec::new();
        let mut out = TextDraw {
            alpha: 1.0,
            ..Default::default()
        };
        if auto_scale && use_available {
            let reference = 48u32;
            let (w, h) = self.layout_ratio(&text, &mut lines, reference, available);
            let ratio = (available[0] / w.max(1) as f32)
                .min(available[1] / h.max(1) as f32)
                .min(1.0);
            let ascender = self.ascender(reference);
            let height = reference as f32 * ratio;
            let rendered = ((height / 6.0).ceil() * 6.0).max(6.0) as u32;
            let measured_w = lines
                .iter()
                .map(|l| self.line_width(&text, l, reference))
                .fold(0.0f32, f32::max);
            let measured_h = (lines.len() as u32 * reference) as f32;
            out.size = [measured_w * ratio, measured_h * ratio];
            out.alpha = (-2.0 + height * 0.5).clamp(0.0, 1.0);
            if out.alpha == 0.0 {
                return out;
            }
            let start_y = adjust_y(position[1], measured_h as i32, ratio, yalign);
            self.emit(
                &text,
                &lines,
                xalign,
                ratio,
                reference,
                rendered,
                ascender,
                (position[0], start_y),
                stroke,
                &mut out.quads,
            );
            return out;
        }
        let height = self.height_for(size);
        let mut fits = false;
        let (w, h) = self.layout(
            &text,
            Some(&mut lines),
            height,
            use_available.then_some((available[0] as i32, available[1] as i32)),
            false,
            &mut fits,
        );
        let ascender = self.ascender(height);
        let start_x = position[0].round();
        let start_y = adjust_y(position[1].round(), h, 1.0, yalign);
        self.emit(
            &text,
            &lines,
            xalign,
            1.0,
            height,
            height,
            ascender,
            (start_x, start_y),
            stroke,
            &mut out.quads,
        );
        out.size = [w as f32, h as f32];
        out
    }

    pub fn scaled_font_size(&self, s: &str, wrap: bool, rect: [f32; 2]) -> f32 {
        let mut biggest = SMALLEST_SIZE;
        let mut min = biggest;
        let mut max = LARGEST_SIZE;
        while biggest <= max {
            let mid = min + ((max - min) / 2.0).floor();
            let (_, fits) = self.measure(s, mid, if wrap { rect } else { [0.0, 0.0] });
            if fits && biggest < mid {
                biggest = mid;
                min = mid + 1.0;
            } else {
                max = mid - 1.0;
            }
        }
        biggest
    }
}

static TYPESETTERS: LazyLock<[Typesetter; 7]> = LazyLock::new(|| {
    [
        Typesetter::new(ARIAL, 1.5),
        Typesetter::new(ARIAL, 1.0),
        Typesetter::new(ARIAL_BOLD, 1.0),
        Typesetter::new(SOURCE_SANS, 1.0),
        Typesetter::new(SOURCE_SANS_BOLD, 1.0),
        Typesetter::new(SOURCE_SANS_LIGHT, 1.0),
        Typesetter::new(SOURCE_SANS_ITALIC, 1.0),
    ]
});

type Loader = Box<dyn Fn(&str) -> Option<Vec<u8>> + Send + Sync>;

static LOADER: std::sync::OnceLock<Loader> = std::sync::OnceLock::new();
static FACES: LazyLock<Mutex<Vec<&'static Typesetter>>> = LazyLock::new(|| Mutex::new(Vec::new()));
static FACE_KEYS: LazyLock<Mutex<HashMap<String, Option<Font>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn set_loader(loader: impl Fn(&str) -> Option<Vec<u8>> + Send + Sync + 'static) {
    let _ = LOADER.set(Box::new(loader));
}

fn load_file(path: &str) -> Option<Font> {
    let bytes = (LOADER.get()?)(path)?;
    let data: &'static [u8] = Box::leak(bytes.into_boxed_slice());
    let t: &'static Typesetter = Box::leak(Box::new(Typesetter::from_bytes(data, 1.0, false)?));
    let mut faces = FACES.lock().ok()?;
    faces.push(t);
    Some(Font::Face((faces.len() - 1) as u16))
}

pub fn face(family: &str, weight: u16, italic: bool) -> Option<Font> {
    let key = format!("{family}|{weight}|{italic}");
    if let Some(found) = FACE_KEYS.lock().ok()?.get(&key) {
        return *found;
    }
    let resolved = (|| {
        let json = (LOADER.get()?)(family)?;
        let v: serde_json::Value = serde_json::from_slice(&json).ok()?;
        let faces = v.get("faces")?.as_array()?;
        let best = faces
            .iter()
            .filter_map(|f| {
                let w = f.get("weight")?.as_u64()? as i32;
                let style_italic = f.get("style")?.as_str()? == "italic";
                let asset = f.get("assetId")?.as_str()?;
                let penalty =
                    (w - weight as i32).abs() + if style_italic == italic { 0 } else { 10000 };
                Some((penalty, asset.to_string()))
            })
            .min_by_key(|(p, _)| *p)?;
        let file_key = format!("file|{}", best.1);
        if let Some(found) = FACE_KEYS.lock().ok()?.get(&file_key) {
            return *found;
        }
        let font = load_file(&best.1);
        FACE_KEYS.lock().ok()?.insert(file_key, font);
        font
    })();
    FACE_KEYS.lock().ok()?.insert(key, resolved);
    resolved
}

pub fn typesetter(font: Font) -> &'static Typesetter {
    if let Font::Face(n) = font
        && let Some(t) = FACES.lock().ok().and_then(|f| f.get(n as usize).copied())
    {
        return t;
    }
    &TYPESETTERS[font.index().min(6)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_single_line() {
        let t = typesetter(Font::SourceSans);
        let (size, fits) = t.measure("Hello", 18.0, [0.0, 0.0]);
        assert!(fits);
        assert_eq!(size[1], 18.0);
        assert!(size[0] > 20.0 && size[0] < 60.0, "{size:?}");
    }

    #[test]
    fn wraps_words() {
        let t = typesetter(Font::Arial);
        let (one, _) = t.measure("hello world", 14.0, [0.0, 0.0]);
        let (wrapped, fits) = t.measure("hello world", 14.0, [one[0] - 5.0, 100.0]);
        assert!(fits);
        assert_eq!(wrapped[1], 28.0);
    }

    #[test]
    fn legacy_is_taller() {
        let (size, _) = typesetter(Font::Legacy).measure("A", 10.0, [0.0, 0.0]);
        assert_eq!(size[1], 15.0);
    }

    #[test]
    fn rasterizes_glyphs() {
        let b = typesetter(Font::SourceSansBold)
            .rasterize('W', 24)
            .expect("glyph");
        assert!(b.width > 10 && b.height > 10);
        assert!(b.data.iter().any(|&p| p > 200));
    }
}
