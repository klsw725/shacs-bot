use super::*;
use crate::runtime::task_method::{MethodRequest, RequestState};
use crate::runtime::task_method_observe::MethodObservation;
use crate::runtime::task_method_render::projection;
use crate::runtime::task_method_store::MethodStore;

fn binding(request: &MethodRequest) -> String {
    format!(
        "task_method_id={};selection_revision={};request_id={}",
        request.selection.method.canonical(),
        request.selection.revision,
        request.request_id
    )
}

pub(super) fn bind(session: &Session, plan: &mut WorkflowHarnessPlan) -> std::io::Result<()> {
    if pending_workflow(session).is_none() {
        if let Some(request) = MethodStore::load(session)?.active {
            plan.constraints.push(binding(&request));
            let context = projection(&request);
            if !context.is_empty() {
                plan.constraints.push(context);
            }
        }
    }
    Ok(())
}

pub(super) fn tracks(
    message: &InboundMessage,
    session: &Session,
    plan: &WorkflowHarnessPlan,
) -> std::io::Result<bool> {
    if method::user_turn(message) {
        return Ok(true);
    }
    Ok(MethodStore::load(session)?
        .active
        .is_some_and(|request| plan.constraints.contains(&binding(&request))))
}

pub(super) fn finish(
    session: &mut Session,
    observation: &Arc<Mutex<MethodObservation>>,
    state: WorkflowRunState,
) -> std::io::Result<String> {
    let state = match state {
        WorkflowRunState::Planned
        | WorkflowRunState::Admitted
        | WorkflowRunState::Running
        | WorkflowRunState::WaitingForChildren
        | WorkflowRunState::Verifying
        | WorkflowRunState::Synthesizing
        | WorkflowRunState::WaitingForUser
        | WorkflowRunState::Blocked => RequestState::InProgress,
        WorkflowRunState::Completed => RequestState::Answered,
        WorkflowRunState::Failed | WorkflowRunState::Stale => RequestState::Failed,
        WorkflowRunState::Cancelled => RequestState::Cancelled,
    };
    method_finish::finish(session, observation, state)
}
