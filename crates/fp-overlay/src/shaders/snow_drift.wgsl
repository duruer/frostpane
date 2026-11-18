// Snow drift height field (snow-sim 3.6, 4.3). One pass per drift.
// uniforms: a = (origin.xy, columnWidth, count), b = (bottom, top, occluderCount, pad).
// buffers: columns (shape[count] then bed[count]), occluders (4 f32 each).
@group(0) @binding(2) var<storage, read> cols: array<f32>;
@group(0) @binding(3) var<storage, read> occ: array<f32>;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) world: vec2<f32>,
};

@vertex
fn drift_vs(@builtin(vertex_index) vi: u32) -> VOut {
    let origin = pu.a.xy;
    let width = pu.a.z * pu.a.w;
    let c = corner(vi);
    let local = vec2<f32>(c.x * width, mix(pu.b.x, pu.b.y, c.y));
    var o: VOut;
    o.local = local;
    o.world = local + origin;
    o.pos = to_clip(o.world);
    return o;
}

// Linear sample between column centres; holds the end values outside them.
fn column(off: u32, x: f32) -> f32 {
    let n = u32(pu.a.w);
    let f = x / pu.a.z - 0.5;
    let last = f32(n) - 1.0;
    let i = clamp(floor(f), 0.0, last);
    let j = min(i + 1.0, last);
    let t = saturate(f - i);
    return mix(cols[off + u32(i)], cols[off + u32(j)], t);
}

@fragment
fn drift_fs(i: VOut) -> @location(0) vec4<f32> {
    let n_occ = u32(pu.b.z);
    for (var k = 0u; k < n_occ; k = k + 1u) {
        let b = k * 4u;
        if (i.world.x >= occ[b] && i.world.x < occ[b + 2u] && i.world.y >= occ[b + 1u] && i.world.y < occ[b + 3u]) {
            discard;
        }
    }
    let n = u32(pu.a.w);
    let s = scene.w;
    let surface = column(0u, i.local.x);
    let bed = column(n, i.local.x);
    let depth = surface - i.local.y;
    let coverage = saturate(depth * s + 0.5) * saturate((i.local.y - bed) * s + 0.5) * saturate((surface - bed) / 0.75);
    if (coverage <= 0.0) {
        discard;
    }
    let slope = (column(0u, i.local.x + 2.0) - column(0u, i.local.x - 2.0)) * 0.25;
    let light = dot(normalize(vec2<f32>(-slope, 1.0)), normalize(vec2<f32>(-0.5, 1.0)));
    let shade = (1.0 - exp(-depth / 30.0)) * 0.32 + (1.0 - light) * 0.6 * exp(-depth / 5.0);
    var col = mix(vec3<f32>(0.925, 0.94, 0.965), vec3<f32>(0.72, 0.78, 0.88), saturate(shade));
    col = mix(col, vec3<f32>(0.975, 0.982, 0.995), exp(-0.9 * depth) * 0.6);
    // Screen-locked grain and sparkle.
    let px = i.pos.xy;
    col = col + vec3<f32>((hash12(floor(px)) - 0.5) * 0.02);
    let seed = hash12(floor(px * 0.5) + vec2<f32>(31.7));
    if (seed > 0.992) {
        let tw = pow(saturate(sin(scene.z * (0.6 + 1.8 * seed) + 90.0 * seed)), 28.0);
        col = mix(col, vec3<f32>(1.0), tw * exp(-depth / 7.0));
    }
    return vec4<f32>(col * coverage, coverage);
}
