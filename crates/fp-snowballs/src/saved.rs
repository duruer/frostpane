//! The `standingSnowmen` record (snowballs 3.10, snowman F6): PascalCase
//! keys, `Place` as an int. Also the train cargo format (the same record
//! without window data).

use crate::{MAX_RADIUS, MIN_RADIUS};
use fp_core::geom::Vec2;
use fp_snowman::Look;
use serde_json::{Value, json};

/// Tallest stack accepted from a record (snowballs 3.3 depth cap).
pub(crate) const MAX_STACK: usize = 50;

/// Where a saved stack stood.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    Ground,
    Dock,
    Window,
}

impl Place {
    fn raw(self) -> u8 {
        match self {
            Place::Ground => 0,
            Place::Dock => 1,
            Place::Window => 2,
        }
    }

    fn from_raw(v: i64) -> Place {
        match v {
            1 => Place::Dock,
            2 => Place::Window,
            _ => Place::Ground,
        }
    }
}

/// One saved stack.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Saved {
    pub place: Place,
    pub id: i64,
    pub owner: String,
    pub middle: f64,
    pub top: f64,
    pub screen: Vec2,
    pub across: f64,
    pub radii: Vec<f64>,
    pub seeds: Option<Vec<u64>>,
    pub angles: Option<Vec<f64>>,
    pub look: Option<Look>,
}

impl Saved {
    /// A record without window data (train cargo, ground stacks).
    pub fn bare(radii: Vec<f64>, seeds: Vec<u64>, angles: Vec<f64>, look: Option<Look>) -> Saved {
        Saved {
            place: Place::Ground,
            id: 0,
            owner: String::new(),
            middle: 0.0,
            top: 0.0,
            screen: Vec2::ZERO,
            across: 0.0,
            radii,
            seeds: Some(seeds),
            angles: Some(angles),
            look,
        }
    }

    pub fn to_value(&self) -> Value {
        let look = self.look.and_then(|l| serde_json::to_value(l).ok()).unwrap_or(Value::Null);
        json!({
            "Place": self.place.raw(),
            "Id": self.id,
            "Owner": self.owner,
            "Middle": self.middle,
            "Top": self.top,
            "ScreenX": self.screen.x,
            "ScreenY": self.screen.y,
            "Across": self.across,
            "Radii": self.radii,
            "Seeds": self.seeds,
            "Angles": self.angles,
            "Look": look,
        })
    }

    /// Lenient read: `None` when `Radii` is missing, empty or not numbers.
    pub fn from_value(value: &Value) -> Option<Saved> {
        let map = value.as_object()?;
        let number = |key: &str| map.get(key).and_then(Value::as_f64).unwrap_or(0.0);
        let radii = map
            .get("Radii")?
            .as_array()?
            .iter()
            .take(MAX_STACK)
            .map(|v| v.as_f64().map(|r| r.clamp(MIN_RADIUS, MAX_RADIUS)))
            .collect::<Option<Vec<f64>>>()
            .filter(|r| !r.is_empty())?;
        let seeds = map.get("Seeds").and_then(Value::as_array).and_then(|a| a.iter().map(Value::as_u64).collect());
        let angles = map.get("Angles").and_then(Value::as_array).and_then(|a| a.iter().map(Value::as_f64).collect());
        let look =
            map.get("Look").filter(|v| v.is_object()).and_then(|v| serde_json::from_value::<Look>(v.clone()).ok());
        Some(Saved {
            place: Place::from_raw(map.get("Place").and_then(Value::as_i64).unwrap_or(0)),
            id: map.get("Id").and_then(Value::as_i64).unwrap_or(0),
            owner: map.get("Owner").and_then(Value::as_str).unwrap_or("").to_string(),
            middle: number("Middle"),
            top: number("Top"),
            screen: Vec2::new(number("ScreenX"), number("ScreenY")),
            across: number("Across"),
            radii,
            seeds,
            angles,
            look,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_snowman::{Face, Hat};

    fn sample() -> Saved {
        Saved {
            place: Place::Window,
            id: 77,
            owner: "app.exe".into(),
            middle: 10.5,
            top: 300.0,
            screen: Vec2::new(-1920.0, 0.0),
            across: -4.0,
            radii: vec![30.0, 20.0, 12.0],
            seeds: Some(vec![1, 2, 3]),
            angles: Some(vec![0.1, 0.2, 0.3]),
            look: Some(Look { hat: Some(Hat::Viking), face: Face::Wink, ..Look::classic() }),
        }
    }

    #[test]
    fn round_trip() {
        let s = sample();
        let v = s.to_value();
        assert_eq!(v["Place"], 2);
        assert_eq!(v["Owner"], "app.exe");
        assert_eq!(v["Look"]["hat"], "viking");
        assert_eq!(Saved::from_value(&v), Some(s));
    }

    #[test]
    fn places() {
        for (raw, place) in [(0, Place::Ground), (1, Place::Dock), (2, Place::Window), (9, Place::Ground)] {
            assert_eq!(Place::from_raw(raw), place);
        }
        assert_eq!(Place::Ground.raw(), 0);
        assert_eq!(Place::Dock.raw(), 1);
    }

    #[test]
    fn bare_has_no_window_data() {
        let s = Saved::bare(vec![9.0], vec![5], vec![0.0], None);
        let v = s.to_value();
        assert_eq!(v["Place"], 0);
        assert_eq!(v["Id"], 0);
        assert!(v["Look"].is_null());
        assert_eq!(Saved::from_value(&v), Some(s));
    }

    #[test]
    fn lenient_read() {
        let v = json!({"Radii": [1.0, 30.0, 900.0], "Place": 5, "Seeds": [1, "x"], "Angles": 3, "Look": 4});
        let s = Saved::from_value(&v).unwrap();
        assert_eq!(s.radii, vec![MIN_RADIUS, 30.0, MAX_RADIUS]);
        assert_eq!(s.place, Place::Ground);
        assert_eq!((s.seeds, s.angles, s.look), (None, None, None));
        assert_eq!((s.id, s.owner.as_str(), s.middle, s.across), (0, "", 0.0, 0.0));
        let many: Vec<f64> = vec![10.0; 80];
        assert_eq!(Saved::from_value(&json!({"Radii": many})).unwrap().radii.len(), MAX_STACK);
    }

    #[test]
    fn rejects_bad_records() {
        for v in [json!(3), json!({}), json!({"Radii": 5}), json!({"Radii": []}), json!({"Radii": [1.0, "a"]})] {
            assert_eq!(Saved::from_value(&v), None);
        }
    }

    #[test]
    fn look_object_variants() {
        let v = json!({"Radii": [10.0], "Look": {"face": "love", "bare": true}});
        let s = Saved::from_value(&v).unwrap();
        assert_eq!(s.look.unwrap().face, Face::Love);
        assert_eq!(s.look.unwrap().outfit, None);
        // NaN radius serialises as null and fails to read back
        let mut nan = sample();
        nan.radii = vec![f64::NAN];
        assert_eq!(Saved::from_value(&nan.to_value()), None);
    }
}
