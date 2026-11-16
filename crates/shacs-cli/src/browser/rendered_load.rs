use serde_json::Value;
use shacs_core::runtime::{BrowserDocument, RenderedWebPage};
use std::collections::{HashMap, HashSet};

struct Response {
    url: String,
    status: u16,
    retry_after: Option<String>,
}

/// Only the initial page session contributes network and lifecycle facts.
#[derive(Default)]
pub(super) struct RenderedLoad {
    requests: HashSet<String>,
    documents: HashMap<String, (String, String)>,
    responses: HashMap<(String, String), Response>,
    loaded: HashSet<(String, String)>,
    quiet_since_ms: Option<u64>,
}

impl RenderedLoad {
    pub fn event(&mut self, event: &Value, session: &str) {
        if event["sessionId"].as_str() != Some(session) {
            return;
        }
        let params = &event["params"];
        let key = || {
            Some((
                params["frameId"].as_str()?.to_owned(),
                params["loaderId"].as_str()?.to_owned(),
            ))
        };
        match event["method"].as_str() {
            Some("Network.requestWillBeSent") => {
                if let Some(id) = params["requestId"].as_str() {
                    if params["request"]["url"].as_str().is_some_and(http_url) {
                        self.requests.insert(id.to_owned());
                        self.quiet_since_ms = None;
                    }
                }
                if params["type"] == "Document" {
                    if let Some(key) = key() {
                        self.responses.remove(&key);
                        if let Some(id) = params["requestId"].as_str() {
                            self.documents.insert(id.to_owned(), key);
                        }
                    }
                }
            }
            Some("Network.loadingFinished" | "Network.loadingFailed") => {
                if let Some(id) = params["requestId"].as_str() {
                    self.requests.remove(id);
                }
            }
            Some("Network.responseReceived") if params["type"] == "Document" => {
                let response = &params["response"];
                let status = response["status"]
                    .as_u64()
                    .and_then(|status| u16::try_from(status).ok())
                    .or_else(|| response["status"].as_f64()?.to_string().parse::<u16>().ok());
                if let (Some(key), Some(status), Some(url)) = (
                    key(),
                    status,
                    response["url"].as_str().filter(|url| http_url(url)),
                ) {
                    if params["requestId"]
                        .as_str()
                        .and_then(|id| self.documents.get(id))
                        != Some(&key)
                    {
                        return;
                    }
                    let retry_after = response["headers"].as_object().and_then(|headers| {
                        headers.iter().find_map(|(name, value)| {
                            name.eq_ignore_ascii_case("retry-after")
                                .then(|| value.as_str().map(str::to_owned))
                                .flatten()
                        })
                    });
                    self.responses.insert(
                        key,
                        Response {
                            url: url.to_owned(),
                            status,
                            retry_after,
                        },
                    );
                }
            }
            Some("Page.lifecycleEvent") if params["name"] == "load" => {
                if let Some(key) = key() {
                    self.loaded.insert(key);
                }
            }
            _ => {}
        }
    }

    pub fn page(&self, document: &BrowserDocument) -> Option<RenderedWebPage> {
        let response = self
            .responses
            .get(&(document.frame_id.clone(), document.loader_id.clone()))?;
        if response.url.split('#').next() != document.url.split('#').next() {
            return None;
        }
        Some(RenderedWebPage {
            text: String::new(),
            final_url: document.url.clone(),
            http_status: response.status,
            retry_after: response.retry_after.clone(),
            truncated: false,
        })
    }

    pub fn quiet(&mut self, document: &BrowserDocument, now_ms: u64) -> bool {
        if !self.requests.is_empty()
            || !self
                .loaded
                .contains(&(document.frame_id.clone(), document.loader_id.clone()))
        {
            self.quiet_since_ms = None;
            return false;
        }
        let since = self.quiet_since_ms.get_or_insert(now_ms);
        now_ms.saturating_sub(*since) >= 500
    }
}

fn http_url(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}
