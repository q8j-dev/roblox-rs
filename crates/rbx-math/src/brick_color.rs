use crate::brick_color_table::{self, ALL_COLORS, BrickColorInfo, PALETTE};
use crate::color::{Color3, Color3uint8};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BrickColor(u16);

impl Default for BrickColor {
    fn default() -> Self {
        BrickColor(brick_color_table::DEFAULT)
    }
}

fn info(number: u16) -> Option<&'static BrickColorInfo> {
    ALL_COLORS.iter().find(|c| c.number == number)
}

impl BrickColor {
    pub const DEFAULT: BrickColor = BrickColor(brick_color_table::DEFAULT);
    pub const WHITE: BrickColor = BrickColor(brick_color_table::WHITE);
    pub const GRAY: BrickColor = BrickColor(brick_color_table::GRAY);
    pub const DARK_GRAY: BrickColor = BrickColor(brick_color_table::DARK_GRAY);
    pub const BLACK: BrickColor = BrickColor(brick_color_table::BLACK);
    pub const RED: BrickColor = BrickColor(brick_color_table::RED);
    pub const YELLOW: BrickColor = BrickColor(brick_color_table::YELLOW);
    pub const GREEN: BrickColor = BrickColor(brick_color_table::GREEN);
    pub const BASEPLATE_GREEN: BrickColor = BrickColor(brick_color_table::BASEPLATE_GREEN);
    pub const BLUE: BrickColor = BrickColor(brick_color_table::BLUE);

    pub fn new(number: i64) -> BrickColor {
        if (0..=u16::MAX as i64).contains(&number) && info(number as u16).is_some() {
            BrickColor(number as u16)
        } else {
            BrickColor::DEFAULT
        }
    }

    pub fn is_valid_number(number: i64) -> bool {
        (0..=u16::MAX as i64).contains(&number) && info(number as u16).is_some()
    }

    pub fn number(&self) -> u16 {
        self.0
    }

    pub fn name(&self) -> &'static str {
        info(self.0).map(|c| c.name).unwrap_or("Medium stone grey")
    }

    pub fn color3uint8(&self) -> Color3uint8 {
        let rgb = info(self.0).map(|c| c.rgb).unwrap_or([163, 162, 165]);
        Color3uint8 {
            r: rgb[0],
            g: rgb[1],
            b: rgb[2],
        }
    }

    pub fn color3(&self) -> Color3 {
        self.color3uint8().to_color3()
    }

    pub fn parse(name: &str) -> BrickColor {
        let mut matches: Vec<u16> = ALL_COLORS
            .iter()
            .filter(|c| c.name == name)
            .map(|c| c.number)
            .collect();
        matches.sort_unstable();
        let mut found: Option<u16> = None;
        for number in matches {
            found = Some(number);
            if PALETTE.contains(&number) {
                break;
            }
        }
        found.map(BrickColor).unwrap_or_default()
    }

    pub fn closest_u8(color: Color3uint8) -> BrickColor {
        let mut best = BrickColor::DEFAULT.0;
        let mut closest = i32::MAX;
        let mut sorted: Vec<&BrickColorInfo> = ALL_COLORS.iter().collect();
        sorted.sort_by_key(|c| c.number);
        for c in sorted {
            let d = (c.rgb[0] as i32 - color.r as i32).abs()
                + (c.rgb[1] as i32 - color.g as i32).abs()
                + (c.rgb[2] as i32 - color.b as i32).abs();
            if d < closest {
                best = c.number;
                closest = d;
                if d == 0 {
                    break;
                }
            }
        }
        BrickColor(best)
    }

    pub fn closest(color: Color3) -> BrickColor {
        let mut best = BrickColor::DEFAULT.0;
        let mut closest = f32::MAX;
        let mut sorted: Vec<&BrickColorInfo> = ALL_COLORS.iter().collect();
        sorted.sort_by_key(|c| c.number);
        for c in sorted {
            let m = Color3::from_rgb_u8(c.rgb[0], c.rgb[1], c.rgb[2]);
            let d = (m.r - color.r).abs() + (m.g - color.g).abs() + (m.b - color.b).abs();
            if d < closest {
                best = c.number;
                closest = d;
                if d == 0.0 {
                    break;
                }
            }
        }
        BrickColor(best)
    }

    pub fn palette() -> impl Iterator<Item = BrickColor> {
        PALETTE.iter().map(|&n| BrickColor(n))
    }

    pub fn palette_index(index: usize) -> BrickColor {
        PALETTE
            .get(index)
            .map(|&n| BrickColor(n))
            .unwrap_or_default()
    }

    pub fn rendering_palette() -> impl Iterator<Item = BrickColor> {
        brick_color_table::RENDERING_PALETTE
            .iter()
            .map(|&n| BrickColor(n))
    }

    pub fn closest_rendering_palette_index(&self) -> usize {
        let target = self.color3();
        let mut best = 0;
        let mut closest = f32::MAX;
        for (i, c) in BrickColor::rendering_palette().enumerate() {
            if c == *self {
                return i;
            }
            let m = c.color3();
            let d = (m.r - target.r).abs() + (m.g - target.g).abs() + (m.b - target.b).abs();
            if d < closest {
                best = i;
                closest = d;
            }
        }
        best
    }

    pub fn all() -> impl Iterator<Item = BrickColor> {
        ALL_COLORS.iter().map(|c| BrickColor(c.number))
    }

    pub fn random(seed: u64) -> BrickColor {
        let mut x = seed ^ 0x9e37_79b9_7f4a_7c15;
        x ^= x >> 33;
        x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
        x ^= x >> 33;
        BrickColor(PALETTE[(x % PALETTE.len() as u64) as usize])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_colors() {
        assert_eq!(BrickColor::new(21).name(), "Bright red");
        assert_eq!(BrickColor::new(99999), BrickColor::DEFAULT);
        assert_eq!(BrickColor::parse("Bright red").number(), 21);
        assert_eq!(BrickColor::default().number(), 194);
        assert_eq!(
            BrickColor::closest_u8(BrickColor::new(21).color3uint8()).number(),
            21
        );
        assert_eq!(BrickColor::palette().count(), 128);
    }
}
