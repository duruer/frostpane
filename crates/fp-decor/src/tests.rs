use super::*;
use fp_core::rng::Lcg64;
use serde_json::json;

const DT: f64 = 1.0 / 60.0;

fn key(id: u64) -> SpriteKey {
    SpriteKey::new(Owner::Decoration, id)
}

fn ptr(x: f64, y: f64, down: bool, hot: Option<u64>) -> PointerInput {
    PointerInput { pos: Some(Vec2::new(x, y)), left_down: down, right_down: false, hot: hot.map(key) }
}

struct Rig {
    d: Decorations,
    t: f64,
}

impl Rig {
    fn new(max: u32) -> Self {
        Rig { d: Decorations::new(max), t: 0.0 }
    }

    fn step(&mut self, p: PointerInput) -> bool {
        self.t += DT;
        self.d.step(DT, self.t, &p)
    }

    /// Hover `id`, press at `at`, drag to `to`, release; returns the save flag.
    fn gesture(&mut self, id: u64, at: Vec2, to: Vec2) -> bool {
        self.step(ptr(at.x, at.y, false, Some(id)));
        self.step(ptr(at.x, at.y, true, Some(id)));
        self.step(ptr(to.x, to.y, true, Some(id)));
        self.step(ptr(to.x, to.y, false, Some(id)))
    }

    fn local(&self, id: u64, lx: f64, ly: f64) -> Vec2 {
        let f = self.d.get(id).unwrap().frame();
        Vec2::new(f.x + lx, f.y + ly)
    }

    fn sprite(&self, id: u64) -> Sprite {
        let mut v = Vec::new();
        self.d.sprites(&mut v);
        *v.iter().find(|s| s.key == key(id)).unwrap()
    }
}

#[test]
fn catalogue() {
    for (i, k) in Kind::ALL.into_iter().enumerate() {
        assert_eq!(Kind::parse(k.raw()), Some(k), "{i}");
        let (w, h) = k.art_size(None);
        let (px, py) = k.pivot(None);
        assert!(px >= 0.0 && px <= w && py >= 0.0 && py <= h);
        assert!(k.sway_angle() >= 0.0);
    }
    assert_eq!(Kind::parse("Lights"), None);
    assert_eq!(Kind::parse(""), None);
    assert_eq!(Kind::Lights.art_size(None), (260.0, 64.0));
    assert_eq!(Kind::Lights.art_size(Some(Strand { length: 500.0, dip: 30.0 })), (500.0, 76.0));
    assert_eq!(Kind::Lights.pivot(None), (130.0, 56.0));
    assert_eq!(Kind::Gift.pivot(None), (40.0, 2.0));
    assert_eq!(Kind::Lights.sway_angle(), 0.0);
    assert_eq!(Kind::Gift.sway_angle(), 0.018);
    assert_eq!(Kind::RedBauble.raw(), "redBauble");
}

#[test]
fn placed_geometry() {
    let p = Placed { id: 1, kind: Kind::Star, center: Vec2::new(100.0, 100.0), zoom: 1.5, strand: None };
    assert_eq!(p.box_size(), (96.0, 96.0));
    assert_eq!(p.outer(), (144.0, 144.0));
    assert_eq!(p.frame(), Frame::new(28.0, 28.0, 144.0, 144.0));
}

#[test]
fn add_remove_cap() {
    let mut r = Rig::new(2);
    assert!(r.d.is_empty());
    let a = r.d.add(Kind::Gift, Vec2::new(500.0, 500.0)).unwrap();
    let b = r.d.add(Kind::Lights, Vec2::new(600.0, 300.0)).unwrap();
    assert!(r.d.is_full());
    assert_eq!(r.d.max(), 2);
    assert_eq!(r.d.add(Kind::Star, Vec2::ZERO), None);
    assert_eq!(r.d.len(), 2);
    r.d.set_max(3);
    assert_eq!(r.d.max(), 3);
    assert!(!r.d.is_full());
    r.d.set_max(2);
    assert_eq!(r.d.items().map(|p| p.id).collect::<Vec<_>>(), vec![a, b]);
    let s = r.sprite(a);
    assert_eq!(s.z, ZBand::Desktop);
    assert_eq!(s.hit, HitMode::Alpha);
    assert_eq!(s.opacity, 1.0);
    assert_eq!(s.frame, r.d.get(a).unwrap().frame());
    // pop-in finishes after 0.55 s and bumps the revision on the way
    let rev0 = r.sprite(a).rev;
    r.step(PointerInput::default());
    assert!(r.sprite(a).rev > rev0);
    for _ in 0..40 {
        r.step(PointerInput::default());
    }
    r.d.remove(a);
    r.d.remove(a);
    r.d.remove(999);
    assert_eq!(r.d.len(), 1);
    assert!(r.d.get(a).is_none());
    let s = r.sprite(a);
    assert_eq!(s.hit, HitMode::Never);
    assert_eq!(s.opacity, 1.0);
    assert!(r.d.paint(a, 1.0).is_some());
    assert!(!r.d.solid(a, 50.0, 50.0));
    r.step(PointerInput::default());
    assert!(r.sprite(a).opacity < 1.0);
    for _ in 0..20 {
        r.step(PointerInput::default());
    }
    let mut v = Vec::new();
    r.d.sprites(&mut v);
    assert_eq!(v.len(), 1);
    assert!(r.d.paint(a, 1.0).is_none());
    r.d.remove_all();
    assert!(r.d.is_empty());
    for _ in 0..20 {
        r.step(PointerInput::default());
    }
    v.clear();
    r.d.sprites(&mut v);
    assert!(v.is_empty());
}

