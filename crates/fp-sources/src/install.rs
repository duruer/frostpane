//! In-app updater decisions, without I/O: which release file fits this
//! build, which URLs the downloader may touch, and the signed checksum list.
//!
//! Trust chain: the release workflow writes `SHA256SUMS.txt` and signs it
//! with the project's Ed25519 key (minisign format, `SHA256SUMS.txt.minisig`).
//! The app embeds the public key ([`RELEASE_KEY`]), checks the signature of
//! the list, then checks the downloaded file's SHA-256 against the list's
//! line for that exact file name. Names carry the version, so an older signed
//! list cannot vouch for a newer file name.

use crate::update::{Asset, Release};

/// Release downloads of this repository (`browser_download_url` prefix).
pub const REPO_DOWNLOADS: &str = "https://github.com/duruer/frostpane/releases/download/";
/// Hosts GitHub redirects release downloads to.
pub const CDN_HOSTS: [&str; 2] = ["release-assets.githubusercontent.com", "objects.githubusercontent.com"];
/// The checksum list attached to every release.
pub const SUMS_NAME: &str = "SHA256SUMS.txt";
/// Its minisign signature.
pub const SIG_NAME: &str = "SHA256SUMS.txt.minisig";
/// Public half of the release signing key (minisign format).
pub const RELEASE_KEY: &str = include_str!("../../../packaging/minisign.pub");
/// Largest release file the updater downloads.
pub const MAX_ASSET_BYTES: u64 = 256 << 20;
/// Largest checksum list or signature.
pub const MAX_SMALL_BYTES: u64 = 64 << 10;

/// Blocking downloads; implemented by `fp-net`. Every URL, including each
/// redirect, must pass [`download_allowed`].
pub trait Download: Send + Sync {
    /// Whole body, at most `max` bytes.
    fn bytes(&self, url: &str, max: u64) -> Result<Vec<u8>, String>;
    /// Streams the body into `dest` (created or truncated), at most `max`
    /// bytes, calling `progress(bytes so far)`. Returns the SHA-256.
    fn file(
        &self,
        url: &str,
        dest: &std::path::Path,
        max: u64,
        progress: &mut dyn FnMut(u64),
    ) -> Result<[u8; 32], String>;
}

/// The release file kinds the updater can install.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Package {
    /// `frostpane-vX-windows-<arch>.zip` (portable exe).
    WindowsZip,
    /// `frostpane-vX-macos-universal.zip` (`Frostpane.app`).
    MacZip,
    /// `frostpane-vX-linux-<arch>.AppImage`.
    AppImage,
    /// `frostpane-vX-linux-<arch>.tar.gz` (plain binary).
    Tarball,
}

/// Release name of a CPU (`std::env::consts::ARCH`) on `os`.
fn arch_label(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        (_, "x86_64") => Some("x64"),
        ("windows", "x86") => Some("x86"),
        (_, "aarch64") => Some("arm64"),
        _ => None,
    }
}

/// File name tail after `frostpane-vX.Y.Z-` for this OS, CPU and package.
pub fn asset_suffix(os: &str, arch: &str, package: Package) -> Option<String> {
    match (os, package) {
        ("windows", Package::WindowsZip) => Some(format!("windows-{}.zip", arch_label(os, arch)?)),
        ("macos", Package::MacZip) => Some("macos-universal.zip".to_string()),
        ("linux", Package::AppImage) => Some(format!("linux-{}.AppImage", arch_label(os, arch)?)),
        ("linux", Package::Tarball) => Some(format!("linux-{}.tar.gz", arch_label(os, arch)?)),
        _ => None,
    }
}

/// The asset called `name`, only when its URL is this repository's download
/// URL for the release's tag.
pub fn find<'a>(release: &'a Release, name: &str) -> Option<&'a Asset> {
    let url = format!("{REPO_DOWNLOADS}{}/{name}", release.tag);
    release.assets.iter().find(|a| a.name == name && a.url == url)
}

/// The file to install for this build, if the release has one.
pub fn pick<'a>(release: &'a Release, os: &str, arch: &str, package: Package) -> Option<&'a Asset> {
    let name = format!("frostpane-{}-{}", release.tag, asset_suffix(os, arch, package)?);
    find(release, &name)
}

/// Byte cap for an asset: its reported size, else [`MAX_ASSET_BYTES`].
pub fn asset_cap(a: &Asset) -> u64 {
    if a.size == 0 { MAX_ASSET_BYTES } else { a.size.min(MAX_ASSET_BYTES) }
}

