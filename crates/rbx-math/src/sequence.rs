use crate::color::Color3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct NumberRange {
    pub min: f32,
    pub max: f32,
}

impl NumberRange {
    pub fn new(min: f32, max: f32) -> NumberRange {
        NumberRange { min, max }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct NumberSequenceKeypoint {
    pub time: f32,
    pub value: f32,
    pub envelope: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct ColorSequenceKeypoint {
    pub time: f32,
    pub value: Color3,
    pub envelope: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NumberSequence {
    pub keypoints: Vec<NumberSequenceKeypoint>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColorSequence {
    pub keypoints: Vec<ColorSequenceKeypoint>,
}

pub const MAX_SEQUENCE_KEYPOINTS: usize = 20;

impl Default for NumberSequence {
    fn default() -> Self {
        NumberSequence::constant(0.0)
    }
}

impl Default for ColorSequence {
    fn default() -> Self {
        ColorSequence::constant(Color3::WHITE)
    }
}

impl NumberSequence {
    pub fn constant(value: f32) -> NumberSequence {
        NumberSequence::range(value, value)
    }

    pub fn range(a: f32, b: f32) -> NumberSequence {
        NumberSequence {
            keypoints: vec![
                NumberSequenceKeypoint {
                    time: 0.0,
                    value: a,
                    envelope: 0.0,
                },
                NumberSequenceKeypoint {
                    time: 1.0,
                    value: b,
                    envelope: 0.0,
                },
            ],
        }
    }

    pub fn validate(keypoints: &[NumberSequenceKeypoint]) -> Result<(), &'static str> {
        if keypoints.len() < 2 {
            return Err("NumberSequence: requires at least 2 keypoints");
        }
        if keypoints.len() > MAX_SEQUENCE_KEYPOINTS {
            return Err("NumberSequence: max. number of keypoints exceeded.");
        }
        for pair in keypoints.windows(2) {
            if pair[1].time < pair[0].time {
                return Err("NumberSequence: all keypoints must be ordered by time");
            }
        }
        if keypoints[0].time.abs() > 1e-4 {
            return Err("NumberSequence must start at time=0.0");
        }
        if (keypoints[keypoints.len() - 1].time - 1.0).abs() > 1e-4 {
            return Err("NumberSequence must end at time=1.0");
        }
        Ok(())
    }

    pub fn evaluate(&self, t: f32) -> f32 {
        let k = &self.keypoints;
        if k.is_empty() {
            return 0.0;
        }
        if t <= k[0].time {
            return k[0].value;
        }
        for pair in k.windows(2) {
            if t <= pair[1].time {
                let span = pair[1].time - pair[0].time;
                let a = if span > 0.0 {
                    (t - pair[0].time) / span
                } else {
                    0.0
                };
                return pair[0].value + (pair[1].value - pair[0].value) * a;
            }
        }
        k[k.len() - 1].value
    }
}

impl ColorSequence {
    pub fn constant(value: Color3) -> ColorSequence {
        ColorSequence::range(value, value)
    }

    pub fn range(a: Color3, b: Color3) -> ColorSequence {
        ColorSequence {
            keypoints: vec![
                ColorSequenceKeypoint {
                    time: 0.0,
                    value: a,
                    envelope: 0.0,
                },
                ColorSequenceKeypoint {
                    time: 1.0,
                    value: b,
                    envelope: 0.0,
                },
            ],
        }
    }

    pub fn validate(keypoints: &[ColorSequenceKeypoint]) -> Result<(), &'static str> {
        if keypoints.len() < 2 {
            return Err("ColorSequence: requires at least 2 keypoints");
        }
        if keypoints.len() > MAX_SEQUENCE_KEYPOINTS {
            return Err("ColorSequence: max. number of keypoints exceeded.");
        }
        for pair in keypoints.windows(2) {
            if pair[1].time < pair[0].time {
                return Err("ColorSequence: all keypoints must be ordered by time");
            }
        }
        for k in keypoints {
            let c = k.value;
            if c.r.min(c.g).min(c.b) < 0.0 || c.r.max(c.g).max(c.b) > 1.0 {
                return Err("ColorSequence: color value out of range");
            }
        }
        if keypoints[0].time.abs() > 1e-4 {
            return Err("ColorSequence must start at time=0.0");
        }
        if (keypoints[keypoints.len() - 1].time - 1.0).abs() > 1e-4 {
            return Err("ColorSequence must end at time=1.0");
        }
        Ok(())
    }

    pub fn evaluate(&self, t: f32) -> Color3 {
        let k = &self.keypoints;
        if k.is_empty() {
            return Color3::WHITE;
        }
        if t <= k[0].time {
            return k[0].value;
        }
        for pair in k.windows(2) {
            if t <= pair[1].time {
                let span = pair[1].time - pair[0].time;
                let a = if span > 0.0 {
                    (t - pair[0].time) / span
                } else {
                    0.0
                };
                return pair[0].value.lerp(pair[1].value, a);
            }
        }
        k[k.len() - 1].value
    }
}
