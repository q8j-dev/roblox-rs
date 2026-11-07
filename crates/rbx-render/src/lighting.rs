use rbx_math::{Color3, Mat3, Vec3};

const SECOND: f64 = 1.0;
const MINUTE: f64 = 60.0 * SECOND;
const HOUR: f64 = 60.0 * MINUTE;
const DAY: f64 = 24.0 * HOUR;
const SUNRISE: f64 = DAY / 4.0;
const SUNSET: f64 = DAY * 3.0 / 4.0;
const MIDNIGHT: f64 = 0.0;
const SUN_RISE_AND_SET_TIME: f64 = HOUR / 2.0;
const SOLAR_YEAR: f64 = 365.2564 * DAY;
const HALF_SOLAR_YEAR: f64 = 182.6282;
const MOON_PHASE_INTERVAL: f64 = DAY * 29.53;
const INITIAL_MOON_PHASE: f64 = 0.75;

#[derive(Clone, Copy, Debug)]
pub struct SkyParameters {
    pub light_direction: Vec3,
    pub light_color: Color3,
    pub ambient: Color3,
    pub diffuse_ambient: Color3,
    pub sky_ambient: Color3,
    pub sky_ambient2: Color3,
    pub true_sun: Vec3,
    pub true_moon: Vec3,
    pub moon_phase: f32,
}

fn c(r: f32, g: f32, b: f32) -> Color3 {
    Color3::new(r, g, b)
}

fn scale(col: Color3, s: f32) -> Color3 {
    Color3::new(col.r * s, col.g * s, col.b * s)
}

fn linear_spline(x: f64, xs: &[f64], ys: &[Color3]) -> Color3 {
    if xs.len() == 1 || x < xs[0] {
        return ys[0];
    }
    for i in 1..xs.len() {
        if x < xs[i] {
            let alpha = ((xs[i] - x) / (xs[i] - xs[i - 1])) as f32;
            let a = ys[i];
            let b = ys[i - 1];
            return Color3::new(
                a.r * (1.0 - alpha) + b.r * alpha,
                a.g * (1.0 - alpha) + b.g * alpha,
                a.b * (1.0 - alpha) + b.b * alpha,
            );
        }
    }
    ys[ys.len() - 1]
}

fn rotate(axis: Vec3, angle: f32, v: Vec3) -> Vec3 {
    let a = axis.normalize_or_zero();
    if a == Vec3::ZERO {
        v
    } else {
        Mat3::from_axis_angle(a, angle) * v
    }
}

