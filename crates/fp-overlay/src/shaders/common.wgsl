// Shared prelude, prepended to every module (misc 5, desktop-native 3.9).
// Bind contract (fp_core::gpu::PassKind): group 0, binding 0 = scene
// {width, height, time, scale}, binding 1 = pass uniforms (4 x vec4),
// bindings 2.. = read-only storage buffers of plain f32 (strides per shader).
diagnostic(off, derivative_uniformity);

@group(0) @binding(0) var<uniform> scene: vec4<f32>;

struct PassUniforms {
    a: vec4<f32>,
    b: vec4<f32>,
    c: vec4<f32>,
    d: vec4<f32>,
};
@group(0) @binding(1) var<uniform> pu: PassUniforms;

// World points (Y up) to clip space: p / size * 2 - 1.
fn to_clip(p: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(p.x / scene.x * 2.0 - 1.0, p.y / scene.y * 2.0 - 1.0, 0.0, 1.0);
}

// Triangle-strip corner of a 4-vertex quad: (id & 1, id >> 1).
fn corner(vi: u32) -> vec2<f32> {
    return vec2<f32>(f32(vi & 1u), f32(vi >> 1u));
}

fn rot(v: vec2<f32>, a: f32) -> vec2<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec2<f32>(c * v.x - s * v.y, s * v.x + c * v.y);
}

fn hash12(p0: vec2<f32>) -> f32 {
    var p = fract(p0 * vec2<f32>(123.34, 456.21));
    p += dot(p, p + 45.32);
    return fract(p.x * p.y);
}

fn tri(x: f32) -> f32 {
    return abs(fract(x) * 2.0 - 1.0);
}
