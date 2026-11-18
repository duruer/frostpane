//! The drift: a 1-D height field of snow on a ledge or on the ground
//! (snow-sim 3.6, 3.7, 3.9 carry / break-up / shake).
//!
//! Coordinates are screen-local points, Y up. A drift never owns grains: it
//! reports what it sheds as [`Emit`] values that the field turns into grains.

use fp_core::geom::{Rect4, Vec2};
use fp_core::rng::{Lcg64, Rng};
use fp_core::surface::blocked;
use std::f64::consts::TAU;

pub(crate) const COLUMN_WIDTH: f64 = 2.0;
const ROUNDING: f64 = 0.3;
const RELAX_PASSES: usize = 6;
const WIDEST_SPREAD: i64 = 24;
const WIDEST_ROLL: usize = 60;
const REPOSE_THRESHOLD: f64 = 1.0;
const GRAVITY: f64 = 2500.0;
const ADHESION: f64 = 3500.0;
const JUMP_SPEED: f64 = 6000.0;
const BREAK_LIFT: f64 = 10.0;
const SHAKING: f64 = 4.0;
const TRACK_TIME: f64 = 0.06;
const GRIP: f64 = 3.0;
const SLIP: f64 = 2.0;
const SMALLEST_CHUNK: f64 = 14.0;
const CARRY_CURSOR_RADIUS: f64 = 10.0;

/// Something a drift sheds; applied by the field.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Emit {
    Release { area: f64, pos: Vec2, vel: Vec2, spread: Vec2, scatter: f64 },
    Lump { area: f64, pos: Vec2, vel: Vec2 },
    Fall { area: f64, pos: Vec2, dir: f64, vel: Vec2 },
}

/// Column count for a surface width.
pub(crate) fn columns_for(width: f64) -> usize {
    ((width / COLUMN_WIDTH).ceil() as usize).max(2)
}

#[derive(Clone, Debug)]
pub(crate) struct Drift {
    pub origin: Vec2,
    pub offset: Vec2,
    pub offset_vel: Vec2,
    pub width: f64,
    depth: f64,
    corner: f64,
    opens_at_ends: bool,
    pub heights: Vec<f64>,
    bed: Vec<f64>,
    capacity: Vec<f64>,
    pub visible: Vec<bool>,
    pub top: f64,
    pub bed_depth: f64,
    ph: [f64; 2],
    pub spill: [f64; 2],
    spill_at: [f64; 2],
    pub velocity: Vec2,
    pub track: Vec2,
    pub moved_at: f64,
    pub agitation: f64,
    swing_peak: f64,
    prev_swing_peak: f64,
    swing_dir: Vec2,
    pub occluders: Vec<Rect4>,
    flag: bool,
    pub size: Option<(f64, f64)>,
}

impl Drift {
    pub fn new(origin: Vec2, width: f64, depth: f64, corner: f64, opens_at_ends: bool, rng: &mut Lcg64) -> Self {
        let ph = [rng.range(0.0, TAU), rng.range(0.0, TAU)];
        let spill_at = [rng.range(2.0, 20.0), rng.range(2.0, 20.0)];
        let n = columns_for(width);
        let mut d = Self {
            origin,
            offset: Vec2::ZERO,
            offset_vel: Vec2::ZERO,
            width,
            depth,
            corner,
            opens_at_ends,
            heights: vec![0.0; n],
            bed: Vec::new(),
            capacity: Vec::new(),
            visible: vec![true; n],
            top: 0.0,
            bed_depth: 0.0,
            ph,
            spill: [0.0; 2],
            spill_at,
            velocity: Vec2::ZERO,
            track: origin,
            moved_at: -10.0,
            agitation: 0.0,
            swing_peak: 0.0,
            prev_swing_peak: 0.0,
            swing_dir: Vec2::ZERO,
            occluders: Vec::new(),
            flag: false,
            size: None,
        };
        d.rebuild();
        d
    }

    pub fn count(&self) -> usize {
        self.heights.len()
    }

    pub fn base(&self) -> Vec2 {
        self.origin + self.offset
    }

    pub fn floor(&self, i: usize) -> f64 {
        self.base().y + self.bed[i]
    }

    pub fn surface(&self, i: usize) -> f64 {
        self.floor(i) + self.heights[i]
    }

    pub fn center_x(&self, i: usize) -> f64 {
        self.base().x + (i as f64 + 0.5) * COLUMN_WIDTH
    }

