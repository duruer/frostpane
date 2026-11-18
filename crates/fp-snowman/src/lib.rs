//! Snowman looks, dress-up card model and snowman art.
//!
//! Spec: snowman F1 (formation rules, used by `fp-snowballs`), F2 (render a
//! dressed ball), F3 (look model), F4 (dress-up card: model and actions; the
//! egui window is in `fp-ui`), F5 (item art, may be redesigned), F6 look JSON
//! (`standingSnowmen` record schema lives in `fp-snowballs`), F9 (limit
//! snowman: walking puppet for the tree/decoration cap, uses [`puppet`]),
//! F10 (mascot), 7 (edge cases); snowman-fight 9 (`Puppet`).
//! Dropped: F7 send-a-snowman cargo (social), F8 friend-request snowman,
//! wardrobe gating (paid-dropped F2): every item is available.

mod art;
pub mod card;
mod mascot;
pub mod puppet;

use fp_paint::Sketch;
use fp_paint::tiny_skia::Pixmap;
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub use mascot::paint_mascot;

/// Implements `ALL`, `id` and `from_id` for an item enum (ids are the enum
/// names with a lowercase first letter, snowman F3).
macro_rules! catalog {
    ($t:ident { $($v:ident => $id:literal),+ $(,)? }) => {
        impl $t {
            /// Every option in UI order (also serialization order).
            pub const ALL: [$t; 8] = [$($t::$v),+];

            /// Item id used in `hat:<id>` strings and look JSON.
            pub fn id(self) -> &'static str {
                match self { $($t::$v => $id),+ }
            }

            /// Exact-match parse of an id; unknown ids give `None`.
            pub fn from_id(id: &str) -> Option<$t> {
                Self::ALL.iter().copied().find(|v| v.id() == id)
            }
        }
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Hat {
    TopHat,
    Bobble,
    Crown,
    Cowboy,
    Party,
    Earmuffs,
    Viking,
    Santa,
}

catalog!(Hat {
    TopHat => "topHat",
    Bobble => "bobble",
    Crown => "crown",
    Cowboy => "cowboy",
    Party => "party",
    Earmuffs => "earmuffs",
    Viking => "viking",
    Santa => "santa",
});

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Face {
    #[default]
    Classic,
    Happy,
    Wink,
    Cool,
    Surprised,
    Sleepy,
    Grumpy,
    Love,
}

catalog!(Face {
    Classic => "classic",
    Happy => "happy",
    Wink => "wink",
    Cool => "cool",
    Surprised => "surprised",
    Sleepy => "sleepy",
    Grumpy => "grumpy",
    Love => "love",
});

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Arms {
    #[default]
    Sticks,
    Waving,
    Mittens,
    Broom,
    CandyCane,
    Cocoa,
    Gift,
    Flowers,
}

catalog!(Arms {
    Sticks => "sticks",
    Waving => "waving",
    Mittens => "mittens",
    Broom => "broom",
    CandyCane => "candyCane",
    Cocoa => "cocoa",
    Gift => "gift",
    Flowers => "flowers",
});

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Outfit {
    Scarf,
    StripedScarf,
    BowTie,
    Tie,
    Sweater,
    Puffer,
    Tuxedo,
    GoldChain,
}

catalog!(Outfit {
    Scarf => "scarf",
    StripedScarf => "stripedScarf",
    BowTie => "bowTie",
    Tie => "tie",
    Sweater => "sweater",
    Puffer => "puffer",
    Tuxedo => "tuxedo",
    GoldChain => "goldChain",
});

impl Outfit {
    /// Neck outfits sit on the head ball; jackets on the body ball.
    pub fn is_neck(self) -> bool {
        matches!(self, Outfit::Scarf | Outfit::StripedScarf | Outfit::BowTie | Outfit::Tie)
    }
}

/// One wardrobe item as the `slot:id` string kept for compatibility (F3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Hat(Hat),
    Face(Face),
    Arms(Arms),
    Outfit(Outfit),
}

impl Item {
    /// `hat:topHat`, `face:wink`, `arms:candyCane`, `outfit:goldChain`.
    pub fn id(self) -> String {
        match self {
            Item::Hat(v) => format!("hat:{}", v.id()),
            Item::Face(v) => format!("face:{}", v.id()),
            Item::Arms(v) => format!("arms:{}", v.id()),
            Item::Outfit(v) => format!("outfit:{}", v.id()),
        }
    }

