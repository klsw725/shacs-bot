use crate::{
    decode_path_segment, error_response, json_response, ApiError, ApiHttpRequest, ApiHttpResponse,
    ApiMethod, ChatCompletionAdapter,
};
use serde::Deserialize;
use serde_json::json;
use shacs_core::runtime::{
    ConfigureObservationRequest, EntrustedObservationPlan, EntrustedWorkReadRequest,
    ObservationComparisonPolicy, ObservationError, ObservationSource,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigureBody {
    expected_revision: u64,
    source: ObservationSource,
    cadence_seconds: u64,
    #[serde(default)]
    comparison: Option<ObservationComparisonPolicy>,
}

pub(crate) fn handle(
    request: &ApiHttpRequest,
    adapter: &(impl ChatCompletionAdapter + ?Sized),
) -> Option<ApiHttpResponse> {
    let suffix = request.path.strip_prefix("/v1/sessions/")?;
    let parts: Vec<_> = suffix.split('/').collect();
    let (requester, owner, work_id) = match parts.as_slice() {
        [requester, "entrusted-work", work_id, "observations"] => {
            (*requester, *requester, *work_id)
        }
        [requester, "entrusted-work", owner, work_id, "observations"] => {
            (*requester, *owner, *work_id)
        }
        _ => return None,
    };
    let read = EntrustedWorkReadRequest {
        owner_session_id: decode_path_segment(owner)?,
        requester_target: decode_path_segment(requester)?,
        work_id: decode_path_segment(work_id)?,
    };
    let Some(workspace) = adapter.session_workspace() else {
        return Some(error_response(ApiError::not_found(
            "session query surface is not configured",
        )));
    };
    let result = match request.method {
        ApiMethod::Get => match adapter.runtime_data_dir() {
            Some(data_dir) => {
                shacs_core::runtime::read_entrusted_observation(&workspace, &data_dir, &read)
                    .map(|snapshot| json!(snapshot))
                    .map_err(core_error)
            }
            None => Err(ApiError::not_found(
                "observation durable store is not configured",
            )),
        },
        ApiMethod::Post => configure(&workspace, read, request),
        ApiMethod::Other => Err(ApiError::not_found("observation method not supported")),
    };
    Some(match result {
        Ok(body) => json_response(200, body),
        Err(error) => error_response(error),
    })
}

fn configure(
    workspace: &std::path::Path,
    work: EntrustedWorkReadRequest,
    request: &ApiHttpRequest,
) -> Result<serde_json::Value, ApiError> {
    let body: ConfigureBody =
        serde_json::from_value(request.body.clone().unwrap_or_else(|| json!({})))
            .map_err(|_| ApiError::invalid_request("invalid observation configuration"))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| ApiError::internal("clock unavailable"))?;
    let now_ms =
        u64::try_from(now.as_millis()).map_err(|_| ApiError::internal("clock overflow"))?;
    let first_wake_at_ms = body
        .cadence_seconds
        .checked_mul(1000)
        .and_then(|delay| now_ms.checked_add(delay))
        .ok_or_else(|| ApiError::invalid_request("invalid cadence"))?;
    shacs_core::runtime::configure_entrusted_observation(
        workspace,
        ConfigureObservationRequest {
            work,
            expected_revision: body.expected_revision,
            plan: EntrustedObservationPlan {
                source: body.source,
                cadence_seconds: body.cadence_seconds,
                first_wake_at_ms,
                comparison: body.comparison,
            },
            changed_at: now_ms.to_string(),
        },
    )
    .map(|snapshot| json!(snapshot))
    .map_err(core_error)
}

fn core_error(error: ObservationError) -> ApiError {
    match error {
        ObservationError::Work(error) => super::spec036_work::core_error(error),
        ObservationError::InvalidInput(reason) => ApiError::invalid_request(reason),
        ObservationError::Blocked(reason) => ApiError {
            status: 409,
            message: format!("observation blocked: {reason:?}"),
            error_type: "conflict".to_owned(),
        },
        ObservationError::Durable(_) | ObservationError::Io(_) => {
            ApiError::internal("observation store unavailable")
        }
    }
}