    /// Column under `x`, if any.
    pub fn index(&self, x: f64) -> Option<usize> {
        let f = ((x - self.base().x) / COLUMN_WIDTH).floor();
        if f < 0.0 || f >= self.count() as f64 { None } else { Some(f as usize) }
    }

    pub fn clamped_index(&self, x: f64) -> usize {
        let f = ((x - self.base().x) / COLUMN_WIDTH).floor();
        f.clamp(0.0, (self.count() - 1) as f64) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.top < 0.05
    }

    /// Surface relative to the base line (no origin).
    fn rel(&self, i: usize) -> f64 {
        self.bed[i] + self.heights[i]
    }

    /// Remaining fraction of capacity under column `i`.
    pub fn fill(&self, i: usize) -> f64 {
        (1.0 - self.heights[i] / self.capacity[i]).max(0.0)
    }

    /// Rebuild bed (rounded corners) and capacity for the current count/width.
    fn rebuild(&mut self) {
        let n = self.count();
        let r = self.corner.min(self.width / 2.0);
        self.bed = (0..n)
            .map(|i| {
                let c = (i as f64 + 0.5) * COLUMN_WIDTH;
                let u = if r > 0.0 { 1.0 - c.min(self.width - c) / r } else { 0.0 };
                if u > 0.0 { -r * (1.0 - (1.0 - u.powf(2.5)).max(0.0).powf(0.4)) } else { 0.0 }
            })
            .collect();
        self.bed_depth = self.bed.iter().fold(0.0_f64, |m, b| m.max(b.abs()));
        self.capacity = (0..n)
            .map(|j| {
                let j2 = 2.0 * j as f64;
                self.depth * (0.95 + 0.12 * (j2 * 0.0105 + self.ph[0]).sin() + 0.07 * (j2 * 0.037 + self.ph[1]).sin())
            })
            .collect();
    }

    /// Change the corner radius; heights are kept.
    pub fn set_corner(&mut self, corner: f64) {
        if corner != self.corner {
            self.corner = corner;
            self.rebuild();
        }
    }

    pub fn refresh_top(&mut self) {
        self.top = self.heights.iter().fold(0.0_f64, |m, h| m.max(*h));
    }

    /// Slide a landing column downhill (snow-sim 3.6 RestingColumn).
    pub fn resting_column(&self, mut col: usize) -> usize {
        let n = self.count();
        for _ in 0..WIDEST_ROLL {
            let s = self.rel(col);
            let sl = if col > 0 { self.rel(col - 1) } else { f64::INFINITY };
            let sr = if col + 1 < n { self.rel(col + 1) } else { f64::INFINITY };
            if sl < s - 1.0 && sl <= sr {
                col -= 1;
            } else if sr < s - 1.0 {
                col += 1;
            } else {
                break;
            }
        }
        col
    }

    /// Triangular-kernel deposit. Returns the area that fell outside.
    pub fn deposit(&mut self, area: f64, x: f64, wide: bool) -> f64 {
        if area.is_nan() || area <= 0.0 {
            return 0.0;
        }
        let n = self.count() as i64;
        // Clamp far-away (or non-finite) columns so `col + i` cannot overflow;
        // a kernel centred that far out deposits nothing either way.
        let far = WIDEST_SPREAD + 1;
        let mut col = (((x - self.base().x) / COLUMN_WIDTH).floor() as i64).clamp(-far, n + far);
        if col >= 0 && col < n {
            col = self.resting_column(col as usize) as i64;
        }
        let k = ((area / 2.0).sqrt().floor() as i64).min(WIDEST_SPREAD).max(if wide { 2 } else { 1 });
        let denom = ((k + 1) * (k + 1)) as f64;
        let mut overflow = 0.0;
        for i in -k..=k {
            let w = area * (k + 1 - i.abs()) as f64 / denom;
            let idx = col + i;
            if idx >= 0 && idx < n {
                self.heights[idx as usize] += w / 2.0;
            } else {
                overflow += w;
            }
        }
        self.refresh_top();
        overflow
    }

    fn exchange(&mut self, j: usize) -> bool {
        let d = self.rel(j) - self.rel(j + 1);
        let (from, to, amt) = if d > REPOSE_THRESHOLD {
            (j, j + 1, ((d - REPOSE_THRESHOLD) / 2.0).min(self.heights[j]))
        } else if d < -REPOSE_THRESHOLD {
            (j + 1, j, ((-d - REPOSE_THRESHOLD) / 2.0).min(self.heights[j + 1]))
        } else {
            return false;
        };
        self.heights[from] -= amt;
        self.heights[to] += amt;
        amt > 0.001
    }

