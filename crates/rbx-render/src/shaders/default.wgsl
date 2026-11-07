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

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var linear_sampler: sampler;
@group(0) @binding(3) var lgrid_texture: texture_3d<f32>;
@group(0) @binding(4) var lgrid_sampler: sampler;
@group(0) @binding(5) var shadow_map: texture_2d<f32>;
@group(0) @binding(6) var shadow_sampler: sampler;

fn shadow_prepare(world: vec3<f32>) -> vec3<f32> {
    let c = vec4<f32>(world, 1.0);
    return vec3<f32>(dot(g.shadow_matrix0, c), dot(g.shadow_matrix1, c), dot(g.shadow_matrix2, c));
}

const SHADOW_BIAS: f32 = 0.0016;

fn shadow_sample(lpos: vec3<f32>, light_shadow: f32) -> f32 {
    let sm = textureSampleLevel(shadow_map, shadow_sampler, lpos.xy, 0.0).rg;
    let sm_shadow = step(sm.x + SHADOW_BIAS, lpos.z) * clamp(9.0 - 20.0 * abs(lpos.z - 0.5), 0.0, 1.0);
    return (1.0 - sm_shadow * sm.y * g.outline_brightness_shadow.w) * light_shadow;
}

fn lgrid_prepare(world: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    return (world + normal * 6.0) * g.lgrid_scale.xyz + g.lgrid_offset.xyz;
}

fn lgrid_sample(c: vec3<f32>) -> vec4<f32> {
    let edge = step(g.lgrid_radius.xyz, abs(c - g.lgrid_center.xyz));
    let edgef = clamp(dot(edge, vec3<f32>(1.0)), 0.0, 1.0);
    let light = textureSampleLevel(lgrid_texture, lgrid_sampler, vec3<f32>(c.x, c.z, c.y), 0.0);
    return mix(light, g.light_border, edgef);
}
@group(0) @binding(2) var studs_map: texture_2d<f32>;
@group(1) @binding(0) var diffuse_map: texture_2d<f32>;

const LQMAT_FADE_FACTOR: f32 = 1.0 / 300.0;
const CFG_FAR_DIFFUSE_CUTOFF: f32 = 0.6;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) uv_studs: vec2<f32>,
    @location(4) tangent: vec3<f32>,
    @location(5) edges: vec4<f32>,
    @location(6) row0: vec4<f32>,
    @location(7) row1: vec4<f32>,
    @location(8) row2: vec4<f32>,
    @location(9) color: vec4<f32>,
    @location(10) extra: vec4<f32>,
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv_edge1: vec4<f32>,
    @location(1) uvstuds_edge2: vec4<f32>,
    @location(2) color: vec4<f32>,
    @location(3) fog: f32,
    @location(4) view_depth: vec4<f32>,
    @location(5) normal_specpower: vec4<f32>,
    @location(6) spec_reflect_kind: vec4<f32>,
    @location(7) lgrid: vec3<f32>,
    @location(8) shadow_pos: vec3<f32>,
};

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    var o: VertexOut;
    let p = vec4<f32>(v.position, 1.0);
    let world = vec3<f32>(dot(v.row0, p), dot(v.row1, p), dot(v.row2, p));
    let normal = normalize(vec3<f32>(dot(v.row0.xyz, v.normal), dot(v.row1.xyz, v.normal), dot(v.row2.xyz, v.normal)));
    let hpos = g.view_projection * vec4<f32>(world, 1.0);
    o.position = hpos;
    o.color = v.color;
    o.fog = (g.fog_params.z - hpos.w) * g.fog_params.w;
    o.view_depth = vec4<f32>(g.camera_position.xyz - world, hpos.w);
    let edges = v.edges * g.fade_distance_glow.z + 0.5 * hpos.w * g.fade_distance_glow.y;
    o.uv_edge1 = vec4<f32>(v.uv, edges.xy);
    o.uvstuds_edge2 = vec4<f32>(v.uv_studs, edges.zw);
    o.normal_specpower = vec4<f32>(normal, v.extra.y);
    o.spec_reflect_kind = vec4<f32>(v.extra.x, v.extra.z, v.extra.w, 0.0);
    o.lgrid = lgrid_prepare(world, normal);
    o.shadow_pos = shadow_prepare(world);
    return o;
}

fn sample_far(uv: vec2<f32>, fade: f32, cutoff: f32) -> vec4<f32> {
    let cscale = 1.0 / (1.0 - cutoff);
    let near_sample = textureSample(diffuse_map, linear_sampler, uv);
    let far_sample = textureSample(diffuse_map, linear_sampler, uv * 0.25);
    return mix(far_sample, near_sample, clamp(fade * cscale - cutoff * cscale, 0.0, 1.0));
}

