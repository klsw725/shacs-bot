//! Network guard and clock for 042 web watches. Product builds only allow public
//! addresses (never `tools.ssrfWhitelist`) and use the system clock. The
//! `spec042-test-profile` build can allow `127.0.0.1/32` and read time from a file so
//! closure tests drive the real worker without short production cadences.

use shacs_core::runtime::NetworkGuard;

#[cfg(not(feature = "spec042-test-profile"))]
pub(crate) fn web_watch_guard() -> NetworkGuard {
    NetworkGuard::default()
}

#[cfg(feature = "spec042-test-profile")]
pub(crate) fn web_watch_guard() -> NetworkGuard {
    match std::env::var("SHACS_SPEC042_ALLOW_LOOPBACK").as_deref() {
        Ok("1") => NetworkGuard::with_ssrf_whitelist(["127.0.0.1/32"]),
        _ => NetworkGuard::default(),
    }
}

#[cfg(feature = "spec042-test-profile")]
pub(crate) fn test_clock_ms() -> Option<u64> {
    let path = std::env::var_os("SHACS_SPEC042_CLOCK_FILE")?;
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}
