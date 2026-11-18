// Starfall (other-weather 2.3.5), additive blend: meteor trails and star sprites.
// Trails: 7 f32 each. Stars: 6 f32 each.
@group(0) @binding(2) var<storage, read> data: array<f32>;

// ---- trails ------------------------------------------------------------

struct TrailOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) along: f32,
    @location(1) across: f32,
    @location(2) @interpolate(flat) len: f32,
    @location(3) @interpolate(flat) width: f32,
    @location(4) @interpolate(flat) alpha: f32,
    @location(5) @interpolate(flat) hue: f32,
};

@vertex
fn trail_vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> TrailOut {
    let b = ii * 7u;
    let head = vec2<f32>(data[b], data[b + 1u]);
    let tail = vec2<f32>(data[b + 2u], data[b + 3u]);
    // Guard: zero-width trails would divide by zero in the fragment.
    let width = max(data[b + 4u], 0.001);
    let delta = head - tail;
    let len = max(length(delta), 0.001);
    let way = delta / len;
    let side = vec2<f32>(-way.y, way.x);
    let g = width * 5.0 + 2.0;
    let c = corner(vi);
    let along = mix(-g, len + g, c.x);
    let across = (c.y * 2.0 - 1.0) * g;
    var o: TrailOut;
    o.pos = to_clip(tail + way * along + side * across);
    o.along = along;
    o.across = across;
    o.len = len;
    o.width = width;
    o.alpha = data[b + 5u];
    o.hue = data[b + 6u];
    return o;
}

@fragment
fn trail_fs(i: TrailOut) -> @location(0) vec4<f32> {
    let t = saturate(i.along / i.len);
    let w = i.width * 0.5 * (0.5 + 0.5 * t);
    let over = max(max(-i.along, i.along - i.len), 0.0);
    let d = length(vec2<f32>(i.across, over));
    let core = exp(-d * d / max(w * w, 0.15)) * pow(max(t, 0.0001), 1.4);
    let halo = exp(-d / (i.width * 1.6)) * 0.3 * t * t;
    let rr = (i.along - i.len) * (i.along - i.len) + i.across * i.across;
    let head = 1.6 * exp(-rr / (2.4 * i.width * i.width)) + 0.55 * exp(-rr / (7.0 * i.width * i.width));
    let light = (core + halo + head) * i.alpha;
    if (light < 0.003) {
        discard;
    }
    var tint = vec3<f32>(0.88, 0.9, 1.0);
    if (i.hue >= 0.25) {
        tint = mix(vec3<f32>(0.9, 0.7, 1.0), vec3<f32>(1.0, 0.52, 0.72), saturate(2.0 * i.hue - 1.0));
    }
    let col = mix(tint, vec3<f32>(1.0, 0.97, 0.98), saturate(head));
    return vec4<f32>(col * light, light * 0.6);
}

// ---- stars -------------------------------------------------------------

struct StarOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) alpha: f32,
    @location(2) @interpolate(flat) hue: f32,
};

@vertex
fn star_vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> StarOut {
    let b = ii * 6u;
    let center = vec2<f32>(data[b], data[b + 1u]);
    let size = max(data[b + 2u], 0.001);
    let angle = data[b + 3u];
    let c = corner(vi) * 2.0 - vec2<f32>(1.0);
    var o: StarOut;
    o.pos = to_clip(center + rot(c * (size + 2.0), angle));
    o.local = c * (size + 2.0) / size;
    o.alpha = data[b + 4u];
    o.hue = data[b + 5u];
    return o;
}

@fragment
fn star_fs(i: StarOut) -> @location(0) vec4<f32> {
    let q = abs(i.local);
    let rays = exp(-14.0 * q.y) * saturate(1.0 - q.x) + exp(-14.0 * q.x) * saturate(1.0 - q.y);
    let core = exp(-dot(q, q) * 10.0);
    let glow = 0.25 * exp(-4.0 * length(q));
    let light = (0.85 * rays + core + glow) * i.alpha;
    if (light < 0.003) {
        discard;
    }
    var tint = vec3<f32>(1.0);
    if (i.hue >= 0.75) {
        tint = vec3<f32>(0.75, 0.86, 1.0);
    } else if (i.hue >= 0.25) {
        tint = vec3<f32>(1.0, 0.9, 0.62);
    }
    let col = mix(tint, vec3<f32>(1.0), core * 0.7);
    return vec4<f32>(col * light, light * 0.6);
}
