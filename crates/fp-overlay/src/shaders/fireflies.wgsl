// Fireflies (other-weather 2.4.4), additive blend. 6 f32 per sprite:
// position.xy, glow (lit), hue, size, pad.
@group(0) @binding(2) var<storage, read> data: array<f32>;

struct FlyOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) glow: f32,
    @location(2) @interpolate(flat) hue: f32,
    @location(3) @interpolate(flat) size: f32,
};

@vertex
fn fly_vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> FlyOut {
    let b = ii * 6u;
    let center = vec2<f32>(data[b], data[b + 1u]);
    let size = max(data[b + 4u], 0.001);
    let off = (corner(vi) * 2.0 - vec2<f32>(1.0)) * 36.0 * size;
    var o: FlyOut;
    o.pos = to_clip(center + off);
    o.local = off;
    o.glow = data[b + 2u];
    o.hue = data[b + 3u];
    o.size = size;
    return o;
}

@fragment
fn fly_fs(i: FlyOut) -> @location(0) vec4<f32> {
    let d = length(i.local) / i.size;
    let core = exp(-d * d / 4.5);
    let halo = (0.5 * exp(-d / 9.0) + 0.35 * exp(-d * d / 60.0)) * (1.0 - smoothstep(19.8, 36.0, d));
    let light = (core + halo) * i.glow;
    if (light < 0.003) {
        discard;
    }
    var col = mix(vec3<f32>(1.0, 0.92, 0.45), vec3<f32>(0.78, 1.0, 0.38), i.hue);
    col = mix(col, vec3<f32>(1.0, 1.0, 0.9), core * 0.6);
    return vec4<f32>(col * light, light * 0.5);
}
