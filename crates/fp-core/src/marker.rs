//! `running.json` unclean-exit marker (core-app 2.13). Local only; nothing is sent.

use crate::rng::Rng;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RunMarker {
    pub session: String,
    pub launched: i64,
    pub version: String,
}

impl RunMarker {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn from_json(s: &str) -> Option<RunMarker> {
        serde_json::from_str(s).ok()
    }
}

/// Uppercase hyphenated random GUID (8-4-4-4-12), version 4 layout.
pub fn session_id(rng: &mut impl Rng) -> String {
    let a = rng.next_u64();
    let b = rng.next_u64();
    let b = (b & 0x3FFF_FFFF_FFFF_FFFF) | 0x8000_0000_0000_0000;
    let a = (a & 0xFFFF_FFFF_FFFF_0FFF) | 0x0000_0000_0000_4000;
    format!(
        "{:08X}-{:04X}-{:04X}-{:04X}-{:012X}",
        a >> 32,
        (a >> 16) & 0xFFFF,
        a & 0xFFFF,
        b >> 48,
        b & 0xFFFF_FFFF_FFFF
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Lcg64;

    #[test]
    fn marker_json() {
        let m = RunMarker { session: session_id(&mut Lcg64::new(1)), launched: 1_700_000_000, version: "0.1.0".into() };
        assert_eq!(m.session.len(), 36);
        assert_eq!(m.session.as_bytes()[14], b'4');
        assert_eq!(m.session, m.session.to_uppercase());
        let j = m.to_json();
        assert!(j.contains("\"Session\"") && j.contains("\"Launched\":1700000000"));
        assert_eq!(RunMarker::from_json(&j), Some(m));
        assert_eq!(RunMarker::from_json("{}"), None);
    }
}
