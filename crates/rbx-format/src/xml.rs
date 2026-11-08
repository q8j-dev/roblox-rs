use crate::FormatError;
use crate::text::{format_f32, format_f64, parse_bool, parse_f32, parse_f64, parse_i32, parse_i64};
use crate::tree::{Tree, TreeInstance};
use base64::Engine;
use rbx_math::{
    Axes, BrickColor, CFrame, Color3, Color3uint8, ColorSequence, ColorSequenceKeypoint, Faces,
    NumberRange, NumberSequence, NumberSequenceKeypoint, PhysicalProperties, Ray, Rect2D,
    Region3int16, UDim, UDim2, Vec2, Vec3, Vector2int16, Vector3int16,
};
use rbx_reflection::Variant;
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct Element {
    pub tag: String,
    pub attributes: Vec<(String, String)>,
    pub text: Option<String>,
    pub raw: Option<Vec<u8>>,
    pub children: Vec<Element>,
}

impl Element {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    fn child(&self, tag: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.tag == tag)
    }

    fn is_nil(&self) -> bool {
        self.attr("xsi:nil").and_then(parse_bool).unwrap_or(false)
    }

    fn text(&self) -> &str {
        self.text.as_deref().unwrap_or("")
    }

    fn child_f32(&self, tag: &str) -> f32 {
        self.child(tag)
            .and_then(|c| parse_f32(c.text()))
            .unwrap_or(0.0)
    }

    fn child_i32(&self, tag: &str) -> i32 {
        self.child(tag)
            .and_then(|c| parse_i32(c.text()))
            .unwrap_or(0)
    }
}

fn is_ws(c: u8) -> bool {
    matches!(c, b'\n' | b'\t' | b' ' | b'\r' | 0x0c)
}

struct Parser<'a> {
    data: &'a [u8],
    pos: usize,
}

fn decode_entities(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len());
    let mut i = 0;
    while i < src.len() {
        let c = src[i];
        i += 1;
        if c != b'&' {
            out.push(c);
            continue;
        }
        let start = i;
        while i < src.len() && src[i] != b';' {
            i += 1;
        }
        let entity = &src[start..i];
        if i < src.len() {
            i += 1;
        }
        match entity {
            b"lt" => out.push(b'<'),
            b"gt" => out.push(b'>'),
            b"amp" => out.push(b'&'),
            b"quot" => out.push(b'"'),
            b"apos" => out.push(b'\''),
            b"nbsp" => out.push(b' '),
            e if e.first() == Some(&b'#') => {
                let body = std::str::from_utf8(&e[1..]).unwrap_or("");
                let code =
                    if let Some(hex) = body.strip_prefix('x').or_else(|| body.strip_prefix('X')) {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        body.parse::<u32>().ok()
                    };
                match code {
                    Some(c) if c < 256 => out.push(c as u8),
                    Some(c) => {
                        if let Some(ch) = char::from_u32(c) {
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                    }
                    None => {}
                }
            }
            other => {
                out.push(b'&');
                out.extend_from_slice(other);
                out.push(b';');
            }
        }
    }
    out
}

