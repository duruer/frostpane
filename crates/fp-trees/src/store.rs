//! Persisted records (trees 2 SavedTree) and the train cargo value.

use crate::look::TreeLook;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
pub fn b64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(B64[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Lenient decode: padding optional, any invalid character gives an empty
/// result.
pub fn b64_decode(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.trim_end_matches('=').bytes() {
        let Some(v) = B64.iter().position(|b| *b == c) else {
            return Vec::new();
        };
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

/// Place of a saved tree: integer on disk, 0 = ground, 1 = dock, else window.
pub const PLACE_GROUND: u8 = 0;
pub const PLACE_DOCK: u8 = 1;
pub const PLACE_WINDOW: u8 = 2;

/// One element of `standingTrees` (PascalCase, missing fields take defaults).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct SavedTree {
    pub place: u8,
    pub id: i64,
    pub owner: String,
    pub middle: f64,
    pub top: f64,
    pub screen_x: f64,
    pub screen_y: f64,
    pub across: f64,
    pub look: Option<TreeLook>,
    pub seed: u64,
    pub depths: Option<String>,
    pub cap: u8,
}

/// Byte of a 0..1 value (floor, clamped).
pub fn to_byte(v: f64) -> u8 {
    (v * 255.0).floor().clamp(0.0, 255.0) as u8
}

/// Parses the `standingTrees` value; any error drops the whole list.
pub fn parse_saved(v: &Value) -> Vec<SavedTree> {
    serde_json::from_value(v.clone()).unwrap_or_default()
}

/// Train cargo: a tree's look, seed and snow, opaque to everyone else.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Cargo {
    pub look: TreeLook,
    pub seed: u64,
    pub depths: String,
    pub cap: u8,
}

impl Default for Cargo {
    fn default() -> Self {
        Cargo { look: TreeLook::default(), seed: 1, depths: String::new(), cap: 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trip() {
        for data in [&b""[..], b"f", b"fo", b"foo", b"foob", &[0, 255, 128, 7, 9]] {
            assert_eq!(b64_decode(&b64_encode(data)), data);
        }
        assert_eq!(b64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(b64_encode(b"fo"), "Zm8=");
        assert_eq!(b64_decode("Zm8"), b"fo");
        assert!(b64_decode("a*b").is_empty());
    }

    #[test]
    fn bytes_and_parse() {
        assert_eq!((to_byte(-1.0), to_byte(0.5), to_byte(1.0), to_byte(9.0)), (0, 127, 255, 255));
        assert!(parse_saved(&serde_json::json!("nope")).is_empty());
        assert!(parse_saved(&serde_json::json!([{"Place": "x"}])).is_empty());
        let v = parse_saved(&serde_json::json!([{"Place": 1, "Look": null, "Depths": null}, {}]));
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].place, 1);
        assert!(v[0].look.is_none() && v[0].depths.is_none());
        assert_eq!(Cargo::default().seed, 1);
    }
}