#[test]
fn paint_all_kinds() {
    let mut r = Rig::new(30);
    for (i, kind) in Kind::ALL.into_iter().enumerate() {
        let id = r.d.add(kind, Vec2::new(300.0 + 10.0 * i as f64, 400.0)).unwrap();
        for _ in 0..50 {
            r.step(PointerInput::default());
        }
        let f = r.d.get(id).unwrap().frame();
        let pm = r.d.paint(id, 1.5).unwrap();
        assert_eq!(pm.width(), (f.w * 1.5).ceil() as u32, "{kind:?}");
        assert_eq!(pm.height(), (f.h * 1.5).ceil() as u32, "{kind:?}");
        assert!(pm.pixels().iter().any(|p| p.alpha() > 200), "{kind:?} has body pixels");
        // hover chrome adds the button and grips
        r.step(ptr(f.mid_x(), f.y + f.h / 2.0, false, Some(id)));
        assert_eq!(r.d.hovered(), Some(id));
        let hov = r.d.paint(id, 1.0).unwrap();
        assert!(hov.pixels().iter().any(|p| p.alpha() > 0));
        r.step(ptr(-1000.0, -1000.0, false, None));
        let big = icon(kind, 2.0);
        let (aw, ah) = kind.art_size(None);
        assert_eq!((big.width(), big.height()), ((aw * 2.0) as u32, (ah * 2.0) as u32));
        assert!(big.pixels().iter().any(|p| p.alpha() > 200));
        let sq = icon_square(kind, 48);
        assert_eq!((sq.width(), sq.height()), (48, 48));
    }
}

#[test]
fn zoomed_paint_and_solid() {
    let mut d = Decorations::new(5);
    d.load_json(&json!([{"kind": "gift", "x": 200, "y": 200, "size": 2.0}, {"kind": "lights", "x": 500, "y": 200}]));
    let ids: Vec<u64> = d.items().map(|p| p.id).collect();
    let gift = d.get(ids[0]).unwrap().clone();
    assert_eq!(gift.outer(), (208.0, 212.0));
    // centre of the body is solid, the far corner of the padded box is not
    assert!(d.solid(ids[0], 104.0, 100.0));
    assert!(!d.solid(ids[0], 1.0, 1.0));
    // outside the box entirely
    assert!(!d.solid(ids[0], -50.0, -50.0));
    assert!(!d.solid(999, 10.0, 10.0));
    let pm = d.paint(ids[0], 1.0).unwrap();
    assert_eq!((pm.width(), pm.height()), (208, 212));
    // lights: a bulb is solid, empty air above the wire is not
    let lf = d.get(ids[1]).unwrap().frame();
    assert_eq!((lf.w, lf.h), (284.0, 88.0));
    assert!(d.solid(ids[1], 142.0, 50.0 - 3.0));
    assert!(!d.solid(ids[1], 142.0, 86.0));
}

#[test]
fn move_gesture() {
    let mut r = Rig::new(10);
    let a = r.d.add(Kind::Gift, Vec2::new(500.0, 500.0)).unwrap();
    let b = r.d.add(Kind::Star, Vec2::new(800.0, 500.0)).unwrap();
    // tiny wiggle: no move, no save
    let save = r.gesture(a, Vec2::new(500.0, 500.0), Vec2::new(501.0, 501.0));
    assert!(!save);
    assert_eq!(r.d.get(a).unwrap().center, Vec2::new(500.0, 500.0));
    // the press raised `a` above `b`
    assert_eq!(r.d.items().map(|p| p.id).collect::<Vec<_>>(), vec![b, a]);
    // real drag
    r.step(ptr(500.0, 500.0, false, Some(a)));
    assert_eq!(r.sprite(a).hit, HitMode::Always);
    r.step(ptr(500.0, 500.0, true, Some(a)));
    assert!(r.d.busy());
    assert_eq!(r.sprite(a).hit, HitMode::Always);
    r.step(ptr(520.0, 530.0, true, Some(a)));
    r.step(ptr(530.0, 540.0, true, None));
    assert_eq!(r.d.get(a).unwrap().center, Vec2::new(530.0, 540.0));
    assert!(r.step(ptr(530.0, 540.0, false, None)));
    assert!(!r.d.busy());
    // idle steps with no press and no gesture
    assert!(!r.step(ptr(530.0, 540.0, false, None)));
    // pressing without a hover does nothing
    r.step(ptr(5000.0, 5000.0, false, None));
    r.step(ptr(5000.0, 5000.0, true, None));
    assert!(!r.d.busy());
    r.step(ptr(5000.0, 5000.0, true, None));
    assert!(!r.step(ptr(5000.0, 5000.0, false, None)));
    // missing pointer position: hover cannot be kept or pressed
    r.step(PointerInput { pos: None, left_down: false, right_down: false, hot: Some(key(a)) });
    r.step(PointerInput { pos: None, left_down: true, right_down: false, hot: Some(key(a)) });
    assert!(!r.d.busy());
    r.step(PointerInput { pos: None, left_down: true, right_down: false, hot: None });
    r.step(PointerInput::default());
}

