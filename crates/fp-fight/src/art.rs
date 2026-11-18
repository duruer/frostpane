//! Procedural art for the fight panel (snowman-fight 5). Frostpane's own
//! drawing: proportions follow the spec, the strokes and shapes are ours.
//! Everything is in Y-up points relative to the panel's bottom-left.

use crate::bout::{Bout, Pose, RIG_HEIGHT, arc, ease, mix};
use crate::story::{Emote, Face};
use fp_paint::tiny_skia::{Color, Path, PathBuilder, Pixmap};
use fp_paint::{Sketch, Trace, hex, rgb, rgba, solid};
use std::f32::consts::{PI, TAU};

/// Team colours: (main, deep).
const TEAMS: [(u32, u32); 2] = [(0x1F70E0, 0x144FAD), (0xDE3340, 0xA31C2B)];
const SWEAT: u32 = 0x8CD1FF;

fn outline() -> Color {
    rgb(0.68, 0.77, 0.9)
}
fn ink() -> Color {
    rgb(0.12, 0.13, 0.18)
}
fn white() -> Color {
    rgb(1.0, 1.0, 1.0)
}
fn brown() -> Color {
    rgb(0.42, 0.28, 0.18)
}

fn build(f: impl FnOnce(&mut Trace)) -> Path {
    let mut t = Trace::new();
    f(&mut t);
    t.finish().expect("art paths are never empty")
}

fn disc(x: f32, y: f32, r: f32) -> Path {
    PathBuilder::from_circle(x, y, r).expect("art circles have a radius")
}

/// A snowball with the soft up-left highlight (r in points).
fn snow_ball(s: &mut Sketch, x: f32, y: f32, r: f32) {
    let p = disc(x, y, r);
    let tones = [white(), rgb(0.97, 0.98, 1.0), rgb(0.8, 0.87, 0.97)];
    s.round(&p, &tones, Some(&[0.0, 0.55, 1.0]), (x - 0.3 * r, y + 0.4 * r), (x, y), 1.25 * r);
    s.stroke(&p, outline(), 0.6);
}

fn arm(s: &mut Sketch, mitten: Color) {
    s.line(0.0, 1.0, 0.0, 16.0, brown(), 1.7);
    s.circle(0.0, 18.0, 3.3, mitten);
}

/// The traitor's white flag; `alpha` fades the tossed one (4.5).
fn flag(s: &mut Sketch, alpha: f64) {
    s.line(10.0, 0.0, 10.0, 23.0, solid(rgb(0.55, 0.39, 0.24), alpha), 1.4);
    let pennant = build(|t| {
        t.lines(&[(10.5, 23.0), (26.0, 18.5), (10.5, 14.0)]).close();
    });
    s.fill(&pennant, solid(white(), alpha));
    s.stroke(&pennant, solid(outline(), alpha), 0.6);
}

fn hat(s: &mut Sketch, team: usize) {
    let (main, deep) = TEAMS[team];
    let dome = build(|t| {
        t.move_to(3.5, 3.0).quad(20.5, 3.0, 12.0, 21.0).close();
    });
    s.fill(&dome, hex(main));
    s.rect(2.0, 0.5, 20.0, 4.5, 2.2, hex(deep));
    s.circle(12.0, 12.6, 3.0, white());
}

/// Hat placed by its (12, 8) anchor.
fn hat_at(s: &mut Sketch, x: f32, y: f32, turn: f32, scale: f32, team: usize) {
    s.at(x, y, turn, scale, |s| {
        s.translate(-12.0, -8.0);
        hat(s, team);
    });
}

fn mound(s: &mut Sketch) {
    let pts: Vec<(f32, f32)> = (0..=61)
        .map(|i| {
            let u = i as f32 / 61.0;
            let base = (PI * u).sin().max(0.0);
            (i as f32, 17.0 * base.powf(0.8) + 2.0 * (9.0 * PI * u).sin() * base)
        })
        .collect();
    let heap = build(|t| {
        t.lines(&pts).close();
    });
    s.vertical(&heap, white(), rgb(0.8, 0.87, 0.97));
    s.stroke(&heap, outline(), 0.6);
    s.circle(22.0, 7.0, 1.5, rgb(0.16, 0.17, 0.22));
    s.circle(29.0, 9.0, 1.5, rgb(0.16, 0.17, 0.22));
    s.polygon(&[(35.0, 5.0), (47.0, 7.4), (35.0, 10.5)], rgb(1.0, 0.55, 0.15));
}

