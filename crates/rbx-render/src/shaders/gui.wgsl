@group(0) @binding(0) var gui_sampler: sampler;
@group(1) @binding(0) var gui_texture: texture_2d<f32>;

struct VertexIn {
    @location(0) position: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) local: vec2<f32>,
    @location(4) shape: vec4<f32>,
    @location(5) radii: vec4<f32>,
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) local: vec2<f32>,
    @location(3) shape: vec4<f32>,
    @location(4) radii: vec4<f32>,
};

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    var o: VertexOut;
    o.position = v.position;
    o.uv = v.uv;
    o.color = v.color;
    o.local = v.local;
    o.shape = v.shape;
    o.radii = v.radii;
    return o;
}

fn rounded_box(p: vec2<f32>, half: vec2<f32>, radii: vec4<f32>) -> f32 {
    var r = radii.x;
    if (p.x > 0.0 && p.y < 0.0) {
        r = radii.y;
    } else if (p.x > 0.0 && p.y > 0.0) {
        r = radii.z;
    } else if (p.x < 0.0 && p.y > 0.0) {
        r = radii.w;
    }
    r = min(r, min(half.x, half.y));
    let q = abs(p) - half + vec2<f32>(r, r);
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fs_main(i: VertexOut) -> @location(0) vec4<f32> {
    var c = textureSample(gui_texture, gui_sampler, i.uv) * i.color;
    if (i.shape.w > 0.5) {
        let d = rounded_box(i.local, i.shape.xy, i.radii);
        var a = clamp(0.5 - d, 0.0, 1.0);
        if (i.shape.z > 0.0) {
            a = a * clamp(0.5 + d + i.shape.z, 0.0, 1.0);
        }
        c = vec4<f32>(c.rgb, c.a * a);
    }
    return c;
}