#[test]
fn press_outside_frame_ignored() {
    let mut r = Rig::new(10);
    let a = r.d.add(Kind::Gift, Vec2::new(500.0, 500.0)).unwrap();
    r.step(ptr(500.0, 500.0, false, Some(a)));
    // press far from the item while it is the hot pick (stale hover)
    r.step(PointerInput { pos: Some(Vec2::new(900.0, 900.0)), left_down: true, right_down: false, hot: Some(key(a)) });
    assert!(!r.d.busy());
}

#[test]
fn hover_tracking() {
    let mut r = Rig::new(10);
    let a = r.d.add(Kind::Gift, Vec2::new(500.0, 500.0)).unwrap();
    r.step(ptr(500.0, 500.0, false, Some(a)));
    assert_eq!(r.d.hovered(), Some(a));
    // kept while the cursor stays inside the frame even without a pick
    r.step(ptr(510.0, 500.0, false, None));
    assert_eq!(r.d.hovered(), Some(a));
    // leaves the frame: cleared
    r.step(ptr(900.0, 900.0, false, None));
    assert_eq!(r.d.hovered(), None);
    // foreign sprite key and dead id are ignored
    r.step(PointerInput {
        pos: Some(Vec2::ZERO),
        left_down: false,
        right_down: false,
        hot: Some(SpriteKey::new(Owner::Tree, a)),
    });
    assert_eq!(r.d.hovered(), None);
    r.step(ptr(0.0, 0.0, false, Some(777)));
    assert_eq!(r.d.hovered(), None);
    // hover grow only for non-lights
    r.step(ptr(500.0, 500.0, false, Some(a)));
    r.d.remove(a);
    assert_eq!(r.d.hovered(), None);
}

#[test]
fn button_remove() {
    let mut r = Rig::new(10);
    let a = r.d.add(Kind::Gift, Vec2::new(500.0, 500.0)).unwrap();
    let btn = r.local(a, 90.0, 92.0);
    assert_eq!(button_center(r.d.get(a).unwrap()), Vec2::new(90.0, 92.0));
    // press on the button, release away: cancelled
    r.step(ptr(btn.x, btn.y, false, Some(a)));
    r.step(ptr(btn.x, btn.y, true, Some(a)));
    assert!(r.d.busy());
    r.step(ptr(btn.x, btn.y, true, Some(a)));
    assert!(!r.step(ptr(300.0, 300.0, false, Some(a))));
    assert_eq!(r.d.len(), 1);
    // release without a position cancels as well
    r.step(ptr(btn.x, btn.y, false, Some(a)));
    r.step(ptr(btn.x, btn.y, true, Some(a)));
    assert!(!r.step(PointerInput::default()));
    assert_eq!(r.d.len(), 1);
    // press and release on the button removes it
    assert!(r.gesture(a, btn, btn));
    assert_eq!(r.d.len(), 0);
    // armed item removed mid-press: release cancels quietly
    let b = r.d.add(Kind::Gift, Vec2::new(500.0, 500.0)).unwrap();
    let btn = r.local(b, 90.0, 92.0);
    r.step(ptr(btn.x, btn.y, false, Some(b)));
    r.step(ptr(btn.x, btn.y, true, Some(b)));
    r.d.remove(b);
    assert!(!r.d.busy());
}

