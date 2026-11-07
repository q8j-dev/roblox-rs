fn numeric_prefix(s: &str) -> &str {
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let start_digits = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i == start_digits || (i == start_digits + 1 && b.get(start_digits) == Some(&b'.')) {
        return "";
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let exp_start = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp_start {
            i = j;
        }
    }
    &s[..i]
}

pub fn parse_f64(text: &str) -> Option<f64> {
    let t = text.trim();
    let lower = t.to_ascii_lowercase();
    let (neg, body) = match lower.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, lower.strip_prefix('+').unwrap_or(&lower)),
    };
    let special = match body {
        "inf" | "infinity" | "1.#inf" | "1.#inf00" => Some(f64::INFINITY),
        "nan" | "1.#qnan" | "1.#ind" | "1.#ind00" | "1.#qnan0" | "nan(ind)" => Some(f64::NAN),
        _ => None,
    };
    if let Some(v) = special {
        return Some(if neg { -v } else { v });
    }
    let p = numeric_prefix(t);
    if p.is_empty() {
        return None;
    }
    p.parse::<f64>().ok()
}

pub fn parse_f32(text: &str) -> Option<f32> {
    parse_f64(text).map(|v| v as f32)
}

pub fn parse_i64(text: &str) -> Option<i64> {
    let t = text.trim();
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let digits = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == digits {
        return None;
    }
    t[..i].parse::<i64>().ok().or_else(|| {
        t[..i]
            .parse::<i128>()
            .ok()
            .map(|v| v.clamp(i64::MIN as i128, i64::MAX as i128) as i64)
    })
}

pub fn parse_i32(text: &str) -> Option<i32> {
    parse_i64(text).map(|v| v as i32)
}

pub fn parse_bool(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Some(true),
        "false" | "0" | "no" => Some(false),
        _ => None,
    }
}

pub fn format_f32(v: f32) -> String {
    if v.is_nan() {
        "NAN".to_string()
    } else if v.is_infinite() {
        if v > 0.0 { "INF" } else { "-INF" }.to_string()
    } else {
        format!("{v}")
    }
}

pub fn format_f64(v: f64) -> String {
    if v.is_nan() {
        "NAN".to_string()
    } else if v.is_infinite() {
        if v > 0.0 { "INF" } else { "-INF" }.to_string()
    } else {
        format!("{v}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(parse_f32("1.5"), Some(1.5));
        assert_eq!(parse_f32(" -2e3 "), Some(-2000.0));
        assert_eq!(parse_f32("3.25abc"), Some(3.25));
        assert!(parse_f32("1.#INF").is_some_and(|v| v.is_infinite()));
        assert!(parse_f32("NAN").is_some_and(|v| v.is_nan()));
        assert_eq!(parse_f32("abc"), None);
        assert_eq!(parse_i32("42"), Some(42));
        assert_eq!(parse_i32("-7x"), Some(-7));
        assert_eq!(parse_bool("true"), Some(true));
        let x = 0.1f32;
        assert_eq!(parse_f32(&format_f32(x)), Some(x));
    }
}