@fragment
fn fs_main(i: VertexOut) -> @location(0) vec4<f32> {
    let flags = i32(i.spec_reflect_kind.z + 0.5);
    let kind = flags % 16;
    let receives_shadow_map = flags < 16;
    let studs = textureSample(studs_map, linear_sampler, i.uvstuds_edge2.xy);
    let fade = clamp(1.0 - i.view_depth.w * LQMAT_FADE_FACTOR, 0.0, 1.0);
    let mat_sample = sample_far(i.uv_edge1.xy, fade, CFG_FAR_DIFFUSE_CUTOFF);

    let near_sample = textureSample(diffuse_map, linear_sampler, i.uv_edge1.xy);
    var albedo: vec4<f32>;
    let low_sample = textureSample(diffuse_map, linear_sampler, i.uvstuds_edge2.xy);
    if (kind == 4) {
        albedo = vec4<f32>(mix(near_sample.rgb, low_sample.rgb, clamp(i.view_depth.w / 200.0, 0.0, 1.0)), 1.0);
    } else if (kind == 3) {
        albedo = near_sample * i.color;
    } else if (kind == 1) {
        albedo = vec4<f32>(mix(vec3<f32>(1.0), i.color.rgb, mat_sample.a) * mat_sample.rgb, i.color.a);
    } else {
        albedo = vec4<f32>(i.color.rgb * (studs.r * 2.0), i.color.a);
    }

    let normal = normalize(i.normal_specpower.xyz);
    let ndotl = dot(normal, -g.lamp0_dir.xyz);
    let key_intensity = clamp(ndotl, 0.0, 1.0) * g.lamp0_color.rgb;
    let fill_intensity = max(-ndotl, 0.0) * g.lamp1_color.rgb;
    let specular_intensity = select(0.0, i.spec_reflect_kind.x, ndotl > 0.0);
    let specular_power = i.normal_specpower.w;

    let light = lgrid_sample(i.lgrid);
    let shadow = select(light.a, shadow_sample(i.shadow_pos, light.a), receives_shadow_map);

    let diffuse = (g.ambient_color.rgb + key_intensity * shadow + fill_intensity * light.a + light.rgb) * albedo.rgb;
    let half_vec = normalize(-g.lamp0_dir.xyz + normalize(i.view_depth.xyz));
    let specular = g.lamp0_color.rgb * (specular_intensity * shadow * pow(clamp(dot(normal, half_vec), 0.0, 1.0), max(specular_power, 1.0)));

    var color = diffuse + specular;

    let view_depth_mul = i.view_depth.w * g.fade_distance_glow.y;
    let outline_fade = clamp(view_depth_mul * g.outline_brightness_shadow.x + g.outline_brightness_shadow.y, 0.0, 1.0);
    let min_inter = min(i.uv_edge1.wz, i.uvstuds_edge2.wz);
    let min_edges = min(min_inter.x, min_inter.y) / max(view_depth_mul, 1e-6);
    color = color * clamp(outline_fade * (1.5 - min_edges) + min_edges, 0.0, 1.0);

    let fog_alpha = clamp(i.fog, 0.0, 1.0);
    var alpha = albedo.a;
    if (kind == 2) {
        color = i.color.rgb * g.fade_distance_glow.w;
        alpha = 1.0 - fog_alpha * i.color.a;
    }
    color = mix(g.fog_color.rgb, color, fog_alpha);
    return vec4<f32>(color, alpha);
}

struct SkyOut {
    @builtin(position) position: vec4<f32>,
    @location(0) dir: vec3<f32>,
};

@group(1) @binding(1) var sky_faces: texture_2d_array<f32>;

@vertex
fn vs_sky(@builtin(vertex_index) index: u32) -> SkyOut {
    var corners = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    let c = corners[index];
    var o: SkyOut;
    o.position = vec4<f32>(c, 1.0, 1.0);
    o.dir = g.view_dir.xyz + c.x * g.view_right.xyz * g.view_right.w + c.y * g.view_up.xyz * g.view_up.w;
    return o;
}

@fragment
fn fs_sky(i: SkyOut) -> @location(0) vec4<f32> {
    let d = normalize(i.dir);
    let a = abs(d);
    var layer: i32;
    var uv: vec2<f32>;
    if (a.x >= a.y && a.x >= a.z) {
        if (d.x > 0.0) {
            layer = 0;
            uv = vec2<f32>(d.z, -d.y) / a.x;
        } else {
            layer = 1;
            uv = vec2<f32>(-d.z, -d.y) / a.x;
        }
    } else if (a.y >= a.z) {
        if (d.y > 0.0) {
            layer = 4;
            uv = vec2<f32>(-d.z, d.x) / a.y;
        } else {
            layer = 5;
            uv = vec2<f32>(d.z, d.x) / a.y;
        }
    } else {
        if (d.z > 0.0) {
            layer = 3;
            uv = vec2<f32>(-d.x, -d.y) / a.z;
        } else {
            layer = 2;
            uv = vec2<f32>(d.x, -d.y) / a.z;
        }
    }
    let c = textureSampleLevel(sky_faces, linear_sampler, uv * 0.5 + 0.5, layer, 0.0);
    let m = max(a.x, max(a.y, a.z));
    let wy = g.camera_position.y + d.y / m * 2700.0;
    let tint = mix(g.sky_color2, g.sky_color, wy / 1700.0);
    return vec4<f32>(c.rgb * tint.rgb, 1.0);
}

struct ShadowOut {
    @builtin(position) position: vec4<f32>,
    @location(0) depth: f32,
};

@vertex
fn vs_shadow(v: VertexIn) -> ShadowOut {
    let p = vec4<f32>(v.position, 1.0);
    let world = vec3<f32>(dot(v.row0, p), dot(v.row1, p), dot(v.row2, p));
    let l = shadow_prepare(world);
    var o: ShadowOut;
    o.position = vec4<f32>(l.x * 2.0 - 1.0, 1.0 - l.y * 2.0, l.z, 1.0);
    o.depth = l.z;
    return o;
}

@fragment
fn fs_shadow(i: ShadowOut) -> @location(0) vec4<f32> {
    return vec4<f32>(i.depth, 1.0, 0.0, 0.0);
}

struct CelestialIn {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct CelestialOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_celestial(v: CelestialIn) -> CelestialOut {
    let h = g.view_projection * vec4<f32>(v.position, 1.0);
    var o: CelestialOut;
    o.position = vec4<f32>(h.xy, h.w, h.w);
    o.uv = v.uv;
    o.color = v.color;
    return o;
}

@fragment
fn fs_celestial(i: CelestialOut) -> @location(0) vec4<f32> {
    return textureSample(diffuse_map, linear_sampler, i.uv) * i.color;
}
