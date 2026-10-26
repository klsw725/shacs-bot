use crate::{
    decode_path_segment, error_response, json_response, ApiError, ApiHttpRequest, ApiHttpResponse,
    ApiMethod, ChatCompletionAdapter,
};
use serde::Deserialize;
use serde_json::json;
use shacs_core::runtime::{
    ConfigureWebNotifyRequest, ConfirmWebWatchRequest, EntrustedWorkReadRequest, ObservationError,
    WebCondition, WebFetchLimits, WebPreviewRequest, WebPreviewTarget, WebTextArea, WebWatchError,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewBody {
    url: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    phrase: Option<String>,
    #[serde(default)]
    until: Option<String>,
    #[serde(default)]
    decimal_comma: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NotifyBody {
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    off: bool,
    #[serde(default)]
    failure_threshold_seconds: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfirmBody {
    expected_revision: u64,
    url: String,
    candidate: String,
    condition: WebCondition,
    cadence_seconds: u64,
}

pub(crate) fn handle(
    request: &ApiHttpRequest,
    adapter: &(impl ChatCompletionAdapter + ?Sized),
) -> Option<ApiHttpResponse> {
    let suffix = request.path.strip_prefix("/v1/sessions/")?;
    let parts: Vec<_> = suffix.split('/').collect();
    let (requester, owner, work_id, action) = match parts.as_slice() {
        [requester, "entrusted-work", work_id, "web-watch"] => {
            (*requester, *requester, *work_id, None)
        }
        [requester, "entrusted-work", work_id, "web-watch", action @ ("preview" | "notify")] => {
            (*requester, *requester, *work_id, Some(*action))
        }
        [requester, "entrusted-work", owner, work_id, "web-watch"] => {
            (*requester, *owner, *work_id, None)
        }
        [requester, "entrusted-work", owner, work_id, "web-watch", action @ ("preview" | "notify")] => {
            (*requester, *owner, *work_id, Some(*action))
        }
        _ => return None,
    };
    let work = EntrustedWorkReadRequest {
        owner_session_id: decode_path_segment(owner)?,
        requester_target: decode_path_segment(requester)?,
        work_id: decode_path_segment(work_id)?,
    };
    let Some(workspace) = adapter.session_workspace() else {
        return Some(error_response(ApiError::not_found(
            "session query surface is not configured",
        )));
    };
    let guard = adapter.web_watch_network_guard();
    let limits = WebFetchLimits::PRODUCTION;
    let result = match (request.method, action) {
        (ApiMethod::Post, Some("notify")) => body::<NotifyBody>(request).and_then(|body| {
            let target = match (body.target, body.off) {
                (Some(target), false) => Some(target),
                (None, true) => None,
                _ => {
                    return Err(ApiError::invalid_request(
                        "notify needs either target or off",
                    ))
                }
            };
            let configure = ConfigureWebNotifyRequest {
                work,
                email_ready: target.is_some() && adapter.web_watch_email_ready(),
                target,
                failure_threshold_seconds: body.failure_threshold_seconds,
                now_ms: adapter.now_ms(),
            };
            shacs_core::runtime::configure_web_watch_notifications(&workspace, configure)
                .map(|settings| json!(settings))
                .map_err(web_error)
        }),
        (ApiMethod::Post, Some(_)) => body::<PreviewBody>(request).and_then(|body| {
            let target =
                match (body.label, body.phrase, body.until) {
                    (Some(label), None, None) => WebPreviewTarget::Number {
                        label,
                        decimal_comma: body.decimal_comma,
                    },
                    (None, Some(phrase), None) if !body.decimal_comma => {
                        WebPreviewTarget::Text { phrase, area: None }
                    }
                    (Some(label), Some(phrase), Some(until)) if !body.decimal_comma => {
                        WebPreviewTarget::Text {
                            phrase,
                            area: Some(WebTextArea { label, until }),
                        }
                    }
                    _ => return Err(ApiError::invalid_request(
                        "preview needs label for a number, or phrase with optional label and until",
                    )),
                };
            let preview = WebPreviewRequest {
                work,
                url: body.url,
                target,
            };
            shacs_core::runtime::preview_web_watch(&workspace, &preview, &guard, &limits)
                .map(|preview| json!(preview))
                .map_err(web_error)
        }),
        (ApiMethod::Post, None) => body::<ConfirmBody>(request).and_then(|body| {
            let confirm = ConfirmWebWatchRequest {
                work,
                expected_revision: body.expected_revision,
                url: body.url,
                candidate_token: body.candidate,
                condition: body.condition,
                cadence_seconds: body.cadence_seconds,
                now_ms: adapter.now_ms(),
            };
            shacs_core::runtime::confirm_web_watch(&workspace, confirm, &guard, &limits)
                .map(|configured| json!(configured))
                .map_err(web_error)
        }),
        (ApiMethod::Get, None) => match adapter.runtime_data_dir() {
            Some(data_dir) => shacs_core::runtime::read_web_watch(&workspace, &data_dir, &work)
                .map(|snapshot| json!(snapshot))
                .map_err(web_error),
            None => Err(ApiError::not_found(
                "web watch durable store is not configured",
            )),
        },
        _ => Err(ApiError::not_found("web watch method not supported")),
    };
    Some(match result {
        Ok(body) => json_response(200, body),
        Err(error) => error_response(error),
    })
}

fn body<T: serde::de::DeserializeOwned>(request: &ApiHttpRequest) -> Result<T, ApiError> {
    serde_json::from_value(request.body.clone().unwrap_or_else(|| json!({})))
        .map_err(|_| ApiError::invalid_request("invalid web watch request"))
}

fn web_error(error: WebWatchError) -> ApiError {
    let status = |status: u16, error_type: &str, message: String| ApiError {
        status,
        message,
        error_type: error_type.to_owned(),
    };
    match error {
        WebWatchError::Observation(ObservationError::Work(error)) => {
            super::spec036_work::core_error(error)
        }
        WebWatchError::Observation(ObservationError::InvalidInput(reason)) => {
            ApiError::invalid_request(reason)
        }
        WebWatchError::Observation(ObservationError::Blocked(reason)) => {
            status(409, "conflict", format!("web watch blocked: {reason:?}"))
        }
        WebWatchError::Observation(_) => ApiError::internal("web watch store unavailable"),
        WebWatchError::InvalidInput(reason) => ApiError::invalid_request(reason),
        error @ WebWatchError::ValueChanged { .. } => status(409, "conflict", error.to_string()),
        error @ WebWatchError::Unreadable { .. } => {
            status(422, "web_page_unreadable", error.to_string())
        }
    }
}
