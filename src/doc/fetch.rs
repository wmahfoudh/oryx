//! Remote image fetching and the on-disk cache it fills. Cache files are
//! keyed by a stable hash of the URL so a reopened document renders
//! instantly and offline.

use std::path::PathBuf;
use std::time::Duration;

/// Total request budget; remote images must never stall the app for long.
const TIMEOUT: Duration = Duration::from_secs(8);
/// Upper bound on a fetched body; anything larger is not a document image.
const MAX_BYTES: u64 = 16 * 1024 * 1024;

/// Stable cache file name for a URL: FNV-1a 64 in hex. The hash must
/// never change between releases or every user's cache is orphaned.
pub fn key(url: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in url.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// The cache directory for fetched images, None when the platform gives
/// no home.
pub fn cache_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "oryx").map(|dirs| dirs.cache_dir().join("images"))
}

/// Downloads a URL, returning the raw bytes on success. TLS rides the
/// operating system's own stack so no crypto is cross-compiled.
pub fn fetch(url: &str) -> Option<Vec<u8>> {
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::NativeTls)
        .build();
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .tls_config(tls)
        .build();
    let agent: ureq::Agent = config.into();
    let mut response = agent.get(url).call().ok()?;
    response
        .body_mut()
        .with_config()
        .limit(MAX_BYTES)
        .read_to_vec()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_stable_and_distinct() {
        let badge = "https://img.shields.io/badge/build-passing-brightgreen";
        // FNV-1a 64 of the URL bytes; the constant pins the format so a
        // refactor cannot silently orphan existing caches.
        assert_eq!(key(badge), key(badge));
        assert_eq!(key("https://a.tld/x.png"), "d029dd937308275d");
        assert_ne!(key("https://a.tld/x.png"), key("https://a.tld/y.png"));
        assert!(key(badge).chars().all(|c| c.is_ascii_hexdigit()));
    }
}