fn bytes_to_string(b: Vec<u8>) -> String {
    match String::from_utf8(b) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    }
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if !is_ws(c) {
                break;
            }
            self.pos += 1;
        }
    }

    fn read_first_tag(&mut self) -> Result<&'a [u8], FormatError> {
        self.skip_ws();
        let mut count = 0;
        loop {
            let Some(c) = self.peek() else {
                return Err(FormatError::Xml("expected '<' but got EOF".into()));
            };
            count += 1;
            if count > 5 {
                return Err(FormatError::Xml(
                    "tag expected after byte order mark".into(),
                ));
            }
            self.pos += 1;
            if c == b'<' {
                break;
            }
        }
        let start = self.pos - 1;
        while let Some(c) = self.peek() {
            self.pos += 1;
            if c == b'>' {
                return Ok(&self.data[start..self.pos]);
            }
        }
        Err(FormatError::Xml("expected '>' but got EOF".into()))
    }

    fn read_tag(&mut self) -> Result<&'a [u8], FormatError> {
        self.skip_ws();
        if self.peek() != Some(b'<') {
            return Err(FormatError::Xml(if self.peek().is_none() {
                "EOF encountered while reading tag start".into()
            } else {
                "tag expected".into()
            }));
        }
        let start = self.pos;
        self.pos += 1;
        while let Some(c) = self.peek() {
            self.pos += 1;
            if c == b'>' {
                return Ok(&self.data[start..self.pos]);
            }
        }
        Err(FormatError::Xml("EOF encountered while reading tag".into()))
    }

    fn read_text(&mut self, decode: bool) -> String {
        bytes_to_string(self.read_text_raw(decode))
    }

    fn read_text_raw(&mut self, decode: bool) -> Vec<u8> {
        if self.data[self.pos..].starts_with(b"<![CDATA[") {
            let start = self.pos + 9;
            let end = self.data[start..]
                .windows(3)
                .position(|w| w == b"]]>")
                .map(|p| start + p);
            let (content, after) = match end {
                Some(e) => (&self.data[start..e], e + 3),
                None => (&self.data[start..], self.data.len()),
            };
            self.pos = after;
            while let Some(c) = self.peek() {
                if c == b'<' {
                    break;
                }
                self.pos += 1;
            }
            return content.to_vec();
        }
        self.skip_ws();
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == b'<' {
                break;
            }
            self.pos += 1;
        }
        let raw = &self.data[start..self.pos];
        if decode && raw.contains(&b'&') {
            decode_entities(raw)
        } else {
            raw.to_vec()
        }
    }

    fn parse_attributes(tag: &[u8]) -> Result<Element, FormatError> {
        let inner = &tag[1..tag.len() - 1];
        let inner = inner.strip_suffix(b"/").unwrap_or(inner);
        let mut i = 0;
        while i < inner.len() && is_ws(inner[i]) {
            i += 1;
        }
        let name_start = i;
        while i < inner.len() && !is_ws(inner[i]) {
            i += 1;
        }
        let mut element = Element {
            tag: bytes_to_string(inner[name_start..i].to_vec()),
            ..Default::default()
        };
        loop {
            while i < inner.len() && is_ws(inner[i]) {
                i += 1;
            }
            if i >= inner.len() {
                break;
            }
            let Some(eq) = inner[i..].iter().position(|&c| c == b'=').map(|p| p + i) else {
                return Err(FormatError::Xml(
                    "unable to parse attributes, '=' not found".into(),
                ));
            };
            let key = bytes_to_string(inner[i..eq].to_vec()).trim().to_string();
            let Some(q1) = inner[eq..].iter().position(|&c| c == b'"').map(|p| p + eq) else {
                return Err(FormatError::Xml(
                    "unable to parse attributes, '\"' not found".into(),
                ));
            };
            let Some(q2) = inner[q1 + 1..]
                .iter()
                .position(|&c| c == b'"')
                .map(|p| p + q1 + 1)
            else {
                return Err(FormatError::Xml(
                    "unable to parse attributes, '\"' not found".into(),
                ));
            };
            let raw = &inner[q1 + 1..q2];
            let value = if raw.contains(&b'&') {
                bytes_to_string(decode_entities(raw))
            } else {
                bytes_to_string(raw.to_vec())
            };
            element.attributes.push((key, value));
            i = q2 + 1;
        }
        Ok(element)
    }

    fn parse(&mut self) -> Result<Element, FormatError> {
        if self.data.is_empty() {
            return Err(FormatError::Xml("empty file".into()));
        }
        let mut stack: Vec<Element> = Vec::new();
        let mut first = true;
        loop {
            let tag = if first {
                first = false;
                let t = self.read_first_tag()?;
                if t.starts_with(b"<?") {
                    continue;
                }
                t
            } else {
                self.read_tag()?
            };
            if tag.starts_with(b"</") {
                let Some(done) = stack.pop() else {
                    return Err(FormatError::Xml("close tag without open tag".into()));
                };
                match stack.last_mut() {
                    Some(parent) => parent.children.push(done),
                    None => return Ok(done),
                }
                continue;
            }
            if tag.starts_with(b"<!--") {
                let mut t = tag;
                while !t.ends_with(b"-->") {
                    t = self.read_tag()?;
                }
                continue;
            }
            let self_closing = tag.ends_with(b"/>");
            let mut element = Self::parse_attributes(tag)?;
            if element.tag == "Content" && !self_closing {
                if element.is_nil() {
                } else {
                    let leading = self.read_text(false);
                    if !leading.is_empty() {
                    } else {
                        let child = self.read_tag()?;
                        let child_self_closing = child.ends_with(b"/>");
                        let name = String::from_utf8_lossy(&child[1..child.len() - 1]).to_string();
                        let name = name.trim_end_matches('/').to_string();
                        if name.starts_with("binary") || name == "hash" {
                            self.read_text(false);
                            element.text = Some(String::new());
                        } else if name.starts_with("url") {
                            element.text = Some(if child_self_closing {
                                String::new()
                            } else {
                                self.read_text(true)
                            });
                        } else if name == "null" {
                            element.text = Some(String::new());
                        } else {
                            return Err(FormatError::Xml(format!(
                                "unknown tag '{}'",
                                name.chars().take(32).collect::<String>()
                            )));
                        }
                        if !child_self_closing {
                            let closing = self.read_tag()?;
                            if !closing.starts_with(b"</") {
                                return Err(FormatError::Xml("expected a closing tag".into()));
                            }
                        }
                    }
                }
            } else if !self_closing {
                match String::from_utf8(self.read_text_raw(true)) {
                    Ok(t) => element.text = Some(t),
                    Err(e) => {
                        let bytes = e.into_bytes();
                        element.text = Some(String::from_utf8_lossy(&bytes).into_owned());
                        element.raw = Some(bytes);
                    }
                }
            }
            if self_closing {
                match stack.last_mut() {
                    Some(parent) => parent.children.push(element),
                    None => return Ok(element),
                }
            } else {
                stack.push(element);
            }
        }
    }
}

