//! Version comparison for the update notice (core-app 2.11).

use std::cmp::Ordering;

/// A 2-4 part dotted version; missing parts compare as -1 (so `1.2.3.0 > 1.2.3`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version(Vec<i64>);

impl Version {
    /// Accepts an optional leading `v` (GitHub tag style) and ignores a
    /// `-suffix`/`+suffix`.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        let s = s.strip_prefix('v').unwrap_or(s);
        let core = s.split(['-', '+']).next()?;
        let parts: Option<Vec<i64>> = core.split('.').map(|p| p.parse::<u32>().ok().map(i64::from)).collect();
        let parts = parts?;
        if (2..=4).contains(&parts.len()) { Some(Version(parts)) } else { None }
    }

    fn part(&self, i: usize) -> i64 {
        self.0.get(i).copied().unwrap_or(-1)
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for Version {
    fn cmp(&self, o: &Self) -> Ordering {
        (0..4).map(|i| self.part(i).cmp(&o.part(i))).find(|c| c.is_ne()).unwrap_or(Ordering::Equal)
    }
}

/// True iff `latest` parses and is greater than `current`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (Version::parse(latest), Version::parse(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare() {
        assert!(is_newer("v0.2.0", "0.1.9"));
        assert!(is_newer("1.16.2.0", "1.16.2"));
        assert!(!is_newer("1.16.2", "1.16.2"));
        assert!(!is_newer("1.2", "1.2.0"));
        assert!(!is_newer("junk", "1.0"));
        assert!(!is_newer("1.0.0", "junk"));
        assert!(Version::parse("1").is_none());
        assert!(Version::parse("1.2.3.4.5").is_none());
        assert!(Version::parse("1.x").is_none());
        assert_eq!(Version::parse("2.0.1-beta"), Version::parse("2.0.1"));
        assert_eq!(Version::parse("1.0").unwrap().partial_cmp(&Version::parse("1.0").unwrap()), Some(Ordering::Equal));
    }
}
