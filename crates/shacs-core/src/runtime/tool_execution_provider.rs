use super::tool_execution::{
    process_gate_input_for_action, PermissionEvaluation, ToolExecutionContext,
};
use crate::runtime::{reply_route, PermissionedAction, SafetyCapability};
use crate::tools::{BrowserHandoffOrigin, ToolCallExecutionContext, ToolRegistry};

pub(super) fn tool_call_execution_context(
    registry: &ToolRegistry,
    call_id: &str,
    action: &PermissionedAction,
    evaluation: PermissionEvaluation,
    context: &ToolExecutionContext,
) -> ToolCallExecutionContext {
    let mut provider_invocation = context
        .cancellation_token
        .as_ref()
        .map_or_else(shacs_providers::ProviderInvocation::default, |token| {
            token.provider_invocation(None)
        });
    if let Some(deadline) = context.deadline {
        provider_invocation = provider_invocation.with_deadline(deadline);
    }
    let cron_grant = evaluation.cron_grant_used();
    let approved_outside = evaluation
        .outside_targets()
        .iter()
        .map(std::path::PathBuf::from)
        .collect();
    let execution = if action.capabilities.contains(&SafetyCapability::ProcExec) {
        ToolCallExecutionContext::new(
            process_gate_input_for_action(registry, action, evaluation).ok(),
        )
    } else {
        ToolCallExecutionContext::default()
    }
    .with_provider_invocation(provider_invocation)
    .with_browser_binding(
        action
            .redacted_arguments
            .get("browser_binding_digest")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
    )
    .with_browser_context(
        context
            .browser_context
            .as_ref()
            .filter(|browser| {
                browser.effective_session_id == action.session_id
                    && browser.permission_mode_snapshot == context.permission_mode_snapshot
                    && browser.permission_interactive == context.permission_interactive
            })
            .cloned(),
    )
    // The raw call id: it is the one the paused call keeps in the history.
    .with_browser_handoff_origin((!call_id.is_empty()).then(|| {
        BrowserHandoffOrigin {
            tool_call_id: call_id.to_owned(),
            channel: context.channel.clone(),
            chat_id: context.chat_id.clone(),
            reply_route: context
                .metadata
                .as_object()
                .map(reply_route)
                .unwrap_or_default(),
        }
    }))
    .with_hidden_paths(context.permission_environment.clone())
    .with_skill_environment(context.skill_environment.clone())
    .with_approved_outside(approved_outside)
    .with_cron_grant(cron_grant);
    match (
        action.capabilities.contains(&SafetyCapability::ProcExec),
        context.cancellation_token.as_ref(),
    ) {
        (true, Some(token)) => execution.with_process_abort(token.controlled_child_abort()),
        (true, None) | (false, Some(_)) | (false, None) => execution,
    }
}