pub fn parse_document(data: &[u8]) -> Result<Element, FormatError> {
    Parser { data, pos: 0 }.parse()
}

enum Pending {
    Value(Variant),
    Ref(String),
}

fn vec3_of(e: &Element) -> Option<Vec3> {
    if e.is_nil() {
        return None;
    }
    e.child("X")?;
    Some(Vec3::new(
        e.child_f32("X"),
        e.child_f32("Y"),
        e.child_f32("Z"),
    ))
}

fn v3i16_of(e: &Element) -> Vector3int16 {
    Vector3int16::new(
        e.child_i32("X") as i16,
        e.child_i32("Y") as i16,
        e.child_i32("Z") as i16,
    )
}

fn parse_number_list(text: &str) -> Vec<f32> {
    text.split_ascii_whitespace().map_while(parse_f32).collect()
}

fn property_value(e: &Element) -> Option<Pending> {
    if e.is_nil() {
        return None;
    }
    let t = e.text();
    let v = match e.tag.as_str() {
        "string" => match &e.raw {
            Some(raw) => Variant::BinaryString(raw.clone()),
            None => Variant::String(t.to_string()),
        },
        "ProtectedString" => Variant::ProtectedString(t.to_string()),
        "Content" => Variant::Content(e.text.clone()?),
        "bool" => Variant::Bool(parse_bool(t)?),
        "int" => Variant::Int(parse_i32(t)?),
        "int64" | "long" => Variant::Int64(parse_i64(t)?),
        "float" => Variant::Float(parse_f32(t)?),
        "double" => Variant::Double(parse_f64(t)?),
        "token" => Variant::Enum(parse_i64(t)? as u32),
        "BrickColor" => Variant::BrickColor(BrickColor::new(parse_i64(t)?)),
        "Ref" => return Some(Pending::Ref(t.to_string())),
        "Vector3" => Variant::Vector3(vec3_of(e)?),
        "Vector2" => {
            e.child("X")?;
            Variant::Vector2(Vec2::new(e.child_f32("X"), e.child_f32("Y")))
        }
        "Vector3int16" => Variant::Vector3int16(v3i16_of(e)),
        "Vector2int16" => Variant::Vector2int16(Vector2int16 {
            x: e.child_i32("X") as i16,
            y: e.child_i32("Y") as i16,
        }),
        "CoordinateFrame" | "CFrame" => {
            let r = [
                e.child_f32("R00"),
                e.child_f32("R01"),
                e.child_f32("R02"),
                e.child_f32("R10"),
                e.child_f32("R11"),
                e.child_f32("R12"),
                e.child_f32("R20"),
                e.child_f32("R21"),
                e.child_f32("R22"),
            ];
            Variant::CFrame(CFrame::from_rows(
                r,
                Vec3::new(e.child_f32("X"), e.child_f32("Y"), e.child_f32("Z")),
            ))
        }
        "Color3" => {
            if let Some(argb) = parse_i64(t).filter(|_| !t.trim().is_empty()) {
                Variant::Color3(Color3uint8::from_argb(argb as u32).to_color3())
            } else if e.child("R").is_some() && e.child("G").is_some() && e.child("B").is_some() {
                Variant::Color3(Color3::new(
                    e.child_f32("R"),
                    e.child_f32("G"),
                    e.child_f32("B"),
                ))
            } else {
                return None;
            }
        }
        "Color3uint8" => Variant::Color3(Color3uint8::from_argb(parse_i64(t)? as u32).to_color3()),
        "UDim" => Variant::UDim(UDim::new(e.child_f32("S"), e.child_i32("O"))),
        "UDim2" => Variant::UDim2(UDim2::new(
            e.child_f32("XS"),
            e.child_i32("XO"),
            e.child_f32("YS"),
            e.child_i32("YO"),
        )),
        "Ray" => Variant::Ray(Ray::new(
            vec3_of(e.child("origin")?)?,
            vec3_of(e.child("direction")?)?,
        )),
        "Faces" => Variant::Faces(Faces(e.child_i32("faces") as u8)),
        "Axes" => Variant::Axes(Axes(e.child_i32("axes") as u8)),
        "Rect2D" => {
            let min = e.child("min")?;
            let max = e.child("max")?;
            Variant::Rect2D(Rect2D::xyxy(
                min.child_f32("X"),
                min.child_f32("Y"),
                max.child_f32("X"),
                max.child_f32("Y"),
            ))
        }
        "Region3int16" => Variant::Region3int16(Region3int16 {
            min: v3i16_of(e.child("min")?),
            max: v3i16_of(e.child("max")?),
        }),
        "PhysicalProperties" => {
            let custom = e
                .child("CustomPhysics")
                .and_then(|c| parse_bool(c.text()))
                .unwrap_or(false);
            if custom {
                let get =
                    |tag: &str, d: f32| e.child(tag).and_then(|c| parse_f32(c.text())).unwrap_or(d);
                Variant::PhysicalProperties(PhysicalProperties::custom(
                    get("Density", 1.0),
                    get("Friction", 0.0),
                    get("Elasticity", 0.0),
                    get("FrictionWeight", 1.0),
                    get("ElasticityWeight", 1.0),
                ))
            } else {
                Variant::PhysicalProperties(PhysicalProperties::default())
            }
        }
        "NumberSequence" => {
            let n = parse_number_list(t);
            Variant::NumberSequence(NumberSequence {
                keypoints: n
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|c| NumberSequenceKeypoint {
                        time: c[0],
                        value: c[1],
                        envelope: c[2],
                    })
                    .collect(),
            })
        }
        "ColorSequence" => {
            let n = parse_number_list(t);
            Variant::ColorSequence(ColorSequence {
                keypoints: n
                    .as_chunks::<5>()
                    .0
                    .iter()
                    .map(|c| ColorSequenceKeypoint {
                        time: c[0],
                        value: Color3::new(c[1], c[2], c[3]),
                        envelope: c[4],
                    })
                    .collect(),
            })
        }
        "NumberRange" => {
            let n = parse_number_list(t);
            Variant::NumberRange(NumberRange::new(
                n.first().copied().unwrap_or(0.0),
                n.get(1).copied().unwrap_or(0.0),
            ))
        }
        "BinaryString" => {
            let cleaned: String = t.chars().filter(|c| !c.is_whitespace()).collect();
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(cleaned.as_bytes())
                .or_else(|_| {
                    base64::engine::general_purpose::STANDARD_NO_PAD
                        .decode(cleaned.trim_end_matches('=').as_bytes())
                })
                .unwrap_or_default();
            Variant::BinaryString(bytes)
        }
        _ => return None,
    };
    Some(Pending::Value(v))
}