fn wall(s: &mut Sketch) {
    const ROWS: [&[(f32, f32)]; 3] =
        [&[(1.0, 12.5), (14.5, 12.5)], &[(0.0, 7.0), (7.5, 13.0), (21.0, 7.0)], &[(2.0, 11.5), (14.5, 11.5)]];
    for (row, bricks) in ROWS.iter().enumerate() {
        for &(x, w) in *bricks {
            let y = row as f32 * 9.6;
            s.rect(x, y, w, 9.2, 2.8, rgb(0.95, 0.97, 1.0));
            s.rect_outline((x, y, w, 9.2), 2.8, outline(), 0.6);
        }
    }
}

fn face(s: &mut Sketch, face: Face) {
    let line = 0.9;
    let (e0, e1) = ((14.0, 15.0), (19.5, 15.0));
    let smile = [(13.0, 6.6), (14.6, 5.4), (16.5, 5.0), (18.4, 5.4), (20.0, 6.6)];
    let frown = [(13.0, 5.0), (16.5, 6.8), (20.0, 5.0)];
    // cheeks and the carrot nose are shared by every face
    s.circle(12.0, 9.0, 2.4, rgba(1.0, 0.55, 0.62, 0.55));
    s.polygon(&[(20.5, 11.5), (30.5, 9.4), (20.5, 7.6)], rgb(1.0, 0.55, 0.15));
    let eyes = |s: &mut Sketch| {
        s.circle(e0.0, e0.1, 1.1, ink());
        s.circle(e1.0, e1.1, 1.1, ink());
    };
    match face {
        Face::Normal => {
            eyes(s);
            s.polyline(&smile, ink(), line);
        }
        Face::Blink => {
            s.line(12.8, 15.0, 15.2, 15.0, ink(), line);
            s.line(18.3, 15.0, 20.7, 15.0, ink(), line);
            s.polyline(&smile, ink(), line);
        }
        Face::Happy => {
            s.polyline(&[(12.6, 14.0), (14.0, 16.0), (15.4, 14.0)], ink(), line);
            s.polyline(&[(18.1, 14.0), (19.5, 16.0), (20.9, 14.0)], ink(), line);
            s.ellipse(13.0, 3.6, 7.0, 4.6, rgb(0.72, 0.2, 0.26));
        }
        Face::Shock => {
            for (x, y) in [e0, e1] {
                s.circle(x, y, 2.4, white());
                s.ring(x - 2.4, y - 2.4, 4.8, 4.8, ink(), 0.5);
                s.circle(x + 0.4, y, 1.1, ink());
            }
            s.circle(16.5, 5.6, 1.7, ink());
        }
        Face::Dizzy => {
            for (x, y) in [e0, e1] {
                s.line(x - 1.4, y - 1.4, x + 1.4, y + 1.4, ink(), line);
                s.line(x - 1.4, y + 1.4, x + 1.4, y - 1.4, ink(), line);
            }
            s.polyline(&[(12.6, 6.0), (14.5, 7.2), (16.5, 5.4), (18.5, 7.2), (20.4, 6.0)], ink(), line);
        }
        Face::Angry => {
            eyes(s);
            s.line(12.2, 18.0, 15.4, 16.6, ink(), 1.1);
            s.line(18.2, 16.6, 21.4, 18.0, ink(), 1.1);
            s.polyline(&frown, ink(), line);
        }
        Face::Sly => {
            s.line(12.6, 15.0, 15.4, 14.6, ink(), line);
            s.line(18.2, 14.6, 21.0, 15.0, ink(), line);
            s.line(18.4, 17.0, 21.2, 17.6, ink(), 0.7);
            s.polyline(&[(13.0, 6.0), (17.0, 5.6), (20.5, 7.4)], ink(), line);
        }
        Face::Sad => {
            for (x, y) in [e0, e1] {
                s.circle(x, y, 1.5, ink());
                s.circle(x + 0.5, y + 0.5, 0.5, white());
            }
            s.line(12.2, 17.2, 15.4, 18.2, ink(), 1.1);
            s.line(18.2, 18.2, 21.4, 17.2, ink(), 1.1);
            s.polyline(&frown, ink(), line);
        }
    }
}

