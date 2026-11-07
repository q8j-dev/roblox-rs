@group(0) @binding(0) var gui_sampler: sampler;
@group(1) @binding(0) var gui_texture: texture_2d<f32>;

struct VertexIn {
    @location(0) position: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    var o: VertexOut;
    o.position = v.position;
    o.uv = v.uv;
    o.color = v.color;
    return o;
}

@fragment
fn fs_main(i: VertexOut) -> @location(0) vec4<f32> {
    return textureSample(gui_texture, gui_sampler, i.uv) * i.color;
}