#[test]
fn resize_gesture() {
    let mut r = Rig::new(10);
    let a = r.d.add(Kind::Gift, Vec2::new(500.0, 500.0)).unwrap();
    let corner = r.local(a, 92.0, 12.0);
    let f0 = r.d.get(a).unwrap().frame();
    // grow: drag right and down
    assert!(r.gesture(a, corner, Vec2::new(corner.x + 100.0, corner.y - 60.0)));
    let p = r.d.get(a).unwrap().clone();
    assert!(p.zoom > 1.5);
    let f1 = p.frame();
    assert_eq!(f1.x, f0.x);
    assert!((f1.max_y() - f0.max_y()).abs() < 1e-9);
    // clamp at the maximum
    let corner = r.local(a, (12.0 + 80.0) * p.zoom, 12.0 * p.zoom);
    r.gesture(a, corner, Vec2::new(corner.x + 5000.0, corner.y - 5000.0));
    assert_eq!(r.d.get(a).unwrap().zoom, ZOOM_MAX);
    // shrink to the minimum
    let corner = r.local(a, ((12.0 + 80.0) * ZOOM_MAX).min(r.d.get(a).unwrap().outer().0 - 7.0), 12.0 * ZOOM_MAX);
    r.gesture(a, corner, Vec2::new(corner.x - 5000.0, corner.y + 5000.0));
    assert_eq!(r.d.get(a).unwrap().zoom, ZOOM_MIN);
    // hover grow is off while resizing: the sprite rev changes only through zoom
    let corner =
        r.local(a, ((12.0 + 80.0) * ZOOM_MIN).min(r.d.get(a).unwrap().outer().0 - 7.0), (12.0 * ZOOM_MIN).max(7.0));
    // a wiggle that ends where it started saves nothing
    let z0 = r.d.get(a).unwrap().zoom;
    r.step(ptr(corner.x, corner.y, false, Some(a)));
    r.step(ptr(corner.x, corner.y, true, Some(a)));
    r.step(ptr(corner.x + 30.0, corner.y - 30.0, true, Some(a)));
    assert!(r.d.get(a).unwrap().zoom > z0);
    r.step(ptr(corner.x, corner.y, true, Some(a)));
    assert!(!r.step(ptr(corner.x, corner.y, false, Some(a))));
    assert_eq!(r.d.get(a).unwrap().zoom, z0);
    // a drag below the 0.004 threshold is ignored
    r.step(ptr(corner.x, corner.y, false, Some(a)));
    r.step(ptr(corner.x, corner.y, true, Some(a)));
    r.step(ptr(corner.x + 0.01, corner.y, true, Some(a)));
    assert_eq!(r.d.get(a).unwrap().zoom, z0);
    r.step(ptr(corner.x, corner.y, false, Some(a)));
}

#[test]
fn stretch_gesture() {
    let mut r = Rig::new(10);
    let a = r.d.add(Kind::Lights, Vec2::new(1000.0, 600.0)).unwrap();
    assert_eq!(r.d.get(a).unwrap().strand, None);
    // cache the default-strand mask; stretching must drop it
    assert!(!r.d.solid(a, 1.0, 1.0));
    let g = grips(r.d.get(a).unwrap());
    assert_eq!(g.iter().map(|(i, _)| *i).collect::<Vec<_>>(), vec![0, 1, 2]);
    let f0 = r.d.get(a).unwrap().frame();
    // right grip: length grows, left edge and top stay
    let right = r.local(a, g[1].1.x, g[1].1.y);
    assert!(r.gesture(a, right, Vec2::new(right.x + 100.0, right.y)));
    let p = r.d.get(a).unwrap().clone();
    assert_eq!(p.strand, Some(Strand { length: 360.0, dip: 18.0 }));
    let f1 = p.frame();
    assert_eq!(f1.x, f0.x);
    assert_eq!(f1.max_y(), f0.max_y());
    // left grip: right edge stays
    let g = grips(&p);
    let left = r.local(a, g[0].1.x, g[0].1.y);
    assert!(r.gesture(a, left, Vec2::new(left.x - 40.0, left.y)));
    let p = r.d.get(a).unwrap().clone();
    assert_eq!(p.strand.unwrap().length, 400.0);
    assert!((p.frame().max_x() - f1.max_x()).abs() < 1e-9);
    // middle grip: drag down = more sag, then clamp
    let g = grips(&p);
    let mid = r.local(a, g[2].1.x, g[2].1.y);
    assert!(r.gesture(a, mid, Vec2::new(mid.x, mid.y - 20.0)));
    assert_eq!(r.d.get(a).unwrap().strand.unwrap().dip, 38.0);
    let p = r.d.get(a).unwrap().clone();
    let g = grips(&p);
    let mid = r.local(a, g[2].1.x, g[2].1.y);
    assert!(r.gesture(a, mid, Vec2::new(mid.x, mid.y - 5000.0)));
    assert_eq!(r.d.get(a).unwrap().strand.unwrap().dip, 260.0);
    // dip to zero via a small step still lands exactly on 0 (boundary accept)
    let p = r.d.get(a).unwrap().clone();
    let g = grips(&p);
    let mid = r.local(a, g[2].1.x, g[2].1.y);
    r.step(ptr(mid.x, mid.y, false, Some(a)));
    r.step(ptr(mid.x, mid.y, true, Some(a)));
    r.step(ptr(mid.x, mid.y + 5000.0, true, Some(a)));
    assert_eq!(r.d.get(a).unwrap().strand.unwrap().dip, 0.0);
    // sub-2 changes are ignored
    r.step(ptr(mid.x, mid.y + 259.0, true, Some(a)));
    assert_eq!(r.d.get(a).unwrap().strand.unwrap().dip, 0.0);
    r.step(ptr(mid.x, mid.y + 258.0, true, Some(a)));
    assert_eq!(r.d.get(a).unwrap().strand.unwrap().dip, 2.0);
    r.step(ptr(mid.x, mid.y + 260.0, true, Some(a)));
    assert_eq!(r.d.get(a).unwrap().strand.unwrap().dip, 0.0);
    r.step(ptr(mid.x, mid.y + 260.5, true, Some(a)));
    assert_eq!(r.d.get(a).unwrap().strand.unwrap().dip, 0.0);
    r.step(ptr(mid.x, mid.y, false, Some(a)));
    // length floor of 110
    let p = r.d.get(a).unwrap().clone();
    let g = grips(&p);
    let left = r.local(a, g[0].1.x, g[0].1.y);
    r.gesture(a, left, Vec2::new(left.x + 9000.0, left.y));
    assert_eq!(r.d.get(a).unwrap().strand.unwrap().length, 110.0);
    let p = r.d.get(a).unwrap().clone();
    let g = grips(&p);
    let right = r.local(a, g[1].1.x, g[1].1.y);
    r.step(ptr(right.x, right.y, false, Some(a)));
    r.step(ptr(right.x, right.y, true, Some(a)));
    r.step(ptr(right.x - 3.0, right.y, true, Some(a)));
    assert_eq!(r.d.get(a).unwrap().strand.unwrap().length, 110.0);
    r.step(ptr(right.x, right.y, false, Some(a)));
    // grabbing and releasing without change saves nothing
    let p = r.d.get(a).unwrap().clone();
    let g = grips(&p);
    let right = r.local(a, g[1].1.x, g[1].1.y);
    assert!(!r.gesture(a, right, right));
    // the mask follows the new strand (110 x 0): a bulb of the short wire is
    // solid, the air above it is not
    assert_eq!(p.strand, Some(Strand { length: 110.0, dip: 0.0 }));
    let ((bx, by), _) = p.strand.unwrap().bulb(1);
    assert!(r.d.solid(a, PAD + bx, PAD + by - 13.5));
    assert!(!r.d.solid(a, PAD + bx, p.outer().1 - 2.0));
}

