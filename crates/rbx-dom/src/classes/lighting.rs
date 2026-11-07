use crate::behavior::{Behavior, SetOutcome};
use crate::datamodel::DataModel;
use rbx_math::{Mat3, Vec3};
use rbx_reflection::{InstanceId, Variant};
use std::collections::HashMap;

const HOUR: f64 = 3600.0;
const DAY: f64 = 86400.0;
const SOLAR_YEAR: f64 = 365.2564 * DAY;
const HALF_SOLAR_YEAR: f64 = 182.6282;
const MOON_PHASE_INTERVAL: f64 = DAY * 29.53;
const INITIAL_MOON_PHASE: f64 = 0.75;
const DEFAULT_LATITUDE: f32 = 41.7333;

pub fn parse_duration(text: &str) -> Option<i64> {
    let t = text.trim();
    let (negative, body) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let fields: Vec<&str> = body.split(':').collect();
    if fields.is_empty() || fields.len() > 3 || fields.iter().any(|f| f.is_empty()) {
        return None;
    }
    let int = |s: &str| -> Option<i64> {
        if s.chars().all(|c| c.is_ascii_digit()) {
            s.parse().ok()
        } else {
            None
        }
    };
    let hours = int(fields[0])?;
    let minutes = match fields.get(1) {
        Some(m) => int(m)?,
        None => 0,
    };
    let seconds = match fields.get(2) {
        Some(s) => {
            let whole = s.split('.').next().unwrap_or("");
            let frac = s.split('.').nth(1);
            if frac.is_some_and(|f| !f.chars().all(|c| c.is_ascii_digit())) {
                return None;
            }
            int(whole)?
        }
        None => 0,
    };
    let total = hours * 3600 + minutes * 60 + seconds;
    Some(if negative { -total } else { total })
}

