use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Color3 {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Color3uint8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color3 {
    pub const WHITE: Color3 = Color3::new(1.0, 1.0, 1.0);
    pub const BLACK: Color3 = Color3::new(0.0, 0.0, 0.0);

    pub const fn new(r: f32, g: f32, b: f32) -> Color3 {
        Color3 { r, g, b }
    }

    pub fn from_rgb_u8(r: u8, g: u8, b: u8) -> Color3 {
        Color3::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
    }

    pub fn from_hsv(h: f32, s: f32, v: f32) -> Color3 {
        let h = (h.rem_euclid(1.0)) * 6.0;
        let i = h.floor();
        let f = h - i;
        let p = v * (1.0 - s);
        let q = v * (1.0 - s * f);
        let t = v * (1.0 - s * (1.0 - f));
        match i as i32 {
            0 => Color3::new(v, t, p),
            1 => Color3::new(q, v, p),
            2 => Color3::new(p, v, t),
            3 => Color3::new(p, q, v),
            4 => Color3::new(t, p, v),
            _ => Color3::new(v, p, q),
        }
    }

    pub fn to_hsv(&self) -> (f32, f32, f32) {
        let max = self.r.max(self.g).max(self.b);
        let min = self.r.min(self.g).min(self.b);
        let d = max - min;
        let v = max;
        let s = if max > 0.0 { d / max } else { 0.0 };
        let h = if d <= 0.0 {
            0.0
        } else if max == self.r {
            ((self.g - self.b) / d).rem_euclid(6.0) / 6.0
        } else if max == self.g {
            ((self.b - self.r) / d + 2.0) / 6.0
        } else {
            ((self.r - self.g) / d + 4.0) / 6.0
        };
        (h, s, v)
    }

    pub fn lerp(&self, other: Color3, t: f32) -> Color3 {
        Color3::new(
            self.r + (other.r - self.r) * t,
            self.g + (other.g - self.g) * t,
            self.b + (other.b - self.b) * t,
        )
    }

    pub fn to_u8(&self) -> Color3uint8 {
        let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        Color3uint8 {
            r: c(self.r),
            g: c(self.g),
            b: c(self.b),
        }
    }

    pub fn to_array(&self) -> [f32; 3] {
        [self.r, self.g, self.b]
    }
}

impl Color3uint8 {
    pub fn from_argb(argb: u32) -> Color3uint8 {
        Color3uint8 {
            r: (argb >> 16) as u8,
            g: (argb >> 8) as u8,
            b: argb as u8,
        }
    }

    pub fn to_argb(&self) -> u32 {
        0xff00_0000 | ((self.r as u32) << 16) | ((self.g as u32) << 8) | self.b as u32
    }

    pub fn to_color3(&self) -> Color3 {
        Color3::from_rgb_u8(self.r, self.g, self.b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argb_round_trip() {
        let c = Color3uint8 {
            r: 12,
            g: 200,
            b: 33,
        };
        assert_eq!(Color3uint8::from_argb(c.to_argb()), c);
        assert_eq!(c.to_color3().to_u8(), c);
    }

    #[test]
    fn hsv_round_trip() {
        let c = Color3::new(0.2, 0.6, 0.9);
        let (h, s, v) = c.to_hsv();
        let back = Color3::from_hsv(h, s, v);
        assert!((back.r - c.r).abs() < 1e-5 && (back.g - c.g).abs() < 1e-5);
    }
}
