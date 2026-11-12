#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tint {
    pub color: Option<[f32; 3]>,
    pub alpha: f32,
}

impl Default for Tint {
    fn default() -> Self {
        Tint {
            color: None,
            alpha: 1.0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RichText {
    pub text: String,
    pub tints: Vec<Tint>,
    pub family: Option<(String, u16)>,
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find(name) {
        let at = from + found;
        from = at + name.len();
        let before = lower[..at].chars().last();
        if before.is_some_and(|c| c.is_ascii_alphanumeric()) {
            continue;
        }
        let rest = tag[from..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim_start();
        let quote = rest.chars().next()?;
        if quote == '"' || quote == '\'' {
            let body = &rest[1..];
            return Some(body[..body.find(quote)?].to_string());
        }
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '/')
            .unwrap_or(rest.len());
        return Some(rest[..end].to_string());
    }
    None
}

pub fn parse_color(s: &str) -> Option<[f32; 3]> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        let v = u32::from_str_radix(hex, 16).ok()?;
        return match hex.len() {
            6 => Some([
                ((v >> 16) & 255) as f32 / 255.0,
                ((v >> 8) & 255) as f32 / 255.0,
                (v & 255) as f32 / 255.0,
            ]),
            3 => Some([
                ((v >> 8) & 15) as f32 * 17.0 / 255.0,
                ((v >> 4) & 15) as f32 * 17.0 / 255.0,
                (v & 15) as f32 * 17.0 / 255.0,
            ]),
            _ => None,
        };
    }
    let inner = s.strip_prefix("rgb(")?.strip_suffix(')')?;
    let mut parts = inner.split(',').map(|p| p.trim().parse::<f32>().ok());
    Some([
        parts.next()?? / 255.0,
        parts.next()?? / 255.0,
        parts.next()?? / 255.0,
    ])
}

fn entity(s: &str) -> Option<(char, usize)> {
    for (name, c) in [
        ("&lt;", '<'),
        ("&gt;", '>'),
        ("&quot;", '"'),
        ("&apos;", '\''),
        ("&amp;", '&'),
    ] {
        if s.starts_with(name) {
            return Some((c, name.len()));
        }
    }
    None
}

pub fn parse(s: &str) -> RichText {
    let mut out = RichText::default();
    let mut stack: Vec<(String, Tint)> = Vec::new();
    let mut tint = Tint::default();
    let mut i = 0;
    while i < s.len() {
        let rest = &s[i..];
        if rest.starts_with('<')
            && let Some(close) = rest.find('>')
        {
            let tag = &rest[1..close];
            let name: String = tag
                .trim_start_matches('/')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_lowercase();
            if !name.is_empty() {
                i += close + 1;
                if tag.starts_with('/') {
                    if let Some(pos) = stack.iter().rposition(|(n, _)| *n == name) {
                        tint = stack[pos].1;
                        stack.truncate(pos);
                    }
                    continue;
                }
                if name == "br" {
                    out.text.push('\n');
                    out.tints.push(tint);
                    continue;
                }
                if tag.trim_end().ends_with('/') {
                    continue;
                }
                stack.push((name.clone(), tint));
                if name == "font" {
                    if let Some(c) = attr(tag, "color").and_then(|c| parse_color(&c)) {
                        tint.color = Some(c);
                    }
                    if let Some(t) =
                        attr(tag, "transparency").and_then(|t| t.trim().parse::<f32>().ok())
                    {
                        tint.alpha *= 1.0 - t.clamp(0.0, 1.0);
                    }
                    if out.family.is_none()
                        && let Some(family) = attr(tag, "family")
                    {
                        let weight = attr(tag, "weight")
                            .and_then(|w| w.parse().ok())
                            .unwrap_or(400);
                        out.family = Some((family, weight));
                    }
                }
                continue;
            }
        }
        if let Some((c, len)) = entity(rest) {
            out.text.push(c);
            out.tints.push(tint);
            i += len;
            continue;
        }
        let c = rest.chars().next().unwrap_or('\0');
        out.text.push(c);
        out.tints.push(tint);
        i += c.len_utf8();
    }
    if out.tints.iter().all(|t| *t == Tint::default()) {
        out.tints.clear();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tags_and_tracks_colors() {
        let r = parse("<font color=\"#ff0000\"><b>Hi:</b></font> there &amp; you");
        assert_eq!(r.text, "Hi: there & you");
        assert_eq!(r.tints[0].color, Some([1.0, 0.0, 0.0]));
        assert_eq!(r.tints[3].color, None);
    }

    #[test]
    fn keeps_literal_angle_text() {
        assert_eq!(parse("a < b").text, "a < b");
        assert!(parse("plain").tints.is_empty());
    }

    #[test]
    fn nested_fonts_restore_outer_color() {
        let r = parse("<font color='rgb(0,255,0)'>a<font color=\"#00f\">b</font>c</font>");
        assert_eq!(r.text, "abc");
        assert_eq!(r.tints[1].color, Some([0.0, 0.0, 1.0]));
        assert_eq!(r.tints[2].color, Some([0.0, 1.0, 0.0]));
    }

    #[test]
    fn transparency_hides_prefix() {
        let r = parse("<font transparency='1.0'><font color=\"#ffffff\">P:</font></font> hi");
        assert_eq!(r.text, "P: hi");
        assert_eq!(r.tints[0].alpha, 0.0);
        assert_eq!(r.tints[0].color, Some([1.0, 1.0, 1.0]));
        assert_eq!(r.tints[3].alpha, 1.0);
    }
}