    /// Exact-match parse; anything unknown is `None` (treated as absent).
    pub fn parse(text: &str) -> Option<Item> {
        let (slot, id) = text.split_once(':')?;
        match slot {
            "hat" => Hat::from_id(id).map(Item::Hat),
            "face" => Face::from_id(id).map(Item::Face),
            "arms" => Arms::from_id(id).map(Item::Arms),
            "outfit" => Outfit::from_id(id).map(Item::Outfit),
            _ => None,
        }
    }
}

/// What a snowman wears. `Look::classic()` is stored as "no look".
///
/// JSON (F6): `{"hat":"<id>"?, "face":"<id>", "arms":"<id>", "outfit":"<id>"?,
/// "bare":true?}`; `hat` is omitted when none, `outfit` is omitted and
/// `"bare": true` written when there is no outfit. Reading is lenient:
/// non-objects give the classic look, unknown or missing ids fall back to the
/// slot default, and `bare` wins over `outfit`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Look {
    pub hat: Option<Hat>,
    pub face: Face,
    pub arms: Arms,
    pub outfit: Option<Outfit>,
}

impl Look {
    pub fn classic() -> Look {
        Look { hat: None, face: Face::Classic, arms: Arms::Sticks, outfit: Some(Outfit::Scarf) }
    }

    pub fn is_classic(&self) -> bool {
        *self == Look::classic()
    }

    /// The value kept on the ball: `None` for the classic look.
    pub fn stored(self) -> Option<Look> {
        if self.is_classic() { None } else { Some(self) }
    }

    fn from_value(value: &serde_json::Value) -> Look {
        let Some(map) = value.as_object() else { return Look::classic() };
        let text = |key: &str| map.get(key).and_then(|v| v.as_str());
        let bare = map.get("bare").and_then(|v| v.as_bool()) == Some(true);
        let outfit = if bare { None } else { Some(text("outfit").and_then(Outfit::from_id).unwrap_or(Outfit::Scarf)) };
        Look {
            hat: text("hat").and_then(Hat::from_id),
            face: text("face").and_then(Face::from_id).unwrap_or_default(),
            arms: text("arms").and_then(Arms::from_id).unwrap_or_default(),
            outfit,
        }
    }
}

impl Serialize for Look {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        if let Some(hat) = self.hat {
            map.serialize_entry("hat", hat.id())?;
        }
        map.serialize_entry("face", self.face.id())?;
        map.serialize_entry("arms", self.arms.id())?;
        match self.outfit {
            Some(outfit) => map.serialize_entry("outfit", outfit.id())?,
            None => map.serialize_entry("bare", &true)?,
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Look {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Look, D::Error> {
        Ok(Look::from_value(&serde_json::Value::deserialize(deserializer)?))
    }
}

/// Role of a ball inside a stack (F1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decor {
    Plain,
    Head,
    Body,
}

/// Extra padding around a ball window for its decorations (snowballs 3.7 `Pad`).
pub fn pad(decor: Decor, radius: f64) -> f64 {
    match decor {
        Decor::Plain => (0.10 * radius).max(3.0),
        Decor::Head => 0.95 * radius,
        Decor::Body => 0.75 * radius,
    }
}

/// Side of the square window of a ball: `2 * (radius + pad)`.
pub fn panel_size(decor: Decor, radius: f64) -> f64 {
    2.0 * (radius + pad(decor, radius))
}

/// Paints one ball (snow sphere + decorations) centred in `pixmap`.
///
/// `radius` is in points (kept within 1..=1024, NaN paints a 1 point ball),
/// `angle` rotates the specks. Decorated balls ignore the angle (it is forced
/// upright); plain balls ignore `look`.
pub fn paint_ball(pixmap: &mut Pixmap, scale: f32, radius: f64, angle: f64, seed: u64, decor: Decor, look: &Look) {
    let (w, h) = (pixmap.width() as f32 / scale, pixmap.height() as f32 / scale);
    let angle = if decor == Decor::Plain { angle as f32 } else { 0.0 };
    let radius = if radius.is_nan() { 1.0 } else { radius.clamp(1.0, MAX_PAINT_RADIUS) };
    let mut sketch = Sketch::new(pixmap, scale);
    sketch.at(w / 2.0, h / 2.0, 0.0, 1.0, |s| {
        art::draw_ball(s, radius as f32, angle, seed, decor, look);
    });
}

/// Largest radius [`paint_ball`] draws (the speck count grows with it).
const MAX_PAINT_RADIUS: f64 = 1024.0;

#[cfg(test)]
mod tests {
    use super::*;