#[test]
fn gesture_item_removed_midway() {
    let mut r = Rig::new(10);
    let a = r.d.add(Kind::Gift, Vec2::new(500.0, 500.0)).unwrap();
    r.step(ptr(500.0, 500.0, false, Some(a)));
    r.step(ptr(500.0, 500.0, true, Some(a)));
    r.d.remove(a);
    r.step(ptr(520.0, 520.0, true, Some(a)));
    assert!(!r.step(ptr(520.0, 520.0, false, Some(a))));
    // drag with the position missing is a no-op
    let b = r.d.add(Kind::Gift, Vec2::new(500.0, 500.0)).unwrap();
    r.step(ptr(500.0, 500.0, false, Some(b)));
    r.step(ptr(500.0, 500.0, true, Some(b)));
    r.step(PointerInput { pos: None, left_down: true, right_down: false, hot: None });
    r.step(ptr(500.0, 500.0, false, Some(b)));
    // a gesture whose item was dropped by an unrelated path
    r.d.items.clear();
    r.step(ptr(500.0, 500.0, true, Some(b)));
}

#[test]
fn moved_item_is_clamped_on_screen() {
    let mut r = Rig::new(10);
    let a = r.d.add(Kind::Gift, Vec2::new(500.0, 500.0)).unwrap();
    assert!(!r.d.set_screens(vec![Frame::new(0.0, 0.0, 1920.0, 1080.0)]));
    assert!(r.gesture(a, Vec2::new(500.0, 500.0), Vec2::new(5000.0, 500.0)));
    assert_eq!(r.d.get(a).unwrap().center.x, 1920.0 - 30.0);
    assert!(r.d.set_screens(vec![Frame::new(0.0, 0.0, 800.0, 600.0)]));
    assert_eq!(r.d.get(a).unwrap().center.x, 770.0);
}

#[test]
fn on_screen_rules() {
    let s = [Frame::new(0.0, 0.0, 100.0, 100.0), Frame::new(100.0, 0.0, 100.0, 100.0)];
    assert_eq!(on_screen(Vec2::new(50.0, 50.0), &s), Vec2::new(50.0, 50.0));
    assert_eq!(on_screen(Vec2::new(-10.0, 50.0), &s), Vec2::new(30.0, 50.0));
    assert_eq!(on_screen(Vec2::new(500.0, 500.0), &s), Vec2::new(170.0, 70.0));
    assert_eq!(on_screen(Vec2::new(5.0, 5.0), &[]), Vec2::new(5.0, 5.0));
    assert_eq!(on_screen(Vec2::new(5.0, 5.0), &[Frame::new(100.0, 100.0, 0.0, 0.0)]), Vec2::new(5.0, 5.0));
}