fn emote(s: &mut Sketch, e: Emote) {
    match e {
        Emote::None => {}
        Emote::Alert => {
            s.line(0.0, 6.5, 0.0, -1.5, white(), 5.6);
            s.circle(0.0, -5.5, 2.8, white());
            s.line(0.0, 6.5, 0.0, -1.5, rgb(0.95, 0.27, 0.27), 3.2);
            s.circle(0.0, -5.5, 1.7, rgb(0.95, 0.27, 0.27));
        }
        Emote::Question => {
            let mut pts: Vec<(f32, f32)> = (0..=10)
                .map(|i| (3.4 * (2.9 - i as f32 * 0.36).cos(), 3.6 + 3.4 * (2.9 - i as f32 * 0.36).sin()))
                .collect();
            pts.push((0.0, -0.6));
            s.polyline(&pts, white(), 5.2);
            s.circle(0.0, -5.5, 2.6, white());
            s.polyline(&pts, rgb(0.2, 0.5, 0.95), 2.6);
            s.circle(0.0, -5.5, 1.5, rgb(0.2, 0.5, 0.95));
        }
        Emote::Heart => {
            let heart = |s: &mut Sketch, g: f32, color: Color| {
                s.circle(-3.0, 2.5, 3.4 + g, color);
                s.circle(3.0, 2.5, 3.4 + g, color);
                s.polygon(&[(-6.4 - g, 1.2), (6.4 + g, 1.2), (0.0, -6.4 - 1.6 * g)], color);
            };
            heart(s, 1.4, white());
            heart(s, 0.0, rgb(0.96, 0.3, 0.46));
        }
    }
}

fn bubble(s: &mut Sketch) {
    let pts: Vec<(f32, f32)> = (0..30)
        .map(|i| {
            let (rx, ry) = if i == 0 {
                (43.0, 28.0)
            } else if i % 2 == 0 {
                (43.0, 20.0)
            } else {
                (36.0, 15.0)
            };
            let a = PI / 2.0 + TAU * i as f32 / 30.0;
            (44.0 + rx * a.cos(), 29.0 + ry * a.sin())
        })
        .collect();
    let burst = build(|t| {
        t.lines(&pts).close();
    });
    s.fill(&burst, white());
    s.stroke(&burst, rgb(0.08, 0.08, 0.1), 1.5);
    // a peace sign in the middle
    let red = rgb(0.88, 0.1, 0.14);
    s.ring(37.0, 22.0, 14.0, 14.0, red, 2.0);
    s.line(44.0, 22.0, 44.0, 36.0, red, 2.0);
    s.line(44.0, 29.0, 39.0, 24.0, red, 2.0);
    s.line(44.0, 29.0, 49.0, 24.0, red, 2.0);
}