    /// Angle-of-repose relaxation (snow-sim 3.6 Relax).
    pub fn relax(&mut self) {
        let n = self.count();
        if n <= 3 || self.top <= 0.0 {
            return;
        }
        self.flag = !self.flag;
        let mut sums = [0.0_f64; 2];
        for pass in 0..RELAX_PASSES {
            let ltr = (pass % 2 == 0) == self.flag;
            let mut moved = false;
            for step in 0..n - 1 {
                let j = if ltr { step } else { n - 2 - step };
                moved |= self.exchange(j);
            }
            if self.opens_at_ends {
                for (side, idx) in [(0, 0), (1, n - 1)] {
                    let h = self.heights[idx];
                    if h > REPOSE_THRESHOLD {
                        let lost = (h - REPOSE_THRESHOLD) / 2.0;
                        self.heights[idx] -= lost;
                        sums[side] += lost;
                    }
                }
            }
            if !moved {
                break;
            }
        }
        for k in 1..n - 2 {
            let s = |i: usize| self.rel(i);
            let curv = (s(k) - s(k + 1)) - ((s(k - 1) - s(k)) + (s(k + 1) - s(k + 2))) / 2.0;
            if curv.abs() > 0.02 {
                let m = ROUNDING * curv;
                let m = if m > 0.0 { m.min(self.heights[k]) } else { m.max(-self.heights[k + 1]) };
                self.heights[k] -= m;
                self.heights[k + 1] += m;
            }
        }
        self.spill[0] += 2.0 * sums[0];
        self.spill[1] += 2.0 * sums[1];
    }

    /// Melt (snow-sim 3.6 Thaw).
    pub fn thaw(&mut self, dt: f64, floor: f64, rate: f64, settle: f64) {
        if self.top <= 0.0 {
            return;
        }
        for h in &mut self.heights {
            if *h > 0.0 {
                *h -= h.min((rate + (*h - floor).max(0.0) / settle) * dt);
            }
        }
    }

    /// Spill off the open ends once enough has gathered.
    fn take_spill(&mut self, rng: &mut Lcg64, out: &mut Vec<Emit>) {
        let n = self.count();
        let ends = [(self.base().x, self.surface(0), -1.0), (self.base().x + self.width, self.surface(n - 1), 1.0)];
        for (side, (x, y, dir)) in ends.into_iter().enumerate() {
            if self.spill[side] > self.spill_at[side] {
                out.push(Emit::Fall { area: self.spill[side], pos: Vec2::new(x, y), dir, vel: self.velocity });
                self.spill[side] = 0.0;
                self.spill_at[side] = rng.range(2.0, 20.0);
            }
        }
    }

    /// One settle tick: thaw, relax, spill, refresh.
    pub fn settle(&mut self, dt: f64, floor: f64, settle: f64, rate: f64, rng: &mut Lcg64, out: &mut Vec<Emit>) {
        self.thaw(dt, floor, rate, settle);
        self.relax();
        self.take_spill(rng, out);
        self.refresh_top();
    }

    /// Drop the end column in `dir`, scroll the rest. Returns (area, position).
    pub fn shift(&mut self, dir: f64) -> (f64, Vec2) {
        let n = self.count();
        let i = if dir > 0.0 { n - 1 } else { 0 };
        let out = (2.0 * self.heights[i], Vec2::new(self.center_x(i), self.surface(i)));
        if dir > 0.0 {
            self.heights.rotate_right(1);
            self.heights[0] = 0.0;
        } else {
            self.heights.rotate_left(1);
            self.heights[n - 1] = 0.0;
        }
        out
    }

    /// New width; returns the area cut off (0 when growing).
    pub fn resize(&mut self, width: f64, keep_left: bool) -> f64 {
        let n = columns_for(width);
        let old = self.count();
        let mut cut = 0.0;
        if n < old {
            if keep_left {
                cut = self.heights[n..].iter().sum::<f64>() * 2.0;
                self.heights.truncate(n);
            } else {
                cut = self.heights[..old - n].iter().sum::<f64>() * 2.0;
                self.heights.drain(..old - n);
            }
        } else if keep_left {
            self.heights.resize(n, 0.0);
        } else {
            let mut v = vec![0.0; n - old];
            v.append(&mut self.heights);
            self.heights = v;
        }
        self.width = width;
        self.visible = vec![true; n];
        self.rebuild();
        self.refresh_top();
        cut
    }