impl SkyParameters {
    pub fn at(game_time: f64, latitude: f32) -> SkyParameters {
        let time = game_time - (game_time / DAY).floor() * DAY;
        let source_angle = (2.0 * std::f64::consts::PI * time / DAY) as f32;
        let sun = Vec3::new(source_angle.sin(), -source_angle.cos(), 0.0);
        let day_of_year_offset =
            ((game_time - game_time * (game_time / SOLAR_YEAR).floor()) / DAY) as f32;
        let moon_phase = ((game_time / MOON_PHASE_INTERVAL).floor() + INITIAL_MOON_PHASE) as f32;
        let earth_tilt = 23.5f32.to_radians();
        let moon_tilt = 5.0f32.to_radians();
        let lat = latitude.to_radians();
        let pi = std::f32::consts::PI;
        let half = HALF_SOLAR_YEAR as f32;
        let sun_offset = -earth_tilt * (pi * (day_of_year_offset - half) / half).cos() - lat;
        let moon_offset = (-earth_tilt + moon_tilt) * (moon_phase * 4.0).sin() - lat;
        let cur_moon_phase = moon_phase * pi * 2.0;
        let true_sun = rotate(Vec3::Z.cross(sun), sun_offset, sun);
        let moon = Vec3::new(
            (cur_moon_phase + source_angle).sin(),
            -(cur_moon_phase + source_angle).cos(),
            0.0,
        );
        let true_moon = rotate(Vec3::Z.cross(moon), moon_offset, moon);
        let light_direction = if true_sun.y > -0.3 {
            true_sun
        } else {
            true_moon
        };

        let day_ambient = scale(c(1.0, 1.0, 1.0), 0.40);
        let day_diffuse = scale(c(1.0, 1.0, 1.0), 0.75);
        let t = [
            MIDNIGHT,
            SUNRISE - HOUR,
            SUNRISE,
            SUNRISE + SUN_RISE_AND_SET_TIME / 4.0,
            SUNRISE + SUN_RISE_AND_SET_TIME,
            SUNSET - SUN_RISE_AND_SET_TIME,
            SUNSET - SUN_RISE_AND_SET_TIME / 2.0,
            SUNSET,
            SUNSET + HOUR / 2.0,
            DAY,
        ];
        let light_color = linear_spline(
            time,
            &t,
            &[
                c(0.2, 0.2, 0.2),
                c(0.1, 0.1, 0.1),
                c(0.0, 0.0, 0.0),
                c(0.6, 0.6, 0.0),
                day_diffuse,
                day_diffuse,
                c(0.1, 0.1, 0.075),
                c(0.1, 0.05, 0.05),
                c(0.1, 0.1, 0.1),
                c(0.2, 0.2, 0.2),
            ],
        );
        let ambient = linear_spline(
            time,
            &t,
            &[
                c(0.0, 0.1, 0.3),
                c(0.0, 0.0, 0.1),
                c(0.0, 0.0, 0.0),
                c(0.0, 0.0, 0.0),
                day_ambient,
                day_ambient,
                c(0.5, 0.2, 0.2),
                c(0.05, 0.05, 0.1),
                c(0.0, 0.0, 0.1),
                c(0.0, 0.1, 0.3),
            ],
        );
        let t2 = [
            MIDNIGHT,
            SUNRISE - HOUR,
            SUNRISE,
            SUNRISE + SUN_RISE_AND_SET_TIME / 2.0,
            SUNRISE + SUN_RISE_AND_SET_TIME,
            SUNSET - SUN_RISE_AND_SET_TIME,
            SUNSET - SUN_RISE_AND_SET_TIME / 2.0,
            SUNSET,
            SUNSET + HOUR / 2.0,
            DAY,
        ];
        let diffuse_ambient = linear_spline(
            time,
            &t2,
            &[
                c(0.1, 0.1, 0.17),
                c(0.05, 0.06, 0.07),
                c(0.08, 0.08, 0.01),
                scale(c(1.0, 1.0, 1.0), 0.75),
                scale(c(1.0, 1.0, 1.0), 0.75),
                scale(c(1.0, 1.0, 1.0), 0.35),
                c(0.5, 0.2, 0.2),
                c(0.05, 0.05, 0.1),
                c(0.06, 0.06, 0.07),
                c(0.1, 0.1, 0.17),
            ],
        );
        let sky_ambient = linear_spline(
            time,
            &[
                MIDNIGHT,
                SUNRISE - 2.0 * HOUR,
                SUNRISE - HOUR,
                SUNRISE - HOUR / 2.0,
                SUNRISE,
                SUNRISE + SUN_RISE_AND_SET_TIME,
                SUNSET - SUN_RISE_AND_SET_TIME,
                SUNSET,
                SUNSET + HOUR / 3.0,
                DAY,
            ],
            &[
                c(0.0, 0.0, 0.0),
                c(0.0, 0.0, 0.0),
                c(0.07, 0.07, 0.1),
                c(0.2, 0.15, 0.01),
                c(0.2, 0.15, 0.01),
                c(1.0, 1.0, 1.0),
                c(1.0, 1.0, 1.0),
                c(0.4, 0.2, 0.05),
                c(0.0, 0.0, 0.0),
                c(0.0, 0.0, 0.0),
            ],
        );
        let sky_ambient2 = linear_spline(
            time,
            &[
                MIDNIGHT,
                SUNRISE - 3.0 * HOUR,
                SUNRISE - 2.0 * HOUR,
                SUNRISE - HOUR / 2.0,
                SUNRISE,
                SUNRISE + SUN_RISE_AND_SET_TIME,
                SUNSET - SUN_RISE_AND_SET_TIME,
                SUNSET,
                SUNSET + HOUR / 3.0,
                SUNSET + 2.0 * HOUR,
                SUNSET + 3.0 * HOUR,
                DAY,
            ],
            &[
                c(0.0, 0.0, 0.0),
                c(0.0, 0.0, 0.0),
                scale(c(0.3, 0.3, 0.4), 0.7),
                c(0.4, 0.3, 0.3),
                c(0.3, 0.2, 0.3),
                c(1.0, 1.0, 1.0),
                c(1.0, 1.0, 1.0),
                c(0.4, 0.3, 0.2),
                c(0.3, 0.2, 0.3),
                c(0.3, 0.2, 0.3),
                c(0.0, 0.0, 0.0),
                c(0.0, 0.0, 0.0),
            ],
        );
        SkyParameters {
            light_direction,
            light_color,
            ambient,
            diffuse_ambient,
            sky_ambient,
            sky_ambient2,
            true_sun,
            true_moon,
            moon_phase,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SceneLighting {
    pub ambient: Color3,
    pub key_direction: Vec3,
    pub key_color: Color3,
    pub fill_color: Color3,
    pub fog_color: Color3,
    pub fog_start: f32,
    pub fog_end: f32,
    pub clear_color: Color3,
    pub outlines: bool,
    pub shadow_intensity: f32,
    pub global_shadows: bool,
    pub sky_ambient: Color3,
}

pub struct LightingInputs {
    pub game_time: f64,
    pub latitude: f32,
    pub ambient: Color3,
    pub brightness: f32,
    pub color_shift_top: Color3,
    pub color_shift_bottom: Color3,
    pub fog_color: Color3,
    pub fog_start: f32,
    pub fog_end: f32,
    pub outlines: bool,
    pub global_shadows: bool,
    pub outdoor_ambient: Color3,
}

fn len(c: Color3) -> f32 {
    (c.r * c.r + c.g * c.g + c.b * c.b).sqrt()
}

fn add(a: Color3, b: Color3) -> Color3 {
    Color3::new(a.r + b.r, a.g + b.g, a.b + b.b)
}

fn min_white(c: Color3) -> Color3 {
    Color3::new(c.r.min(1.0), c.g.min(1.0), c.b.min(1.0))
}

fn adjustment(shift: Color3) -> Color3 {
    let base = (shift.r + shift.g + shift.b) * 0.3333;
    Color3::new(shift.r - base, shift.g - base, shift.b - base)
}

pub const RENDER_SHADOW_INTENSITY: f32 = 0.75;

impl SceneLighting {
    pub fn compute(i: &LightingInputs) -> SceneLighting {
        let sky = SkyParameters::at(i.game_time, i.latitude);
        let brightness = i.brightness.clamp(0.05, 5.0);
        let sun_direction = -sky.light_direction.normalize_or(Vec3::NEG_Y);
        let sun_color = scale(sky.light_color, 0.9);
        let mut key = scale(sun_color, brightness);
        key = add(key, scale(adjustment(i.color_shift_top), len(key)));
        let mut fill = scale(sun_color, brightness * 0.4);
        fill = add(fill, scale(adjustment(i.color_shift_bottom), len(fill)));
        let fog_end = i.fog_end.max(0.2);
        let fog_start = i.fog_start.clamp(0.0, fog_end - 0.1);
        let shadow_fade = sky
            .light_direction
            .normalize_or(Vec3::Y)
            .y
            .clamp(0.0, 1.0)
            .powf(0.25);
        SceneLighting {
            ambient: i.ambient,
            key_direction: sun_direction,
            key_color: min_white(key),
            fill_color: min_white(fill),
            fog_color: i.fog_color,
            fog_start,
            fog_end,
            clear_color: sky.sky_ambient,
            outlines: i.outlines,
            shadow_intensity: shadow_fade * RENDER_SHADOW_INTENSITY,
            global_shadows: i.global_shadows,
            sky_ambient: Color3::new(
                (i.outdoor_ambient.r - i.ambient.r).max(0.0),
                (i.outdoor_ambient.g - i.ambient.g).max(0.0),
                (i.outdoor_ambient.b - i.ambient.b).max(0.0),
            ),
        }
    }
}
