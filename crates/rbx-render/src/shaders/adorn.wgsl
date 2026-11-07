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
    light_border: vec4<f32>,
    fade_distance_glow: vec4<f32>,
    outline_brightness_shadow: vec4<f32>,
    lgrid_scale: vec4<f32>,
    lgrid_offset: vec4<f32>,
    lgrid_center: vec4<f32>,
    lgrid_radius: vec4<f32>,
    shadow_matrix0: vec4<f32>,
    shadow_matrix1: vec4<f32>,
    shadow_matrix2: vec4<f32>,
    sky_color: vec4<f32>,
    sky_color2: vec4<f32>,
};

struct Pixel {
    info: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var adorn_texture: texture_2d<f32>;
@group(1) @binding(1) var adorn_sampler: sampler;
@group(1) @binding(2) var<uniform> pixel: Pixel;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) normal: vec3<f32>,
    @location(3) row0: vec4<f32>,
    @location(4) row1: vec4<f32>,
    @location(5) row2: vec4<f32>,
    @location(6) color: vec4<f32>,
    @location(7) params: vec4<f32>,
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) fog: f32,
    @location(3) world: vec3<f32>,
    @location(4) center_radius: vec4<f32>,
    @location(5) clip: vec4<f32>,
    @location(6) line_start: vec4<f32>,
    @location(7) line_end: vec4<f32>,
    @location(8) @interpolate(flat) params: vec4<f32>,
};

fn to_world(v: VertexIn, p: vec3<f32>) -> vec3<f32> {
    let c = vec4<f32>(p, 1.0);
    return vec3<f32>(dot(v.row0, c), dot(v.row1, c), dot(v.row2, c));
}

fn to_world_dir(v: VertexIn, n: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(dot(v.row0.xyz, n), dot(v.row1.xyz, n), dot(v.row2.xyz, n));
}

fn safe_normalize(n: vec3<f32>) -> vec3<f32> {
    let l = length(n);
    if (l < 1e-8) {
        return vec3<f32>(0.0);
    }
    return n / l;
}

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    var o: VertexOut;
    let material = u32(v.params.x + 0.5);
    var world = to_world(v, v.position);
    let normal = safe_normalize(to_world_dir(v, v.normal));
    o.uv = v.uv;
    o.params = v.params;
    o.color = v.color;
    o.center_radius = vec4<f32>(v.row0.w, v.row1.w, v.row2.w, length(vec3<f32>(v.row0.x, v.row1.x, v.row2.x)));
    if (material == 1u) {
        let ndotl = dot(normal, -g.lamp0_dir.xyz);
        let lighting = g.ambient_color.rgb + clamp(ndotl, 0.0, 1.0) * g.lamp0_color.rgb + clamp(-ndotl, 0.0, 1.0) * g.lamp1_color.rgb;
        o.color = vec4<f32>(v.color.rgb * lighting, v.color.a);
    } else if (material == 2u || material == 3u) {
        let ambient = select(0.5, 0.75, material == 3u);
        let light = normalize(g.camera_position.xyz - world);
        let ndotl = clamp(dot(normal, light), 0.0, 1.0);
        let lighting = ambient + (1.0 - ambient) * ndotl;
        let specular = pow(ndotl, 64.0);
        o.color = vec4<f32>(v.color.rgb * lighting + vec3<f32>(specular), v.color.a);
    } else if (material == 5u) {
        let start_w = to_world(v, vec3<f32>(1.0, 0.0, 0.0));
        let end_w = to_world(v, vec3<f32>(-1.0, 0.0, 0.0));
        let radius = v.params.y + 2.0;
        if (length(world - start_w) < length(world - end_w)) {
            let w = (g.view_projection * vec4<f32>(start_w, 1.0)).w;
            world = start_w + normal * (radius * w * pixel.info.x);
        } else {
            let w = (g.view_projection * vec4<f32>(end_w, 1.0)).w;
            world = end_w + normal * (radius * w * pixel.info.x);
        }
        var s = g.view_projection * vec4<f32>(start_w, 1.0);
        var e = g.view_projection * vec4<f32>(end_w, 1.0);
        s.y = s.y * pixel.info.z;
        e.y = e.y * pixel.info.z;
        o.line_start = s;
        o.line_end = e;
    }
    let hpos = g.view_projection * vec4<f32>(world, 1.0);
    o.position = hpos;
    o.world = world;
    o.clip = vec4<f32>(hpos.x, hpos.y * pixel.info.z, hpos.z, hpos.w);
    o.fog = (g.fog_params.z - hpos.w) * g.fog_params.w;
    return o;
}

fn apply_fog(rgb: vec3<f32>, fog: f32) -> vec3<f32> {
    return mix(g.fog_color.rgb, rgb, clamp(fog, 0.0, 1.0));
}

@fragment
fn fs_main(i: VertexOut) -> @location(0) vec4<f32> {
    let material = u32(i.params.x + 0.5);
    if (material == 4u) {
        let ray_o = i.world - i.center_radius.xyz;
        let ray_d = normalize(i.world - g.camera_position.xyz);
        let r0 = i.center_radius.w;
        let r1 = max(0.0, i.center_radius.w - 1.0);
        let b = 2.0 * dot(ray_o, ray_d);
        let c0 = dot(ray_o, ray_o) - r0 * r0;
        let c1 = dot(ray_o, ray_o) - r1 * r1;
        if (b * b < 4.0 * c0 || b * b > 4.0 * c1) {
            discard;
        }
        return i.color;
    }
    if (material == 5u) {
        let p = i.clip / i.clip.w;
        let s = i.line_start / i.line_start.w;
        let e = i.line_end / i.line_end.w;
        let dir = normalize(e.xy - s.xy);
        let f = p.xy - s.xy;
        if (dot(dir, f) < 0.0 || dot(dir, e.xy - p.xy) < 0.0) {
            discard;
        }
        let perp = vec2<f32>(dir.y, -dir.x);
        let dist = abs(dot(perp, f));
        let thickness = i.params.y;
        let high = 1.0 + (thickness - 1.0) * 0.5;
        var a = clamp(high - dist * 0.5 * pixel.info.y, 0.0, 1.0) * i.color.a;
        a = 1.0 - pow(clamp(1.0 - a, 0.0, 1.0), 1.0 / 2.2);
        return vec4<f32>(apply_fog(i.color.rgb, i.fog), a);
    }
    let result = textureSampleLevel(adorn_texture, adorn_sampler, i.uv, 0.0) * i.color;
    return vec4<f32>(apply_fog(result.rgb, i.fog), result.a);
}
