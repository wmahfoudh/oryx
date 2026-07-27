//! Remote image fetching and the on-disk cache it fills. Cache files are
//! keyed by a stable hash of the URL so a reopened document renders
//! instantly and offline.

use std::path::PathBuf;
use std::time::Duration;

/// Total request budget; remote images must never stall the app for long.
const TIMEOUT: Duration = Duration::from_secs(8);
/// Upper bound on a fetched body; anything larger is not a document image.
const MAX_BYTES: u64 = 16 * 1024 * 1024;
/// Pauses between fetch attempts, so a transient network failure at open
/// heals instead of blanking an image for the whole session.
const BACKOFF: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(3)];

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

/// Downloads a URL with retries, returning the raw bytes on success.
/// Runs on a background thread, so the sleeps stall nothing visible.
pub fn fetch(url: &str) -> Option<Vec<u8>> {
    retry(&BACKOFF, || fetch_once(url))
}

/// Calls `attempt` until it succeeds: one initial try, then one more
/// after each pause in `backoff`.
fn retry<T>(backoff: &[Duration], mut attempt: impl FnMut() -> Option<T>) -> Option<T> {
    if let Some(value) = attempt() {
        return Some(value);
    }
    for pause in backoff {
        std::thread::sleep(*pause);
        if let Some(value) = attempt() {
            return Some(value);
        }
    }
    None
}

/// The two shapes the registry proxy value takes: one server for every
/// protocol, or per-protocol entries separated by semicolons. The https
/// entry wins for these fetches, then http; other protocols are noise.
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_system_proxy(enable: u32, server: &str) -> Option<String> {
    if enable == 0 {
        return None;
    }
    let server = server.trim();
    if server.is_empty() {
        return None;
    }
    let with_scheme = |host: &str| {
        if host.contains("://") {
            host.to_string()
        } else {
            format!("http://{host}")
        }
    };
    if !server.contains('=') {
        return Some(with_scheme(server));
    }
    let (mut http, mut https) = (None, None);
    for entry in server.split(';') {
        let mut parts = entry.splitn(2, '=');
        match (parts.next().map(str::trim), parts.next().map(str::trim)) {
            (Some("https"), Some(value)) if !value.is_empty() => https = Some(value),
            (Some("http"), Some(value)) if !value.is_empty() => http = Some(value),
            _ => {}
        }
    }
    https.or(http).map(with_scheme)
}

/// The proxy Windows configures system-wide. A GUI app launched from
/// the shell sees no proxy environment variables, so the registry is
/// where a corporate network's setting actually lives. PAC scripts are
/// out of scope.
#[cfg(windows)]
fn system_proxy() -> Option<ureq::Proxy> {
    let key = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
        .ok()?;
    let enable: u32 = key.get_value("ProxyEnable").ok()?;
    let server: String = key.get_value("ProxyServer").unwrap_or_default();
    let url = parse_system_proxy(enable, &server)?;
    ureq::Proxy::new(&url).ok()
}

#[cfg(not(windows))]
fn system_proxy() -> Option<ureq::Proxy> {
    None
}

/// One download attempt. TLS rides the operating system's own stack so
/// no crypto is cross-compiled. The environment's proxy applies through
/// ureq's own detection; the system proxy fills in where the
/// environment has none. A failure lands one line on stderr, since a
/// silent placeholder is undiagnosable from a screenshot.
fn fetch_once(url: &str) -> Option<Vec<u8>> {
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::NativeTls)
        .build();
    let mut builder = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .tls_config(tls);
    if ureq::Proxy::try_from_env().is_none() {
        if let Some(proxy) = system_proxy() {
            builder = builder.proxy(Some(proxy));
        }
    }
    let agent: ureq::Agent = builder.build().into();
    let mut response = match agent.get(url).call() {
        Ok(response) => response,
        Err(err) => {
            eprintln!("oryx: fetch {url}: {err}");
            return None;
        }
    };
    match response
        .body_mut()
        .with_config()
        .limit(MAX_BYTES)
        .read_to_vec()
    {
        Ok(bytes) => Some(bytes),
        Err(err) => {
            eprintln!("oryx: fetch {url}: {err}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_returns_first_success_without_retrying() {
        let mut calls = 0;
        let result = retry(&[Duration::ZERO; 2], || {
            calls += 1;
            Some(7)
        });
        assert_eq!(result, Some(7));
        assert_eq!(calls, 1);
    }

    #[test]
    fn retry_keeps_trying_until_success() {
        let mut calls = 0;
        let result = retry(&[Duration::ZERO; 2], || {
            calls += 1;
            (calls == 3).then_some("ok")
        });
        assert_eq!(result, Some("ok"));
        assert_eq!(calls, 3);
    }

    #[test]
    fn retry_gives_up_when_backoff_is_exhausted() {
        let mut calls = 0;
        let result: Option<()> = retry(&[Duration::ZERO; 2], || {
            calls += 1;
            None
        });
        assert_eq!(result, None);
        assert_eq!(calls, 3);
    }

    #[test]
    fn the_system_proxy_parser_covers_the_registry_shapes() {
        let some = |s: &str| Some(s.to_string());
        assert_eq!(
            parse_system_proxy(1, "proxy.corp:8080"),
            some("http://proxy.corp:8080")
        );
        assert_eq!(
            parse_system_proxy(1, "http=proxy.corp:8080;https=secure.corp:8443"),
            some("http://secure.corp:8443")
        );
        assert_eq!(
            parse_system_proxy(1, "http=proxy.corp:8080"),
            some("http://proxy.corp:8080")
        );
        assert_eq!(parse_system_proxy(1, "ftp=ftp.corp:2121"), None);
        assert_eq!(parse_system_proxy(0, "proxy.corp:8080"), None);
        assert_eq!(parse_system_proxy(1, "  "), None);
        assert_eq!(
            parse_system_proxy(1, "http://proxy.corp:8080"),
            some("http://proxy.corp:8080")
        );
    }

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