pub fn read_xml(data: &[u8]) -> Result<Tree, FormatError> {
    let root = parse_document(data)?;
    if root.tag != "roblox" {
        return Err(FormatError::Xml("ill-formed XML, no roblox tag".into()));
    }
    let mut tree = Tree::new();
    let Some(version) = root.attr("version") else {
        return Ok(tree);
    };
    match parse_i32(version) {
        Some(v) if v >= 4 => {}
        Some(_) => return Err(FormatError::Xml("schema version is older than 4".into())),
        None => return Err(FormatError::Xml("no version number".into())),
    }
    let mut referents: HashMap<String, usize> = HashMap::new();
    let mut pending_refs: Vec<(usize, String, String)> = Vec::new();
    let mut stack: Vec<(&Element, Option<usize>)> = root
        .children
        .iter()
        .rev()
        .filter(|c| c.tag == "Item")
        .map(|c| (c, None))
        .collect();
    while let Some((item, parent)) = stack.pop() {
        let class = item.attr("class").unwrap_or("").to_string();
        let mut instance = TreeInstance::new(class);
        let mut refs = Vec::new();
        if let Some(props) = item.child("Properties") {
            for p in &props.children {
                let Some(name) = p.attr("name") else { continue };
                match property_value(p) {
                    Some(Pending::Value(v)) => instance.set(name, v),
                    Some(Pending::Ref(r)) => refs.push((name.to_string(), r)),
                    None => {}
                }
            }
        }
        let index = tree.push(instance, parent);
        if let Some(r) = item.attr("referent") {
            referents.insert(r.to_string(), index);
        }
        for (name, r) in refs {
            pending_refs.push((index, name, r));
        }
        for child in item.children.iter().rev().filter(|c| c.tag == "Item") {
            stack.push((child, Some(index)));
        }
    }
    for (index, name, r) in pending_refs {
        let target = match r.as_str() {
            "null" | "nil" | "" => None,
            other => referents.get(other).copied().map(Tree::id_of),
        };
        tree.instances[index].set(&name, Variant::Ref(target));
    }
    Ok(tree)
}

