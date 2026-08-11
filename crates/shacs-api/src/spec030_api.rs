use crate::{error_response, json_response, ApiError, ApiHttpResponse, ChatCompletionAdapter};

pub const TRUSTED_RUNTIME_PATH: &str = "/v1/trusted-runtime";

pub fn handle_trusted_runtime_request(
    path: &str,
    adapter: &(impl ChatCompletionAdapter + ?Sized),
) -> Option<ApiHttpResponse> {
    let (route, query) = path
        .split_once('?')
        .map_or((path, None), |(route, query)| (route, Some(query)));
    if route != TRUSTED_RUNTIME_PATH {
        return None;
    }
    if query.is_some_and(|query| query != "schema_version=1") {
        return Some(error_response(ApiError::invalid_request(
            "unsupported Spec030 schema version selector",
        )));
    }
    Some(json_response(
        200,
        serde_json::json!(adapter.trusted_runtime_projection()),
    ))
}
