struct Globals {
    view_projection: mat4x4<f32>,
    view_right: vec4<f32>,
    view_up: vec4<f32>,
    view_dir: vec4<f32>,
    camera_position: vec4<f32>,
    ambient_color: vec4<f32>,
    lamp0_color: vec4<f32>,
    lamp0_dir: vec4<f32>,
    lamp1_color: vec4<f32>,
    fog_color: vec4<f32>,
    fog_params: vec4<f32>,
};

struct Params {
    modulate: vec4<f32>,
    throttle: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var<uniform> params: Params;
@group(1) @binding(1) var tex_sampler: sampler;
@group(1) @binding(2) var strip_sampler: sampler;
@group(1) @binding(3) var main_tex: texture_2d<f32>;
@group(1) @binding(4) var color_strip: texture_2d<f32>;
@group(1) @binding(5) var alpha_strip: texture_2d<f32>;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) lookup: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) z_offset: f32,
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) lookup: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) fog: f32,
};

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    var o: VertexOut;
    let p = vec4<f32>(v.position, 1.0);
    var h = g.view_projection * p;
    let h2 = g.view_projection * (p + vec4<f32>(g.view_dir.xyz * v.z_offset, 0.0));
    h.z = h2.z * h.w / h2.w;
    o.position = h;
    o.uv = v.uv;
    o.lookup = v.lookup;
    o.color = v.color;
    o.fog = (g.fog_params.z - h.w) * g.fog_params.w;
    return o;
}

fn strips(i: VertexOut) -> vec4<f32> {
    let c = textureSample(color_strip, strip_sampler, i.lookup);
    let a = textureSample(alpha_strip, strip_sampler, i.lookup).r;
    return vec4<f32>(c.rgb, a);
}

@fragment
fn fs_add(i: VertexOut) -> @location(0) vec4<f32> {
    let t = textureSample(main_tex, tex_sampler, i.uv);
    let v = strips(i);
    var r: vec4<f32>;
    r = vec4<f32>((t.rgb + v.rgb) * params.modulate.rgb, t.a * v.a);
    r = vec4<f32>(r.rgb * r.a, r.a);
    return vec4<f32>(mix(vec3<f32>(0.0), r.rgb, clamp(i.fog, 0.0, 1.0)), r.a);
}

@fragment
fn fs_modulate(i: VertexOut) -> @location(0) vec4<f32> {
    let t = textureSample(main_tex, tex_sampler, i.uv);
    let s = strips(i);
    let v = vec4<f32>(s.rgb * params.modulate.rgb, s.a * params.modulate.a);
    let rgb = mix(g.fog_color.rgb, t.rgb * v.rgb, clamp(i.fog, 0.0, 1.0));
    return vec4<f32>(rgb, t.a * v.a);
}

@fragment
fn fs_crazy(i: VertexOut) -> @location(0) vec4<f32> {
    let t = textureSample(main_tex, tex_sampler, i.uv);
    let a = strips(i).a;
    let r = vec4<f32>(t.rgb * params.modulate.rgb * a * t.a, params.throttle.w * t.a * a);
    return mix(vec4<f32>(0.0), r, clamp(i.fog, 0.0, 1.0));
}

@fragment
fn fs_crazy_sparkles(i: VertexOut) -> @location(0) vec4<f32> {
    let t = textureSample(main_tex, tex_sampler, i.uv);
    let v = strips(i);
    var rgb: vec3<f32>;
    if (t.a < 0.5) {
        rgb = v.rgb * params.modulate.rgb * (2.0 * t.a);
    } else {
        rgb = mix(v.rgb * params.modulate.rgb, t.rgb, 2.0 * t.a - 1.0);
    }
    let r = vec4<f32>(rgb * v.a, params.throttle.w * t.a * v.a);
    return mix(vec4<f32>(0.0), r, clamp(i.fog, 0.0, 1.0));
}

@fragment
fn fs_custom(i: VertexOut) -> @location(0) vec4<f32> {
    let t = textureSample(main_tex, tex_sampler, i.uv);
    let v = i.color;
    let r = vec4<f32>(t.rgb * v.rgb * v.a * t.a, params.throttle.w * t.a * v.a);
    return mix(vec4<f32>(0.0), r, clamp(i.fog, 0.0, 1.0));
}