fn encode_text(out: &mut String, text: &str, allow_cdata: bool) {
    if allow_cdata && text.contains('\n') && !text.contains("]]>") {
        out.push_str("<![CDATA[");
        out.push_str(text);
        out.push_str("]]>");
        return;
    }
    let mut leading = true;
    for ch in text.chars() {
        if leading && (ch == ' ' || ch == '\t' || ch == '\n' || ch == '\r' || ch == '\u{c}') {
            out.push_str(&format!("&#{};", ch as u32));
            continue;
        }
        leading = false;
        match ch {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if (c as u32) < 32 && c != '\n' && c != '\r' => {
                out.push_str(&format!("&#{};", c as u32))
            }
            c => out.push(c),
        }
    }
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push('\t');
    }
}

fn simple(out: &mut String, depth: usize, tag: &str, name: &str, value: &str, cdata: bool) {
    indent(out, depth);
    out.push('<');
    out.push_str(tag);
    out.push_str(" name=\"");
    encode_text(out, name, false);
    out.push_str("\">");
    encode_text(out, value, cdata);
    out.push_str("</");
    out.push_str(tag);
    out.push_str(">\n");
}

fn compound(out: &mut String, depth: usize, tag: &str, name: &str, fields: &[(&str, String)]) {
    indent(out, depth);
    out.push('<');
    out.push_str(tag);
    out.push_str(" name=\"");
    encode_text(out, name, false);
    out.push_str("\">\n");
    for (k, v) in fields {
        indent(out, depth + 1);
        out.push_str(&format!("<{k}>{v}</{k}>\n"));
    }
    indent(out, depth);
    out.push_str(&format!("</{tag}>\n"));
}

fn vec3_fields(v: Vec3) -> Vec<(&'static str, String)> {
    vec![
        ("X", format_f32(v.x)),
        ("Y", format_f32(v.y)),
        ("Z", format_f32(v.z)),
    ]
}

