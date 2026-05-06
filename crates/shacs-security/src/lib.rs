use regex::Regex;
use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedUrl {
    pub scheme: String,
    pub authority: String,
    pub hostname: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkSecurityConfig {
    pub ssrf_whitelist: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkGuard {
    allowed_networks: Vec<IpNetwork>,
}

impl NetworkGuard {
    pub fn new(config: NetworkSecurityConfig) -> Self {
        Self::with_ssrf_whitelist(config.ssrf_whitelist)
    }

    pub fn with_ssrf_whitelist(cidrs: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        let allowed_networks = cidrs
            .into_iter()
            .filter_map(|cidr| IpNetwork::parse(cidr.as_ref()))
            .collect();
        Self { allowed_networks }
    }

    pub fn validate_url_target(&self, url: &str) -> Result<ParsedUrl, String> {
        let parsed = parse_http_url(url)?;
        self.validate_host_target(&parsed.hostname).map(|()| parsed)
    }

    pub fn validate_resolved_url(&self, url: &str) -> Result<(), String> {
        let parsed = parse_http_url(url)?;
        self.validate_host_target(&parsed.hostname)
    }

    pub fn contains_internal_url(&self, text: &str) -> bool {
        let Ok(regex) = Regex::new(r#"(?i)https?://[^\s\"'`;|<>]+"#) else {
            return true;
        };
        let has_internal_url = regex
            .find_iter(text)
            .any(|match_| self.validate_url_target(match_.as_str()).is_err());
        has_internal_url
    }

    pub fn is_private(&self, addr: IpAddr) -> bool {
        if self
            .allowed_networks
            .iter()
            .any(|network| network.contains(addr))
        {
            return false;
        }
        ip_is_internal(addr)
    }

    fn validate_host_target(&self, hostname: &str) -> Result<(), String> {
        if hostname.eq_ignore_ascii_case("localhost") {
            return Err("Blocked: localhost resolves to private/internal address".to_owned());
        }
        if let Ok(ip) = hostname.parse::<IpAddr>() {
            if self.is_private(ip) {
                return Err(format!(
                    "Blocked: {hostname} resolves to private/internal address {ip}"
                ));
            }
            return Ok(());
        }
        let addresses = (hostname, 80)
            .to_socket_addrs()
            .map_err(|_| format!("Cannot resolve hostname: {hostname}"))?;
        for address in addresses {
            let ip = address.ip();
            if self.is_private(ip) {
                return Err(format!(
                    "Blocked: {hostname} resolves to private/internal address {ip}"
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IpNetwork {
    V4 { network: u32, prefix: u8 },
    V6 { network: u128, prefix: u8 },
}

impl IpNetwork {
    fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.is_empty() {
            return None;
        }
        let (addr, prefix) = value
            .split_once('/')
            .map_or((value, None), |(addr, prefix)| (addr, Some(prefix)));
        let ip = addr.parse::<IpAddr>().ok()?;
        match ip {
            IpAddr::V4(ip) => {
                let prefix = prefix.map_or(Some(32), |prefix| prefix.parse::<u8>().ok())?;
                if prefix > 32 {
                    return None;
                }
                let mask = prefix_mask_v4(prefix);
                Some(Self::V4 {
                    network: u32::from(ip) & mask,
                    prefix,
                })
            }
            IpAddr::V6(ip) => {
                let prefix = prefix.map_or(Some(128), |prefix| prefix.parse::<u8>().ok())?;
                if prefix > 128 {
                    return None;
                }
                let mask = prefix_mask_v6(prefix);
                Some(Self::V6 {
                    network: u128::from(ip) & mask,
                    prefix,
                })
            }
        }
    }

    fn contains(self, ip: IpAddr) -> bool {
        match (self, ip) {
            (Self::V4 { network, prefix }, IpAddr::V4(ip)) => {
                u32::from(ip) & prefix_mask_v4(prefix) == network
            }
            (Self::V6 { network, prefix }, IpAddr::V6(ip)) => {
                u128::from(ip) & prefix_mask_v6(prefix) == network
            }
            _ => false,
        }
    }
}

pub fn validate_url_target(url: &str) -> Result<ParsedUrl, String> {
    NetworkGuard::default().validate_url_target(url)
}

pub fn validate_resolved_url(url: &str) -> Result<(), String> {
    NetworkGuard::default().validate_resolved_url(url)
}

pub fn contains_internal_url(text: &str) -> bool {
    NetworkGuard::default().contains_internal_url(text)
}

pub fn parse_http_url(url: &str) -> Result<ParsedUrl, String> {
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| "Only http/https allowed, got 'none'".to_owned())?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return Err(format!("Only http/https allowed, got '{scheme}'"));
    }
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .to_owned();
    if authority.is_empty() {
        return Err("Missing domain".to_owned());
    }
    let hostname = hostname_from_authority(&authority)?;
    Ok(ParsedUrl {
        scheme: scheme.to_ascii_lowercase(),
        authority,
        hostname,
    })
}

pub fn resolve_redirect_url(base: &str, location: &str) -> Result<String, String> {
    if location.starts_with("http://") || location.starts_with("https://") {
        return Ok(location.to_owned());
    }
    let parsed = parse_http_url(base)?;
    if location.starts_with("//") {
        return Ok(format!("{}:{location}", parsed.scheme));
    }
    if location.starts_with('/') {
        return Ok(format!(
            "{}://{}{}",
            parsed.scheme, parsed.authority, location
        ));
    }
    let base_path = base
        .split_once("://")
        .and_then(|(_, rest)| rest.split_once('/').map(|(_, path)| path))
        .unwrap_or_default();
    let directory = base_path
        .rsplit_once('/')
        .map_or("", |(directory, _)| directory);
    let prefix = if directory.is_empty() {
        String::new()
    } else {
        format!("/{directory}")
    };
    Ok(format!(
        "{}://{}{}/{}",
        parsed.scheme, parsed.authority, prefix, location
    ))
}

fn hostname_from_authority(authority: &str) -> Result<String, String> {
    let without_auth = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = if let Some(rest) = without_auth.strip_prefix('[') {
        let end = rest
            .find(']')
            .ok_or_else(|| "Missing hostname".to_owned())?;
        &rest[..end]
    } else {
        without_auth
            .split_once(':')
            .map_or(without_auth, |(host, _)| host)
    };
    let host = host.trim_end_matches('.');
    if host.is_empty() {
        Err("Missing hostname".to_owned())
    } else {
        Ok(host.to_owned())
    }
}

fn ip_is_internal(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [first, second, ..] = ip.octets();
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_multicast()
                || first == 0
                || (first == 100 && (64..=127).contains(&second))
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            if segments[..5] == [0, 0, 0, 0, 0] && segments[5] == 0xffff {
                let mapped = Ipv4Addr::new(
                    (segments[6] >> 8) as u8,
                    segments[6] as u8,
                    (segments[7] >> 8) as u8,
                    segments[7] as u8,
                );
                return ip_is_internal(IpAddr::V4(mapped));
            }
            let first_segment = ip.segments()[0];
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || first_segment & 0xfe00 == 0xfc00
                || first_segment & 0xffc0 == 0xfe80
        }
    }
}

fn prefix_mask_v4(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - u32::from(prefix))
    }
}

fn prefix_mask_v6(prefix: u8) -> u128 {
    if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - u32::from(prefix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_http_scheme_and_hostname_shape() {
        let parsed = parse_http_url("https://user@example.com:443/path").expect("valid url");
        assert_eq!(parsed.scheme, "https");
        assert_eq!(parsed.authority, "user@example.com:443");
        assert_eq!(parsed.hostname, "example.com");
        let uppercase = parse_http_url("HTTP://example.com/path").expect("uppercase scheme");
        assert_eq!(uppercase.scheme, "http");
        assert!(parse_http_url("file:///etc/passwd")
            .expect_err("non-http scheme")
            .contains("Only http/https allowed"));
        assert_eq!(
            parse_http_url("https:///path"),
            Err("Missing domain".to_owned())
        );
    }

    #[test]
    fn blocks_private_loopback_link_local_cgnat_and_mapped_addresses() {
        let guard = NetworkGuard::default();
        for url in [
            "http://10.0.0.1/",
            "http://0.1.2.3/",
            "http://100.64.0.1/",
            "http://127.0.0.1/",
            "http://169.254.169.254/",
            "http://192.168.1.1/",
            "http://[::1]/",
            "http://[fc00::1]/",
            "http://[fe80::1]/",
            "http://[::ffff:127.0.0.1]/",
        ] {
            assert!(
                guard.validate_url_target(url).is_err(),
                "{url} should block"
            );
        }
        assert!(guard.validate_url_target("http://93.184.216.34/").is_ok());
    }

    #[test]
    fn ssrf_whitelist_allows_specific_cidrs_and_ignores_invalid_entries() {
        let guard = NetworkGuard::with_ssrf_whitelist(["bad-cidr", "100.64.0.0/10"]);
        assert!(guard.validate_url_target("http://100.64.0.42/").is_ok());
        assert!(guard.validate_url_target("http://127.0.0.1/").is_err());
        assert!(guard
            .validate_url_target("http://169.254.169.254/")
            .is_err());
        let v6_guard = NetworkGuard::with_ssrf_whitelist(["fc00::/7"]);
        assert!(v6_guard.validate_url_target("http://[fd00::1]/").is_ok());
    }

    #[test]
    fn validates_redirects_and_detects_internal_urls_in_text() {
        assert_eq!(
            resolve_redirect_url("https://example.com/a/b", "../c").expect("relative redirect"),
            "https://example.com/a/../c"
        );
        assert!(validate_resolved_url("http://127.0.0.1/private").is_err());
        assert!(contains_internal_url("curl http://169.254.169.254/latest"));
        assert!(contains_internal_url("curl HTTP://169.254.169.254/latest"));
        assert!(!contains_internal_url("curl http://93.184.216.34/"));
    }
}
