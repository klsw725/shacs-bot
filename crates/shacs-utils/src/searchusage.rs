use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchUsageInfo {
    pub provider: String,
    pub used: Option<u64>,
    pub limit: Option<u64>,
    pub remaining: Option<i64>,
    pub reset_date: Option<String>,
    pub search_used: Option<u64>,
    pub extract_used: Option<u64>,
    pub crawl_used: Option<u64>,
    pub supported: bool,
    pub error: Option<String>,
}

impl SearchUsageInfo {
    pub fn unsupported(provider: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            used: None,
            limit: None,
            remaining: None,
            reset_date: None,
            search_used: None,
            extract_used: None,
            crawl_used: None,
            supported: false,
            error: None,
        }
    }

    pub fn format(&self) -> String {
        let mut lines = vec![format!("🔍 Web Search: {}", self.provider)];
        if !self.supported {
            lines.push("   Usage tracking: not available for this provider".to_owned());
            return lines.join("\n");
        }
        if let Some(error) = &self.error {
            lines.push(format!("   Usage: unavailable ({error})"));
            return lines.join("\n");
        }
        if let (Some(used), Some(limit)) = (self.used, self.limit) {
            lines.push(format!("   Usage: {used} / {limit} requests"));
        } else if let Some(used) = self.used {
            lines.push(format!("   Usage: {used} requests"));
        }
        let mut breakdown = Vec::new();
        if let Some(value) = self.search_used {
            breakdown.push(format!("Search: {value}"));
        }
        if let Some(value) = self.extract_used {
            breakdown.push(format!("Extract: {value}"));
        }
        if let Some(value) = self.crawl_used {
            breakdown.push(format!("Crawl: {value}"));
        }
        if !breakdown.is_empty() {
            lines.push(format!("   Breakdown: {}", breakdown.join(" | ")));
        }
        if let Some(remaining) = self.remaining {
            lines.push(format!("   Remaining: {} requests", remaining.max(0)));
        }
        if let Some(reset_date) = &self.reset_date {
            lines.push(format!("   Resets: {reset_date}"));
        }
        lines.join("\n")
    }
}

pub fn parse_tavily_usage(value: &Value) -> SearchUsageInfo {
    let account = value.get("account").unwrap_or(value);
    let used = account
        .get("plan_usage")
        .or_else(|| account.get("usage"))
        .or_else(|| account.get("used"))
        .and_then(Value::as_u64);
    let limit = account
        .get("plan_limit")
        .or_else(|| account.get("limit"))
        .or_else(|| account.get("monthly_limit"))
        .and_then(Value::as_u64);
    let remaining = account
        .get("remaining")
        .and_then(Value::as_i64)
        .or_else(|| {
            used.zip(limit)
                .map(|(used, limit)| (limit as i64 - used as i64).max(0))
        });
    SearchUsageInfo {
        provider: "tavily".to_owned(),
        used,
        limit,
        remaining,
        reset_date: account
            .get("reset_date")
            .and_then(Value::as_str)
            .map(str::to_owned),
        search_used: account.get("search_usage").and_then(Value::as_u64),
        extract_used: account.get("extract_usage").and_then(Value::as_u64),
        crawl_used: account.get("crawl_usage").and_then(Value::as_u64),
        supported: true,
        error: None,
    }
}

pub trait SearchUsageClient {
    fn fetch_usage_json(&self, provider: &str) -> Result<Option<Value>, String>;
}

#[derive(Debug, Clone)]
pub struct UreqSearchUsageClient {
    api_key: Option<String>,
    timeout: Duration,
}

