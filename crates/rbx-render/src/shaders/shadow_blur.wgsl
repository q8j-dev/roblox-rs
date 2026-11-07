struct BlurParams {
    step: vec4<f32>,
};

@group(0) @binding(0) var<uniform> params: BlurParams;
@group(0) @binding(1) var blur_sampler: sampler;
@group(0) @binding(2) var source: texture_2d<f32>;
@group(0) @binding(3) var mask: texture_2d<f32>;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    var corners = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    let c = corners[index];
    var o: VertexOut;
    o.position = vec4<f32>(c, 0.0, 1.0);
    o.uv = vec2<f32>(c.x * 0.5 + 0.5, 0.5 - c.y * 0.5);
    return o;
}

@fragment
fn fs_main(i: VertexOut) -> @location(0) vec4<f32> {
    let n = 3;
    let sigma = 1.5;
    let sigma_n1 = 1.0 / sqrt(2.0 * 3.1415926 * sigma * sigma);
    let sigma_n2 = 1.0 / (2.0 * sigma * sigma);
    var depth = 1.0;
    var color = 0.0;
    var weight = 0.0;
    for (var k = -n; k <= n; k = k + 1) {
        let x = f32(k);
        let w = exp(-x * x * sigma_n2) * sigma_n1;
        let data = textureSampleLevel(source, blur_sampler, i.uv + params.step.xy * x, 0.0);
        depth = min(depth, data.x);
        color = color + data.y * w;
        weight = weight + w;
    }
    let m = textureSampleLevel(mask, blur_sampler, i.uv, 0.0).r;
    return vec4<f32>(depth, color * m / weight, 0.0, 0.0);
}
