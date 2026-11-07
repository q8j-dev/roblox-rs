use bytemuck::{Pod, Zeroable};
use rbx_dom::InstanceId;
use rbx_math::{CFrame, ColorSequence, NumberSequence, Vec3};
use std::collections::HashMap;
use std::f32::consts::PI;

const THROTTLE_DIST: f32 = 200.0;
const CUTOFF_DIST: f32 = 1000.0;
const MAX_LIFE: f32 = 20.0;
const MAX_EMISSION_RATE: f32 = 400.0;
const MAX_PARTICLES_PER_EMITTER: usize = 60000;
const CACHE_POINTS: usize = 32;
const MAX_PARTICLE_SIZE: f32 = 200.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shader {
    Add,
    Modulate,
    Crazy,
    CrazySparkles,
    Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Blend {
    None,
    Additive,
    Multiplicative,
    AlphaBlend,
    Premultiplied,
    AlphaOne,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Appearance {
    pub main: String,
    pub color_strip: String,
    pub alpha_strip: String,
    pub baseline: i32,
    pub blend: Blend,
    pub shader: Shader,
}

#[derive(Clone, Copy, Debug)]
struct Particle {
    zpos: f32,
    pos: Vec3,
    vel: Vec3,
    rot: f32,
    spin: f32,
    life: f32,
    life_span: f32,
    sx: f32,
    sy: f32,
    cline: u32,
}

#[derive(Clone, Debug)]
struct Curves {
    color_min: [Vec3; CACHE_POINTS],
    color_max: [Vec3; CACHE_POINTS],
    alpha: [(f32, f32); CACHE_POINTS],
    size: [(f32, f32); CACHE_POINTS],
}

pub struct Emitter {
    pub appearance: Appearance,
    particles: Vec<Particle>,
    cframe: CFrame,
    prev_cframe: CFrame,
    emission_counter: f32,
    life: (f32, f32),
    extent: Vec3,
    emission_rate: f32,
    speed: (f32, f32),
    spread: (f32, f32),
    local_force: Vec3,
    global_force: Vec3,
    dampening: f32,
    rotation: (f32, f32),
    spin: (f32, f32),
    size_x: (f32, f32),
    size_y: (f32, f32),
    growth: (f32, f32),
    linear_velocity: Vec3,
    angular_velocity: Vec3,
    velocity_inheritance: f32,
    locked: bool,
    spherical: (f32, f32),
    curves: Option<Curves>,
    pub modulate: [f32; 4],
    pub z_offset: f32,
    pub blend_ratio: f32,
    strip_height: u32,
}

fn lerp(a: f32, b: f32, s: f32) -> f32 {
    a + (b - a) * s
}

fn sample_pair(p: &[(f32, f32); CACHE_POINTS], t: f32, tr: f32) -> f32 {
    let ut = (CACHE_POINTS - 1) as f32 * t.clamp(0.0, 1.0);
    let i = (ut as usize).min(CACHE_POINTS - 2);
    let r = ut - i as f32;
    let lo = lerp(p[i].0, p[i + 1].0, r);
    let hi = lerp(p[i].1, p[i + 1].1, r);
    lerp(lo, hi, tr)
}

fn sample_color(a: &[Vec3; CACHE_POINTS], b: &[Vec3; CACHE_POINTS], t: f32, tr: f32) -> Vec3 {
    let ut = (CACHE_POINTS - 1) as f32 * t.clamp(0.0, 1.0);
    let i = (ut as usize).min(CACHE_POINTS - 2);
    let r = ut - i as f32;
    a[i].lerp(a[i + 1], r).lerp(b[i].lerp(b[i + 1], r), tr)
}

fn resample_numbers(seq: &NumberSequence, lo: f32, hi: f32) -> [(f32, f32); CACHE_POINTS] {
    let mut out = [(0.0, 0.0); CACHE_POINTS];
    let k = &seq.keypoints;
    if k.len() < 2 {
        let v = k
            .first()
            .map(|p| (p.value - p.envelope, p.value + p.envelope))
            .unwrap_or((0.0, 0.0));
        return [(v.0.clamp(lo, hi), v.1.clamp(lo, hi)); CACHE_POINTS];
    }
    let mut src = 0usize;
    let mut t = 0.0f32;
    let dt = 1.0 / (CACHE_POINTS as f32 - 1.0) - 1e-5;
    out[0] = (
        (k[0].value - k[0].envelope).clamp(lo, hi),
        (k[0].value + k[0].envelope).clamp(lo, hi),
    );
    for o in out.iter_mut().skip(1) {
        t += dt;
        while src + 2 < k.len() && k[src + 1].time < t {
            src += 1;
        }
        let span = (k[src + 1].time - k[src].time).max(1e-6);
        let s = (t - k[src].time) / span;
        let v = lerp(k[src].value, k[src + 1].value, s);
        let e = lerp(k[src].envelope, k[src + 1].envelope, s);
        *o = ((v - e).clamp(lo, hi), (v + e).clamp(lo, hi));
    }
    out
}

fn resample_colors(seq: &ColorSequence) -> [Vec3; CACHE_POINTS] {
    let mut out = [Vec3::ONE; CACHE_POINTS];
    let k = &seq.keypoints;
    let c = |i: usize| Vec3::new(k[i].value.r, k[i].value.g, k[i].value.b);
    if k.len() < 2 {
        let v = if k.is_empty() { Vec3::ONE } else { c(0) };
        return [v; CACHE_POINTS];
    }
    let mut src = 0usize;
    let mut t = 0.0f32;
    let dt = 1.0 / (CACHE_POINTS as f32 - 1.0) - 1e-5;
    out[0] = c(0);
    for o in out.iter_mut().skip(1) {
        t += dt;
        while src + 2 < k.len() && k[src + 1].time < t {
            src += 1;
        }
        let span = (k[src + 1].time - k[src].time).max(1e-6);
        let s = (t - k[src].time) / span;
        *o = c(src).lerp(c(src + 1), s);
    }
    out
}

struct Random(u64);

impl Random {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    fn uniform(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }
}

impl Emitter {
    pub fn new(appearance: Appearance) -> Emitter {
        Emitter {
            appearance,
            particles: Vec::new(),
            cframe: CFrame::IDENTITY,
            prev_cframe: CFrame::IDENTITY,
            emission_counter: 0.0,
            life: (5.0, 5.0),
            extent: Vec3::splat(0.5),
            emission_rate: 10.0,
            speed: (0.0, 0.0),
            spread: (0.0, 0.0),
            local_force: Vec3::ZERO,
            global_force: Vec3::ZERO,
            dampening: 0.0,
            rotation: (0.0, 0.0),
            spin: (0.0, 0.0),
            size_x: (1.0, 1.0),
            size_y: (1.0, 1.0),
            growth: (0.0, 0.0),
            linear_velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
            velocity_inheritance: 0.0,
            locked: false,
            spherical: (PI / 2.0, PI / 2.0),
            curves: None,
            modulate: [1.0; 4],
            z_offset: 0.0,
            blend_ratio: 0.5,
            strip_height: 256,
        }
    }

    pub fn count(&self) -> usize {
        self.particles.len()
    }

    fn set_life(&mut self, a: f32, b: f32) {
        self.life = (a.min(b).clamp(0.0, MAX_LIFE), a.max(b).clamp(0.0, MAX_LIFE));
    }

    fn set_rate(&mut self, r: f32) {
        self.emission_rate = r.clamp(0.0, MAX_EMISSION_RATE);
    }

    fn set_box(&mut self, half: Vec3) {
        self.extent = half;
    }

    fn emit(&mut self, n: i32, rng: &mut Random) -> i32 {
        if self.particles.is_empty() {
            self.prev_cframe = self.cframe;
        }
        let n = n.min((MAX_PARTICLES_PER_EMITTER - self.particles.len()) as i32);
        if n <= 0 {
            return 0;
        }
        let fixed =
            (32766.99 * self.appearance.baseline as f32 / self.strip_height.max(1) as f32) as u32;
        for _ in 0..n {
            let local = Vec3::new(
                rng.uniform(-self.extent.x, self.extent.x),
                rng.uniform(-self.extent.y, self.extent.y),
                rng.uniform(-self.extent.z, self.extent.z),
            );
            let base = if self.locked {
                self.prev_cframe
            } else {
                self.cframe
            };
            let pos = base.point_to_world_space(local);
            let theta = self.spherical.0 + rng.uniform(-self.spread.0, self.spread.0);
            let phi = self.spherical.1 + rng.uniform(-self.spread.1, self.spread.1);
            let radius = rng.uniform(self.speed.0, self.speed.1);
            let lv = Vec3::new(
                radius * phi.sin() * theta.cos(),
                radius * phi.sin() * theta.sin(),
                radius * phi.cos(),
            );
            let mut vel = base.vector_to_world_space(lv);
            if !self.locked && self.velocity_inheritance != 0.0 {
                let offset = pos - self.cframe.translation;
                vel += (self.linear_velocity + self.angular_velocity.cross(offset))
                    * self.velocity_inheritance;
            }
            let life = rng.uniform(self.life.0, self.life.1);
            self.particles.push(Particle {
                zpos: 0.0,
                pos,
                vel,
                rot: rng.uniform(self.rotation.0, self.rotation.1),
                spin: rng.uniform(self.spin.0, self.spin.1),
                life,
                life_span: life,
                sx: rng.uniform(self.size_x.0, self.size_x.1),
                sy: rng.uniform(self.size_y.0, self.size_y.1),
                cline: if self.appearance.baseline < 0 {
                    rng.uniform(0.0, 32766.99) as u32
                } else {
                    fixed
                },
            });
        }
        n
    }

    fn simulate(&self, p: &mut Particle, dt: f32, ws_accel: Vec3) {
        p.life = (p.life - dt).max(0.0);
        let mut accel = ws_accel;
        if self.locked {
            accel = self.cframe.vector_to_world_space(self.global_force)
                + self.cframe.vector_to_world_space(self.local_force);
            p.vel = self
                .cframe
                .vector_to_world_space(self.prev_cframe.vector_to_object_space(p.vel));
            p.pos = self
                .cframe
                .point_to_world_space(self.prev_cframe.point_to_object_space(p.pos));
        }
        p.pos += p.vel * dt + accel * (0.5 * dt * dt);
        p.vel += accel * dt;
        p.vel = p.vel.lerp(Vec3::ZERO, self.dampening * dt);
        p.sx += self.growth.0 * dt;
        p.sy += self.growth.1 * dt;
    }

    fn sim(&mut self, dt: f32, cam: Vec3, cam_dir: Vec3, rng: &mut Random) {
        let dist = (cam - self.cframe.translation).length();
        let dist_factor =
            (1.0 - (dist - THROTTLE_DIST) / (CUTOFF_DIST - THROTTLE_DIST)).clamp(0.0, 1.0);
        let n = self.emission_counter as i32;
        self.emission_counter -= self.emit(n, rng) as f32;
        self.emission_counter += dt * self.emission_rate * dist_factor;
        if !self.particles.is_empty() {
            let ws_accel = self.cframe.vector_to_world_space(self.local_force) + self.global_force;
            let mut j = 0;
            while j < self.particles.len() {
                let p = self.particles[j];
                if p.life < 0.001 || p.sx < 0.0 || p.sy < 0.0 {
                    self.particles.swap_remove(j);
                    continue;
                }
                let mut p = p;
                self.simulate(&mut p, dt, ws_accel);
                p.zpos = (p.pos - cam).dot(cam_dir);
                self.particles[j] = p;
                j += 1;
            }
        }
        self.prev_cframe = self.cframe;
    }

    fn vertices(&mut self, right: Vec3, up: Vec3, out: &mut Vec<ParticleVertex>) {
        self.particles.sort_by(|a, b| {
            b.zpos
                .partial_cmp(&a.zpos)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for p in &self.particles {
            let mut sx = p.sx;
            let mut sy = p.sy;
            let mut color = [1.0f32; 4];
            if let Some(c) = &self.curves {
                let nl = 1.0 - p.life / p.life_span.max(1e-6);
                let tr = p.cline as f32 / 32767.0;
                sx = sample_pair(&c.size, nl, tr);
                sy = sx;
                let col = sample_color(&c.color_min, &c.color_max, nl, tr);
                color = [col.x, col.y, col.z, sample_pair(&c.alpha, nl, tr)];
            }
            let angle = p.rot + p.spin * p.life;
            let (sr, cr) = angle.sin_cos();
            let rs = [cr * sx, -sr * sx, sr * sy, cr * sy];
            let life = 1.0 - (p.life / p.life_span.max(1e-6)).clamp(0.0, 1.0);
            let lookup = [life, p.cline as f32 / 32767.0];
            let corner = |dx: f32, dy: f32| {
                let offset = right * (dx * rs[0] + dy * rs[1]) + up * (dx * rs[2] + dy * rs[3]);
                ParticleVertex {
                    position: (p.pos + offset).to_array(),
                    uv: [(dx + 1.0) * 0.5, 1.0 - (dy + 1.0) * 0.5],
                    lookup,
                    color,
                    z_offset: self.z_offset,
                }
            };
            let v = [
                corner(-1.0, -1.0),
                corner(1.0, -1.0),
                corner(-1.0, 1.0),
                corner(1.0, 1.0),
            ];
            for i in [0usize, 1, 2, 2, 1, 3] {
                out.push(v[i]);
            }
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct ParticleVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub lookup: [f32; 2],
    pub color: [f32; 4],
    pub z_offset: f32,
}

impl ParticleVertex {
    pub const ATTRIBUTES: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x2,
        2 => Float32x2,
        3 => Float32x4,
        4 => Float32,
    ];
}

#[derive(Clone, Debug, PartialEq)]
pub enum EffectKind {
    Sparkles {
        enabled: bool,
        color: [f32; 3],
    },
    Smoke {
        enabled: bool,
        color: [f32; 3],
        size: f32,
        rise: f32,
        opacity: f32,
    },
    Fire {
        enabled: bool,
        color: [f32; 3],
        secondary: [f32; 3],
        size: f32,
        heat: f32,
    },
    ForceField,
    Explosion {
        position: Vec3,
    },
    Custom(Box<CustomEmitterDesc>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct CustomEmitterDesc {
    pub enabled: bool,
    pub rate: f32,
    pub speed: (f32, f32),
    pub spread: f32,
    pub rotation: (f32, f32),
    pub rot_speed: (f32, f32),
    pub lifetime: (f32, f32),
    pub acceleration: Vec3,
    pub drag: f32,
    pub locked: bool,
    pub velocity_inheritance: f32,
    pub light_emission: f32,
    pub z_offset: f32,
    pub direction: u32,
    pub texture: String,
    pub color: ColorSequence,
    pub transparency: NumberSequence,
    pub size: NumberSequence,
    pub emit_total: u64,
}

#[derive(Clone, Debug)]
pub struct EffectDesc {
    pub id: InstanceId,
    pub kind: EffectKind,
    pub cframe: CFrame,
    pub part_size: Vec3,
    pub linear_velocity: Vec3,
    pub angular_velocity: Vec3,
}

fn appearance(
    main: &str,
    color: &str,
    alpha: &str,
    baseline: i32,
    shader: Shader,
    blend: Blend,
) -> Appearance {
    Appearance {
        main: main.into(),
        color_strip: color.into(),
        alpha_strip: alpha.into(),
        baseline,
        blend,
        shader,
    }
}

const DEG: f32 = 1.0 / 57.3;

type ExplosionSpec = (
    Appearance,
    f32,
    f32,
    (f32, f32),
    (f32, f32),
    f32,
    (f32, f32),
    (f32, f32),
    (f32, f32),
    Vec3,
    Vec3,
    (f32, f32),
    Vec3,
    i32,
    f32,
);

pub type ParticleDraw = (InstanceId, usize, u32, u32);

pub struct Effect {
    pub emitters: Vec<(Emitter, CFrame)>,
    kind: EffectKind,
    emitted: u64,
}

impl Effect {
    fn configure(&mut self, kind: &EffectKind, part_size: Vec3, rng: &mut Random) {
        let rebuild = std::mem::discriminant(&self.kind) != std::mem::discriminant(kind)
            || self.emitters.is_empty();
        let common_alpha = "rbxasset://textures/particles/common_alpha.dds";
        let ensure = |emitters: &mut Vec<(Emitter, CFrame)>, i: usize, a: Appearance| {
            while emitters.len() <= i {
                emitters.push((Emitter::new(a.clone()), CFrame::IDENTITY));
            }
            emitters[i].0.appearance = a;
        };
        match kind {
            EffectKind::Sparkles { enabled, color } => {
                let c = [color[0], color[1], color[2], 0.9];
                let a = appearance(
                    "rbxasset://textures/particles/sparkles_main.dds",
                    "rbxasset://textures/particles/sparkles_color.dds",
                    common_alpha,
                    -1,
                    Shader::CrazySparkles,
                    Blend::Premultiplied,
                );
                ensure(&mut self.emitters, 0, a.clone());
                ensure(&mut self.emitters, 1, a);
                let en = *enabled as i32 as f32;
                let e = &mut self.emitters[0].0;
                e.dampening = 0.2;
                e.global_force = Vec3::ZERO;
                e.growth = (0.5, 0.5);
                e.local_force = Vec3::new(0.0, 1.0, 0.0);
                e.rotation = (-90.0 * DEG, 90.0 * DEG);
                e.size_x = (0.37, 0.37);
                e.size_y = (0.37, 0.37);
                e.speed = (5.0, 5.0);
                e.spin = (40.0 * DEG, 100.0 * DEG);
                e.spread = (100.0 * DEG, 100.0 * DEG);
                e.set_rate(30.0 * en);
                e.set_box(Vec3::splat(0.2));
                e.set_life(1.3, 1.3);
                e.modulate = c;
                e.blend_ratio = 0.6;
                let e = &mut self.emitters[1].0;
                e.dampening = 2.0;
                e.global_force = Vec3::ZERO;
                e.growth = (0.2, 0.2);
                e.local_force = Vec3::ZERO;
                e.rotation = (-90.0 * DEG, 90.0 * DEG);
                e.size_x = (0.1, 0.1);
                e.size_y = (0.1, 0.1);
                e.speed = (8.0, 8.0);
                e.spin = (-500.0 * DEG, 500.0 * DEG);
                e.spread = (150.0 * DEG, 150.0 * DEG);
                e.set_rate(5.0 * en);
                e.set_box(Vec3::splat(0.2));
                e.set_life(1.7, 1.7);
                e.modulate = c;
                e.blend_ratio = 0.6;
                self.emitters[1].1 = CFrame::from_translation(Vec3::new(0.0, 4.0, 0.0));
                self.emitters.truncate(2);
            }
            EffectKind::Smoke {
                enabled,
                color,
                size,
                rise,
                opacity,
            } => {
                let size = size.clamp(0.1, 100.0);
                let max_size = 10.0 + size;
                let rise = rise.clamp(-25.0, 25.0);
                let opacity = opacity.clamp(0.0, 1.0);
                let life = 5.0;
                ensure(
                    &mut self.emitters,
                    0,
                    appearance(
                        "rbxasset://textures/particles/smoke_main.dds",
                        "rbxasset://textures/particles/smoke_color.dds",
                        common_alpha,
                        -1,
                        Shader::Modulate,
                        Blend::AlphaBlend,
                    ),
                );
                let e = &mut self.emitters[0].0;
                e.dampening = 0.1;
                e.global_force = Vec3::new(0.0, 0.0, 0.4);
                let g = (max_size - size) / life;
                e.growth = (g, g);
                e.local_force = Vec3::ZERO;
                e.rotation = (-90.0 * DEG, 90.0 * DEG);
                e.size_x = (size, size);
                e.size_y = (size, size);
                e.speed = (rise * 0.9, rise);
                e.spin = (-20.0 * DEG, 20.0 * DEG);
                e.spread = (30.0 * DEG, 30.0 * DEG);
                e.set_rate(7.0 * (*enabled as i32 as f32));
                e.set_box(Vec3::ONE);
                e.set_life(life, life);
                e.modulate = [color[0], color[1], color[2], opacity];
                self.emitters.truncate(1);
            }
            EffectKind::Fire {
                enabled,
                color,
                secondary,
                size,
                heat,
            } => {
                let size = size.clamp(2.0, 30.0) / 3.5;
                let boxsize = size / 8.0;
                let heat = heat.clamp(-25.0, 25.0);
                let en = *enabled as i32 as f32;
                ensure(
                    &mut self.emitters,
                    0,
                    appearance(
                        "rbxasset://textures/particles/fire_main.dds",
                        "rbxasset://textures/particles/fire_color.dds",
                        "rbxasset://textures/particles/fire_alpha.dds",
                        -1,
                        Shader::Crazy,
                        Blend::Premultiplied,
                    ),
                );
                ensure(
                    &mut self.emitters,
                    1,
                    appearance(
                        "rbxasset://textures/particles/fire_sparks_main.dds",
                        "rbxasset://textures/particles/fire_sparks_color.dds",
                        common_alpha,
                        -1,
                        Shader::Add,
                        Blend::Additive,
                    ),
                );
                let force = Vec3::new(0.0, 0.5 * (size * size / 4.0 + 0.7 * heat), 0.0);
                let e = &mut self.emitters[0].0;
                e.dampening = 0.4;
                e.global_force = Vec3::ZERO;
                e.growth = (-0.8 * size, -0.8 * size);
                e.local_force = force;
                e.rotation = (-90.0 * DEG, 90.0 * DEG);
                e.size_x = (size * 1.1, size * 1.1);
                e.size_y = (size * 1.1, size * 1.1);
                let sp = 0.4 * (0.2 * size * size + 0.2 * heat);
                e.speed = (sp, sp);
                e.spin = (100.0 * DEG, 100.0 * DEG);
                e.spread = (10.0 * DEG, 10.0 * DEG);
                e.set_rate(65.0 * en);
                e.set_box(Vec3::splat(boxsize));
                e.set_life(1.0, 2.0);
                e.modulate = [4.0 * color[0], 4.0 * color[1], 4.0 * color[2], 4.0];
                e.blend_ratio = 0.4;
                let size2 = 0.2 * size;
                let life2 = 2.0;
                let growth2 = -size2 / life2;
                let e = &mut self.emitters[1].0;
                e.dampening = 0.4;
                e.global_force = Vec3::ZERO;
                e.growth = (growth2, growth2);
                e.local_force = force;
                e.rotation = (-90.0 * DEG, 90.0 * DEG);
                e.size_x = (size2, size2);
                e.size_y = (size2, size2);
                let sp2 = 0.4 * (0.1 * size * size + 0.2 * heat);
                e.speed = (sp2, sp2);
                e.spin = (100.0 * DEG, 100.0 * DEG);
                e.spread = (10.0 * DEG, 10.0 * DEG);
                e.set_rate(65.0 * en);
                e.set_box(Vec3::splat(boxsize * 2.0));
                e.set_life(life2 / 2.0, life2);
                e.modulate = [secondary[0], secondary[1], secondary[2], 1.0];
                self.emitters[1].1 =
                    CFrame::from_translation(Vec3::new(0.0, 3.0 * (0.3 * size + 0.05 * heat), 0.0));
                self.emitters.truncate(2);
            }
            EffectKind::ForceField => {
                ensure(
                    &mut self.emitters,
                    0,
                    appearance(
                        "rbxasset://textures/particles/forcefield_vortex_main.dds",
                        "rbxasset://textures/particles/forcefield_vortex_color.dds",
                        common_alpha,
                        -1,
                        Shader::Add,
                        Blend::AlphaOne,
                    ),
                );
                ensure(
                    &mut self.emitters,
                    1,
                    appearance(
                        "rbxasset://textures/sparkle.png",
                        "rbxasset://textures/particles/forcefield_vortex_color.dds",
                        common_alpha,
                        -1,
                        Shader::Add,
                        Blend::AlphaOne,
                    ),
                );
                ensure(
                    &mut self.emitters,
                    2,
                    appearance(
                        "rbxasset://textures/particles/forcefield_glow_main.dds",
                        "rbxasset://textures/particles/forcefield_glow_color.dds",
                        "rbxasset://textures/particles/forcefield_glow_alpha.dds",
                        -1,
                        Shader::Crazy,
                        Blend::Premultiplied,
                    ),
                );
                let e = &mut self.emitters[0].0;
                e.dampening = 0.0;
                e.global_force = Vec3::new(0.0, -2.0, 0.0);
                e.growth = (1.6, 1.6);
                e.local_force = Vec3::new(0.0, 5.0, 0.0);
                e.rotation = (400.0 * DEG, 400.0 * DEG);
                e.size_x = (2.0, 2.0);
                e.size_y = (0.2, 0.2);
                e.speed = (2.0, 2.0);
                e.spin = (200.0 * DEG, 200.0 * DEG);
                e.spread = (10.0 * DEG, 10.0 * DEG);
                e.set_rate(40.0);
                e.set_box(Vec3::splat(0.5));
                e.set_life(1.0, 1.0);
                e.modulate = [1.0; 4];
                e.locked = true;
                self.emitters[0].1 = CFrame::from_translation(Vec3::new(0.0, -1.9, 0.0));
                let e = &mut self.emitters[1].0;
                e.dampening = 0.0;
                e.global_force = Vec3::ZERO;
                e.growth = (0.14, 0.14);
                e.local_force = Vec3::new(0.0, 2.0, 0.0);
                e.rotation = (0.0, 0.0);
                e.size_x = (0.04, 0.04);
                e.size_y = (0.04, 0.04);
                e.speed = (3.0, 3.0);
                e.spin = (-500.0 * DEG, 500.0 * DEG);
                e.spread = (30.0 * DEG, 30.0 * DEG);
                e.set_rate(12.0);
                e.set_box(Vec3::splat(0.8));
                e.set_life(1.3, 1.3);
                e.modulate = [1.0; 4];
                e.locked = true;
                let e = &mut self.emitters[2].0;
                e.dampening = 0.0;
                e.global_force = Vec3::ZERO;
                e.growth = (0.5, 0.5);
                e.local_force = Vec3::ZERO;
                e.rotation = (0.0, 0.0);
                e.size_x = (4.4, 4.4);
                e.size_y = (4.4, 4.4);
                e.speed = (0.0, 0.0);
                e.spin = (200.0 * DEG, 200.0 * DEG);
                e.spread = (2.0 * DEG, 2.0 * DEG);
                e.set_rate(15.0);
                e.set_box(Vec3::ZERO);
                e.set_life(2.0, 2.0);
                e.modulate = [1.0; 4];
                e.z_offset = 2.5;
                e.blend_ratio = 0.15;
                e.locked = true;
                self.emitters[2].1 = CFrame::from_translation(Vec3::new(0.0, -0.5, 0.0));
                self.emitters.truncate(3);
            }
            EffectKind::Explosion { position } => {
                if !rebuild {
                    self.kind = kind.clone();
                    return;
                }
                self.emitters.clear();
                let at = CFrame::from_translation(*position);
                let specs: [ExplosionSpec; 5] = [
                    (
                        appearance(
                            "rbxasset://textures/particles/explosion01_core_main.dds",
                            "rbxasset://textures/particles/explosion_color.dds",
                            "rbxasset://textures/particles/explosion01_core_alpha.png",
                            -1,
                            Shader::Crazy,
                            Blend::Premultiplied,
                        ),
                        0.2,
                        4.0,
                        (2.0, 2.0),
                        (400.0 * DEG, -400.0 * DEG),
                        5.0,
                        (20.0, 20.0),
                        (90.0 * DEG, -90.0 * DEG),
                        (PI, PI),
                        Vec3::ZERO,
                        Vec3::ONE,
                        (0.5, 0.7),
                        Vec3::ZERO,
                        100,
                        0.0,
                    ),
                    (
                        appearance(
                            "rbxasset://textures/particles/explosion01_core_main.dds",
                            "rbxasset://textures/particles/explosion_color.dds",
                            "rbxasset://textures/particles/explosion01_core_alpha.png",
                            -1,
                            Shader::Crazy,
                            Blend::Premultiplied,
                        ),
                        0.15,
                        2.0,
                        (-2.0, -2.0),
                        (100.0 * DEG, -100.0 * DEG),
                        1.3,
                        (60.0, 80.0),
                        (0.0, 0.0),
                        (PI, PI),
                        Vec3::new(0.0, 23.0, 0.0),
                        Vec3::ONE,
                        (1.0, 1.0),
                        Vec3::ZERO,
                        50,
                        0.01,
                    ),
                    (
                        appearance(
                            "rbxasset://textures/particles/explosion01_shockwave_main.dds",
                            "rbxasset://textures/particles/smoke_color.dds",
                            "rbxasset://textures/particles/explosion01_core_alpha.png",
                            0,
                            Shader::Add,
                            Blend::AlphaOne,
                        ),
                        0.0,
                        0.0,
                        (200.0, 200.0),
                        (100.0 * DEG, -100.0 * DEG),
                        0.0,
                        (0.0, 0.0),
                        (0.0, 0.0),
                        (0.0, 0.0),
                        Vec3::ZERO,
                        Vec3::ZERO,
                        (0.5, 0.7),
                        Vec3::ZERO,
                        3,
                        0.0,
                    ),
                    (
                        appearance(
                            "rbxasset://textures/particles/explosion01_core_main.dds",
                            "rbxasset://textures/particles/explosion01_smoke_color_new.dds",
                            "rbxasset://textures/particles/explosion01_smoke_alpha.dds",
                            0,
                            Shader::Modulate,
                            Blend::AlphaBlend,
                        ),
                        0.0,
                        2.0,
                        (1.0, 1.0),
                        (200.0 * DEG, -200.0 * DEG),
                        6.0,
                        (5.0, 20.0),
                        (-30.0 * DEG, 30.0 * DEG),
                        (PI, PI),
                        Vec3::new(0.0, 10.0, 0.0),
                        Vec3::ZERO,
                        (1.0, 2.0),
                        Vec3::ZERO,
                        60,
                        0.02,
                    ),
                    (
                        appearance(
                            "rbxasset://textures/particles/explosion01_implosion_main.dds",
                            "rbxasset://textures/particles/explosion01_implosion_color.png",
                            "rbxasset://textures/particles/explosion01_core_alpha.png",
                            0,
                            Shader::Add,
                            Blend::Additive,
                        ),
                        0.0,
                        0.0,
                        (14.0, 14.0),
                        (300.0 * DEG, -300.0 * DEG),
                        20.0,
                        (20.0, 20.0),
                        (0.0, 0.0),
                        (0.0, 0.0),
                        Vec3::ZERO,
                        Vec3::ZERO,
                        (0.1, 0.3),
                        Vec3::ZERO,
                        2,
                        -0.02,
                    ),
                ];
                for (
                    a,
                    ratio,
                    damp,
                    growth,
                    rot,
                    size,
                    speed,
                    spin,
                    spread,
                    local_force,
                    half,
                    life,
                    global,
                    count,
                    z,
                ) in specs
                {
                    let mut e = Emitter::new(a);
                    e.blend_ratio = ratio;
                    e.dampening = damp;
                    e.growth = growth;
                    e.rotation = rot;
                    e.size_x = (size, size);
                    e.size_y = (size, size);
                    e.speed = speed;
                    e.spin = spin;
                    e.spread = spread;
                    e.local_force = local_force;
                    e.global_force = global;
                    e.set_rate(0.0);
                    e.set_box(half);
                    e.set_life(life.0, life.1);
                    e.z_offset = z;
                    e.cframe = at;
                    e.prev_cframe = at;
                    e.emit(count, rng);
                    self.emitters.push((e, CFrame::IDENTITY));
                }
            }
            EffectKind::Custom(c) => {
                ensure(
                    &mut self.emitters,
                    0,
                    appearance(&c.texture, "", "", -1, Shader::Custom, Blend::Premultiplied),
                );
                let e = &mut self.emitters[0].0;
                let half = part_size * 0.5;
                e.blend_ratio = 1.0 - c.light_emission;
                e.set_rate(c.rate * (c.enabled as i32 as f32));
                e.speed = c.speed;
                let spread = c.spread / 180.0 * PI;
                e.spread = (spread, spread);
                e.rotation = (c.rotation.0 / 180.0 * PI, c.rotation.1 / 180.0 * PI);
                e.spin = (c.rot_speed.0 / 180.0 * PI, c.rot_speed.1 / 180.0 * PI);
                e.z_offset = c.z_offset;
                e.set_life(c.lifetime.0, c.lifetime.1);
                e.global_force = c.acceleration;
                e.set_box(half);
                e.dampening = c.drag;
                e.locked = c.locked;
                e.velocity_inheritance = c.velocity_inheritance;
                e.spherical = match c.direction {
                    1 => (PI / 2.0, PI / 2.0),
                    4 => (-PI / 2.0, PI / 2.0),
                    0 => (0.0, PI / 2.0),
                    3 => (0.0, -PI / 2.0),
                    5 => (0.0, PI),
                    2 => (0.0, 0.0),
                    _ => (PI / 2.0, PI / 2.0),
                };
                let colors = resample_colors(&c.color);
                let mut alpha = resample_numbers(&c.transparency, 0.0, 1.0);
                for a in alpha.iter_mut() {
                    *a = (1.0 - a.1, 1.0 - a.0);
                }
                e.curves = Some(Curves {
                    color_min: colors,
                    color_max: colors,
                    alpha,
                    size: resample_numbers(&c.size, 0.0, MAX_PARTICLE_SIZE),
                });
                if c.emit_total > self.emitted {
                    let n =
                        (c.emit_total - self.emitted).min(MAX_PARTICLES_PER_EMITTER as u64) as i32;
                    e.emit(n, rng);
                }
                self.emitted = c.emit_total;
                self.emitters.truncate(1);
            }
        }
        self.kind = kind.clone();
    }
}

pub struct ParticleSystem {
    pub effects: HashMap<InstanceId, Effect>,
    rng: Random,
}

impl Default for ParticleSystem {
    fn default() -> Self {
        ParticleSystem {
            effects: HashMap::new(),
            rng: Random(0x9e37_79b9_7f4a_7c15),
        }
    }
}

impl ParticleSystem {
    pub fn update(&mut self, effects: &[EffectDesc], dt: f32, cam: Vec3, cam_dir: Vec3) {
        let present: std::collections::HashSet<InstanceId> = effects.iter().map(|e| e.id).collect();
        self.effects.retain(|id, _| present.contains(id));
        let dt = dt.min(0.066);
        for desc in effects {
            let fresh = !self.effects.contains_key(&desc.id);
            let effect = self.effects.entry(desc.id).or_insert_with(|| Effect {
                emitters: Vec::new(),
                kind: desc.kind.clone(),
                emitted: match &desc.kind {
                    EffectKind::Custom(c) => c.emit_total,
                    _ => 0,
                },
            });
            if fresh || effect.kind != desc.kind {
                effect.configure(&desc.kind, desc.part_size, &mut self.rng);
            }
            let explosion = matches!(desc.kind, EffectKind::Explosion { .. });
            for (e, offset) in effect.emitters.iter_mut() {
                if !explosion {
                    e.cframe = desc.cframe * *offset;
                    e.linear_velocity = desc.linear_velocity;
                    e.angular_velocity = desc.angular_velocity;
                }
                if dt > 0.0 {
                    e.sim(dt, cam, cam_dir, &mut self.rng);
                }
            }
        }
    }

    pub fn draw_list(
        &mut self,
        right: Vec3,
        up: Vec3,
        cam: Vec3,
        cam_dir: Vec3,
    ) -> (Vec<ParticleVertex>, Vec<ParticleDraw>) {
        let mut vertices = Vec::new();
        let mut draws: Vec<(f32, InstanceId, usize, u32, u32)> = Vec::new();
        for (id, effect) in self.effects.iter_mut() {
            for (k, (e, _)) in effect.emitters.iter_mut().enumerate() {
                if e.count() == 0 {
                    continue;
                }
                let start = vertices.len() as u32;
                e.vertices(right, up, &mut vertices);
                let depth = (e.cframe.translation - cam).dot(cam_dir) - 0.1 - e.z_offset;
                draws.push((depth, *id, k, start, vertices.len() as u32 - start));
            }
        }
        draws.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        (
            vertices,
            draws.into_iter().map(|d| (d.1, d.2, d.3, d.4)).collect(),
        )
    }

    pub fn particle_count(&self) -> usize {
        self.effects
            .values()
            .flat_map(|e| e.emitters.iter())
            .map(|(e, _)| e.count())
            .sum()
    }

    pub fn emitter(&self, id: InstanceId, k: usize) -> Option<&Emitter> {
        self.effects
            .get(&id)
            .and_then(|e| e.emitters.get(k))
            .map(|e| &e.0)
    }

    pub fn set_strip_height(&mut self, url: &str, height: u32) {
        for effect in self.effects.values_mut() {
            for (e, _) in effect.emitters.iter_mut() {
                if e.appearance.color_strip == url {
                    e.strip_height = height;
                }
            }
        }
    }
}
