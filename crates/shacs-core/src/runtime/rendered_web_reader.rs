use shacs_security::NetworkGuard;

use super::web_watch_fetch::{self, ReadPage};
use super::{
    web_watch_text, BrowserUnavailableReason, WebCheckFailure, WebCheckResult, WebFetchLimits,
    WebReadingMode, WebResponseFacts,
};

/// A bounded, ephemeral document. It is never a serializable observation or tool result.
#[derive(Clone)]
pub struct RenderedWebPage {
    pub text: String,
    pub final_url: String,
    pub http_status: u16,
    pub retry_after: Option<String>,
    pub truncated: bool,
}

/// A classified read failure. A response, when verified, contains no discarded text.
#[derive(Debug, Clone)]
pub struct RenderedWebFailure {
    pub failure: WebCheckFailure,
    pub browser_reason: Option<BrowserUnavailableReason>,
    pub response: Option<RenderedWebPage>,
}

impl std::fmt::Debug for RenderedWebPage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RenderedWebPage")
            .field("http_status", &self.http_status)
            .field("truncated", &self.truncated)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Display for RenderedWebFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:?}", self.failure)?;
        if let Some(reason) = self.browser_reason {
            write!(formatter, " ({})", reason.code())?;
        }
        Ok(())
    }
}

impl std::error::Error for RenderedWebFailure {}

/// Fixed DOM observation expression. The bound is applied before a browser
/// returns its text; no page-supplied source is inserted into the expression.
pub fn rendered_text_expression(limits: &WebFetchLimits) -> String {
    format!(
        "(()=>{{const source=document.body?document.body.innerText:'';\
         const limit={};let bytes=0,end=0;for(const point of source){{\
         const code=point.codePointAt(0);const size=code<=127?1:code<=2047?2:\
         code<=65535?3:4;if(bytes+size>limit)break;bytes+=size;end+=point.length;\
         }}return {{text:source.slice(0,end),truncated:end<source.length}};}})()",
        limits.max_bytes
    )
}

/// The confirmed profile choice accompanies every preview and scheduled read.
pub struct RenderedWebRequest<'a> {
    pub url: &'a str,
    pub limits: &'a WebFetchLimits,
    pub use_saved_login: bool,
}

/// Opens, observes and completely cleans up one browser generation.
pub trait RenderedWebReader: Send + Sync {
    fn read(&self, request: &RenderedWebRequest<'_>)
        -> Result<RenderedWebPage, RenderedWebFailure>;

    /// Configuration only; the real process gate still runs at each start.
    fn enabled(&self) -> bool;

    fn saved_login_enabled(&self) -> bool;
}

impl std::fmt::Debug for dyn RenderedWebReader {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RenderedWebReader")
    }
}

pub(super) fn read_selected(
    url: &str,
    mode: WebReadingMode,
    use_saved_login: bool,
    guard: &NetworkGuard,
    limits: &WebFetchLimits,
    reader: Option<&dyn RenderedWebReader>,
) -> Result<ReadPage, RenderedWebFailure> {
    match mode {
        WebReadingMode::Fetch => {
            web_watch_fetch::read_page(url, guard, limits).map_err(|failure| RenderedWebFailure {
                failure,
                browser_reason: None,
                response: None,
            })
        }
        WebReadingMode::Browser => {
            let reader = reader.ok_or(RenderedWebFailure {
                failure: WebCheckFailure::BrowserUnavailable,
                browser_reason: Some(BrowserUnavailableReason::Disabled),
                response: None,
            })?;
            if use_saved_login && !reader.saved_login_enabled() {
                return Err(RenderedWebFailure {
                    failure: WebCheckFailure::BrowserUnavailable,
                    browser_reason: Some(BrowserUnavailableReason::PersistLoginDisabled),
                    response: None,
                });
            }
            let page = reader.read(&RenderedWebRequest {
                url,
                limits,
                use_saved_login,
            })?;
            let text = web_watch_text::normalize_fragment(&page.text);
            let digest = if web_watch_fetch::status_failure(page.http_status).is_some() {
                String::new()
            } else {
                web_watch_fetch::sha256_ref(&text)
            };
            let response = facts(&page, digest);
            Ok(ReadPage {
                response,
                text,
                retry_after: page.retry_after,
            })
        }
    }
}

pub(super) fn failed_check(error: RenderedWebFailure, now_ms: u64) -> WebCheckResult {
    let retry_after_ms = error.response.as_ref().and_then(|page| {
        page.retry_after
            .as_deref()
            .filter(|_| matches!(page.http_status, 429 | 503))
            .and_then(|value| web_watch_fetch::retry_after_ms(value, now_ms))
    });
    WebCheckResult::Failed {
        failure: error.failure,
        browser_reason: error.browser_reason,
        response: error
            .response
            .as_ref()
            .map(|page| facts(page, String::new())),
        retry_after_ms,
    }
}

fn facts(page: &RenderedWebPage, text_digest: String) -> WebResponseFacts {
    WebResponseFacts {
        final_url: web_watch_fetch::display_url(&page.final_url),
        final_url_digest: web_watch_fetch::sha256_ref(&page.final_url),
        http_status: page.http_status,
        text_digest,
        truncated: page.truncated,
    }
}