pub fn format_duration(seconds: i64) -> String {
    let sign = if seconds < 0 { "-" } else { "" };
    let s = seconds.unsigned_abs();
    format!("{sign}{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

impl DataModel {
    pub fn lighting_seconds(&self, id: InstanceId) -> i64 {
        self.get_raw(id, "TimeOfDay")
            .and_then(|v| match v {
                Variant::String(s) => parse_duration(s),
                _ => None,
            })
            .unwrap_or(14 * 3600)
    }

    fn lighting_set_seconds(&mut self, id: InstanceId, seconds: i64) {
        self.set_unchecked(id, "TimeOfDay", Variant::String(format_duration(seconds)));
    }

    pub fn lighting_minutes_after_midnight(&self, id: InstanceId) -> f64 {
        self.lighting_seconds(id) as f64 * 1000.0 / 60000.0
    }

    pub fn lighting_set_minutes_after_midnight(&mut self, id: InstanceId, minutes: f64) {
        let seconds = (60.0 * minutes) as i64;
        self.lighting_set_time(id, seconds);
    }

    fn lighting_set_time(&mut self, id: InstanceId, seconds: i64) {
        let wrapped = seconds % 86400;
        if self.lighting_seconds(id) != wrapped {
            self.lighting_set_seconds(id, wrapped);
            self.notify_changed(id, "TimeOfDay");
        }
    }

    fn lighting_latitude(&self, id: InstanceId) -> f32 {
        self.get_raw(id, "GeographicLatitude")
            .and_then(|v| v.as_f32())
            .unwrap_or(DEFAULT_LATITUDE)
    }

    fn lighting_sky(&self, id: InstanceId) -> (Vec3, Vec3, f32) {
        let game_time = self.lighting_seconds(id) as f64;
        let time = game_time - (game_time / DAY).floor() * DAY;
        let source_angle = (2.0 * std::f64::consts::PI * time / DAY) as f32;
        let sun = Vec3::new(source_angle.sin(), -source_angle.cos(), 0.0);
        let day_of_year_offset =
            ((game_time - game_time * (game_time / SOLAR_YEAR).floor()) / DAY) as f32;
        let moon_phase = ((game_time / MOON_PHASE_INTERVAL).floor() + INITIAL_MOON_PHASE) as f32;
        let earth_tilt = 23.5f32.to_radians();
        let moon_tilt = 5.0f32.to_radians();
        let lat = self.lighting_latitude(id).to_radians();
        let pi = std::f32::consts::PI;
        let half = HALF_SOLAR_YEAR as f32;
        let sun_offset = -earth_tilt * (pi * (day_of_year_offset - half) / half).cos() - lat;
        let moon_offset = (-earth_tilt + moon_tilt) * (moon_phase * 4.0).sin() - lat;
        let cur_moon_phase = moon_phase * pi * 2.0;
        let rotate = |axis: Vec3, angle: f32, v: Vec3| {
            let a = axis.normalize_or_zero();
            if a == Vec3::ZERO {
                v
            } else {
                Mat3::from_axis_angle(a, angle) * v
            }
        };
        let true_sun = rotate(Vec3::Z.cross(sun), sun_offset, sun);
        let moon = Vec3::new(
            (cur_moon_phase + source_angle).sin(),
            -(cur_moon_phase + source_angle).cos(),
            0.0,
        );
        let true_moon = rotate(Vec3::Z.cross(moon), moon_offset, moon);
        (true_sun, true_moon, moon_phase)
    }

    pub fn lighting_sun_direction(&self, id: InstanceId) -> Vec3 {
        self.lighting_sky(id).0
    }

    pub fn lighting_moon_direction(&self, id: InstanceId) -> Vec3 {
        self.lighting_sky(id).1
    }

    pub fn lighting_moon_phase(&self, id: InstanceId) -> f32 {
        self.lighting_sky(id).2
    }
}

pub struct LightingBehavior;

impl Behavior for LightingBehavior {
    fn computed(&self) -> &'static [&'static str] {
        &["ClockTime"]
    }

    fn get(&self, dm: &DataModel, id: InstanceId, prop: &str) -> Option<Variant> {
        match prop {
            "ClockTime" => Some(Variant::Float((dm.lighting_seconds(id) / 3600) as f32)),
            _ => None,
        }
    }

    fn set(&self, dm: &mut DataModel, id: InstanceId, prop: &str, value: &Variant) -> SetOutcome {
        match prop {
            "TimeOfDay" => {
                let text = match value {
                    Variant::String(s) => s.clone(),
                    _ => return SetOutcome::Unhandled,
                };
                match parse_duration(&text) {
                    Some(seconds) => {
                        dm.lighting_set_time(id, seconds);
                        SetOutcome::Handled
                    }
                    None => SetOutcome::Error(format!("could not parse time \"{text}\"")),
                }
            }
            "ClockTime" => {
                let Some(v) = value.as_f32() else {
                    return SetOutcome::Unhandled;
                };
                let seconds = (v.clamp(0.0, 24.0) as f64 * HOUR).ceil() as i64;
                if dm.lighting_seconds(id) != seconds {
                    dm.lighting_set_seconds(id, seconds);
                    dm.notify_changed(id, "ClockTime");
                }
                SetOutcome::Handled
            }
            _ => SetOutcome::Unhandled,
        }
    }
}

pub fn register(map: &mut HashMap<&'static str, &'static dyn Behavior>) {
    map.insert("Lighting", &LightingBehavior);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_round_trip() {
        assert_eq!(parse_duration("14:00:00"), Some(50400));
        assert_eq!(parse_duration("1:2:3.5"), Some(3723));
        assert_eq!(parse_duration("-01:00:00"), Some(-3600));
        assert_eq!(parse_duration("7"), Some(25200));
        assert_eq!(parse_duration("abc"), None);
        assert_eq!(format_duration(50400), "14:00:00");
        assert_eq!(format_duration(-3600), "-01:00:00");
    }
}
