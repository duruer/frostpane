// Sakura petals and autumn leaves (other-weather 2.2.7). 10 f32 per sprite:
// position.xy, size, angle, flip, alpha, soft, tint, shape, pad.
@group(0) @binding(2) var<storage, read> data: array<f32>;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) flip: f32,
    @location(2) @interpolate(flat) alpha: f32,
    @location(3) @interpolate(flat) soft: f32,
    @location(4) @interpolate(flat) tint: f32,
    @location(5) @interpolate(flat) shape: f32,
};

@vertex
fn petal_vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VOut {
    let b = ii * 10u;
    let center = vec2<f32>(data[b], data[b + 1u]);
    let size = data[b + 2u];
    let angle = data[b + 3u];
    let flip = data[b + 4u];
    let soft = data[b + 6u];
    let shape = data[b + 8u];

    var wide = 0.72;
    if (shape > 1.5) {
        wide = 0.56;
    } else if (shape > 0.5) {
        wide = 1.0;
    }
    let reach = vec2<f32>(size * wide * max(abs(flip), 0.12), size);
    let margin = 1.5 + soft * size * 0.6;
    let c = corner(vi) * 2.0 - vec2<f32>(1.0);
    let local = c * (vec2<f32>(1.0) + vec2<f32>(margin) / max(reach, vec2<f32>(0.5)));
    var o: VOut;
    o.pos = to_clip(center + rot(local * reach, angle));
    o.local = local;
    o.flip = flip;
    o.alpha = data[b + 5u];
    o.soft = soft;
    o.tint = data[b + 7u];
    o.shape = shape;
    return o;
}

// Anti-aliased edge of a signed distance-like field (negative inside).
fn edge(f: f32, soft: f32) -> f32 {
    return saturate(0.5 - f / (max(fwidth(f), 0.0001) * (1.0 + soft * 9.0)));
}

fn petal_color(p: vec2<f32>, w: f32, f: f32, tint: f32, flip: f32) -> vec3<f32> {
    let t = smoothstep(-1.0, 0.75, p.y);
    let base = mix(vec3<f32>(0.96, 0.56, 0.7), vec3<f32>(0.91, 0.46, 0.63), tint);
    let mid = mix(vec3<f32>(1.0, 0.8, 0.87), vec3<f32>(0.99, 0.73, 0.82), tint);
    let tip = vec3<f32>(1.0, 0.95, 0.97);
    var c = mix(base, mid, saturate(t / 0.5));
    if (t > 0.5) {
        c = mix(mid, tip, (t - 0.5) / 0.5);
    }
    let fan = atan2(p.x, p.y + 1.15);
    c = c * (1.0 - 0.06 * pow(abs(cos(15.0 * fan)), 8.0) * (1.0 - smoothstep(-0.6, 0.95, p.y)));
    c = c * (0.95 + 0.08 * clamp(-p.x / w, -1.0, 1.0));
    c = mix(c, c * 0.9, smoothstep(-0.3, 0.0, f) * 0.7);
    if (flip < 0.0) {
        c = mix(c, vec3<f32>(1.0, 0.93, 0.95), 0.3);
    }
    return c * (0.84 + 0.16 * abs(flip));
}

fn autumn(t: f32) -> vec3<f32> {
    let red = vec3<f32>(0.78, 0.17, 0.1);
    let orange = vec3<f32>(0.94, 0.45, 0.12);
    let gold = vec3<f32>(0.96, 0.72, 0.2);
    let brown = vec3<f32>(0.58, 0.33, 0.16);
    if (t < 0.33) {
        return mix(red, orange, t / 0.33);
    }
    if (t < 0.7) {
        return mix(orange, gold, (t - 0.33) / 0.37);
    }
    return mix(gold, brown, saturate((t - 0.7) / 0.3));
}

@fragment
fn petal_fs(i: VOut) -> @location(0) vec4<f32> {
    let p = i.local;
    var color = vec3<f32>(0.0);
    var shape_alpha = 0.0;

    if (i.shape > 0.5) {
        var base = autumn(i.tint);
        var vein = 0.0;
        var rim = 0.0;
        if (i.shape < 1.5) {
            // Maple: five lobes around (0, -0.08), one pointing straight up.
            let q = p - vec2<f32>(0.0, -0.08);
            let r = length(q);
            let theta = atan2(q.x, q.y);
            let sector = 6.2831853 / 5.0;
            let s = theta / sector;
            let lobe = tri(s);
            let tooth = tri(s * 3.0);
            let grow = 0.6 + 0.4 * smoothstep(-0.9, 0.3, cos(theta));
            let reach = (0.38 + 0.6 * pow(lobe, 0.7)) * grow * (0.9 + 0.1 * tooth);
            let body = edge(r - reach, i.soft);
            var stem = 0.0;
            if (q.y > -0.95 && q.y < -0.05) {
                stem = edge(abs(q.x) - 0.035, i.soft);
            }
            shape_alpha = max(body, stem);
            let off = (fract(s + 0.5) - 0.5) * sector;
            let vdist = r * abs(sin(off));
            if (r <= 0.92 * reach) {
                vein = 1.0 - smoothstep(0.012, 0.035, vdist);
            }
            rim = smoothstep(0.35, 1.0, r / max(reach, 0.01));
        } else {
            // Plain oval leaf with midrib and slanted side veins.
            let yy = (p.y + 0.72) / 1.72 * 2.0 - 1.0;
            let w = pow(max(1.0 - yy * yy, 0.0), 0.75) * (1.0 - 0.22 * yy);
            var body = 0.0;
            if (yy >= -1.0 && yy <= 1.0) {
                body = edge(abs(p.x) - w, i.soft);
            }
            var stem = 0.0;
            if (p.y > -1.0 && p.y < -0.7) {
                stem = edge(abs(p.x) - 0.04, i.soft);
            }
            shape_alpha = max(body, stem);
            let midrib = 1.0 - smoothstep(0.015, 0.04, abs(p.x));
            let side = 0.6 * (1.0 - smoothstep(0.03, 0.08, abs(fract((yy - 1.3 * abs(p.x)) * 3.5) - 0.5)));
            if (abs(p.x) <= 0.85 * w) {
                vein = max(midrib, side);
            }
            rim = smoothstep(0.4, 1.0, abs(p.x) / max(w, 0.01));
        }
        var c = mix(base, base * 0.68 + vec3<f32>(0.08, 0.02, 0.0), rim * 0.55);
        let noise = hash12(floor(p * 9.0) + vec2<f32>(i.tint * 17.0));
        c = c * (0.92 + 0.12 * noise);
        c = mix(c, min(c * 1.25 + vec3<f32>(0.05), vec3<f32>(1.0)), vein * 0.5);
        if (i.flip < 0.0) {
            c = mix(c, c * 0.75 + vec3<f32>(0.1, 0.08, 0.05), 0.4);
        }
        color = c * (0.8 + 0.2 * abs(i.flip));
    } else {
        // Sakura petal: teardrop with a heart notch at the top.
        let w = 0.3 + 0.7 * smoothstep(-1.2, 0.4, p.y);
        let f = length(vec2<f32>(p.x / w, p.y)) - 1.0;
        let body = edge(f, i.soft);
        let n = p.y - 0.8 - 1.05 * abs(p.x);
        let cut = edge(n, i.soft);
        shape_alpha = body * cut;
        color = petal_color(p, w, f, i.tint, i.flip);
    }

    let a = shape_alpha * i.alpha;
    if (a < 0.003) {
        discard;
    }
    return vec4<f32>(color * a, a);
}
