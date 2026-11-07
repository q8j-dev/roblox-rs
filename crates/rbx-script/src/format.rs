pub fn format_g(value: f64, precision: usize) -> String {
    if value.is_nan() {
        return if value.is_sign_negative() {
            "-nan"
        } else {
            "nan"
        }
        .to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    let p = precision.max(1);
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.to_string();
    }
    let sci = format!("{:.*e}", p - 1, value);
    let (mantissa, exp) = match sci.split_once('e') {
        Some((m, e)) => (m.to_string(), e.parse::<i32>().unwrap_or(0)),
        None => (sci.clone(), 0),
    };
    if exp < -4 || exp >= p as i32 {
        let m = strip_zeros(&mantissa);
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{m}e{sign}{:02}", exp.abs())
    } else {
        let decimals = (p as i32 - 1 - exp).max(0) as usize;
        strip_zeros(&format!("{value:.decimals$}"))
    }
}

fn strip_zeros(s: &str) -> String {
    if s.contains('.') {
        let t = s.trim_end_matches('0');
        t.trim_end_matches('.').to_string()
    } else {
        s.to_string()
    }
}

pub fn float_str(v: f32) -> String {
    if v == f32::INFINITY {
        return "INF".into();
    }
    if v == f32::NEG_INFINITY {
        return "-INF".into();
    }
    if v.is_nan() {
        return "NAN".into();
    }
    format_g(v as f64, 9)
}

pub fn lua_number_str(v: f64) -> String {
    if v.is_nan() {
        return "nan".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "inf" } else { "-inf" }.into();
    }
    format_g(v, 14)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_printf_g() {
        assert_eq!(format_g(0.1f32 as f64, 9), "0.100000001");
        assert_eq!(format_g(1.0, 9), "1");
        assert_eq!(format_g(100000.0, 9), "100000");
        assert_eq!(format_g(1e10, 9), "1e+10");
        assert_eq!(format_g(1.5e-7, 9), "1.5e-07");
        assert_eq!(format_g(-2.25, 9), "-2.25");
        assert_eq!(format_g(123456789.0, 9), "123456789");
        assert_eq!(format_g(1234567890.0, 9), "1.23456789e+09");
        assert_eq!(lua_number_str(3.0), "3");
        assert_eq!(lua_number_str(0.1), "0.1");
    }
}