/// One snowman rig in rig-local units (anchor (35, 0)).
fn rig(s: &mut Sketch, p: &Pose, team: usize) {
    let (main, deep) = TEAMS[team];
    s.translate(-35.0, 0.0);
    s.at(29.0, 28.0, p.back as f32, 1.0, |s| arm(s, hex(main)));
    snow_ball(s, 35.0, 11.5, 11.5);
    snow_ball(s, 35.0, 26.0, 8.5);
    for (dx, dy) in [(2.5, 2.0), (3.0, -2.5)] {
        s.circle(35.0 + dx, 26.0 + dy, 1.05, rgb(0.16, 0.17, 0.22));
    }
    s.at(23.0, 22.5, 0.0, 1.0, |s| {
        s.rect(5.0, 0.5, 5.0, 9.0, 2.0, hex(deep));
        s.rect(2.5, 6.0, 19.0, 5.2, 2.6, hex(main));
    });
    snow_ball(s, 35.0, 38.0, 9.0);
    s.at(23.0, 25.0, 0.0, 1.0, |s| face(s, p.face));
    if p.hat {
        hat_at(s, 35.0, 50.5, -0.1, 1.0, team);
    }
    s.at(41.0, 28.0, p.front as f32, 1.0, |s| {
        arm(s, hex(main));
        if p.holding {
            snow_ball(s, 0.0, 22.0, 4.0);
        }
        if p.flag {
            s.translate(-8.0, 10.0);
            flag(s, 1.0);
        }
    });
    s.at(38.0, 69.0, 0.0, 1.0, |s| {
        s.scale_by(p.way as f32, 1.0);
        emote(s, p.emote);
    });
    if p.bubble > 0.0 {
        s.at(35.0, 60.0, 0.0, 1.0, |s| {
            s.scale_by(p.way as f32 * p.bubble as f32, p.bubble as f32);
            s.translate(-44.0, 0.0);
            bubble(s);
        });
    }
}

