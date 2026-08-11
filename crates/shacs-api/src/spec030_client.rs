use crate::TRUSTED_RUNTIME_PATH;
use shacs_config::{load_config, LoadOptions};
use shacs_projection::{
    Spec030RuntimeProjection, Spec030UnavailableReason, SPEC030_SCHEMA_VERSION,
};
use std::net::IpAddr;
use std::path::PathBuf;
use std::time::Duration;

const OWNER_QUERY_TIMEOUT: Duration = Duration::from_millis(750);
const MAX_PROJECTION_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustedRuntimeProjectionSource {
    ActiveRuntime,
    UnavailablePreview,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedRuntimeObservation {
    pub projection: Spec030RuntimeProjection,
    pub source: TrustedRuntimeProjectionSource,
}

pub fn observe_trusted_runtime(
    config_path: Option<PathBuf>,
    workspace_override: Option<PathBuf>,
) -> TrustedRuntimeObservation {
    match load_active_projection(config_path, workspace_override) {
        Some(projection) => TrustedRuntimeObservation {
            projection,
            source: TrustedRuntimeProjectionSource::ActiveRuntime,
        },
        None => TrustedRuntimeObservation {
            projection: Spec030RuntimeProjection::unavailable(
                Spec030UnavailableReason::OwnerUnavailable,
            ),
            source: TrustedRuntimeProjectionSource::UnavailablePreview,
        },
    }
}

fn load_active_projection(
    config_path: Option<PathBuf>,
    workspace_override: Option<PathBuf>,
) -> Option<Spec030RuntimeProjection> {
    let bundle = load_config(LoadOptions {
        config_path,
        workspace_override,
        resolve_env: true,
        write_back_migrations: false,
    })
    .ok()?;
    let authority = loopback_authority(&bundle.config.api.host, bundle.config.api.port)?;
    let url =
        format!("http://{authority}{TRUSTED_RUNTIME_PATH}?schema_version={SPEC030_SCHEMA_VERSION}");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(OWNER_QUERY_TIMEOUT))
        .build()
        .into();
    let mut response = agent.get(&url).call().ok()?;
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_PROJECTION_BYTES)
        .lossy_utf8(false)
        .read_to_string()
        .ok()?;
    Spec030RuntimeProjection::parse_json(&body).ok()
}

fn loopback_authority(host: &str, port: u16) -> Option<String> {
    let host = host.trim();
    if host.eq_ignore_ascii_case("localhost") {
        return Some(format!("localhost:{port}"));
    }
    let address = host.parse::<IpAddr>().ok()?;
    if !address.is_loopback() {
        return None;
    }
    Some(match address {
        IpAddr::V4(address) => format!("{address}:{port}"),
        IpAddr::V6(address) => format!("[{address}]:{port}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_authority_accepts_only_loopback_hosts() {
        assert_eq!(
            loopback_authority("127.0.0.1", 8080).as_deref(),
            Some("127.0.0.1:8080")
        );
        assert_eq!(
            loopback_authority("::1", 8080).as_deref(),
            Some("[::1]:8080")
        );
        assert_eq!(loopback_authority("0.0.0.0", 8080), None);
        assert_eq!(loopback_authority("example.com", 8080), None);
    }
}
