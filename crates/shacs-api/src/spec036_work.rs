use serde::Deserialize;
use serde_json::{json, Value};
use shacs_core::runtime::{
    EntrustedWorkConditions, EntrustedWorkConsentProof, EntrustedWorkCreateRequest,
    EntrustedWorkError, EntrustedWorkLifecycleAction, EntrustedWorkLifecycleRequest,
    EntrustedWorkReadRequest, EntrustedWorkReviseRequest,
};

use crate::{
    decode_path_segment, error_response, json_response, ApiError, ApiHttpRequest, ApiHttpResponse,
    ApiMethod, ChatCompletionAdapter,
};

pub const ENTRUSTED_WORK_SUFFIX: &str = "entrusted-work";

#[derive(Debug)]
enum WorkRoute {
    Create {
        requester: String,
    },
    Read {
        requester: String,
        owner: String,
        work_id: String,
    },
    Mutate {
        requester: String,
        owner: String,
        work_id: String,
        action: WorkMutation,
    },
}

#[derive(Debug, Clone, Copy)]
enum WorkMutation {
    Revise,
    Lifecycle(EntrustedWorkLifecycleAction),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateBody {
    goal_text: String,
    #[serde(default = "default_turn_budget")]
    turn_budget: u32,
    consent: EntrustedWorkConsentProof,
    conditions: EntrustedWorkConditions,
    #[serde(default)]
    linked_targets: Vec<String>,
    observed_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviseBody {
    expected_revision: u64,
    conditions: EntrustedWorkConditions,
    #[serde(default)]
    linked_targets: Option<Vec<String>>,
    changed_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LifecycleBody {
    observed_at: String,
}

pub(crate) fn is_entrusted_work_path(path: &str) -> bool {
    path.strip_prefix("/v1/sessions/").is_some_and(|suffix| {
        suffix
            .split('/')
            .any(|segment| segment == ENTRUSTED_WORK_SUFFIX)
    })
}

pub(crate) fn handle_entrusted_work_request(
    request: ApiHttpRequest,
    adapter: &(impl ChatCompletionAdapter + ?Sized),
) -> ApiHttpResponse {
    if let Some(response) = super::spec042_web_watch::handle(&request, adapter) {
        return response;
    }
    if let Some(response) = super::spec036_observation::handle(&request, adapter) {
        return response;
    }
    let route = match work_route(request.method, &request.path) {
        Some(route) => route,
        None => return error_response(ApiError::not_found("entrusted work API route not found")),
    };
    let Some(workspace) = adapter.session_workspace() else {
        return error_response(ApiError::not_found(
            "session query surface is not configured",
        ));
    };
    let result = match route {
        WorkRoute::Create { requester } => {
            parse_body::<CreateBody>(request.body).and_then(|body| {
                shacs_core::runtime::create_entrusted_work(
                    &workspace,
                    EntrustedWorkCreateRequest {
                        owner_session_id: requester,
                        goal_text: body.goal_text,
                        turn_budget: body.turn_budget,
                        consent: body.consent,
                        conditions: body.conditions,
                        linked_targets: body.linked_targets,
                        observed_at: body.observed_at,
                    },
                )
                .map(|snapshot| (201, json!(snapshot)))
                .map_err(core_error)
            })
        }
        WorkRoute::Read {
            requester,
            owner,
            work_id,
        } => shacs_core::runtime::read_entrusted_work(
            &workspace,
            &EntrustedWorkReadRequest {
                owner_session_id: owner,
                requester_target: requester,
                work_id,
            },
        )
        .map(|snapshot| (200, json!(snapshot)))
        .map_err(core_error),
        WorkRoute::Mutate {
            requester,
            owner,
            work_id,
            action: WorkMutation::Revise,
        } => parse_body::<ReviseBody>(request.body).and_then(|body| {
            shacs_core::runtime::revise_entrusted_work(
                &workspace,
                EntrustedWorkReviseRequest {
                    owner_session_id: owner,
                    requester_target: requester,
                    work_id,
                    expected_revision: body.expected_revision,
                    conditions: body.conditions,
                    linked_targets: body.linked_targets,
                    changed_at: body.changed_at,
                },
            )
            .map(|snapshot| (200, json!(snapshot)))
            .map_err(core_error)
        }),
        WorkRoute::Mutate {
            requester,
            owner,
            work_id,
            action,
        } => parse_body::<LifecycleBody>(request.body).and_then(|body| {
            let WorkMutation::Lifecycle(action) = action else {
                return Err(ApiError::internal(
                    "entrusted work mutation routing was inconsistent",
                ));
            };
            shacs_core::runtime::transition_entrusted_work(
                &workspace,
                EntrustedWorkLifecycleRequest {
                    owner_session_id: owner,
                    requester_target: requester,
                    work_id,
                    action,
                    observed_at: body.observed_at,
                },
            )
            .map(|snapshot| (200, json!(snapshot)))
            .map_err(core_error)
        }),
    };
    match result {
        Ok((status, body)) => json_response(status, body),
        Err(error) => error_response(error),
    }
}

fn work_route(method: ApiMethod, path: &str) -> Option<WorkRoute> {
    let suffix = path.strip_prefix("/v1/sessions/")?;
    let mut segments = suffix.split('/');
    let requester = decode_nonempty(segments.next()?)?;
    if segments.next()? != ENTRUSTED_WORK_SUFFIX {
        return None;
    }
    let remaining = segments.collect::<Vec<_>>();
    match (method, remaining.as_slice()) {
        (ApiMethod::Post, []) => Some(WorkRoute::Create { requester }),
        (ApiMethod::Get, [work_id]) => Some(WorkRoute::Read {
            owner: requester.clone(),
            requester,
            work_id: decode_nonempty(work_id)?,
        }),
        (ApiMethod::Get, [owner, work_id]) => Some(WorkRoute::Read {
            requester,
            owner: decode_nonempty(owner)?,
            work_id: decode_nonempty(work_id)?,
        }),
        (ApiMethod::Post, [work_id, action]) => Some(WorkRoute::Mutate {
            owner: requester.clone(),
            requester,
            work_id: decode_nonempty(work_id)?,
            action: mutation(action)?,
        }),
        (ApiMethod::Post, [owner, work_id, action]) => Some(WorkRoute::Mutate {
            requester,
            owner: decode_nonempty(owner)?,
            work_id: decode_nonempty(work_id)?,
            action: mutation(action)?,
        }),
        (ApiMethod::Get | ApiMethod::Post | ApiMethod::Other, _) => None,
    }
}

fn decode_nonempty(value: &str) -> Option<String> {
    decode_path_segment(value).filter(|decoded| !decoded.is_empty())
}

fn mutation(value: &str) -> Option<WorkMutation> {
    match value {
        "revise" => Some(WorkMutation::Revise),
        "pause" => Some(WorkMutation::Lifecycle(EntrustedWorkLifecycleAction::Pause)),
        "resume" => Some(WorkMutation::Lifecycle(
            EntrustedWorkLifecycleAction::Resume,
        )),
        "done" => Some(WorkMutation::Lifecycle(EntrustedWorkLifecycleAction::Done)),
        "clear" => Some(WorkMutation::Lifecycle(EntrustedWorkLifecycleAction::Clear)),
        _ => None,
    }
}

fn parse_body<T: for<'de> Deserialize<'de>>(body: Option<Value>) -> Result<T, ApiError> {
    serde_json::from_value(body.unwrap_or_else(|| json!({})))
        .map_err(|error| ApiError::invalid_request(format!("invalid entrusted work body: {error}")))
}

pub(super) fn core_error(error: EntrustedWorkError) -> ApiError {
    let message = error.to_string();
    match error {
        EntrustedWorkError::SessionMissing | EntrustedWorkError::WorkMissing => {
            ApiError::not_found(message)
        }
        EntrustedWorkError::AccessDenied => ApiError::forbidden(message),
        EntrustedWorkError::InvalidInput(_) => ApiError::invalid_request(message),
        EntrustedWorkError::WorkAlreadyExists
        | EntrustedWorkError::GoalAlreadyActive
        | EntrustedWorkError::GoalBindingMismatch
        | EntrustedWorkError::RevisionConflict { .. }
        | EntrustedWorkError::GoalTransition(_) => ApiError {
            status: 409,
            message,
            error_type: "conflict".to_owned(),
        },
        EntrustedWorkError::Io(_)
        | EntrustedWorkError::Metadata(_)
        | EntrustedWorkError::GoalMetadata(_)
        | EntrustedWorkError::MissingGoalTransitionId => ApiError::internal(message),
    }
}

const fn default_turn_budget() -> u32 {
    shacs_core::runtime::DEFAULT_GOAL_TURN_BUDGET
}