    /// Hide columns under blockers or outside the screen.
    pub fn recompute_visible(&mut self, blockers: &[Rect4], screen_w: f64) {
        for i in 0..self.count() {
            let cx = self.center_x(i);
            self.visible[i] = cx >= 0.0 && cx <= screen_w && !blocked(blockers, cx, self.floor(i) + 1.0);
        }
    }

    /// Render shape buffer: smoothed surface for every column, then the bed.
    pub fn shape(&self) -> Vec<f32> {
        let n = self.count();
        let h = |i: i64| self.heights[i.clamp(0, n as i64 - 1) as usize];
        let mut v: Vec<f32> = (0..n as i64)
            .map(|j| {
                (self.bed[j as usize] + (h(j - 2) + h(j + 2)) / 16.0 + (h(j - 1) + h(j + 1)) / 4.0 + h(j) * 3.0 / 8.0)
                    as f32
            })
            .collect();
        v.extend(self.bed.iter().map(|b| *b as f32));
        v
    }

    // ---- cursor and plough ----

    /// Cut a cursor circle out of the pile. Returns the removed area.
    pub fn cut(&mut self, cx: f64, cy: f64) -> f64 {
        let r = CARRY_CURSOR_RADIUS;
        let b = self.base();
        if !(cx + r > b.x && cx - r < b.x + self.width && cy - r < b.y + self.top && cy + r > b.y - self.bed_depth) {
            return 0.0;
        }
        let mut removed = 0.0;
        for i in self.clamped_index(cx - r)..=self.clamped_index(cx + r) {
            let dx = self.center_x(i) - cx;
            if !self.visible[i] || dx.abs() >= r {
                continue;
            }
            let c = (r * r - dx * dx).sqrt();
            if cy + c > self.floor(i) && self.surface(i) > cy - c {
                let new = ((cy - c) - self.floor(i)).max(0.0);
                removed += 2.0 * (self.heights[i] - new);
                self.heights[i] = new;
            }
        }
        self.refresh_top();
        removed
    }

    /// Plough blade carve over `[lo, hi]`. Returns the removed area.
    pub fn plough_cut(&mut self, lo: f64, hi: f64, leave: f64) -> f64 {
        let b = self.base();
        if hi <= b.x || lo >= b.x + self.width {
            return 0.0;
        }
        let mut removed = 0.0;
        for i in self.clamped_index(lo)..=self.clamped_index(hi) {
            let limit = leave * (0.5 + (0.7 * i as f64).sin().abs());
            if self.heights[i] > limit {
                removed += 2.0 * (self.heights[i] - limit);
                self.heights[i] = limit;
            }
        }
        self.refresh_top();
        removed
    }

    // ---- movement ----

    pub fn note_swing(&mut self, accel: Vec2, dt: f64) {
        self.agitation *= (-dt / 0.3).exp();
        let m = accel.len();
        if m > 1.0 && accel.dot(self.swing_dir) < 0.0 {
            self.agitation = self.agitation.max(self.prev_swing_peak.min(self.swing_peak) / GRAVITY);
            self.prev_swing_peak = self.swing_peak;
            self.swing_peak = m;
            self.swing_dir = accel * (1.0 / m);
        } else {
            self.swing_peak = self.swing_peak.max(m);
            if m > 1.0 && self.swing_dir == Vec2::ZERO {
                self.swing_dir = accel * (1.0 / m);
            }
        }
    }

