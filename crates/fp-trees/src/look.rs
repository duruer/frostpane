//! Look model: toppers, trims, `TreeLook` and the style card operations
//! (trees 2 TreeLook, 3.9 card behaviour, 3.10).

use fp_core::rng::Rng;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// Reads a persisted enum value: its PascalCase name or, as the saved
/// format also allows, its integer position in `all` (trees 2).
fn enum_value<'de, D: Deserializer<'de>, T: Copy + std::fmt::Debug>(d: D, all: &[T]) -> Result<T, D::Error> {
    let v = Value::deserialize(d)?;
    let found = match &v {
        Value::String(name) => all.iter().find(|t| format!("{t:?}") == *name),
        Value::Number(n) => n.as_u64().and_then(|i| usize::try_from(i).ok()).and_then(|i| all.get(i)),
        _ => None,
    };
    found.copied().ok_or_else(|| D::Error::custom(format!("unknown value {v}")))
}

/// Any integer level, clamped into 0..=5 on load (trees 2: the original
/// never clamped the record, which could break the height name lookup).
fn level_value<'de, D: Deserializer<'de>>(d: D) -> Result<u8, D::Error> {
    let level = i64::deserialize(d)?;
    Ok(level.clamp(0, i64::from(MAX_LEVEL)) as u8)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum Topper {
    Star,
    Snowflake,
    Bow,
    Bell,
    Heart,
    Spire,
}

impl Topper {
    /// All toppers in enum order.
    pub const ALL: [Topper; 6] =
        [Topper::Star, Topper::Snowflake, Topper::Bow, Topper::Bell, Topper::Heart, Topper::Spire];
}

impl<'de> Deserialize<'de> for Topper {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        enum_value(d, &Topper::ALL)
    }
}

impl<'de> Deserialize<'de> for Trim {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        enum_value(d, &Trim::ALL)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub enum Trim {
    Baubles,
    Lights,
    Tinsel,
    Candy,
    Bells,
    Gingerbread,
    Bows,
    Stars,
}

impl Trim {
    /// All trims in enum order.
    pub const ALL: [Trim; 8] = [
        Trim::Baubles,
        Trim::Lights,
        Trim::Tinsel,
        Trim::Candy,
        Trim::Bells,
        Trim::Gingerbread,
        Trim::Bows,
        Trim::Stars,
    ];

    /// Trims that hang from slots (all except the two draped swags).
    pub fn hangs(self) -> bool {
        !matches!(self, Trim::Lights | Trim::Tinsel)
    }
}

/// Highest height class.
pub const MAX_LEVEL: u8 = 5;

/// Persisted look (PascalCase keys, string enums, trees 2). Absent keys take
/// the defaults; an explicit `null` topper means no topper.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct TreeLook {
    #[serde(deserialize_with = "level_value")]
    pub level: u8,
    pub topper: Option<Topper>,
    pub trims: Vec<Trim>,
}

impl Default for TreeLook {
    fn default() -> Self {
        TreeLook { level: 2, topper: Some(Topper::Star), trims: vec![Trim::Baubles, Trim::Lights] }
    }
}

impl TreeLook {
    /// Same look with the level forced into 0..=5 and trims in enum order
    /// without duplicates.
    pub fn clamped(mut self) -> TreeLook {
        self.level = self.level.min(MAX_LEVEL);
        self.trims.sort();
        self.trims.dedup();
        self
    }

    pub fn has(&self, trim: Trim) -> bool {
        self.trims.contains(&trim)
    }

    /// Trim toggled on or off, kept in enum order.
    pub fn toggled(&self, trim: Trim) -> TreeLook {
        let mut look = self.clone();
        if look.has(trim) {
            look.trims.retain(|t| *t != trim);
        } else {
            look.trims.push(trim);
            look.trims.sort();
        }
        look
    }

    /// Chip click: the selected topper clears, any other becomes the topper.
    pub fn topper_clicked(&self, topper: Topper) -> TreeLook {
        let mut look = self.clone();
        look.topper = if look.topper == Some(topper) { None } else { Some(topper) };
        look
    }

