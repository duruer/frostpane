// Snow flakes (closed form, snow-sim 3.3) and loose sprites (snow-sim 4.1, 4.2).
// SnowFlakes buffer: 13 f32 per flake. SnowLoose buffer: 8 f32 per sprite.
// Flake uniforms: a = (now, wind, breeze, span), b.x = margin (30).
@group(0) @binding(2) var<storage, read> data: array<f32>;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) coord: vec2<f32>,
    @location(1) @interpolate(flat) radius: f32,
    @location(2) @interpolate(flat) streak: vec2<f32>,
    @location(3) @interpolate(flat) alpha: f32,
    @location(4) @interpolate(flat) soft: f32,
    @location(5) @interpolate(flat) lump: f32,
};

fn sprite(vi: u32, pos: vec2<f32>, vel: vec2<f32>, size: f32, alpha: f32, soft: f32, lump: f32) -> VOut {
    let s = scene.w;
    let pixels = size * s;
    let travel = vel * 0.018;
    let tl = length(travel);
    let smear = min(tl, size * select(2.2, 1.2, lump > 0.0));
    var way = vec2<f32>(0.0);
    if (smear > 0.25) {
        way = travel / tl * smear;
    }
    let wl = length(way);
    let extent = max(pixels * select(1.0, 1.3, lump > 0.0) + wl * s + 2.0, 3.0);
    let center = pos - way * 0.5;
    let c = corner(vi);
    let world = center + (c - vec2<f32>(0.5)) * extent / s;
    var o: VOut;
    o.pos = to_clip(world);
    o.coord = vec2<f32>(c.x, 1.0 - c.y);
    o.radius = pixels / extent;
    o.streak = vec2<f32>(way.x, -way.y) * s / extent;
    o.alpha = alpha * min(pixels / 2.0, 1.0) * pixels / max(pixels + wl * s * 0.6, 0.001);
    o.soft = soft;
    o.lump = lump;
    return o;
}

@vertex
fn flake_vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VOut {
    let b = ii * 13u;
    let x0 = data[b];
    let top = data[b + 1u];
    let born = data[b + 2u];
    let fall = data[b + 3u];
    let drift = data[b + 4u];
    let wind_birth = data[b + 5u];
    let phase = data[b + 6u];
    let rate = data[b + 7u];
    let sway = data[b + 8u];
    let size = data[b + 9u];
    var alpha = data[b + 10u];
    let soft = data[b + 11u];
    let gone = data[b + 12u];

    let now = pu.a.x;
    let wind = pu.a.y;
    let breeze = pu.a.z;
    let span = pu.a.w;

    let age = now - born;
    let p = phase + rate * age;
    let y = top - fall * (age + 0.12 / (1.3 * rate) * (cos(1.3 * phase) - cos(1.3 * p)));
    let sway_x = sway / rate * (sin(p) - sin(phase) + (0.3 / 2.3) * (sin(2.3 * p) - sin(2.3 * phase)));
    var x = x0 + drift * (wind - wind_birth) + sway_x;
    var margin = pu.b.x;
    if (margin <= 0.0) {
        margin = 30.0;
    }
    x = x - span * floor((x + margin) / span);
    let vel = vec2<f32>(drift * breeze + sway * (cos(p) + 0.3 * cos(2.3 * p)), -fall * (1.0 + 0.12 * sin(1.3 * p)));
    if (now >= gone) {
        alpha = 0.0;
    }
    var o = sprite(vi, vec2<f32>(x, y), vel, size, alpha, soft, 0.0);
    if (alpha <= 0.0) {
        o.pos = vec4<f32>(2.0, 2.0, 0.0, 1.0);
    }
    return o;
}

@vertex
fn loose_vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VOut {
    let b = ii * 8u;
    return sprite(
        vi,
        vec2<f32>(data[b], data[b + 1u]),
        vec2<f32>(data[b + 2u], data[b + 3u]),
        data[b + 4u],
        data[b + 5u],
        data[b + 6u],
        data[b + 7u],
    );
}

@fragment
fn sprite_fs(i: VOut) -> @location(0) vec4<f32> {
    let p = i.coord - vec2<f32>(0.5);
    let h = i.streak * 0.5;
    let span = dot(h, h);
    var along = 0.0;
    if (span > 0.0) {
        along = clamp(dot(p, h) / span, -1.0, 1.0);
    }
    let d = p - h * along;
    var r = length(d) * 2.0 / max(i.radius, 0.001);
    var shading = 0.0;
    if (i.lump > 0.0) {
        var ang = 0.0;
        if (dot(d, d) > 0.0) {
            ang = atan2(d.y, d.x);
        }
        r = r / (1.0 + 0.14 * sin(3.0 * ang + i.lump * 40.0) + 0.08 * sin(5.0 * ang + i.lump * 21.0));
        shading = saturate(dot(2.0 * d / max(i.radius, 0.001), vec2<f32>(0.45, 0.6))) * 0.4;
    }
    var head = 1.0;
    if (span > 0.0) {
        head = 0.15 + 0.85 * pow(along * 0.5 + 0.5, 1.5);
    }
    let a = (1.0 - smoothstep(mix(0.6, 0.0, i.soft), 1.0, r)) * i.alpha * head;
    if (a < 0.002) {
        discard;
    }
    let col = mix(
        vec3<f32>(0.965, 0.975, 0.995),
        vec3<f32>(0.62, 0.67, 0.77),
        saturate(smoothstep(0.35, 1.0, r) * 0.65 + shading),
    );
    return vec4<f32>(col * a, a);
}