    fn painted(decor: Decor, look: &Look) -> Pixmap {
        let mut pm = Pixmap::new(320, 320).unwrap();
        paint_ball(&mut pm, 2.0, 30.0, 0.5, 42, decor, look);
        pm
    }

    fn alpha_sum(pm: &Pixmap) -> u64 {
        pm.pixels().iter().map(|p| u64::from(p.alpha())).sum()
    }

    #[test]
    fn ids_round_trip() {
        for h in Hat::ALL {
            assert_eq!(Hat::from_id(h.id()), Some(h));
            assert_eq!(Item::parse(&Item::Hat(h).id()), Some(Item::Hat(h)));
        }
        for f in Face::ALL {
            assert_eq!(Item::parse(&Item::Face(f).id()), Some(Item::Face(f)));
        }
        for a in Arms::ALL {
            assert_eq!(Item::parse(&Item::Arms(a).id()), Some(Item::Arms(a)));
        }
        for o in Outfit::ALL {
            assert_eq!(Item::parse(&Item::Outfit(o).id()), Some(Item::Outfit(o)));
        }
        assert_eq!(Item::Arms(Arms::CandyCane).id(), "arms:candyCane");
        assert_eq!(Item::Outfit(Outfit::GoldChain).id(), "outfit:goldChain");
        assert_eq!(Item::parse("hat:nope"), None);
        assert_eq!(Item::parse("hat"), None);
        assert_eq!(Item::parse("shoe:topHat"), None);
        assert_eq!(Item::parse("hat:TopHat"), None);
    }

    #[test]
    fn neck_and_jacket_outfits() {
        assert_eq!(Outfit::ALL.iter().filter(|o| o.is_neck()).count(), 4);
        assert!(!Outfit::Sweater.is_neck());
    }

    #[test]
    fn classic_is_stored_as_nothing() {
        assert!(Look::classic().is_classic());
        assert_eq!(Look::classic().stored(), None);
        let l = Look { hat: Some(Hat::Crown), ..Look::classic() };
        assert_eq!(l.stored(), Some(l));
    }