/// Paints the whole panel (5): mounds, rigs, wall, hats, flag, balls, specks.
pub(crate) fn paint_panel(b: &Bout, scale: f32) -> Option<Pixmap> {
    let (w, h) = ((b.panel.w * f64::from(scale)).ceil() as u32, (b.panel.h * f64::from(scale)).ceil() as u32);
    let mut pixmap = Pixmap::new(w, h)?;
    let mut s = Sketch::new(&mut pixmap, scale);
    let left = b.panel.x;
    let fading = b.fading();
    let poses = [b.pose(0), b.pose(1)];
    // z-order (5): both mounds, then both rigs
    for r in &b.f {
        if let Some(at) = r.pile_at {
            let k = ease((b.age - at) / 0.25) * (1.0 - fading);
            s.at((r.x - left) as f32, 0.0, 0.0, (r.size * k) as f32, |s| {
                s.translate(-31.0, 0.0);
                mound(s);
            });
        }
    }
    for (i, r) in b.f.iter().enumerate() {
        let p = &poses[i];
        if p.visible {
            s.save();
            s.translate((p.x - left) as f32, p.lift as f32);
            s.rotate((-p.lean * p.way) as f32);
            let wide = p.wide.unwrap_or_else(|| 1.0 / p.squash.max(0.5).sqrt());
            s.scale_by((p.way * r.size * wide) as f32, (r.size * p.squash) as f32);
            rig(&mut s, p, i);
            s.restore();
        }
    }
    if b.wall_height > 0.0 {
        s.at((b.wall_x - left) as f32, 0.0, 0.0, 1.0, |s| {
            s.scale_by(b.scale as f32, (b.scale * b.wall_height.max(0.01)) as f32);
            s.translate(-14.0, 0.0);
            wall(s);
        });
    }
    for (i, r) in b.f.iter().enumerate() {
        let Some(from) = r.hat_from else {
            continue;
        };
        if let Some(thief) = r.worn_by {
            let (tp, t) = (&poses[thief], &b.f[thief]);
            if tp.visible {
                let h = RIG_HEIGHT * t.size * tp.squash;
                let rot = -tp.lean * tp.way;
                let (x, y) = (tp.x - rot.sin() * h, tp.lift + tp.lean.cos() * h);
                hat_at(&mut s, (x - left) as f32, y as f32, (rot + 0.2) as f32, t.size as f32, i);
            }
        } else if fading < 1.0 {
            let t = ((b.age - from) / 0.95).clamp(0.0, 1.0);
            let side = if r.last_way > 0.0 { -1.0 } else { 1.0 };
            let rot = if t < 1.0 { t * 9.0 * side } else { 0.22 * side };
            let x = r.x + side * 18.0 * r.size * t;
            let y = (mix(50.0 * r.size, 22.0 * r.size, t) + arc(t) * 62.0 * r.size) * (1.0 - fading);
            hat_at(&mut s, (x - left) as f32, y as f32, rot as f32, (r.size * (1.0 - fading)) as f32, i);
        }
    }
    if let Some(t) = &b.toss {
        let (pos, k) = (t.pos(), t.t());
        s.at((pos.x - left) as f32, pos.y as f32, (k * 7.0 * t.dir) as f32, b.scale as f32, |s| {
            s.translate(-10.0, -12.0);
            flag(s, 1.0 - k * k);
        });
    }
    for fl in b.flights.iter().take(4) {
        let pos = fl.pos();
        s.at((pos.x - left) as f32, pos.y as f32, 0.0, fl.size as f32, |s| snow_ball(s, 0.0, 0.0, 4.0));
    }
    for k in b.specks.iter().flatten() {
        let o = (k.life / k.total).max(0.0);
        let color = if k.sweat { hex(SWEAT) } else { white() };
        s.circle((k.x - left) as f32, k.y as f32, (k.size * (0.5 + o / 2.0) / 2.0) as f32, solid(color, o));
    }
    Some(pixmap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::story::{Aim, Move, React, Story};
    use fp_core::rng::Lcg64;

    fn ink_count(px: &Pixmap) -> usize {
        px.pixels().iter().filter(|p| p.alpha() > 0).count()
    }

    fn blank(w: u32, h: u32) -> Pixmap {
        Pixmap::new(w, h).unwrap()
    }

    #[test]
    fn every_face_draws_something_different() {
        let faces =
            [Face::Normal, Face::Blink, Face::Happy, Face::Shock, Face::Dizzy, Face::Angry, Face::Sly, Face::Sad];
        let mut sums = Vec::new();
        for f in faces {
            let mut px = blank(120, 120);
            Sketch::new(&mut px, 3.0).at(5.0, 5.0, 0.0, 1.0, |s| face(s, f));
            assert!(ink_count(&px) > 100);
            sums.push(px.data().iter().map(|b| u64::from(*b)).sum::<u64>());
        }
        sums.sort_unstable();
        sums.dedup();
        assert_eq!(sums.len(), 8, "faces differ");
    }

    #[test]
    fn emotes_and_bubble_draw() {
        assert_eq!(
            ink_count(&{
                let mut px = blank(60, 60);
                Sketch::new(&mut px, 2.0).at(15.0, 15.0, 0.0, 1.0, |s| emote(s, Emote::None));
                px
            }),
            0
        );
        for e in [Emote::Alert, Emote::Question, Emote::Heart] {
            let mut px = blank(60, 60);
            Sketch::new(&mut px, 2.0).at(15.0, 15.0, 0.0, 1.0, |s| emote(s, e));
            assert!(ink_count(&px) > 50, "{e:?}");
        }
        let mut px = blank(200, 120);
        Sketch::new(&mut px, 2.0).at(0.0, 0.0, 0.0, 1.0, bubble);
        assert!(ink_count(&px) > 1000);
        // the burst is red-on-white: both a red and a white pixel exist
        assert!(px.pixels().iter().any(|p| p.red() > 200 && p.green() < 60));
    }

    #[test]
    fn mound_wall_flag_hat_paint() {
        for draw in [mound as fn(&mut Sketch), wall, |s: &mut Sketch| flag(s, 1.0)] {
            let mut px = blank(80, 50);
            draw(&mut Sketch::new(&mut px, 1.0));
            assert!(ink_count(&px) > 30);
        }
        let mut px = blank(60, 60);
        hat_at(&mut Sketch::new(&mut px, 1.0), 30.0, 30.0, 0.3, 1.0, 1);
        assert!(ink_count(&px) > 100);
    }

    fn wait(t: f64) -> Move {
        Move::Wait { t, look: None, sign: Emote::None, tremble: false }
    }

    #[test]
    fn kitchen_sink_fight_paints_every_layer() {
        let f0 = vec![
            Move::Build { at: 400.0, t: 0.4 },
            Move::FlagOut,
            Move::Wave(0.4),
            Move::FlagAway,
            Move::Scoop,
            Move::WindUp(Face::Angry),
            Move::Release(Aim::Head),
            wait(1.2),
            Move::GrabHat,
            Move::Cheer(0.3),
            Move::Dive,
        ];
        let f1 = vec![
            Move::Jump { look: Some(Face::Shock), sign: Emote::Alert },
            Move::Wait { t: 0.3, look: Some(Face::Sad), sign: Emote::Question, tremble: false },
            Move::Handshake(0.3),
            Move::Brace(React::Take),
            Move::Crumble,
        ];
        let story = Story { acts: vec![[f0, f1]], spawn: [200.0, 600.0], stand: [200.0, 600.0], ending: None };
        let mut b = Bout::new(story, Lcg64::new(3), 100.0, 900.0, 0.76, 0.0);
        let mut out = Vec::new();
        let mut seen = [false; 6];
        let mut n = 0;
        while !b.finished() && n < 2000 {
            b.step(1.0 / 60.0, None, &mut out);
            seen[0] |= b.wall_height > 0.0;
            seen[1] |= b.toss.is_some();
            seen[2] |= !b.flights.is_empty();
            seen[3] |= b.f[1].worn_by.is_some() && b.f[0].visible();
            seen[4] |= b.f[1].pile_at.is_some();
            seen[5] |= b.pose(0).bubble > 0.0;
            if n % 3 == 0 {
                let px = paint_panel(&b, 1.0).unwrap();
                assert_eq!((px.width(), px.height()), (920, 180));
                assert!(ink_count(&px) > 0);
            }
            n += 1;
        }
        assert!(seen.iter().all(|s| *s), "{seen:?}");
        assert!(b.finished());
    }

    #[test]
    fn flying_hat_fades_away_with_the_bout() {
        let story = Story {
            acts: vec![[vec![Move::Crumble], vec![Move::Crumble]]],
            spawn: [200.0, 600.0],
            stand: [200.0, 600.0],
            ending: None,
        };
        let mut b = Bout::new(story, Lcg64::new(3), 100.0, 900.0, 0.76, 0.0);
        let mut out = Vec::new();
        let mut counts = Vec::new();
        while !b.finished() {
            b.step(1.0 / 60.0, None, &mut out);
            if b.fading() > 0.0 {
                counts.push(ink_count(&paint_panel(&b, 1.0).unwrap()));
            }
        }
        assert!(counts.len() > 5);
        assert!(counts.first() > counts.last(), "the mound and hat shrink away: {counts:?}");
        assert_eq!(b.fading(), 1.0);
        let end = paint_panel(&b, 1.0).unwrap();
        assert_eq!(ink_count(&end), 0);
    }

    #[test]
    fn tossed_flag_fades_out() {
        let story = Story { acts: vec![[vec![], vec![]]], spawn: [200.0, 600.0], stand: [200.0, 600.0], ending: None };
        let mut b = Bout::new(story, Lcg64::new(3), 100.0, 900.0, 0.76, 0.0);
        b.f[0].gone = true;
        b.f[1].gone = true;
        let peak = |b: &Bout| paint_panel(b, 1.0).unwrap().pixels().iter().map(|p| p.alpha()).max().unwrap();
        assert_eq!(peak(&b), 0);
        b.toss = Some(crate::bout::Toss { x: 300.0, y: 30.0, dir: 1.0, age: 0.0 });
        assert_eq!(peak(&b), 255);
        // t = 0.9: opacity 1 - 0.81 = 0.19
        b.toss = Some(crate::bout::Toss { x: 300.0, y: 30.0, dir: 1.0, age: 0.63 });
        let a = peak(&b);
        // overlapping fill and stroke stack a little above 0.19
        assert!((40..=100).contains(&a), "{a}");
    }

    #[test]
    fn zero_size_panel_is_none() {
        let b = Bout::new(
            Story { acts: vec![[vec![], vec![]]], spawn: [1.0, 2.0], stand: [1.0, 2.0], ending: None },
            Lcg64::new(1),
            0.0,
            10.0,
            0.5,
            0.0,
        );
        assert!(paint_panel(&b, 0.0).is_none());
        assert!(paint_panel(&b, 1.0).is_some());
    }
}