impl UreqSearchUsageClient {
    pub fn new(api_key: Option<String>) -> Self {
        Self {
            api_key,
            timeout: Duration::from_secs(8),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl Default for UreqSearchUsageClient {
    fn default() -> Self {
        Self::new(std::env::var("TAVILY_API_KEY").ok())
    }
}

impl SearchUsageClient for UreqSearchUsageClient {
    fn fetch_usage_json(&self, provider: &str) -> Result<Option<Value>, String> {
        if !provider.trim().eq_ignore_ascii_case("tavily") {
            return Ok(None);
        }
        let Some(api_key) = self
            .api_key
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        else {
            return Ok(None);
        };
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(self.timeout))
            .http_status_as_error(false)
            .build()
            .new_agent();
        let mut response = agent
            .get("https://api.tavily.com/usage")
            .header("Authorization", format!("Bearer {api_key}"))
            .call()
            .map_err(|error| error.to_string())?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|error| error.to_string())?;
        if status >= 400 {
            return Err(format!("HTTP {status}"));
        }
        serde_json::from_str(&body)
            .map(Some)
            .map_err(|error| error.to_string())
    }
}

pub fn search_usage_from_json(provider: &str, value: Option<Value>) -> SearchUsageInfo {
    let provider = provider.trim().to_ascii_lowercase();
    match (provider.as_str(), value) {
        ("tavily", Some(value)) => parse_tavily_usage(&value),
        ("tavily", None) => SearchUsageInfo {
            provider,
            supported: true,
            error: Some("TAVILY_API_KEY not configured".to_owned()),
            used: None,
            limit: None,
            remaining: None,
            reset_date: None,
            search_used: None,
            extract_used: None,
            crawl_used: None,
        },
        _ => SearchUsageInfo::unsupported(provider),
    }
}

pub fn fetch_search_usage(
    client: &impl SearchUsageClient,
    provider: &str,
) -> Result<SearchUsageInfo, String> {
    let normalized = provider.trim().to_ascii_lowercase();
    if normalized != "tavily" {
        return Ok(SearchUsageInfo::unsupported(normalized));
    }
    client
        .fetch_usage_json(&normalized)
        .map(|value| search_usage_from_json(&normalized, value))
        .or_else(|error| {
            Ok(SearchUsageInfo {
                provider: normalized,
                supported: true,
                error: Some(error.chars().take(80).collect()),
                used: None,
                limit: None,
                remaining: None,
                reset_date: None,
                search_used: None,
                extract_used: None,
                crawl_used: None,
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn formats_and_parses_tavily_usage() {
        let usage = parse_tavily_usage(
            &json!({"account": {"plan_usage": 142, "plan_limit": 1000, "search_usage": 120, "extract_usage": 15, "crawl_usage": 7}}),
        );
        assert_eq!(usage.used, Some(142));
        assert_eq!(usage.limit, Some(1000));
        assert_eq!(usage.remaining, Some(858));
        assert_eq!(usage.search_used, Some(120));
        assert_eq!(usage.extract_used, Some(15));
        assert_eq!(usage.crawl_used, Some(7));
        assert!(usage.format().contains("142 / 1000"));
        assert!(usage.format().contains("Search: 120"));
        assert!(SearchUsageInfo::unsupported("kagi")
            .format()
            .contains("not available"));
    }

    #[test]
    fn tavily_remaining_is_clamped_for_over_limit_usage() {
        let usage =
            parse_tavily_usage(&json!({"account": {"plan_usage": 1100, "plan_limit": 1000}}));
        assert_eq!(usage.remaining, Some(0));
    }

    #[test]
    fn fetch_usage_routes_supported_and_unsupported_providers() -> Result<(), String> {
        struct FixtureClient;

        impl SearchUsageClient for FixtureClient {
            fn fetch_usage_json(&self, provider: &str) -> Result<Option<Value>, String> {
                assert_eq!(provider, "tavily");
                Ok(Some(
                    json!({"account": {"plan_usage": 1, "plan_limit": 10}}),
                ))
            }
        }

        let usage = fetch_search_usage(&FixtureClient, "Tavily")?;
        assert_eq!(usage.remaining, Some(9));
        assert!(!fetch_search_usage(&FixtureClient, "duckduckgo")?.supported);
        assert!(search_usage_from_json("tavily", None)
            .error
            .unwrap_or_default()
            .contains("TAVILY_API_KEY"));
        Ok(())
    }

    #[test]
    fn fetch_usage_returns_error_info_instead_of_failing_status() -> Result<(), String> {
        struct ErrorClient;

        impl SearchUsageClient for ErrorClient {
            fn fetch_usage_json(&self, _provider: &str) -> Result<Option<Value>, String> {
                Err("HTTP 500".to_owned())
            }
        }

        let usage = fetch_search_usage(&ErrorClient, "tavily")?;
        assert_eq!(usage.error, Some("HTTP 500".to_owned()));
        assert!(usage.format().contains("unavailable"));
        Ok(())
    }
}
