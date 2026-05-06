use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::protocol::BootstrapResponse;

pub const TOKEN_PREFIX: &str = "nbwt_";
pub const DEFAULT_MAX_ISSUED_TOKENS: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenError {
    TooManyOutstandingTokens,
    RandomFailure,
}

#[derive(Debug, Clone)]
pub struct WebTokenPools {
    issued_tokens: HashMap<String, Instant>,
    api_tokens: HashMap<String, Instant>,
    max_issued_tokens: usize,
}

impl Default for WebTokenPools {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_ISSUED_TOKENS)
    }
}

impl WebTokenPools {
    pub fn new(max_issued_tokens: usize) -> Self {
        Self {
            issued_tokens: HashMap::new(),
            api_tokens: HashMap::new(),
            max_issued_tokens,
        }
    }

    pub fn issue_webui_bootstrap(
        &mut self,
        ttl: Duration,
        ws_path: impl Into<String>,
        model_name: Option<String>,
    ) -> Result<BootstrapResponse, TokenError> {
        self.purge_expired();
        if self.issued_tokens.len() >= self.max_issued_tokens
            || self.api_tokens.len() >= self.max_issued_tokens
        {
            return Err(TokenError::TooManyOutstandingTokens);
        }
        let token = generate_token()?;
        let expiry = Instant::now() + ttl;
        self.issued_tokens.insert(token.clone(), expiry);
        self.api_tokens.insert(token.clone(), expiry);
        Ok(BootstrapResponse {
            token,
            ws_path: normalize_ws_path(ws_path.into()),
            expires_in: ttl.as_secs(),
            model_name,
        })
    }

    pub fn take_ws_token_if_valid(&mut self, token: Option<&str>) -> bool {
        let Some(token) = token.filter(|value| !value.is_empty()) else {
            return false;
        };
        self.purge_expired();
        let Some(expiry) = self.issued_tokens.remove(token) else {
            return false;
        };
        expiry > Instant::now()
    }

    pub fn check_api_token(&mut self, token: Option<&str>) -> bool {
        let Some(token) = token.filter(|value| !value.is_empty()) else {
            return false;
        };
        self.purge_expired();
        self.api_tokens
            .get(token)
            .is_some_and(|expiry| *expiry > Instant::now())
    }

    pub fn purge_expired(&mut self) {
        let now = Instant::now();
        self.issued_tokens.retain(|_, expiry| *expiry > now);
        self.api_tokens.retain(|_, expiry| *expiry > now);
    }
}

pub fn authorize_static_or_issued_token(
    supplied: Option<&str>,
    static_token: Option<&str>,
    pools: &mut WebTokenPools,
    websocket_requires_token: bool,
) -> bool {
    let supplied = supplied.filter(|value| !value.is_empty());
    let static_token = static_token.filter(|value| !value.is_empty());
    if let Some(static_token) = static_token {
        return supplied.is_some_and(|token| {
            constant_time_eq(token.as_bytes(), static_token.as_bytes())
                || pools.take_ws_token_if_valid(Some(token))
        });
    }
    if websocket_requires_token {
        return supplied.is_some_and(|token| pools.take_ws_token_if_valid(Some(token)));
    }
    if let Some(token) = supplied {
        pools.take_ws_token_if_valid(Some(token));
    }
    true
}

fn generate_token() -> Result<String, TokenError> {
    let mut bytes = [0_u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|_| TokenError::RandomFailure)?;
    Ok(format!("{TOKEN_PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes)))
}

fn normalize_ws_path(path: String) -> String {
    if path.is_empty() {
        "/".to_owned()
    } else if path.starts_with('/') {
        path
    } else {
        format!("/{path}")
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |acc, (left, right)| acc | (left ^ right))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_token_is_ws_single_use_and_api_multi_use() -> Result<(), TokenError> {
        let mut pools = WebTokenPools::new(10);
        let boot =
            pools.issue_webui_bootstrap(Duration::from_secs(30), "ws", Some("m".to_owned()))?;
        assert!(boot.token.starts_with(TOKEN_PREFIX));
        assert_eq!(boot.ws_path, "/ws");
        assert!(pools.check_api_token(Some(&boot.token)));
        assert!(pools.check_api_token(Some(&boot.token)));
        assert!(pools.take_ws_token_if_valid(Some(&boot.token)));
        assert!(!pools.take_ws_token_if_valid(Some(&boot.token)));
        Ok(())
    }

    #[test]
    fn static_and_required_token_authorization_matches_reference() -> Result<(), TokenError> {
        let mut pools = WebTokenPools::new(10);
        let boot = pools.issue_webui_bootstrap(Duration::from_secs(30), "/", None)?;
        assert!(authorize_static_or_issued_token(
            Some("static"),
            Some("static"),
            &mut pools,
            true
        ));
        assert!(authorize_static_or_issued_token(
            Some(&boot.token),
            Some("static"),
            &mut pools,
            true
        ));
        assert!(!authorize_static_or_issued_token(
            None, None, &mut pools, true
        ));
        assert!(authorize_static_or_issued_token(
            None, None, &mut pools, false
        ));
        Ok(())
    }
}
