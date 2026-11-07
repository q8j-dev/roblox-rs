use bytemuck::{Pod, Zeroable};
use rbx_math::Vec3;

pub const SKY_SIZE: f32 = 2700.0;
const STAR_DISTANCE: f32 = 2500.0;
const TWINKLE_RATE: u32 = 40;
const TWINKLE_RATIO: f32 = 0.2;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct CelestialVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

impl CelestialVertex {
    pub const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Brightness {
    None,
    Dim,
    Dimmer,
    Bright,
}

#[derive(Clone, Copy)]
struct Star {
    position: Vec3,
    intensity: f32,
    color: [f32; 4],
    twinkle: bool,
}

pub struct StarField {
    stars: Vec<Star>,
    count: u32,
    brightness: Brightness,
    counter: u32,
    seed: u64,
}

impl Default for StarField {
    fn default() -> Self {
        StarField {
            stars: Vec::new(),
            count: 0,
            brightness: Brightness::None,
            counter: 0,
            seed: 0x2545_f491_4f6c_dd1d,
        }
    }
}

impl StarField {
    fn random(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed >> 40) as f32 / (1u64 << 24) as f32
    }

    fn uniform(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.random()
    }

    fn unit_vector(&mut self) -> Vec3 {
        loop {
            let v = Vec3::new(
                self.uniform(-1.0, 1.0),
                self.uniform(-1.0, 1.0),
                self.uniform(-1.0, 1.0),
            );
            let l = v.length_squared();
            if l > 1e-4 && l <= 1.0 {
                return v / l.sqrt();
            }
        }
    }

    fn color(&mut self, intensity: f32) -> [f32; 4] {
        let t = self.random();
        let rgb = if t < 0.15 {
            [1.0, 0.8, 0.7]
        } else if t < 0.55 {
            [1.0, 1.0, 1.0]
        } else if t < 0.8 {
            [0.7, 0.8, 1.0]
        } else {
            [0.75, 0.5, 1.0]
        };
        [rgb[0], rgb[1], rgb[2], intensity.clamp(0.0, 1.0)]
    }

    fn create(&mut self, count: u32) {
        self.stars.clear();
        let twinkle = (count as f32 * TWINKLE_RATIO) as u32;
        for i in 0..count {
            let is_twinkle = i >= count - twinkle;
            let mut r = self.unit_vector();
            let mut intensity;
            if is_twinkle {
                r.y = r.y.abs();
                intensity = self.uniform(0.2, 0.7);
                let t = r.y.abs();
                if t < 0.3 {
                    intensity *= 0.7;
                } else if t < 0.6 {
                    intensity *= 0.9;
                }
            } else {
                intensity = self.uniform(0.3, 0.8);
                let t = r.y.abs();
                if t < 0.15 {
                    intensity *= 0.6;
                } else if t < 0.3 {
                    intensity *= 0.75;
                } else if t < 0.6 {
                    intensity *= 0.9;
                }
            }
            intensity *= intensity;
            self.stars.push(Star {
                position: r * STAR_DISTANCE,
                intensity,
                color: [1.0; 4],
                twinkle: is_twinkle,
            });
        }
        self.count = count;
    }

    fn update_normal(&mut self) {
        for i in 0..self.stars.len() {
            if self.stars[i].twinkle {
                continue;
            }
            let mut intensity = self.stars[i].intensity;
            intensity *= match self.brightness {
                Brightness::Bright => 5.0 * 0.3,
                Brightness::Dimmer => 2.5 * 0.15,
                _ => 0.6,
            };
            self.stars[i].color = self.color(intensity);
        }
    }

    fn update_twinkle(&mut self) {
        for i in 0..self.stars.len() {
            if !self.stars[i].twinkle {
                continue;
            }
            let random = self.uniform(0.3, 1.7);
            let factor = if self.brightness == Brightness::Dimmer {
                0.5
            } else {
                1.0
            };
            let intensity = self.stars[i].intensity * random * factor + 0.2;
            self.stars[i].color = self.color(intensity);
        }
    }

    pub fn update(&mut self, count: u32, shown: bool, moon_naive_y: f32) {
        if !shown || count == 0 {
            self.stars.clear();
            self.count = 0;
            return;
        }
        let old = self.brightness;
        self.brightness = if moon_naive_y <= -0.3 {
            Brightness::None
        } else if moon_naive_y <= -0.1 {
            Brightness::Dim
        } else if moon_naive_y <= 0.2 {
            Brightness::Dimmer
        } else {
            Brightness::Bright
        };
        if self.count != count {
            self.create(count);
            self.update_normal();
            self.update_twinkle();
        } else if old != self.brightness {
            self.update_normal();
            self.update_twinkle();
        }
        self.counter += 1;
        if self.counter >= TWINKLE_RATE {
            self.counter = 0;
            if self.brightness != Brightness::None {
                self.update_twinkle();
            }
        }
    }

    pub fn vertices(
        &self,
        eye: Vec3,
        right: Vec3,
        up: Vec3,
        world_per_pixel: f32,
        out: &mut Vec<CelestialVertex>,
    ) {
        if self.brightness == Brightness::None {
            return;
        }
        let half = world_per_pixel;
        for s in &self.stars {
            if s.twinkle && self.brightness == Brightness::Dim {
                continue;
            }
            quad(eye + s.position, right * half, up * half, s.color, out);
        }
    }
}

pub fn quad(center: Vec3, right: Vec3, up: Vec3, color: [f32; 4], out: &mut Vec<CelestialVertex>) {
    let corners = [
        (center - right - up, [0.0, 1.0]),
        (center - right + up, [0.0, 0.0]),
        (center + right + up, [1.0, 0.0]),
        (center + right - up, [1.0, 1.0]),
    ];
    for i in [0usize, 1, 2, 0, 2, 3] {
        out.push(CelestialVertex {
            position: corners[i].0.to_array(),
            uv: corners[i].1,
            color,
        });
    }
}
