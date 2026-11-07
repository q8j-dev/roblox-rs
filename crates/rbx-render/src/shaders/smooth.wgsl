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
};

struct TerrainParams {
    layer_scale: vec4<f32>,
    water_color: vec4<f32>,
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

fn shadow_sample(lpos: vec3<f32>, light_shadow: f32) -> f32 {
    let sm = textureSampleLevel(shadow_map, shadow_sampler, lpos.xy, 0.0).rg;
    let sm_shadow = step(sm.x, lpos.z) * clamp(9.0 - 20.0 * abs(lpos.z - 0.5), 0.0, 1.0);
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
@group(1) @binding(0) var albedo_map: texture_2d<f32>;
@group(1) @binding(1) var specular_map: texture_2d<f32>;
@group(1) @binding(2) var<uniform> params: TerrainParams;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) weights: vec3<f32>,
    @location(3) uv0: vec4<f32>,
    @location(4) uv1: vec4<f32>,
    @location(5) uv2: vec4<f32>,
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) weights: vec3<f32>,
    @location(1) uv0: vec4<f32>,
    @location(2) uv1: vec4<f32>,
    @location(3) uv2: vec4<f32>,
    @location(4) normal: vec3<f32>,
    @location(5) view_depth: vec4<f32>,
    @location(6) fog: f32,
    @location(7) lgrid: vec3<f32>,
    @location(8) shadow_pos: vec3<f32>,
};

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    var o: VertexOut;
    let hpos = g.view_projection * vec4<f32>(v.position, 1.0);
    o.position = hpos;
    o.weights = v.weights;
    o.uv0 = v.uv0;
    o.uv1 = v.uv1;
    o.uv2 = v.uv2;
    o.normal = v.normal;
    o.view_depth = vec4<f32>(g.camera_position.xyz - v.position, hpos.w);
    o.fog = (g.fog_params.z - hpos.w) * g.fog_params.w;
    o.lgrid = lgrid_prepare(v.position, normalize(v.normal));
    o.shadow_pos = shadow_prepare(v.position);
    return o;
}

fn sample_map(t: texture_2d<f32>, uv: vec4<f32>) -> vec4<f32> {
    let scale = params.layer_scale.xy;
    let uvs = uv.xy * scale;
    return textureSampleGrad(t, linear_sampler, fract(uv.xy) * scale + uv.zw, dpdx(uvs), dpdy(uvs));
}

fn sample_blend(t: texture_2d<f32>, i: VertexOut) -> vec4<f32> {
    return sample_map(t, i.uv0) * i.weights.x + sample_map(t, i.uv1) * i.weights.y + sample_map(t, i.uv2) * i.weights.z;
}

@fragment
fn fs_main(i: VertexOut) -> @location(0) vec4<f32> {
    let light = lgrid_sample(i.lgrid);
    let shadow = shadow_sample(i.shadow_pos, light.a);
    let albedo = sample_blend(albedo_map, i);
    let p = sample_blend(specular_map, i);
    let fade = clamp(1.0 - i.view_depth.w * g.fade_distance_glow.y, 0.0, 1.0);
    let normal = normalize(i.normal);
    let ndotl = dot(normal, -g.lamp0_dir.xyz);
    let diffuse = (g.ambient_color.rgb + (clamp(ndotl, 0.0, 1.0) * g.lamp0_color.rgb + max(-ndotl, 0.0) * g.lamp1_color.rgb) * shadow + light.rgb + p.b * 2.0) * albedo.rgb;
    let specular_intensity = select(0.0, p.r * fade, ndotl >= 0.0);
    let specular_power = p.g * 128.0 + 0.01;
    let half_vec = normalize(-g.lamp0_dir.xyz + normalize(i.view_depth.xyz));
    let specular = g.lamp0_color.rgb * (specular_intensity * shadow * pow(clamp(dot(normal, half_vec), 0.0, 1.0), specular_power));
    let fog_alpha = clamp(i.fog, 0.0, 1.0);
    let color = mix(g.fog_color.rgb, diffuse + specular, fog_alpha);
    return vec4<f32>(color, 1.0);
}

@fragment
fn fs_water(i: VertexOut) -> @location(0) vec4<f32> {
    let light = lgrid_sample(i.lgrid);
    let normal = normalize(i.normal);
    let view = normalize(i.view_depth.xyz);
    let ndotl = dot(normal, -g.lamp0_dir.xyz);
    let base = params.water_color.rgb;
    let diffuse = (g.ambient_color.rgb + clamp(ndotl, 0.0, 1.0) * g.lamp0_color.rgb * light.a + light.rgb) * base;
    let half_vec = normalize(-g.lamp0_dir.xyz + view);
    let specular = g.lamp0_color.rgb * pow(clamp(dot(normal, half_vec), 0.0, 1.0), 256.0) * select(0.0, 1.0, ndotl > 0.0);
    let fresnel = pow(1.0 - clamp(abs(dot(normal, view)), 0.0, 1.0), 5.0) * 0.8 + 0.2;
    let sky = mix(base, g.fog_color.rgb, 0.5);
    let fog_alpha = clamp(i.fog, 0.0, 1.0);
    let color = mix(g.fog_color.rgb, mix(diffuse, sky, fresnel * 0.5) + specular, fog_alpha);
    let alpha = clamp(params.water_color.a + fresnel * 0.3, 0.0, 1.0);
    return vec4<f32>(color, alpha);
}
