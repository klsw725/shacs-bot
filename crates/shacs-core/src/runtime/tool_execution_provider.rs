use super::tool_execution::{
    process_gate_input_for_action, PermissionEvaluation, ToolExecutionContext,
};
use crate::runtime::{PermissionedAction, SafetyCapability};
use crate::tools::{ToolCallExecutionContext, ToolRegistry};

pub(super) fn tool_call_execution_context(
    registry: &ToolRegistry,
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
    let execution = if action.capabilities.contains(&SafetyCapability::ProcExec) {
        ToolCallExecutionContext::new(
            process_gate_input_for_action(registry, action, evaluation).ok(),
        )
    } else {
        ToolCallExecutionContext::default()
    }
    .with_provider_invocation(provider_invocation);
    match (
        action.capabilities.contains(&SafetyCapability::ProcExec),
        context.cancellation_token.as_ref(),
    ) {
        (true, Some(token)) => execution.with_process_abort(token.controlled_child_abort()),
        (true, None) | (false, Some(_)) | (false, None) => execution,
    }
}
