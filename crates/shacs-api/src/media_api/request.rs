use super::{ChatCompletionAdapter, MEDIA_DIAGNOSTICS_PATH};
use crate::{
    error_response, handle_chat_completion_request, handle_local_improvement,
    handle_session_query_request, handle_spec033_goal_action, json_response, spec030_api,
    spec031_api, spec033_snapshot_response, spec033_snapshot_session_key, ApiError, ApiHttpRequest,
    ApiHttpResponse, ApiMethod, CHAT_COMPLETIONS_PATH, DIAGNOSTICS_PATH, HEALTH_PATH, MODELS_PATH,
    PERMISSIONS_PATH, READINESS_PATH, SESSIONS_PATH, SUBAGENTS_PATH, TOOLS_PATH,
    TRUSTED_RUNTIME_PATH, WORKFLOW_RECIPES_PATH,
};
use serde_json::json;

pub fn handle_api_request(
    request: ApiHttpRequest,
    adapter: &(impl ChatCompletionAdapter + ?Sized),
) -> ApiHttpResponse {
    let path = spec031_api::spec031_projection_path(&request.path);
    match (request.method, path) {
        (ApiMethod::Get, HEALTH_PATH) => json_response(200, crate::health_response()),
        (ApiMethod::Get, MODELS_PATH) => {
            json_response(200, crate::models_response_with_owned_by(&adapter.models()))
        }
        (ApiMethod::Get, MEDIA_DIAGNOSTICS_PATH) => match adapter.media_projection() {
            Some(projection) => json_response(200, json!(projection)),
            None => error_response(ApiError::not_found("media projection is unavailable")),
        },
        (ApiMethod::Get, SESSIONS_PATH) => handle_session_query_request(path, adapter),
        (ApiMethod::Get, CHAT_COMPLETIONS_PATH) => error_response(ApiError::method_not_allowed(
            "method is not supported for this endpoint",
        )),
        (ApiMethod::Get, _) => {
            if let Some(response) =
                spec030_api::handle_trusted_runtime_request(&request.path, adapter)
            {
                return response;
            }
            if let Some(response) =
                spec031_api::handle_spec031_projection_request(&request.path, adapter)
            {
                return response;
            }
            match path {
                WORKFLOW_RECIPES_PATH => match adapter.workflow_recipes_projection() {
                    Some(projection) => json_response(200, projection),
                    None => error_response(ApiError::not_found(
                        "workflow recipe projection is not configured",
                    )),
                },
                PERMISSIONS_PATH => match adapter.remembered_permissions_projection() {
                    Some(projection) => json_response(200, json!(projection)),
                    None => error_response(ApiError::not_found(
                        "remembered permission projection is not configured",
                    )),
                },
                _ if path.starts_with("/v1/improvements/") => {
                    handle_local_improvement(request, adapter)
                }
                _ if path.starts_with("/v1/sessions/") => {
                    if let Some(session_key) = spec033_snapshot_session_key(path) {
                        return spec033_snapshot_response(adapter, &session_key);
                    }
                    handle_session_query_request(path, adapter)
                }
                _ => error_response(ApiError::not_found("API route not found")),
            }
        }
        (ApiMethod::Post, CHAT_COMPLETIONS_PATH) => {
            handle_chat_completion_request(request, adapter)
        }
        (ApiMethod::Post, _) if path.starts_with("/v1/improvements/") => {
            handle_local_improvement(request, adapter)
        }
        (ApiMethod::Post, _) if path.starts_with("/v1/sessions/") => {
            handle_spec033_goal_action(request, adapter)
        }
        (_, HEALTH_PATH)
        | (_, MODELS_PATH)
        | (_, DIAGNOSTICS_PATH)
        | (_, MEDIA_DIAGNOSTICS_PATH)
        | (_, SUBAGENTS_PATH)
        | (_, TOOLS_PATH)
        | (_, READINESS_PATH)
        | (_, TRUSTED_RUNTIME_PATH)
        | (_, WORKFLOW_RECIPES_PATH)
        | (_, PERMISSIONS_PATH)
        | (_, CHAT_COMPLETIONS_PATH)
        | (_, SESSIONS_PATH) => error_response(ApiError::method_not_allowed(
            "method is not supported for this endpoint",
        )),
        (_, path) if path.starts_with("/v1/sessions/") => error_response(
            ApiError::method_not_allowed("method is not supported for this endpoint"),
        ),
        _ => error_response(ApiError::not_found("API route not found")),
    }
}