/// Download URL rule: https, default port, no user info, and either this
/// repository's release downloads on github.com or GitHub's download CDN.
pub fn download_allowed(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, path) = rest.split_at(end);
    if authority.contains('@') {
        return false;
    }
    let host = authority.strip_suffix(":443").unwrap_or(authority).to_ascii_lowercase();
    if host == "github.com" {
        return path.starts_with("/duruer/frostpane/releases/download/") && !path.contains("..");
    }
    CDN_HOSTS.contains(&host.as_str())
}

/// Lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 64 hex digits to 32 bytes.
pub fn parse_hex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 || !s.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

/// The SHA-256 the list gives for `name` (`sha256sum` format: hash, two
/// spaces or space-star, name). A name listed twice with different hashes
/// counts as absent.
pub fn sums_entry(sums: &str, name: &str) -> Option<[u8; 32]> {
    let mut found = None;
    for line in sums.lines() {
        let Some((hash, file)) = line.trim_end().split_once(' ') else { continue };
        let file = file.strip_prefix(['*', ' ']).unwrap_or(file);
        if file != name {
            continue;
        }
        let h = parse_hex32(&hash.to_ascii_lowercase())?;
        if found.is_some_and(|f| f != h) {
            return None;
        }
        found = Some(h);
    }
    found
}

/// Why the downloaded files were refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The public key or signature could not be read.
    Malformed(String),
    /// The signature does not match the list.
    BadSignature,
    /// The list has no line for the file.
    NotListed,
    /// The file's SHA-256 differs from the list.
    Mismatch,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::Malformed(e) => write!(f, "unreadable signature or key: {e}"),
            Refused::BadSignature => f.write_str("the checksum list's signature does not verify"),
            Refused::NotListed => f.write_str("the checksum list has no entry for the file"),
            Refused::Mismatch => f.write_str("the file's SHA-256 does not match the signed list"),
        }
    }
}

/// Checks the minisign signature `sig` of the list `sums` with `key` (a
/// minisign public key file or its base64 line).
pub fn verify_sums(sums: &[u8], sig: &str, key: &str) -> Result<(), Refused> {
    let key_line = key.lines().map(str::trim).rfind(|l| !l.is_empty() && !l.starts_with("untrusted comment:"));
    let pk = minisign_verify::PublicKey::from_base64(key_line.unwrap_or(""))
        .map_err(|e| Refused::Malformed(e.to_string()))?;
    let sig = minisign_verify::Signature::decode(sig.trim()).map_err(|e| Refused::Malformed(e.to_string()))?;
    pk.verify(sums, &sig, false).map_err(|_| Refused::BadSignature)
}