#[test]
fn persistence_roundtrip() {
    let mut d = Decorations::new(30);
    let a = d.add(Kind::Lights, Vec2::new(960.0, 80.0)).unwrap();
    d.add(Kind::Star, Vec2::new(300.504, 400.0)).unwrap();
    let c = d.add(Kind::Gift, Vec2::new(1.0, 2.0)).unwrap();
    d.items.iter_mut().find(|i| i.p.id == c).unwrap().p.zoom = 1.40004;
    d.items.iter_mut().find(|i| i.p.id == a).unwrap().p.strand = Some(Strand { length: 260.0, dip: 18.0 });
    let j = d.save_json();
    assert_eq!(
        j,
        json!([
            {"kind": "lights", "x": 960, "y": 80, "length": 260, "dip": 18},
            {"kind": "star", "x": 300.5, "y": 400},
            {"kind": "gift", "x": 1, "y": 2, "size": 1.4}
        ])
    );
    let mut e = Decorations::new(30);
    e.load_json(&j);
    assert_eq!(e.len(), 3);
    assert_eq!(e.save_json(), j);
    // fresh lights omit the strand
    let mut f = Decorations::new(30);
    f.add(Kind::Lights, Vec2::new(1.5, 2.5));
    assert_eq!(f.save_json(), json!([{"kind": "lights", "x": 1.5, "y": 2.5}]));
}

#[test]
fn load_rules() {
    let mut d = Decorations::new(3);
    d.add(Kind::Star, Vec2::ZERO);
    d.load_json(&json!("nope"));
    assert!(d.is_empty());
    let v = json!([
        {"kind": "bogus", "x": 1, "y": 1},
        {"kind": "Star", "x": 1, "y": 1},
        {"kind": "star", "x": 10},
        {"kind": "gift", "x": 10, "y": 20, "size": 9, "length": 500},
        {"kind": "star", "x": 99, "y": 99},
        {"kind": "gift", "x": 5, "y": 5}
    ]);
    d.load_json(&v);
    // the first 3 entries are taken before filtering: bogus, Star, star-without-y
    assert!(d.is_empty());
    let mut d = Decorations::new(10);
    d.load_json(&v);
    let items: Vec<_> = d.items().cloned().collect();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].zoom, ZOOM_MAX);
    assert_eq!(items[0].strand, None);
    assert_eq!(items[2].zoom, 1.0);
    let w = json!([
        {"kind": "lights", "x": 1, "y": 1, "length": 99999, "dip": -4, "size": 2},
        {"kind": "lights", "x": 2, "y": 2, "length": 300},
        {"kind": "star", "x": 3, "y": 3, "size": 0.1}
    ]);
    d.load_json(&w);
    let items: Vec<_> = d.items().cloned().collect();
    assert_eq!(items[0].strand, Some(Strand { length: 4000.0, dip: 0.0 }));
    assert_eq!(items[0].zoom, 1.0);
    assert_eq!(items[1].strand, Some(Strand { length: 300.0, dip: 18.0 }));
    assert_eq!(items[2].zoom, ZOOM_MIN);
    // screens clamp loaded points
    d.set_screens(vec![Frame::new(0.0, 0.0, 200.0, 200.0)]);
    d.load_json(&json!([{"kind": "star", "x": 9000, "y": 100}]));
    assert_eq!(d.items().next().unwrap().center, Vec2::new(170.0, 100.0));
}

#[test]
fn spot_choice() {
    let mut rng = Lcg64::new(3);
    let mut d = Decorations::new(10);
    let work = Frame::new(0.0, 0.0, 1920.0, 1080.0);
    for kind in [Kind::Lights, Kind::Gift] {
        let p = d.spot(kind, work, &[], &mut rng);
        let (bw, bh) = render::box_size(kind, None);
        assert!(p.x >= bw / 2.0 + 16.0 && p.x <= 1920.0 - bw / 2.0 - 16.0);
        assert!(p.y <= 1080.0 - bh / 2.0 - 4.0);
        if kind == Kind::Lights {
            assert!(p.y >= 1080.0 - bh / 2.0 - 124.0);
        }
    }
    // obstacles cover everything but a strip: the strip wins
    let wall = [Frame::new(0.0, 0.0, 1920.0, 1080.0)];
    let p = d.spot(Kind::Gift, work, &wall, &mut rng);
    assert!(work.contains(p));
    // existing decorations repel too: the chosen box is clear of them
    let g = d.add(Kind::Gift, Vec2::new(960.0, 500.0)).unwrap();
    let q = d.spot(Kind::Gift, work, &[], &mut rng);
    let (bw, bh) = render::box_size(Kind::Gift, None);
    let qr = Frame::new(q.x - bw / 2.0, q.y - bh / 2.0, bw, bh);
    assert_eq!(overlap(&qr, &d.get(g).unwrap().frame()), 0.0);
    // NaN costs never keep a candidate: work-area centre (NaN here too)
    let nan = Frame::new(f64::NAN, f64::NAN, f64::NAN, f64::NAN);
    let p = d.spot(Kind::Gift, nan, &[nan], &mut rng);
    assert!(p.x.is_nan() && p.y.is_nan());
    // tiny work area (xmax clamp) and a NaN area fall back to the centre
    let small = Frame::new(0.0, 0.0, 10.0, 10.0);
    let p = d.spot(Kind::Gift, small, &[], &mut rng);
    assert!(p.x.is_finite());
}

