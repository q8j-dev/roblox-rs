struct Screen {
    size: vec4<f32>,
};

@group(0) @binding(0) var<uniform> screen: Screen;
@group(0) @binding(1) var gui_sampler: sampler;
@group(1) @binding(0) var gui_texture: texture_2d<f32>;

struct VertexIn {
    @location(0) position: vec2<f32>,
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
    let ndc = vec2<f32>(v.position.x / screen.size.x * 2.0 - 1.0, 1.0 - v.position.y / screen.size.y * 2.0);
    o.position = vec4<f32>(ndc, 0.0, 1.0);
    o.uv = v.uv;
    o.color = v.color;
    return o;
}

@fragment
fn fs_main(i: VertexOut) -> @location(0) vec4<f32> {
    return textureSample(gui_texture, gui_sampler, i.uv) * i.color;
}