fn nested(
    out: &mut String,
    depth: usize,
    tag: &str,
    name: &str,
    groups: &[(&str, Vec<(&str, String)>)],
) {
    indent(out, depth);
    out.push_str(&format!("<{tag} name=\""));
    encode_text(out, name, false);
    out.push_str("\">\n");
    for (g, fields) in groups {
        indent(out, depth + 1);
        out.push_str(&format!("<{g}>\n"));
        for (k, v) in fields {
            indent(out, depth + 2);
            out.push_str(&format!("<{k}>{v}</{k}>\n"));
        }
        indent(out, depth + 1);
        out.push_str(&format!("</{g}>\n"));
    }
    indent(out, depth);
    out.push_str(&format!("</{tag}>\n"));
}

fn referent(index: usize) -> String {
    format!("RBX{index:08X}")
}

fn write_property(out: &mut String, depth: usize, name: &str, value: &Variant) {
    let f = format_f32;
    match value {
        Variant::Nil => {}
        Variant::Bool(b) => simple(
            out,
            depth,
            "bool",
            name,
            if *b { "true" } else { "false" },
            false,
        ),
        Variant::Int(v) => simple(out, depth, "int", name, &v.to_string(), false),
        Variant::Int64(v) => simple(out, depth, "int64", name, &v.to_string(), false),
        Variant::Float(v) => simple(out, depth, "float", name, &f(*v), false),
        Variant::Double(v) => simple(out, depth, "double", name, &format_f64(*v), false),
        Variant::String(s) => simple(out, depth, "string", name, s, true),
        Variant::ProtectedString(s) => simple(out, depth, "ProtectedString", name, s, true),
        Variant::BinaryString(b) => {
            let enc = base64::engine::general_purpose::STANDARD.encode(b);
            simple(out, depth, "BinaryString", name, &enc, false)
        }
        Variant::Content(s) => {
            indent(out, depth);
            out.push_str("<Content name=\"");
            encode_text(out, name, false);
            out.push_str("\">");
            if s.is_empty() {
                out.push_str("<null></null>");
            } else {
                out.push_str("<url>");
                encode_text(out, s, false);
                out.push_str("</url>");
            }
            out.push_str("</Content>\n");
        }
        Variant::Vector2(v) => {
            compound(out, depth, "Vector2", name, &[("X", f(v.x)), ("Y", f(v.y))])
        }
        Variant::Vector3(v) => compound(out, depth, "Vector3", name, &vec3_fields(*v)),
        Variant::Vector2int16(v) => compound(
            out,
            depth,
            "Vector2int16",
            name,
            &[("X", v.x.to_string()), ("Y", v.y.to_string())],
        ),
        Variant::Vector3int16(v) => compound(
            out,
            depth,
            "Vector3int16",
            name,
            &[
                ("X", v.x.to_string()),
                ("Y", v.y.to_string()),
                ("Z", v.z.to_string()),
            ],
        ),
        Variant::Vector3int32(v) => compound(
            out,
            depth,
            "Vector3int32",
            name,
            &[
                ("X", v.x.to_string()),
                ("Y", v.y.to_string()),
                ("Z", v.z.to_string()),
            ],
        ),
        Variant::CFrame(c) => {
            let r = c.rows();
            let t = c.translation;
            compound(
                out,
                depth,
                "CoordinateFrame",
                name,
                &[
                    ("X", f(t.x)),
                    ("Y", f(t.y)),
                    ("Z", f(t.z)),
                    ("R00", f(r[0])),
                    ("R01", f(r[1])),
                    ("R02", f(r[2])),
                    ("R10", f(r[3])),
                    ("R11", f(r[4])),
                    ("R12", f(r[5])),
                    ("R20", f(r[6])),
                    ("R21", f(r[7])),
                    ("R22", f(r[8])),
                ],
            )
        }
        Variant::Color3(c) if c.to_u8().to_color3() == *c => simple(
            out,
            depth,
            "Color3",
            name,
            &c.to_u8().to_argb().to_string(),
            false,
        ),
        Variant::Color3(c) => compound(
            out,
            depth,
            "Color3",
            name,
            &[("R", f(c.r)), ("G", f(c.g)), ("B", f(c.b))],
        ),
        Variant::BrickColor(b) => simple(out, depth, "int", name, &b.number().to_string(), false),
        Variant::UDim(u) => compound(
            out,
            depth,
            "UDim",
            name,
            &[("S", f(u.scale)), ("O", u.offset.to_string())],
        ),
        Variant::UDim2(u) => compound(
            out,
            depth,
            "UDim2",
            name,
            &[
                ("XS", f(u.x.scale)),
                ("XO", u.x.offset.to_string()),
                ("YS", f(u.y.scale)),
                ("YO", u.y.offset.to_string()),
            ],
        ),
        Variant::Ray(r) => nested(
            out,
            depth,
            "Ray",
            name,
            &[
                ("origin", vec3_fields(r.origin)),
                ("direction", vec3_fields(r.direction)),
            ],
        ),
        Variant::Rect2D(r) => nested(
            out,
            depth,
            "Rect2D",
            name,
            &[
                ("min", vec![("X", f(r.min.x)), ("Y", f(r.min.y))]),
                ("max", vec![("X", f(r.max.x)), ("Y", f(r.max.y))]),
            ],
        ),
        Variant::Region3(r) => nested(
            out,
            depth,
            "Region3",
            name,
            &[("min", vec3_fields(r.min)), ("max", vec3_fields(r.max))],
        ),
        Variant::Region3int16(r) => {
            let fields = |v: Vector3int16| {
                vec![
                    ("X", v.x.to_string()),
                    ("Y", v.y.to_string()),
                    ("Z", v.z.to_string()),
                ]
            };
            nested(
                out,
                depth,
                "Region3int16",
                name,
                &[("min", fields(r.min)), ("max", fields(r.max))],
            )
        }
        Variant::Faces(x) => compound(out, depth, "Faces", name, &[("faces", x.0.to_string())]),
        Variant::Axes(x) => compound(out, depth, "Axes", name, &[("axes", x.0.to_string())]),
        Variant::PhysicalProperties(p) => match p.custom {
            Some(c) => compound(
                out,
                depth,
                "PhysicalProperties",
                name,
                &[
                    ("CustomPhysics", "true".into()),
                    ("Density", f(c.density)),
                    ("Friction", f(c.friction)),
                    ("Elasticity", f(c.elasticity)),
                    ("FrictionWeight", f(c.friction_weight)),
                    ("ElasticityWeight", f(c.elasticity_weight)),
                ],
            ),
            None => compound(
                out,
                depth,
                "PhysicalProperties",
                name,
                &[("CustomPhysics", "false".into())],
            ),
        },
        Variant::NumberSequence(s) => {
            let mut text = String::new();
            for k in &s.keypoints {
                text.push_str(&format!("{} {} {} ", f(k.time), f(k.value), f(k.envelope)));
            }
            simple(out, depth, "NumberSequence", name, &text, false)
        }
        Variant::ColorSequence(s) => {
            let mut text = String::new();
            for k in &s.keypoints {
                text.push_str(&format!(
                    "{} {} {} {} {} ",
                    f(k.time),
                    f(k.value.r),
                    f(k.value.g),
                    f(k.value.b),
                    f(k.envelope)
                ));
            }
            simple(out, depth, "ColorSequence", name, &text, false)
        }
        Variant::NumberRange(r) => simple(
            out,
            depth,
            "NumberRange",
            name,
            &format!("{} {} ", f(r.min), f(r.max)),
            false,
        ),
        Variant::Enum(v) => simple(out, depth, "token", name, &v.to_string(), false),
        Variant::Ref(r) => {
            let text = match r {
                Some(id) => referent(Tree::index_of(*id)),
                None => "null".to_string(),
            };
            simple(out, depth, "Ref", name, &text, false)
        }
        Variant::SystemAddress(_) => {}
    }
}