#[test]
fn overlap_math() {
    let a = Frame::new(0.0, 0.0, 10.0, 10.0);
    assert_eq!(overlap(&a, &Frame::new(5.0, 5.0, 10.0, 10.0)), 25.0);
    assert_eq!(overlap(&a, &Frame::new(20.0, 0.0, 10.0, 10.0)), 0.0);
}

#[test]
fn art_internals() {
    use crate::art::{Motion, PartArt};
    assert_eq!(art::parts(Kind::Lights).len(), 2);
    assert_eq!(art::parts(Kind::Boat).len(), 0);
    assert!(art::parts(Kind::Cocoa)[0].motion == Motion::Steam);
    assert!(art::parts(Kind::Star)[0].behind);
    assert!(art::parts(Kind::Candle)[1].solid);
    assert!(matches!(art::parts(Kind::Lights)[1].art, PartArt::Lit(1)));
    // custom strands render and mask
    for strand in [Strand { length: 110.0, dip: 0.0 }, Strand { length: 700.0, dip: 200.0 }] {
        let m = render::mask(Kind::Lights, Some(strand));
        assert_eq!(m.w, strand.length as u32 + 24);
        assert!(m.data.iter().any(|a| *a > 60));
        let look =
            Look { zoom: 1.0, grow: 1.0, pop: 0.0, sway: 0.1, parts: vec![motion::PartState::REST; 2], chrome: 1.0 };
        let pm = render::paint(Kind::Lights, Some(strand), &look, 1.0);
        assert_eq!(pm.width(), strand.length as u32 + 24);
    }
    let m = render::mask(Kind::Candle, None);
    assert!(!m.solid(-100.0, -100.0));
    assert!(!m.solid(1000.0, 1000.0));
    assert!(!m.solid(1.0, 1.0));
    assert!(m.solid(f64::from(m.w) / 2.0, f64::from(m.h) / 2.0 + 10.0));
}

#[test]
fn bulbs_blink_and_candle_flicker_change_rev() {
    let mut r = Rig::new(10);
    let l = r.d.add(Kind::Lights, Vec2::new(500.0, 500.0)).unwrap();
    let c = r.d.add(Kind::Candle, Vec2::new(900.0, 500.0)).unwrap();
    let mut seen = std::collections::HashSet::new();
    for _ in 0..400 {
        r.step(PointerInput::default());
        seen.insert((r.sprite(l).rev, r.sprite(c).rev));
    }
    assert!(seen.len() > 20);
    // lights do not sway: the key only moves through blinking
    assert_eq!(r.d.get(l).unwrap().strand, None);
}

#[test]
fn hover_fades_and_grows() {
    let mut r = Rig::new(10);
    let a = r.d.add(Kind::Gift, Vec2::new(500.0, 500.0)).unwrap();
    let l = r.d.add(Kind::Lights, Vec2::new(500.0, 900.0)).unwrap();
    for _ in 0..40 {
        r.step(PointerInput::default());
    }
    let look = |r: &Rig, id: u64| r.d.items.iter().find(|i| i.p.id == id).unwrap().look.clone();
    assert_eq!(look(&r, a).chrome, 0.0);
    // one 1/60 s step into a 0.15 s fade
    r.step(ptr(500.0, 500.0, false, Some(a)));
    let lk = look(&r, a);
    assert!((f64::from(lk.chrome) - DT / 0.15).abs() < 1e-6);
    assert!((lk.grow - DT / 0.15).abs() < 1e-9);
    for _ in 0..10 {
        r.step(ptr(500.0, 500.0, false, Some(a)));
    }
    let lk = look(&r, a);
    assert_eq!((lk.chrome, lk.grow), (1.0, 1.0));
    // grown and chrome-on pixmap differs from the plain one, same size
    let hov = r.d.paint(a, 1.0).unwrap();
    // leaving fades back out
    for _ in 0..10 {
        r.step(ptr(5000.0, 5000.0, false, None));
    }
    let lk = look(&r, a);
    assert_eq!((lk.chrome, lk.grow), (0.0, 0.0));
    let plain = r.d.paint(a, 1.0).unwrap();
    assert_eq!((hov.width(), hov.height()), (plain.width(), plain.height()));
    assert_ne!(hov.data(), plain.data());
    // lights get chrome but never grow
    let lf = r.d.get(l).unwrap().frame();
    for _ in 0..10 {
        r.step(ptr(lf.mid_x(), lf.y + lf.h / 2.0, false, Some(l)));
    }
    let lk = look(&r, l);
    assert_eq!((lk.chrome, lk.grow), (1.0, 0.0));
    // a resize snaps the grow off at once while the chrome stays
    for _ in 0..10 {
        r.step(ptr(5000.0, 5000.0, false, None));
    }
    let corner = r.local(a, 92.0, 12.0);
    for _ in 0..10 {
        r.step(ptr(corner.x, corner.y, false, Some(a)));
    }
    assert_eq!(look(&r, a).grow, 1.0);
    r.step(ptr(corner.x, corner.y, true, Some(a)));
    let lk = look(&r, a);
    assert_eq!((lk.chrome, lk.grow), (1.0, 0.0));
    r.step(ptr(corner.x, corner.y, false, Some(a)));
}

