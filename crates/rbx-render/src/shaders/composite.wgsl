struct Layer {
    color: vec4<f32>,
    canvas: vec4<f32>,
};

@group(0) @binding(0) var<uniform> layer: Layer;
@group(0) @binding(1) var layer_sampler: sampler;
@group(0) @binding(2) var layer_texture: texture_2d<f32>;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    var o: VertexOut;
    o.position = vec4<f32>(v.position.x * layer.canvas.x - 1.0, v.position.y * layer.canvas.y - 1.0, 0.5, 1.0);
    o.uv = v.uv;
    return o;
}

@fragment
fn fs_blit(i: VertexOut) -> @location(0) vec4<f32> {
    return textureSampleLevel(layer_texture, layer_sampler, i.uv, 0.0) * layer.color;
}

@fragment
fn fs_blend(i: VertexOut) -> @location(0) vec4<f32> {
    let t = textureSampleLevel(layer_texture, layer_sampler, i.uv, 0.0);
    return vec4<f32>(t.rgb * t.a * layer.color.rgb, t.a * layer.color.a);
}
