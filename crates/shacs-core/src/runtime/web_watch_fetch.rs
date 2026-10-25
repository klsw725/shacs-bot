use std::io::Read;
use std::time::{Duration, Instant};

use encoding_rs::Encoding;
use sha2::{Digest, Sha256};
use shacs_security::{parse_http_url, resolve_redirect_url, NetworkGuard};
use ureq::http::header::{LOCATION, RETRY_AFTER};
use ureq::Agent;

use super::web_watch_extract::extract;
use super::web_watch_text::normalized_static_text_v1;
use super::{WebCheckFailure, WebCheckResult, WebResponseFacts, WebWatchPlan};

const MAX_URL_CHARS: usize = 2048;
const MAX_REDIRECTS: usize = 5;
const MAX_RETRY_AFTER_MS: u64 = 7 * 86_400_000;
const USER_AGENT: &str = "shacs-bot-web-watch/1";

/// Bounds for one watcher read: one redirect chain under one deadline, no retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebFetchLimits {
    pub timeout: Duration,
    pub max_bytes: usize,
}

impl WebFetchLimits {
    pub const PRODUCTION: Self = Self {
        timeout: Duration::from_secs(20),
        max_bytes: 1024 * 1024,
    };
}

pub(super) struct ReadPage {
    pub response: WebResponseFacts,
    pub text: String,
    pub retry_after: Option<String>,
}

/// Configuration-time URL check: http/https, no userinfo, public host for `guard`.
pub(super) fn validate_watch_url(url: &str, guard: &NetworkGuard) -> Result<(), String> {
    if url.chars().count() > MAX_URL_CHARS
        || url
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err("URL must be at most 2048 characters without spaces".to_owned());
    }
    if parse_http_url(url)?.authority.contains('@') {
        return Err("URLs with a user name or password are not supported".to_owned());
    }
    guard.validate_url_target(url).map(|_| ())
}

/// The URL without query and fragment, as shown in every output.
pub(super) fn display_url(url: &str) -> String {
    let base = &url[..url.find(['?', '#']).unwrap_or(url.len())];
    match base.split_once("://") {
        Some((scheme, rest)) if !rest.contains('/') => format!("{scheme}://{rest}/"),
        _ => base.to_owned(),
    }
}

pub(super) fn sha256_ref(value: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(value.as_bytes()))
}

pub(super) fn read_page(
    url: &str,
    guard: &NetworkGuard,
    limits: &WebFetchLimits,
) -> Result<ReadPage, WebCheckFailure> {
    let agent: Agent = Agent::config_builder()
        .user_agent(USER_AGENT)
        // A proxy from the environment would bypass the guard and could carry credentials.
        .proxy(None)
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .new_agent();
    let deadline = Instant::now()
        .checked_add(limits.timeout)
        .ok_or(WebCheckFailure::NetworkError)?;
    let mut current = url.to_owned();
    for _ in 0..=MAX_REDIRECTS {
        // Every request, the first included, gets the checks a redirect target gets.
        if parse_http_url(&current)
            .map_err(|_| WebCheckFailure::HttpError)?
            .authority
            .contains('@')
        {
            return Err(WebCheckFailure::GuardBlocked);
        }
        guard.validate_url_target(&current).map_err(guard_failure)?;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or(WebCheckFailure::NetworkError)?;
        let mut response = agent
            .get(&current)
            .config()
            .timeout_global(Some(remaining))
            .build()
            .header(
                "Accept",
                "text/html,application/xhtml+xml,text/plain;q=0.9,*/*;q=0.5",
            )
            .call()
            .map_err(|_| WebCheckFailure::NetworkError)?;
        if response.status().is_redirection() {
            current = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|location| resolve_redirect_url(&current, location).ok())
                .ok_or(WebCheckFailure::HttpError)?;
            continue;
        }
        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        // ureq converts only `text/*` bodies from their declared charset; every other type
        // is converted here with the same label rules so each response follows its charset.
        let declared = response
            .body()
            .charset()
            .and_then(|label| Encoding::for_label(label.as_bytes()))
            .filter(|_| {
                !response
                    .body()
                    .mime_type()
                    .is_some_and(|mime| mime.starts_with("text/"))
            });
        let mut bytes = Vec::new();
        response
            .body_mut()
            .with_config()
            .reader()
            .take(
                u64::try_from(limits.max_bytes)
                    .unwrap_or(u64::MAX)
                    .saturating_add(1),
            )
            .read_to_end(&mut bytes)
            .map_err(|_| WebCheckFailure::NetworkError)?;
        let truncated = bytes.len() > limits.max_bytes;
        bytes.truncate(limits.max_bytes);
        let text = normalized_static_text_v1(&match declared {
            Some(encoding) => encoding.decode_without_bom_handling(&bytes).0,
            None => String::from_utf8_lossy(&bytes),
        });
        return Ok(ReadPage {
            response: WebResponseFacts {
                final_url: display_url(&current),
                final_url_digest: sha256_ref(&current),
                http_status: response.status().as_u16(),
                text_digest: sha256_ref(&text),
                truncated,
            },
            text,
            retry_after,
        });
    }
    Err(WebCheckFailure::HttpError)
}

/// One watcher check of the stored plan: read, classify, extract.
pub(super) fn check_plan(
    plan: &WebWatchPlan,
    guard: &NetworkGuard,
    limits: &WebFetchLimits,
    now_ms: u64,
) -> WebCheckResult {
    let page = match read_page(&plan.url, guard, limits) {
        Ok(page) => page,
        Err(failure) => {
            return WebCheckResult::Failed {
                failure,
                response: None,
                retry_after_ms: None,
            }
        }
    };
    if let Some(failure) = status_failure(page.response.http_status) {
        let throttled = matches!(page.response.http_status, 429 | 503);
        return WebCheckResult::Failed {
            failure,
            retry_after_ms: page
                .retry_after
                .as_deref()
                .filter(|_| throttled)
                .and_then(|value| retry_after_ms(value, now_ms)),
            response: Some(page.response),
        };
    }
    match extract(&page.text, &plan.extraction) {
        Ok(value) => WebCheckResult::Observed {
            response: page.response,
            value,
        },
        Err(failure) => WebCheckResult::Failed {
            failure,
            response: Some(page.response),
            retry_after_ms: None,
        },
    }
}

pub(super) fn status_failure(status: u16) -> Option<WebCheckFailure> {
    match status {
        200..=299 => None,
        429 => Some(WebCheckFailure::RateLimited),
        _ => Some(WebCheckFailure::HttpError),
    }
}

fn retry_after_ms(value: &str, now_ms: u64) -> Option<u64> {
    let value = value.trim();
    let delay = match value.parse::<u64>() {
        Ok(seconds) => seconds.saturating_mul(1000),
        Err(_) => {
            let at = chrono::DateTime::parse_from_rfc2822(value)
                .ok()?
                .timestamp_millis();
            u64::try_from(at).ok()?.saturating_sub(now_ms)
        }
    };
    Some(delay.min(MAX_RETRY_AFTER_MS))
}

fn guard_failure(message: String) -> WebCheckFailure {
    if message.starts_with("Blocked:") {
        WebCheckFailure::GuardBlocked
    } else if message.starts_with("Cannot resolve") {
        WebCheckFailure::NetworkError
    } else {
        WebCheckFailure::HttpError
    }
}
