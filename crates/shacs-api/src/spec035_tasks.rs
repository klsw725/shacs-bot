use crate::{
    error_response, json_response, ApiError, ApiHttpRequest, ApiHttpResponse, ChatCompletionAdapter,
};
use serde::Deserialize;
use serde_json::Value;
use shacs_core::runtime::{
    accept_spec035_surface_action_outcome, build_spec035_tasks_projection, recover_runtime_surface,
    serialize_spec035_tasks_projection, validate_spec035_task_action, Spec035TasksSemanticAction,
    Spec035TasksSourceError,
};
use std::time::{SystemTime, UNIX_EPOCH};

pub const TASKS_PATH: &str = "/v1/tasks";
pub const TASKS_ACTIONS_PATH: &str = "/v1/tasks/actions";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TasksActionRequest {
    owner: String,
    action: String,
    locator: String,
    session_id: String,
    transport_hello: Value,
}

pub fn tasks_response(
    request: &ApiHttpRequest,
    adapter: &(impl ChatCompletionAdapter + ?Sized),
) -> ApiHttpResponse {
    let session_id = match session_id_from_query(&request.path) {
        Ok(session_id) => session_id,
        Err(error) => return error_response(ApiError::invalid_request(error)),
    };
    let Some(workspace) = adapter.session_workspace() else {
        return error_response(ApiError::not_found(
            "tasks owner sources are not configured",
        ));
    };
    let data_dir = adapter
        .runtime_data_dir()
        .unwrap_or_else(|| workspace.clone());
    projection_response(&workspace, &data_dir, &session_id)
}

pub fn tasks_action_response(
    request: ApiHttpRequest,
    adapter: &(impl ChatCompletionAdapter + ?Sized),
) -> ApiHttpResponse {
    let parsed = request
        .body
        .and_then(|body| serde_json::from_value::<TasksActionRequest>(body).ok());
    let Some(parsed) = parsed else {
        return error_response(ApiError::invalid_request("invalid tasks action request"));
    };
    if parsed.session_id.trim().is_empty() {
        return error_response(ApiError::invalid_request("invalid tasks action request"));
    }
    let action =
        match Spec035TasksSemanticAction::parse(&parsed.owner, &parsed.action, &parsed.locator) {
            Ok(action) => action,
            Err(error) => return error_response(ApiError::invalid_request(error.to_string())),
        };
    let capability = action.transport_capability();
    if let Err(response) =
        crate::spec035_transport::negotiate_api_mutation(&parsed.transport_hello, capability)
    {
        return response;
    }
    let Some(workspace) = adapter.session_workspace() else {
        return error_response(ApiError::not_found(
            "tasks owner sources are not configured",
        ));
    };
    let data_dir = adapter
        .runtime_data_dir()
        .unwrap_or_else(|| workspace.clone());
    let current = match build_spec035_tasks_projection(&workspace, &data_dir, &parsed.session_id) {
        Ok(projection) => projection,
        Err(error) => return error_response(ApiError::internal(error.to_string())),
    };
    if let Err(error) = validate_spec035_task_action(&current, &action) {
        return match error {
            Spec035TasksSourceError::OwnerLocatorNotFound => {
                error_response(ApiError::not_found(error.to_string()))
            }
            _ => error_response(ApiError::invalid_request(error.to_string())),
        };
    }
    if let Err(error) = dispatch_action(&workspace, &data_dir, &parsed.session_id, action) {
        return error_response(ApiError::invalid_request(error));
    }
    projection_response(&workspace, &data_dir, &parsed.session_id)
}

fn dispatch_action(
    workspace: &std::path::Path,
    data_dir: &std::path::Path,
    session_id: &str,
    action: Spec035TasksSemanticAction,
) -> Result<(), String> {
    match action {
        Spec035TasksSemanticAction::GoalPause { .. } => {
            shacs_core::runtime::apply_goal_surface_action(
                workspace,
                session_id,
                shacs_core::runtime::GoalSurfaceAction::Pause,
                &now_ms().to_string(),
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
        }
        Spec035TasksSemanticAction::GoalResume { .. } => {
            shacs_core::runtime::apply_goal_surface_action(
                workspace,
                session_id,
                shacs_core::runtime::GoalSurfaceAction::Resume,
                &now_ms().to_string(),
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
        }
        Spec035TasksSemanticAction::AppStop { app_id } => {
            shacs_core::app_lifecycle::AppSupervisorJournal::new(data_dir.join("runtime/apps"))
                .request(&app_id, shacs_core::app_lifecycle::AppLifecycleAction::Stop)
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
        Spec035TasksSemanticAction::AppRecover { app_id } => {
            let journal =
                shacs_core::app_lifecycle::AppSupervisorJournal::new(data_dir.join("runtime/apps"));
            shacs_core::runtime::AppSupervisor::new(&journal)
                .recover(&app_id, false)
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
        Spec035TasksSemanticAction::RuntimeRecover => recover_runtime_surface(data_dir, now_ms())
            .map_err(|error| error.to_string())
            .and_then(|outcome| {
                accept_spec035_surface_action_outcome(outcome).map_err(|error| error.to_string())
            }),
    }
}

fn projection_response(
    workspace: &std::path::Path,
    data_dir: &std::path::Path,
    session_id: &str,
) -> ApiHttpResponse {
    match build_spec035_tasks_projection(workspace, data_dir, session_id) {
        Ok(projection) => {
            match serialize_spec035_tasks_projection(&projection).and_then(|encoded| {
                serde_json::from_str(&encoded).map_err(|error| {
                    shacs_core::runtime::Spec035TasksSourceError::Owner(error.to_string())
                })
            }) {
                Ok(value) => json_response(200, value),
                Err(error) => error_response(ApiError::internal(error.to_string())),
            }
        }
        Err(error) => error_response(ApiError::internal(error.to_string())),
    }
}

fn session_id_from_query(path: &str) -> Result<String, &'static str> {
    let Some((_, query)) = path.split_once('?') else {
        return Ok(crate::API_SESSION_KEY.to_owned());
    };
    let Some(encoded) = query.strip_prefix("session_id=") else {
        return Err("invalid session_id query");
    };
    if encoded.is_empty() || encoded.contains('&') {
        return Err("invalid session_id query");
    }
    super::decode_path_segment(encoded).ok_or("invalid session_id query")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}