#[test]
fn bad_clock_and_scale_are_ignored() {
    let mut r = Rig::new(10);
    let a = r.d.add(Kind::Candle, Vec2::new(500.0, 500.0)).unwrap();
    r.d.step(DT, 1.0, &PointerInput::default());
    let rev = r.sprite(a).rev;
    let look = r.d.items[0].look.clone();
    // NaN / infinite clock and negative dt: nothing advances
    r.d.step(f64::NAN, f64::NAN, &PointerInput::default());
    r.d.step(-1.0, f64::INFINITY, &PointerInput::default());
    assert_eq!(r.sprite(a).rev, rev);
    assert_eq!(r.d.items[0].look, look);
    assert!(r.d.items[0].look.sway.is_finite());
    // a removed item still fades with a sane dt afterwards
    r.d.remove(a);
    r.d.step(f64::NAN, 1.0, &PointerInput::default());
    assert_eq!(r.sprite(a).opacity, 1.0);
    r.d.step(0.1, 1.1, &PointerInput::default());
    assert!((r.sprite(a).opacity - 0.5).abs() < 1e-6);
    // paint with a bad scale falls back to 1x
    let b = r.d.add(Kind::Star, Vec2::new(200.0, 200.0)).unwrap();
    let f = r.d.get(b).unwrap().frame();
    for s in [f32::NAN, 0.0, -2.0, f32::INFINITY] {
        let pm = r.d.paint(b, s).unwrap();
        assert_eq!((pm.width(), pm.height()), (f.w as u32, f.h as u32));
    }
}

#[test]
fn part_frame_rates() {
    use crate::art::Motion;
    // glow runs at 12 fps: times inside one 1/12 s frame look identical
    let g = |t| motion::part_state(Motion::Glow, 2.4, t, 0.3);
    assert_eq!(g(0.0), g(0.08));
    assert_ne!(g(0.0), g(0.09));
    // steam at 15 fps, blink at 20 fps, flicker at 24 fps
    let s = |t| motion::part_state(Motion::Steam, 0.0, t, 0.3);
    assert_eq!(s(0.0), s(0.066));
    assert_ne!(s(0.0), s(0.067));
    let b = |t| motion::part_state(Motion::Blink, 0.0, t, 0.0);
    assert_eq!(b(0.75), b(0.799));
    assert_ne!(b(0.75), b(0.8));
    let f = |t| motion::part_state(Motion::Flicker, 0.0, t, 0.0);
    assert_eq!(f(0.0), f(0.041));
    assert_ne!(f(0.0), f(0.042));
}

fn right(x: f64, y: f64, down: bool, hot: Option<u64>) -> PointerInput {
    PointerInput { right_down: down, ..ptr(x, y, false, hot) }
}

#[test]
fn right_release_over_hovered_requests_menu() {
    let mut r = Rig::new(4);
    let a = r.d.add(Kind::RedBauble, Vec2::new(400.0, 400.0)).unwrap();
    let at = Vec2::new(400.0, 400.0);
    r.step(right(at.x, at.y, true, Some(a)));
    assert_eq!(r.d.take_menu_request(), None);
    r.step(right(at.x, at.y, false, Some(a)));
    assert_eq!(r.d.take_menu_request(), Some((a, at)));
    assert_eq!(r.d.take_menu_request(), None);
    assert!(!r.d.busy());
    assert_eq!(r.d.items().last().unwrap().id, a);
}

#[test]
fn right_release_without_hover_or_while_dragging_is_ignored() {
    let mut r = Rig::new(4);
    let a = r.d.add(Kind::RedBauble, Vec2::new(400.0, 400.0)).unwrap();
    r.step(right(900.0, 900.0, true, None));
    r.step(right(900.0, 900.0, false, None));
    assert_eq!(r.d.take_menu_request(), None);
    r.step(right(400.0, 400.0, true, Some(a)));
    r.step(PointerInput { right_down: true, ..ptr(400.0, 400.0, true, Some(a)) });
    r.step(PointerInput { right_down: false, ..ptr(400.0, 400.0, true, Some(a)) });
    assert!(r.d.busy());
    assert_eq!(r.d.take_menu_request(), None);
}