    /// Per-step chase of the window and pile-relative physics (snow-sim 3.9 Carry).
    pub fn carry(&mut self, dt: f64, rng: &mut Lcg64, out: &mut Vec<Emit>) {
        let prev_v = self.velocity;
        let omega = 2.0 / TRACK_TIME;
        let x = omega * dt;
        let k = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
        let c = self.track - self.origin;
        let t = (self.velocity + c * omega) * dt;
        self.velocity = (self.velocity - t * omega) * k;
        self.track = self.origin + (c + t) * k;
        let accel = (self.velocity - prev_v) * (1.0 / dt);
        self.note_swing(accel, dt);
        if self.is_empty() {
            self.offset = Vec2::ZERO;
            self.offset_vel = Vec2::ZERO;
            return;
        }
        // vertical
        let a = -GRAVITY - accel.y;
        if self.offset.y > 0.0 || a > ADHESION {
            self.offset_vel.y += a * dt;
            self.offset.y += self.offset_vel.y * dt;
            if self.offset.y <= 0.0 {
                self.offset.y = 0.0;
                self.offset_vel.y = 0.0;
            }
        }
        // horizontal
        let push = -accel.x;
        let f = if self.offset.y > 0.0 { 0.0 } else { (JUMP_SPEED + accel.y).max(0.0) };
        let mut vx = self.offset_vel.x;
        if vx != 0.0 || push.abs() > GRIP * f {
            // |push| > 3F >= 0 means push != 0 whenever vx == 0.
            let sign = if vx != 0.0 { vx.signum() } else { push.signum() };
            let next = vx + (push - sign * SLIP * f) * dt;
            vx = if f > 0.0 && next * sign <= 0.0 { 0.0 } else { next };
        }
        self.offset.x += vx * dt;
        self.offset_vel.x = vx;
        let carried = self.velocity + Vec2::new(vx, self.offset_vel.y);
        if !self.offset.x.is_finite() {
            self.offset.x = 0.0;
        }
        // One shift per whole column of offset. After `count` shifts the pile
        // is empty, so further shifts would only release nothing.
        let steps = (self.offset.x.abs() / COLUMN_WIDTH).floor();
        let dir = self.offset.x.signum();
        self.offset.x -= steps * COLUMN_WIDTH * dir;
        for _ in 0..(steps as usize).min(self.count()) {
            let (area, pos) = self.shift(dir);
            out.push(Emit::Release {
                area,
                pos: Vec2::new(pos.x + COLUMN_WIDTH * dir, pos.y),
                vel: carried,
                spread: Vec2::new(1.0, 2.0),
                scatter: 0.25,
            });
        }
        self.refresh_top();
        if self.offset.y > BREAK_LIFT {
            self.break_up(1.0, carried, rng, out);
            self.offset = Vec2::ZERO;
            self.offset_vel = Vec2::ZERO;
        } else if self.agitation > SHAKING {
            self.shake(carried, dt, rng, out);
        }
    }

    pub fn shake(&mut self, carried: Vec2, dt: f64, rng: &mut Lcg64, out: &mut Vec<Emit>) {
        let n = self.count();
        let p = (0.06 * (self.agitation - SHAKING)).min(0.8);
        let times = (n as f64 * p * dt * 2.0 + rng.unit()) as usize;
        for _ in 0..times {
            let i = rng.below(n as u64) as usize;
            let h = self.heights[i];
            if h > 0.2 {
                self.heights[i] = h / 2.0;
                out.push(Emit::Release {
                    area: h,
                    pos: Vec2::new(self.center_x(i), self.floor(i) + h / 2.0),
                    vel: carried + Vec2::new(rng.range(-80.0, 80.0), rng.range(50.0, 200.0)),
                    spread: Vec2::new(1.0, h / 2.0),
                    scatter: 0.25,
                });
            }
        }
        self.refresh_top();
    }

    /// Shatter `share` of the pile into grains and lumps (snow-sim 3.9 BreakUp).
    pub fn break_up(&mut self, share: f64, vel: Vec2, rng: &mut Lcg64, out: &mut Vec<Emit>) {
        let n = self.count();
        let mut i = 0;
        while i < n {
            let end = (i + rng.inclusive(2, 6) as usize).min(n);
            let mut group = 0.0;
            let mut y_low = f64::INFINITY;
            let mut y_high = f64::NEG_INFINITY;
            for j in i..end {
                let h = self.heights[j];
                if h > 0.05 {
                    group += h * share * 2.0;
                    y_low = y_low.min(self.floor(j) + h * (1.0 - share));
                    y_high = y_high.max(self.floor(j) + h);
                }
            }
            let lump = if group > SMALLEST_CHUNK { rng.range(0.4, 0.7) } else { 0.0 };
            for j in i..end {
                let h = self.heights[j];
                if h > 0.05 {
                    let r = h * share;
                    self.heights[j] = h - r;
                    out.push(Emit::Release {
                        area: r * 2.0 * (1.0 - lump),
                        pos: Vec2::new(self.center_x(j), self.floor(j) + h - r),
                        vel,
                        spread: Vec2::new(1.0, r),
                        scatter: 0.25,
                    });
                }
            }
            if lump > 0.0 {
                let mid = (self.center_x(i) + self.center_x(end - 1)) / 2.0;
                out.push(Emit::Lump { area: group * lump, pos: Vec2::new(mid, (y_low + y_high) / 2.0), vel });
            }
            i = end;
        }
        self.refresh_top();
    }
}