/// Full check of a downloaded file: signed list, then its line for `name`,
/// then `digest`.
pub fn verify_file(sums: &[u8], sig: &str, key: &str, name: &str, digest: &[u8; 32]) -> Result<(), Refused> {
    verify_sums(sums, sig, key)?;
    match sums_entry(&String::from_utf8_lossy(sums), name) {
        None => Err(Refused::NotListed),
        Some(h) if &h == digest => Ok(()),
        Some(_) => Err(Refused::Mismatch),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUMS: &[u8] = include_bytes!("../testdata/SHA256SUMS.txt");
    const SIG: &str = include_str!("../testdata/SHA256SUMS.txt.minisig");
    const TEST_KEY: &str = include_str!("../testdata/test.pub");
    /// SHA-256 of b"payload", the content every fixture line vouches for.
    const PAYLOAD: &str = "239f59ed55e737c77147cf55ad0c1b030b6d7ee748a7426952f9b852d5a935e5";

    fn release(names: &[&str]) -> Release {
        let assets = names
            .iter()
            .map(|n| Asset { name: n.to_string(), url: format!("{REPO_DOWNLOADS}v9.0.0/{n}"), size: 7 })
            .collect();
        Release { tag: "v9.0.0".into(), url: "https://github.com/duruer/frostpane/releases/tag/v9.0.0".into(), assets }
    }

    #[test]
    fn suffixes() {
        let s = |os, arch, p| asset_suffix(os, arch, p);
        assert_eq!(s("windows", "x86_64", Package::WindowsZip).as_deref(), Some("windows-x64.zip"));
        assert_eq!(s("windows", "x86", Package::WindowsZip).as_deref(), Some("windows-x86.zip"));
        assert_eq!(s("windows", "aarch64", Package::WindowsZip).as_deref(), Some("windows-arm64.zip"));
        assert_eq!(s("macos", "aarch64", Package::MacZip).as_deref(), Some("macos-universal.zip"));
        assert_eq!(s("macos", "x86_64", Package::MacZip).as_deref(), Some("macos-universal.zip"));
        assert_eq!(s("linux", "x86_64", Package::AppImage).as_deref(), Some("linux-x64.AppImage"));
        assert_eq!(s("linux", "aarch64", Package::Tarball).as_deref(), Some("linux-arm64.tar.gz"));
        assert_eq!(s("linux", "x86", Package::Tarball), None);
        assert_eq!(s("linux", "riscv64", Package::AppImage), None);
        assert_eq!(s("windows", "riscv64", Package::WindowsZip), None);
        assert_eq!(s("windows", "x86_64", Package::MacZip), None);
        assert_eq!(s("freebsd", "x86_64", Package::Tarball), None);
    }

    #[test]
    fn picks_the_file_for_this_build() {
        let all = [
            "frostpane-v9.0.0-windows-x64.zip",
            "frostpane-v9.0.0-windows-arm64.zip",
            "frostpane-v9.0.0-macos-universal.dmg",
            "frostpane-v9.0.0-macos-universal.zip",
            "frostpane-v9.0.0-linux-x64.AppImage",
            "frostpane-v9.0.0-linux-x64.tar.gz",
            "frostpane-v9.0.0-linux-x64.deb",
            "SHA256SUMS.txt",
        ];
        let r = release(&all);
        let name = |os, arch, p| pick(&r, os, arch, p).map(|a| a.name.as_str());
        assert_eq!(name("windows", "aarch64", Package::WindowsZip), Some(all[1]));
        assert_eq!(name("macos", "aarch64", Package::MacZip), Some(all[3]));
        assert_eq!(name("linux", "x86_64", Package::AppImage), Some(all[4]));
        assert_eq!(name("linux", "x86_64", Package::Tarball), Some(all[5]));
        // Missing from the release, or no such build.
        assert_eq!(name("windows", "x86", Package::WindowsZip), None);
        assert_eq!(name("linux", "aarch64", Package::Tarball), None);
        assert_eq!(name("linux", "riscv64", Package::Tarball), None);
        assert_eq!(find(&r, SUMS_NAME).map(|a| a.size), Some(7));
        assert!(find(&r, SIG_NAME).is_none());
    }

    #[test]
    fn foreign_urls_are_never_picked() {
        let mut r = release(&["frostpane-v9.0.0-windows-x64.zip"]);
        r.assets[0].url = "https://evil.example/frostpane-v9.0.0-windows-x64.zip".into();
        assert!(pick(&r, "windows", "x86_64", Package::WindowsZip).is_none());
        // Same repository, other tag.
        r.assets[0].url = format!("{REPO_DOWNLOADS}v1.0.0/frostpane-v9.0.0-windows-x64.zip");
        assert!(pick(&r, "windows", "x86_64", Package::WindowsZip).is_none());
    }

    #[test]
    fn caps() {
        let a = Asset { name: "a".into(), url: String::new(), size: 0 };
        assert_eq!(asset_cap(&a), MAX_ASSET_BYTES);
        assert_eq!(asset_cap(&Asset { size: 10, ..a.clone() }), 10);
        assert_eq!(asset_cap(&Asset { size: u64::MAX, ..a }), MAX_ASSET_BYTES);
    }

    #[test]
    fn url_rule() {
        let ok = [
            "https://github.com/duruer/frostpane/releases/download/v1.2.0/frostpane-v1.2.0-windows-x64.zip",
            "https://GitHub.com:443/duruer/frostpane/releases/download/v1/SHA256SUMS.txt",
            "https://release-assets.githubusercontent.com/github-production-release-asset/1?sig=x",
            "https://objects.githubusercontent.com/github-production-release-asset-2e65be/1",
            "https://objects.githubusercontent.com",
        ];
        for u in ok {
            assert!(download_allowed(u), "{u}");
        }
        let bad = [
            "http://github.com/duruer/frostpane/releases/download/v1/x",
            "https://github.com/other/frostpane/releases/download/v1/x",
            "https://github.com/duruer/frostpane/releases/tag/v1",
            "https://github.com/duruer/frostpane/releases/download/../../x",
            "https://github.com.evil.com/duruer/frostpane/releases/download/v1/x",
            "https://user@github.com/duruer/frostpane/releases/download/v1/x",
            "https://github.com:8443/duruer/frostpane/releases/download/v1/x",
            "https://evil.com/?h=release-assets.githubusercontent.com",
            "https://evil.com#objects.githubusercontent.com",
            "https://githubusercontent.com/x",
            "ftp://objects.githubusercontent.com/x",
            "",
        ];
        for u in bad {
            assert!(!download_allowed(u), "{u}");
        }
    }

    #[test]
    fn hex_round_trip() {
        let h = parse_hex32(PAYLOAD).unwrap();
        assert_eq!(hex(&h), PAYLOAD);
        assert_eq!(parse_hex32("00"), None);
        assert_eq!(parse_hex32(&"g".repeat(64)), None);
        // 64 bytes but not 64 ASCII digits.
        assert_eq!(parse_hex32(&"é".repeat(32)), None);
    }

    #[test]
    fn sums_lines() {
        let h = parse_hex32(PAYLOAD).unwrap();
        let text = std::str::from_utf8(SUMS).unwrap();
        assert_eq!(sums_entry(text, "frostpane-v9.0.0-linux-x64.AppImage"), Some(h));
        assert_eq!(sums_entry(text, "frostpane-v9.0.0-linux-x64"), None);
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        // Binary marker, upper case, CRLF, junk lines.
        let t = format!("junk\n\n{}  x.zip\r\n{} *y.zip\nzz  z.zip\n", a.to_uppercase(), b);
        assert_eq!(sums_entry(&t, "x.zip"), parse_hex32(&a));
        assert_eq!(sums_entry(&t, "y.zip"), parse_hex32(&b));
        assert_eq!(sums_entry(&t, "z.zip"), None);
        // Listed twice: same hash is fine, different hashes are not.
        assert_eq!(sums_entry(&format!("{a}  x\n{a}  x\n"), "x"), parse_hex32(&a));
        assert_eq!(sums_entry(&format!("{a}  x\n{b}  x\n"), "x"), None);
    }

    #[test]
    fn signature_checks() {
        assert_eq!(verify_sums(SUMS, SIG, TEST_KEY), Ok(()));
        // The base64 line alone works too.
        let line = TEST_KEY.lines().nth(1).unwrap();
        assert_eq!(verify_sums(SUMS, SIG, line), Ok(()));
        // One changed byte.
        let mut evil = SUMS.to_vec();
        evil[0] ^= 1;
        assert_eq!(verify_sums(&evil, SIG, TEST_KEY), Err(Refused::BadSignature));
        // Signed by another key: the embedded release key refuses it.
        assert_eq!(verify_sums(SUMS, SIG, RELEASE_KEY), Err(Refused::BadSignature));
        assert!(matches!(verify_sums(SUMS, "nonsense", TEST_KEY), Err(Refused::Malformed(_))));
        assert!(matches!(verify_sums(SUMS, SIG, ""), Err(Refused::Malformed(_))));
        assert!(matches!(verify_sums(SUMS, SIG, "untrusted comment: x\n!!"), Err(Refused::Malformed(_))));
    }

    #[test]
    fn release_key_is_well_formed() {
        assert!(RELEASE_KEY.starts_with("untrusted comment: minisign public key"));
        // A malformed key would give Malformed, not BadSignature.
        assert_eq!(verify_sums(b"x", SIG, RELEASE_KEY), Err(Refused::BadSignature));
    }

    #[test]
    fn file_checks() {
        let good = parse_hex32(PAYLOAD).unwrap();
        let name = "frostpane-v9.0.0-windows-x64.zip";
        assert_eq!(verify_file(SUMS, SIG, TEST_KEY, name, &good), Ok(()));
        assert_eq!(verify_file(SUMS, SIG, TEST_KEY, name, &[0; 32]), Err(Refused::Mismatch));
        assert_eq!(
            verify_file(SUMS, SIG, TEST_KEY, "frostpane-v9.0.1-windows-x64.zip", &good),
            Err(Refused::NotListed)
        );
        assert_eq!(verify_file(SUMS, SIG, RELEASE_KEY, name, &good), Err(Refused::BadSignature));
    }

    #[test]
    fn refusals_read_well() {
        for r in [Refused::Malformed("x".into()), Refused::BadSignature, Refused::NotListed, Refused::Mismatch] {
            assert!(r.to_string().len() > 10, "{r:?}");
        }
    }
}
