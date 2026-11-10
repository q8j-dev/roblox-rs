pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parts {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub millisecond: u32,
    pub weekday: u32,
}

pub fn parts(millis: i64) -> Parts {
    let days = millis.div_euclid(86_400_000);
    let rem = millis.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    Parts {
        year,
        month,
        day,
        hour: (rem / 3_600_000) as u32,
        minute: (rem / 60_000 % 60) as u32,
        second: (rem / 1000 % 60) as u32,
        millisecond: (rem % 1000) as u32,
        weekday: (days + 4).rem_euclid(7) as u32,
    }
}

pub fn millis_from_parts(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64, ms: i64) -> i64 {
    let y = y + (mo - 1).div_euclid(12);
    let mo = (mo - 1).rem_euclid(12) + 1;
    let days = days_from_civil(y, mo as u32, 1) + d - 1;
    days * 86_400_000 + h * 3_600_000 + mi * 60_000 + s * 1000 + ms
}

pub fn iso(millis: i64) -> String {
    let p = parts(millis);
    if p.millisecond == 0 {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            p.year, p.month, p.day, p.hour, p.minute, p.second
        )
    } else {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
            p.year, p.month, p.day, p.hour, p.minute, p.second, p.millisecond
        )
    }
}

pub fn parse_iso(s: &str) -> Option<i64> {
    let s = s.trim();
    let (date, rest) = s.split_once('T').unwrap_or((s, ""));
    let mut d = date.split('-');
    let y: i64 = d.next()?.parse().ok()?;
    let mo: i64 = d.next().unwrap_or("1").parse().ok()?;
    let da: i64 = d.next().unwrap_or("1").parse().ok()?;
    let (time, offset) = if let Some(t) = rest.strip_suffix('Z') {
        (t, 0)
    } else if let Some(i) = rest.rfind(['+', '-']).filter(|&i| i > 0) {
        let (t, o) = rest.split_at(i);
        let sign = if o.starts_with('-') { -1 } else { 1 };
        let o = &o[1..];
        let (oh, om) = o
            .split_once(':')
            .unwrap_or((&o[..o.len().min(2)], &o[o.len().min(2)..]));
        let mins = oh.parse::<i64>().ok()? * 60 + om.parse::<i64>().unwrap_or(0);
        (t, sign * mins * 60_000)
    } else {
        (rest, 0)
    };
    let mut t = time.split(':');
    let h: i64 = t
        .next()
        .filter(|x| !x.is_empty())
        .unwrap_or("0")
        .parse()
        .ok()?;
    let mi: i64 = t.next().unwrap_or("0").parse().ok()?;
    let sec = t.next().unwrap_or("0");
    let (s, frac) = sec.split_once('.').unwrap_or((sec, ""));
    let s: i64 = s.parse().ok()?;
    let ms: i64 = format!("{frac:0<3}")[..3].parse().unwrap_or(0);
    Some(millis_from_parts(y, mo, da, h, mi, s, ms) - offset)
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

const TOKENS: [&str; 33] = [
    "LLLL", "llll", "LLL", "lll", "LTS", "LL", "ll", "LT", "L", "l", "YYYY", "YY", "MMMM", "MMM",
    "MM", "Mo", "M", "DDDD", "DDD", "Do", "DD", "D", "dddd", "ddd", "dd", "d", "HH", "H", "hh",
    "h", "mm", "ss", "SSS",
];

pub fn format(millis: i64, fmt: &str) -> String {
    let p = parts(millis);
    let h12 = match p.hour % 12 {
        0 => 12,
        h => h,
    };
    let mut out = String::new();
    let mut rest = fmt;
    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix('[') {
            let end = stripped.find(']').unwrap_or(stripped.len());
            out.push_str(&stripped[..end]);
            rest = stripped.get(end + 1..).unwrap_or("");
            continue;
        }
        let token = TOKENS
            .iter()
            .chain(["m", "s", "A", "a", "SS", "S"].iter())
            .find(|t| rest.starts_with(**t));
        let Some(token) = token else {
            let c = rest.chars().next().unwrap_or(' ');
            out.push(c);
            rest = &rest[c.len_utf8()..];
            continue;
        };
        let doy =
            (days_from_civil(p.year, p.month, p.day) - days_from_civil(p.year, 1, 1) + 1) as u32;
        let piece = match *token {
            "LT" => format!(
                "{}:{:02} {}",
                h12,
                p.minute,
                if p.hour < 12 { "AM" } else { "PM" }
            ),
            "LTS" => format!(
                "{}:{:02}:{:02} {}",
                h12,
                p.minute,
                p.second,
                if p.hour < 12 { "AM" } else { "PM" }
            ),
            "L" => format!("{:02}/{:02}/{:04}", p.month, p.day, p.year),
            "l" => format!("{}/{}/{:04}", p.month, p.day, p.year),
            "LL" => format!("{} {}, {}", MONTHS[p.month as usize - 1], p.day, p.year),
            "ll" => format!(
                "{} {}, {}",
                &MONTHS[p.month as usize - 1][..3],
                p.day,
                p.year
            ),
            "LLL" => format!("{} {}", format(millis, "LL"), format(millis, "LT")),
            "lll" => format!("{} {}", format(millis, "ll"), format(millis, "LT")),
            "LLLL" => format!("{}, {}", DAYS[p.weekday as usize], format(millis, "LLL")),
            "llll" => format!(
                "{}, {}",
                &DAYS[p.weekday as usize][..3],
                format(millis, "lll")
            ),
            "YYYY" => format!("{:04}", p.year),
            "YY" => format!("{:02}", p.year.rem_euclid(100)),
            "MMMM" => MONTHS[p.month as usize - 1].to_string(),
            "MMM" => MONTHS[p.month as usize - 1][..3].to_string(),
            "MM" => format!("{:02}", p.month),
            "Mo" => ordinal(p.month),
            "M" => p.month.to_string(),
            "DDDD" => format!("{doy:03}"),
            "DDD" => doy.to_string(),
            "Do" => ordinal(p.day),
            "DD" => format!("{:02}", p.day),
            "D" => p.day.to_string(),
            "dddd" => DAYS[p.weekday as usize].to_string(),
            "ddd" => DAYS[p.weekday as usize][..3].to_string(),
            "dd" => DAYS[p.weekday as usize][..2].to_string(),
            "d" => p.weekday.to_string(),
            "HH" => format!("{:02}", p.hour),
            "H" => p.hour.to_string(),
            "hh" => format!("{h12:02}"),
            "h" => h12.to_string(),
            "mm" => format!("{:02}", p.minute),
            "m" => p.minute.to_string(),
            "ss" => format!("{:02}", p.second),
            "s" => p.second.to_string(),
            "A" => (if p.hour < 12 { "AM" } else { "PM" }).to_string(),
            "a" => (if p.hour < 12 { "am" } else { "pm" }).to_string(),
            "SSS" => format!("{:03}", p.millisecond),
            "SS" => format!("{:02}", p.millisecond / 10),
            "S" => (p.millisecond / 100).to_string(),
            _ => String::new(),
        };
        out.push_str(&piece);
        rest = &rest[token.len()..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_formats() {
        let t = millis_from_parts(2026, 11, 9, 17, 5, 3, 250);
        assert_eq!(iso(t), "2026-11-09T17:05:03.250Z");
        assert_eq!(parse_iso("2026-11-09T17:05:03.250Z"), Some(t));
        assert_eq!(parse_iso("2026-11-09T19:05:03.250+02:00"), Some(t));
        assert_eq!(format(t, "LL LT"), "November 9, 2026 5:05 PM");
        assert_eq!(format(t, "dddd [at] HH:mm"), "Monday at 17:05");
        assert_eq!(parts(0).weekday, 4);
    }
}