pub fn write_xml(tree: &Tree) -> String {
    let mut out = String::new();
    out.push_str("<roblox xmlns:xmime=\"http://www.w3.org/2005/05/xmlmime\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:noNamespaceSchemaLocation=\"http://www.roblox.com/roblox.xsd\" version=\"4\">\n");
    out.push_str("\t<External>null</External>\n\t<External>nil</External>\n");
    let mut stack: Vec<(usize, usize, bool)> =
        tree.roots.iter().rev().map(|&r| (r, 1, false)).collect();
    while let Some((index, depth, closing)) = stack.pop() {
        if closing {
            indent(&mut out, depth);
            out.push_str("</Item>\n");
            continue;
        }
        let inst = &tree.instances[index];
        indent(&mut out, depth);
        out.push_str("<Item class=\"");
        encode_text(&mut out, &inst.class, false);
        out.push_str(&format!("\" referent=\"{}\">\n", referent(index)));
        indent(&mut out, depth + 1);
        out.push_str("<Properties>\n");
        for (name, value) in &inst.properties {
            write_property(&mut out, depth + 2, name, value);
        }
        indent(&mut out, depth + 1);
        out.push_str("</Properties>\n");
        stack.push((index, depth, true));
        for &c in inst.children.iter().rev() {
            stack.push((c, depth + 1, false));
        }
    }
    out.push_str("</roblox>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<roblox xmlns:xmime="http://www.w3.org/2005/05/xmlmime" version="4">
	<External>null</External>
	<Item class="Model" referent="RBX1">
		<Properties>
			<string name="Name">Tree &amp; leaves</string>
			<Ref name="PrimaryPart">RBX2</Ref>
		</Properties>
		<Item class="Part" referent="RBX2">
			<Properties>
				<CoordinateFrame name="CFrame"><X>1</X><Y>2</Y><Z>3</Z><R00>1</R00><R01>0</R01><R02>0</R02><R10>0</R10><R11>1</R11><R12>0</R12><R20>0</R20><R21>0</R21><R22>1</R22></CoordinateFrame>
				<int name="BrickColor">21</int>
				<Color3 name="Color">4294901760</Color3>
				<token name="Material">256</token>
				<Content name="TextureId"><url>rbxasset://textures/x.png</url></Content>
				<Content name="Empty"><null></null></Content>
				<ProtectedString name="Source"><![CDATA[print("a")
print("b")]]></ProtectedString>
			</Properties>
		</Item>
	</Item>
</roblox>"#;

    #[test]
    fn reads_sample() {
        let tree = read_xml(SAMPLE.as_bytes()).expect("parse");
        assert_eq!(tree.roots.len(), 1);
        let model = &tree.instances[tree.roots[0]];
        assert_eq!(model.name(), "Tree & leaves");
        let part_index = model.children[0];
        let part = &tree.instances[part_index];
        assert_eq!(part.get("BrickColor"), Some(&Variant::Int(21)));
        assert_eq!(part.get("Material"), Some(&Variant::Enum(256)));
        assert_eq!(
            part.get("TextureId"),
            Some(&Variant::Content("rbxasset://textures/x.png".into()))
        );
        assert_eq!(part.get("Empty"), Some(&Variant::Content(String::new())));
        assert_eq!(
            part.get("Source"),
            Some(&Variant::ProtectedString(
                "print(\"a\")\nprint(\"b\")".into()
            ))
        );
        assert_eq!(
            model.get("PrimaryPart"),
            Some(&Variant::Ref(Some(Tree::id_of(part_index))))
        );
        let cf = part
            .get("CFrame")
            .and_then(|v| v.as_cframe())
            .unwrap_or_default();
        assert_eq!(cf.translation, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(
            part.get("Color"),
            Some(&Variant::Color3(Color3::new(1.0, 0.0, 0.0)))
        );
    }

    #[test]
    fn round_trips() {
        let tree = read_xml(SAMPLE.as_bytes()).expect("parse");
        let text = write_xml(&tree);
        let back = read_xml(text.as_bytes()).expect("reparse");
        assert_eq!(tree, back);
    }

    #[test]
    fn keeps_leading_whitespace() {
        let mut tree = Tree::new();
        let mut v = TreeInstance::new("StringValue");
        v.set("Value", Variant::String("  padded <tag>".into()));
        tree.push(v, None);
        let back = read_xml(write_xml(&tree).as_bytes()).expect("reparse");
        assert_eq!(back, tree);
    }
}