    #[test]
    fn look_json_shape() {
        let l = Look { hat: Some(Hat::TopHat), face: Face::Wink, arms: Arms::CandyCane, outfit: Some(Outfit::Tie) };
        assert_eq!(
            serde_json::to_string(&l).unwrap(),
            r#"{"hat":"topHat","face":"wink","arms":"candyCane","outfit":"tie"}"#
        );
        let bare = Look { hat: None, outfit: None, ..l };
        assert_eq!(serde_json::to_string(&bare).unwrap(), r#"{"face":"wink","arms":"candyCane","bare":true}"#);
        assert_eq!(serde_json::from_str::<Look>(&serde_json::to_string(&l).unwrap()).unwrap(), l);
        assert_eq!(serde_json::from_str::<Look>(&serde_json::to_string(&bare).unwrap()).unwrap(), bare);
    }

    #[test]
    fn look_json_is_lenient() {
        let parse = |t: &str| serde_json::from_str::<Look>(t).unwrap();
        assert_eq!(parse("42"), Look::classic());
        assert_eq!(parse("null"), Look::classic());
        assert_eq!(parse("{}"), Look::classic());
        assert_eq!(parse(r#"{"hat":"fez","face":"??","arms":3,"outfit":"cape"}"#), Look::classic());
        let l = parse(r#"{"face":"love","outfit":"puffer","bare":true}"#);
        assert_eq!((l.face, l.outfit), (Face::Love, None));
        let l = parse(r#"{"outfit":"puffer","bare":false}"#);
        assert_eq!(l.outfit, Some(Outfit::Puffer));
    }

    #[test]
    fn pad_table() {
        assert_eq!(pad(Decor::Plain, 10.0), 3.0);
        assert_eq!(pad(Decor::Plain, 50.0), 5.0);
        assert_eq!(pad(Decor::Head, 20.0), 19.0);
        assert_eq!(pad(Decor::Body, 20.0), 15.0);
        assert_eq!(panel_size(Decor::Head, 20.0), 78.0);
    }

    #[test]
    fn plain_ball_is_a_sphere() {
        let pm = painted(Decor::Plain, &Look::classic());
        let centre = pm.pixel(160, 160).unwrap();
        assert!(centre.alpha() > 200 && centre.red() > 200);
        assert_eq!(pm.pixel(2, 2).unwrap().alpha(), 0);
        let again = painted(Decor::Plain, &Look::classic());
        assert_eq!(pm.data(), again.data());
        // The look is ignored on plain balls, the angle is not.
        let mut dressed = Look::classic();
        dressed.hat = Some(Hat::Party);
        assert_eq!(painted(Decor::Plain, &dressed).data(), pm.data());
        let mut turned = Pixmap::new(320, 320).unwrap();
        paint_ball(&mut turned, 2.0, 30.0, 2.0, 42, Decor::Plain, &Look::classic());
        assert_ne!(turned.data(), pm.data());
    }

    #[test]
    fn decorated_balls_ignore_angle() {
        let a = painted(Decor::Head, &Look::classic());
        let mut b = Pixmap::new(320, 320).unwrap();
        paint_ball(&mut b, 2.0, 30.0, 3.0, 42, Decor::Head, &Look::classic());
        assert_eq!(a.data(), b.data());
    }

    fn all_different(images: &[Vec<u8>]) {
        for i in 0..images.len() {
            for j in 0..i {
                assert_ne!(images[i], images[j], "{i} vs {j}");
            }
        }
    }

    #[test]
    fn every_item_paints_something_new() {
        let base_head = alpha_sum(&painted(Decor::Head, &Look { outfit: None, ..Look::classic() }));
        for hat in Hat::ALL {
            let l = Look { hat: Some(hat), outfit: None, ..Look::classic() };
            assert!(alpha_sum(&painted(Decor::Head, &l)) > base_head, "{hat:?}");
        }
        let faces: Vec<_> = Face::ALL
            .iter()
            .map(|&face| painted(Decor::Head, &Look { face, outfit: None, ..Look::classic() }).data().to_vec())
            .collect();
        all_different(&faces);
        let outfits: Vec<_> = Outfit::ALL
            .iter()
            .map(|&o| {
                let decor = if o.is_neck() { Decor::Head } else { Decor::Body };
                painted(decor, &Look { outfit: Some(o), ..Look::classic() }).data().to_vec()
            })
            .collect();
        all_different(&outfits);
        let arms: Vec<_> = Arms::ALL
            .iter()
            .map(|&a| painted(Decor::Body, &Look { arms: a, ..Look::classic() }).data().to_vec())
            .collect();
        all_different(&arms);
    }

    #[test]
    fn tiny_and_odd_radii_do_not_panic() {
        let mut pm = Pixmap::new(40, 40).unwrap();
        for decor in [Decor::Plain, Decor::Head, Decor::Body] {
            paint_ball(&mut pm, 1.0, 0.0, 0.0, 1, decor, &Look::classic());
            paint_ball(&mut pm, 1.0, 6.0, 0.0, 1, decor, &Look::classic());
        }
        // NaN paints the 1 point ball; infinity is capped (and must return).
        let one = |radius: f64| {
            let mut pm = Pixmap::new(40, 40).unwrap();
            paint_ball(&mut pm, 1.0, radius, 0.0, 1, Decor::Plain, &Look::classic());
            pm
        };
        assert_eq!(one(f64::NAN).data(), one(1.0).data());
        assert_eq!(one(0.2).data(), one(1.0).data());
        let capped = one(f64::INFINITY);
        assert_eq!(capped.data(), one(MAX_PAINT_RADIUS).data());
        assert!(capped.pixels().iter().all(|p| p.alpha() > 0));
    }

    /// Pixel of the 320 x 320 test image at ball units `(x, y)` (Y-up,
    /// radius 30 points, scale 2).
    fn at(pm: &Pixmap, x: f32, y: f32) -> fp_paint::tiny_skia::PremultipliedColorU8 {
        let px = ((80.0 + 30.0 * x) * 2.0) as u32;
        let py = (320.0 - (80.0 + 30.0 * y) * 2.0) as u32;
        pm.pixel(px, py).unwrap()
    }

    fn is_red(p: fp_paint::tiny_skia::PremultipliedColorU8) -> bool {
        p.alpha() == 255 && p.red() > 180 && p.green() < 90 && p.blue() < 90
    }

    fn is_white(p: fp_paint::tiny_skia::PremultipliedColorU8) -> bool {
        p.alpha() == 255 && p.red() > 230 && p.green() > 230 && p.blue() > 230
    }

    #[test]
    fn candy_cane_is_striped() {
        let pm = painted(Decor::Body, &Look { arms: Arms::CandyCane, ..Look::classic() });
        // Walk up the straight part of the cane (x = 1.6 R), below the twig.
        let column: Vec<_> = (0..60).map(|i| at(&pm, 1.6, -0.4 + i as f32 * 0.013)).collect();
        let red = column.iter().filter(|&&p| is_red(p)).count();
        let white = column.iter().filter(|&&p| is_white(p)).count();
        assert!(red > 15 && white > 15, "red {red} white {white}");
    }

    #[test]
    fn gold_chain_is_dashed_and_unshaded() {
        let chain = painted(Decor::Body, &Look { outfit: Some(Outfit::GoldChain), ..Look::classic() });
        let gold = |p: fp_paint::tiny_skia::PremultipliedColorU8| p.red() > 230 && p.green() > 150 && p.blue() < 120;
        // Along the necklace curve both gold dashes and the gaps between them.
        let mut hits = (0, 0);
        for i in 0..200 {
            let t = 0.1 + 0.3 * i as f32 / 200.0;
            let u = 1.0 - t;
            let x = u * u * u * -0.5 + 3.0 * u * u * t * -0.4 + 3.0 * u * t * t * 0.4 + t * t * t * 0.5;
            let y = u * u * u * 0.72 + t * t * t * 0.72;
            if gold(at(&chain, x, y)) {
                hits.0 += 1;
            } else {
                hits.1 += 1;
            }
        }
        assert!(hits.0 > 40 && hits.1 > 40, "{hits:?}");
        // Bare snow away from the chain is not darkened by a jacket shade.
        let bare = painted(Decor::Body, &Look { outfit: None, ..Look::classic() });
        for (x, y) in [(0.6, -0.3), (-0.6, -0.5), (0.75, 0.2)] {
            assert_eq!(at(&chain, x, y), at(&bare, x, y), "{x} {y}");
        }
        // The cloth jackets are shaded and clipped to the ball.
        let sweater = painted(Decor::Body, &Look { outfit: Some(Outfit::Sweater), ..Look::classic() });
        assert!(is_red(at(&sweater, 0.0, -0.3)) || at(&sweater, 0.0, -0.3).red() > 120);
        assert_eq!(at(&sweater, 1.2, -0.6).alpha(), 0);
    }

    #[test]
    fn party_hat_leans_back() {
        let pm = painted(Decor::Head, &Look { hat: Some(Hat::Party), outfit: None, ..Look::classic() });
        // The pom sits left of centre (away from the nose), not right.
        assert!(at(&pm, -0.12, 1.73).alpha() == 255);
        assert_eq!(at(&pm, 0.25, 1.73).alpha(), 0);
    }

    #[test]
    fn scarf_stripes_follow_their_minimum_width() {
        // At radius 10 the plain scarf stripe is 1 point (0.05 R would be 0.5).
        let draw = |outfit| {
            let mut pm = Pixmap::new(80, 80).unwrap();
            paint_ball(&mut pm, 4.0, 10.0, 0.0, 3, Decor::Head, &Look { outfit: Some(outfit), ..Look::classic() });
            pm
        };
        let pale = |pm: &Pixmap| {
            pm.pixels()
                .iter()
                .filter(|p| p.alpha() == 255 && p.red() > 240 && p.green() > 200 && p.blue() > 200 && p.green() < 235)
                .count()
        };
        assert!(pale(&draw(Outfit::Scarf)) > 40, "{}", pale(&draw(Outfit::Scarf)));
    }

    #[test]
    fn seed_moves_the_specks() {
        let mut a = Pixmap::new(160, 160).unwrap();
        let mut b = Pixmap::new(160, 160).unwrap();
        paint_ball(&mut a, 2.0, 30.0, 0.0, 1, Decor::Plain, &Look::classic());
        paint_ball(&mut b, 2.0, 30.0, 0.0, 2, Decor::Plain, &Look::classic());
        // Lcg64 seeds with `seed | 1`, so 0 and 1 match; 1 and 2 do not.
        let mut c = Pixmap::new(160, 160).unwrap();
        paint_ball(&mut c, 2.0, 30.0, 0.0, 0, Decor::Plain, &Look::classic());
        assert_ne!(a.data(), b.data());
        assert_eq!(a.data(), c.data());
    }
}
