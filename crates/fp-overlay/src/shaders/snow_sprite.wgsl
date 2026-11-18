// Snow flakes (closed form, snow-sim 3.3) and loose sprites (snow-sim 4.1, 4.2).
// SnowFlakes buffer: 16 f32 per flake. SnowLoose buffer: 8 f32 per sprite.
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
    // Falling flakes only: (cos, sin) of the turn, and (kind, seed, apparent
    // width, diameter in pixels). Loose sprites carry kind -1.
    @location(6) @interpolate(flat) turn: vec2<f32>,
    @location(7) @interpolate(flat) look: vec4<f32>,
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
    o.turn = vec2<f32>(1.0, 0.0);
    o.look = vec4<f32>(-1.0, 0.0, 1.0, pixels);
    return o;
}

@vertex
fn flake_vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VOut {
    let b = ii * 16u;
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
    let shape = data[b + 13u];
    let spin = data[b + 14u];
    let wobble = data[b + 15u];

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
    // Visual tumble: turn about the view axis, and a flutter that narrows
    // the apparent width as the flake tips edge-on (about twice the spin).
    let kind = floor(shape);
    let seed = shape - kind;
    let turn = seed * 97.0 + spin * age;
    let tip = 0.5 - 0.5 * cos(seed * 41.0 + 1.9 * spin * age);
    // A little per-flake brightness, mean 1.06 to make up for the lumpy
    // outlines covering less than the disc; edge-on flakes dim by their
    // narrower outline.
    alpha = alpha * (0.94 + 0.24 * fract(seed * 13.7));
    var o = sprite(vi, vec2<f32>(x, y), vel, size, alpha, soft, 0.0);
    o.turn = vec2<f32>(cos(turn), sin(turn));
    o.look = vec4<f32>(kind, seed, 1.0 - wobble * tip, o.look.w);
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

// Shape scale, see flake_shape.
const FIT: f32 = 0.85;

// Smooth minimum (polynomial), fuses blobs into one soft outline.
fn smin(a: f32, b: f32, k: f32) -> f32 {
    let h = saturate(0.5 + 0.5 * (b - a) / k);
    return mix(b, a, h) - k * h * (1.0 - h);
}

// Distance to a segment from the origin along `dir` of length `len`.
fn seg(p: vec2<f32>, dir: vec2<f32>, len: f32) -> f32 {
    return length(p - dir * clamp(dot(p, dir), 0.0, len));
}

// Coverage of a stroke of half-width `w` at distance `d`, all in flake units,
// `px` = one pixel. Strokes thinner than ~0.75 px are drawn at that width and
// dimmed to keep their weight, so thin arms never alias or shimmer.
fn stroke(d: f32, w: f32, px: f32) -> f32 {
    let wide = max(w, 0.75 * px);
    return (1.0 - smoothstep(wide - 0.5 * px, wide + px, d)) * (w / wide);
}

// Faint six-armed dendrite on a small hexagonal plate, as coverage in [0, 1].
fn crystal(q: vec2<f32>, px: f32, seed: f32) -> f32 {
    let sector = 1.0471976;
    let a = atan2(q.y, q.x);
    let la = a - sector * round(a / sector);
    let l = length(q) * vec2<f32>(cos(la), abs(sin(la)));
    let reach = 0.8 + 0.12 * seed;
    let arm = stroke(seg(l, vec2<f32>(1.0, 0.0), reach), mix(0.13, 0.055, saturate(l.x / reach)), px);
    let up = vec2<f32>(0.5, 0.8660254);
    let b1 = seg(l - vec2<f32>(0.36 + 0.1 * seed, 0.0), up, 0.24);
    let b2 = seg(l - vec2<f32>(0.6, 0.0), up, 0.14);
    let twig = stroke(min(b1, b2), 0.045, px) * 0.85;
    let plate = dot(l, vec2<f32>(0.8660254, 0.5)) - 0.3;
    let disc = 1.0 - smoothstep(-px, 1.5 * px, plate);
    return max(max(arm, twig), disc);
}

// Falling flake look: `d` is the offset from the (smeared) centre in quad
// units. Shapes are signed distances `sd` in flake units (edge at 0, 1 = the
// old disc radius) fed through the old soft profile, with an edge never
// narrower than 1.5 px, so tiny flakes stay soft dots and nothing aliases.
fn flake_shape(i: VOut, d: vec2<f32>, head: f32) -> vec4<f32> {
    var q = d * 2.0 / max(i.radius, 0.001);
    let r0 = dot(q, q);
    if (r0 > 1.7) {
        discard;
    }
    let kind = i.look.x;
    let seed = i.look.y;
    let pixels = i.look.w;
    let px = 2.0 / max(pixels, 0.001);
    q = vec2<f32>(i.turn.x * q.x + i.turn.y * q.y, -i.turn.y * q.x + i.turn.x * q.y);
    q.x = q.x / max(i.look.z, 0.3);
    let r = sqrt(r0);
    // Lumpy outlines cover less than the disc; drawn this much larger they
    // keep the old flake's overall weight (same size distribution).
    let qs = q * FIT;
    // Shape detail needs pixels: below ~2 px every kind is the old dot.
    let detail = smoothstep(1.8, 5.0, pixels);
    let inner = mix(0.6, 0.0, i.soft);
    var sd = r - 1.0;
    var shade = 0.0;
    var a = -1.0;
    if (kind < 0.5) {
        // Speck: an off-centre pair of soft ellipses, a lumpy grain.
        let o = vec2<f32>(cos(seed * 6.283), sin(seed * 6.283)) * 0.3;
        let e1 = length(qs * vec2<f32>(1.0, 1.12)) - 0.96;
        let e2 = length(qs - o) - 0.66;
        sd = smin(e1, e2, 0.3) / 0.96;
    } else if (kind < 2.5) {
        // Clump: a few fused crystals around a middle one, unevenly spaced.
        let a0 = seed * 6.283;
        let s2 = fract(seed * 7.31);
        let c0 = length(qs - vec2<f32>(0.06, -0.04)) - 0.8;
        let c1 = length(qs - vec2<f32>(cos(a0), sin(a0)) * 0.38) - 0.52;
        let c2 = length(qs - vec2<f32>(cos(a0 + 1.9 + s2), sin(a0 + 1.9 + s2)) * 0.44) - 0.46;
        let c3 = length(qs - vec2<f32>(cos(a0 + 4.0), sin(a0 + 4.0)) * (0.34 + 0.12 * s2)) - 0.48;
        sd = smin(smin(c0, c1, 0.3), smin(c2, c3, 0.3), 0.3) / 0.85;
        // Each crystal of the clump keeps a slightly brighter middle.
        shade = 0.25 * smoothstep(-0.45, -0.1, min(min(c0, c1), min(c2, c3)));
        if (kind > 1.5) {
            // Crystal: the dendrite where there are pixels to show it, over
            // a soft glow; small ones stay clumps.
            let star = crystal(qs, px * FIT, seed);
            let glow = 1.0 - smoothstep(0.0, 1.0, r / 0.76);
            let show = smoothstep(5.0, 12.0, pixels);
            let blob = 1.0 - smoothstep(inner, 1.0, mix(r, sd + 1.0, detail));
            a = mix(blob, max(star * 0.42, glow * 0.92), show);
            shade = mix(shade, 0.35 * smoothstep(0.25, 0.9, r), show);
        }
    } else {
        // Big near flake: a blurry disc, faintly hexagonal like lens bokeh.
        let hx = max(abs(q.x), max(abs(dot(q, vec2<f32>(0.5, 0.8660254))), abs(dot(q, vec2<f32>(-0.5, 0.8660254)))));
        sd = mix(r, hx / 0.8660254 * 0.95, 0.4) - 1.0;
    }
    let rr = mix(r, sd + 1.0, detail);
    if (a < 0.0) {
        a = 1.0 - smoothstep(inner, 1.0, rr);
        if (kind > 2.5) {
            a = a * (0.9 + 0.18 * smoothstep(0.35, 0.85, rr));
        }
    }
    a = a * i.alpha * head;
    if (a < 0.002) {
        discard;
    }
    let col = mix(
        vec3<f32>(0.965, 0.975, 0.995),
        vec3<f32>(0.62, 0.67, 0.77),
        saturate(smoothstep(0.35, 1.0, rr) * 0.65 + shade * detail),
    );
    return vec4<f32>(col * a, a);
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
    var head = 1.0;
    if (span > 0.0) {
        head = 0.15 + 0.85 * pow(along * 0.5 + 0.5, 1.5);
    }
    if (i.look.x >= 0.0) {
        return flake_shape(i, d, head);
    }
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
