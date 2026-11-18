// Rain (other-weather 2.1.8): screen flash, ripples, strokes.
// Flash uniforms: a = (flash, target height). Strokes: 8 f32 each, ripples: 4 f32 each.
@group(0) @binding(2) var<storage, read> data: array<f32>;

// ---- flash -------------------------------------------------------------

struct FlashOut {
    @builtin(position) pos: vec4<f32>,
};

@vertex
fn flash_vs(@builtin(vertex_index) vi: u32) -> FlashOut {
    var o: FlashOut;
    o.pos = vec4<f32>(corner(vi) * 2.0 - vec2<f32>(1.0), 0.0, 1.0);
    return o;
}

@fragment
fn flash_fs(i: FlashOut) -> @location(0) vec4<f32> {
    let from_top = saturate(i.pos.y / max(pu.a.y, 1.0));
    let a = pu.a.x * (1.0 - 0.6 * from_top);
    return vec4<f32>(vec3<f32>(0.86, 0.9, 1.0) * a, a);
}

// ---- ripples -----------------------------------------------------------

struct RippleOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) radius: f32,
    @location(2) @interpolate(flat) alpha: f32,
};

@vertex
fn ripple_vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> RippleOut {
    let b = ii * 4u;
    let center = vec2<f32>(data[b], data[b + 1u]);
    let radius = data[b + 2u];
    let ex = radius + 1.5;
    let ey = ex * 0.28 + 1.5;
    let off = (corner(vi) * 2.0 - vec2<f32>(1.0)) * vec2<f32>(ex, ey);
    var o: RippleOut;
    o.pos = to_clip(center + off);
    // Un-squash the ground-plane ellipse.
    o.local = vec2<f32>(off.x, off.y / 0.28);
    o.radius = radius;
    o.alpha = data[b + 3u];
    return o;
}

@fragment
fn ripple_fs(i: RippleOut) -> @location(0) vec4<f32> {
    let d = abs(length(i.local) - i.radius);
    var a = (1.0 - smoothstep(0.3, 1.2, d)) * i.alpha;
    if (i.local.y > 0.0) {
        a = a * 0.55;
    }
    if (a < 0.002) {
        discard;
    }
    return vec4<f32>(vec3<f32>(0.82, 0.88, 0.97) * a, a);
}

// ---- strokes -----------------------------------------------------------

struct StrokeOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) along: f32,
    @location(1) across: f32,
    @location(2) @interpolate(flat) len: f32,
    @location(3) @interpolate(flat) hw: f32,
    @location(4) @interpolate(flat) alpha: f32,
    @location(5) @interpolate(flat) glow: f32,
};

@vertex
fn stroke_vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> StrokeOut {
    let b = ii * 8u;
    let head = vec2<f32>(data[b], data[b + 1u]);
    let tail = vec2<f32>(data[b + 2u], data[b + 3u]);
    let width = data[b + 4u];
    let alpha = data[b + 5u];
    let glow = data[b + 6u];
    let delta = head - tail;
    let len = max(length(delta), 0.001);
    let way = delta / len;
    let side = vec2<f32>(-way.y, way.x);
    let reach = width * 0.5 + 1.0;
    let c = corner(vi);
    let along = mix(-reach, len + reach, c.x);
    let across = (c.y * 2.0 - 1.0) * reach;
    var o: StrokeOut;
    o.pos = to_clip(tail + way * along + side * across);
    o.along = along;
    o.across = across;
    o.len = len;
    o.hw = width * 0.5;
    o.alpha = alpha * min(width, 1.0);
    o.glow = glow;
    return o;
}

@fragment
fn stroke_fs(i: StrokeOut) -> @location(0) vec4<f32> {
    let beyond = max(max(-i.along, i.along - i.len), 0.0);
    let d = length(vec2<f32>(i.across, beyond));
    var edge = 0.0;
    if (i.glow > 1.5) {
        edge = pow(saturate(1.0 - d / (i.hw + 1.0)), 2.0);
    } else {
        edge = saturate(max(i.hw, 0.5) + 0.5 - d);
    }
    var head_fade = 1.0;
    if (i.glow <= 0.5) {
        head_fade = 0.1 + 0.9 * pow(saturate(i.along / i.len), 1.4);
    }
    let a = edge * i.alpha * head_fade;
    if (a < 0.002) {
        discard;
    }
    let k = saturate(d / max(i.hw, 0.5));
    var col = mix(vec3<f32>(0.9, 0.94, 1.0), vec3<f32>(0.5, 0.58, 0.7), k);
    if (i.glow > 0.5) {
        col = mix(vec3<f32>(1.0), vec3<f32>(0.68, 0.72, 1.0), k);
    }
    return vec4<f32>(col * a, a);
}