    /// Stepper: level moved by `delta`, kept inside 0..=5.
    pub fn stepped(&self, delta: i8) -> TreeLook {
        let mut look = self.clone();
        look.level = (i16::from(look.level) + i16::from(delta)).clamp(0, i16::from(MAX_LEVEL)) as u8;
        look
    }

    /// Randomise button: any topper (never none), each trim with chance 1/2.
    pub fn randomised(&self, rng: &mut impl Rng) -> TreeLook {
        let topper = Topper::ALL[rng.below(Topper::ALL.len() as u64) as usize];
        let trims = Trim::ALL.iter().copied().filter(|_| rng.coin()).collect();
        TreeLook { level: self.level, topper: Some(topper), trims }
    }

    /// Enabled trims that hang from slots, in enum order.
    pub fn hanging(&self) -> Vec<Trim> {
        self.trims.iter().copied().filter(|t| t.hangs()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_core::rng::Lcg64;

    #[test]
    fn defaults_and_clamp() {
        let d = TreeLook::default();
        assert_eq!((d.level, d.topper, d.trims.clone()), (2, Some(Topper::Star), vec![Trim::Baubles, Trim::Lights]));
        let c = TreeLook { level: 9, topper: None, trims: vec![Trim::Stars, Trim::Baubles, Trim::Stars] }.clamped();
        assert_eq!(c.level, 5);
        assert_eq!(c.trims, vec![Trim::Baubles, Trim::Stars]);
    }

    #[test]
    fn toggles_and_steps() {
        let d = TreeLook::default();
        let t = d.toggled(Trim::Candy).toggled(Trim::Tinsel);
        assert_eq!(t.trims, vec![Trim::Baubles, Trim::Lights, Trim::Tinsel, Trim::Candy]);
        assert!(t.has(Trim::Candy));
        assert_eq!(t.toggled(Trim::Baubles).trims, vec![Trim::Lights, Trim::Tinsel, Trim::Candy]);
        assert_eq!(t.hanging(), vec![Trim::Baubles, Trim::Candy]);
        assert!(Trim::Bows.hangs() && !Trim::Lights.hangs() && !Trim::Tinsel.hangs());
        assert_eq!(d.topper_clicked(Topper::Star).topper, None);
        assert_eq!(d.topper_clicked(Topper::Bell).topper, Some(Topper::Bell));
        assert_eq!(d.stepped(1).level, 3);
        assert_eq!(d.stepped(-9).level, 0);
        assert_eq!(d.stepped(9).level, 5);
    }

    #[test]
    fn randomise_keeps_level() {
        let mut rng = Lcg64::new(5);
        for _ in 0..200 {
            let r = TreeLook { level: 4, ..TreeLook::default() }.randomised(&mut rng);
            assert_eq!(r.level, 4);
            assert!(r.topper.is_some());
            assert!(r.trims.windows(2).all(|w| w[0] < w[1]));
        }
    }

    #[test]
    fn serde_defaults() {
        let l: TreeLook = serde_json::from_str("{}").unwrap();
        assert_eq!(l, TreeLook::default());
        let l: TreeLook = serde_json::from_str(r#"{"Level":4,"Topper":null,"Trims":["Candy"]}"#).unwrap();
        assert_eq!((l.level, l.topper, l.trims), (4, None, vec![Trim::Candy]));
        assert_eq!(serde_json::to_value(TreeLook::default()).unwrap()["Topper"], "Star");
        // integer enum values, out-of-range levels clamp on load
        let l: TreeLook = serde_json::from_str(r#"{"Level":-3,"Topper":4,"Trims":[2,"Stars"]}"#).unwrap();
        assert_eq!((l.level, l.topper, l.trims), (0, Some(Topper::Heart), vec![Trim::Tinsel, Trim::Stars]));
        let l: TreeLook = serde_json::from_str(r#"{"Level":300}"#).unwrap();
        assert_eq!(l.level, MAX_LEVEL);
        // unknown names, out-of-range indices and wrong types are errors
        for bad in [r#"{"Topper":"Moon"}"#, r#"{"Topper":6}"#, r#"{"Trims":[true]}"#, r#"{"Level":"x"}"#] {
            assert!(serde_json::from_str::<TreeLook>(bad).is_err(), "{bad}");
        }
        assert_eq!(serde_json::to_value(Trim::Candy).unwrap(), "Candy");
    }
}
