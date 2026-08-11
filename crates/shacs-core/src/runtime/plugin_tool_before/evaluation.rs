use super::*;

pub(super) fn run_command(
    owner: &PluginRuntimeHookAgentHook,
    context: &AgentHookContext,
    call: &RuntimeToolCall,
    plugin: &PluginRuntimePlugin,
    hook: &PluginRuntimeHook,
    attempts: &mut Vec<PluginHookDispatchAttempt>,
) -> Option<(String, String, HookDenialReason)> {
    let hook_ref = hook.plugin_id.clone();
    let result = match owner.mode {
        PluginHookDispatchMode::LiveDiagnostics => catch_unwind(AssertUnwindSafe(|| {
            owner.executor.execute(&hook_invocation(
                plugin,
                hook,
                tool_before_context_payload(context, std::slice::from_ref(call)),
            ))
        }))
        .unwrap_or_else(|_| {
            owner.facts.diagnostic(&hook_ref, HookDiagnosticKind::Panic);
            PluginHookCallbackResult::Error("plugin hook panicked".to_owned())
        }),
        PluginHookDispatchMode::Replay => PluginHookCallbackResult::ReplayRejected(
            "runtime replay does not execute live plugin hook commands".to_owned(),
        ),
    };
    let outcome = command_outcome(owner, call, &hook_ref, &result);
    attempts.push(PluginHookDispatchAttempt {
        plugin_id: hook_ref,
        event: PluginHookEvent::ToolBefore,
        timeout_ms: hook.command.timeout_ms,
        result,
    });
    outcome
}

pub(super) fn run_trusted(
    owner: &PluginRuntimeHookAgentHook,
    call: &RuntimeToolCall,
    handler: &Arc<dyn ToolBeforeHandler>,
) -> Option<(String, String, HookDenialReason)> {
    let hook_ref = handler.hook_ref().to_owned();
    let timeout = handler.timeout();
    let handler = handler.clone();
    let call = call.clone();
    let interaction = owner.interaction.clone();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let spawned = std::thread::Builder::new()
        .name(format!("tool-before-{hook_ref}"))
        .spawn(move || {
            let context = ToolBeforeContext::new(&call, interaction.as_ref());
            let decision = catch_unwind(AssertUnwindSafe(|| handler.evaluate(&context)));
            let _ = sender.send(decision);
        });
    if spawned.is_err() {
        owner.facts.diagnostic(&hook_ref, HookDiagnosticKind::Panic);
        return None;
    }
    match receiver.recv_timeout(timeout) {
        Ok(Ok(ToolBeforeDecision::Allow)) => None,
        Ok(Ok(ToolBeforeDecision::Block { reason })) => {
            Some((hook_ref, reason, HookDenialReason::ExtensionBlocked))
        }
        Ok(Ok(ToolBeforeDecision::InvalidOutput)) => {
            owner
                .facts
                .diagnostic(&hook_ref, HookDiagnosticKind::InvalidOutput);
            None
        }
        Ok(Err(_)) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            owner.facts.diagnostic(&hook_ref, HookDiagnosticKind::Panic);
            None
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            owner
                .facts
                .diagnostic(&hook_ref, HookDiagnosticKind::Timeout);
            None
        }
    }
}

fn command_outcome(
    owner: &PluginRuntimeHookAgentHook,
    call: &RuntimeToolCall,
    hook_ref: &str,
    result: &PluginHookCallbackResult,
) -> Option<(String, String, HookDenialReason)> {
    match result {
        PluginHookCallbackResult::Timeout(_) => {
            owner
                .facts
                .diagnostic(hook_ref, HookDiagnosticKind::Timeout);
            None
        }
        PluginHookCallbackResult::Error(_) | PluginHookCallbackResult::ReplayRejected(_) => None,
        PluginHookCallbackResult::Output(output) => output_outcome(owner, call, hook_ref, output),
    }
}

fn output_outcome(
    owner: &PluginRuntimeHookAgentHook,
    call: &RuntimeToolCall,
    hook_ref: &str,
    output: &Value,
) -> Option<(String, String, HookDenialReason)> {
    let Some(object) = output.as_object() else {
        owner
            .facts
            .diagnostic(hook_ref, HookDiagnosticKind::InvalidOutput);
        return None;
    };
    if let Some(block) = object.get("block") {
        let Some(reason) = block.get("reason").and_then(Value::as_str) else {
            owner
                .facts
                .diagnostic(hook_ref, HookDiagnosticKind::InvalidOutput);
            return None;
        };
        return Some((
            hook_ref.to_owned(),
            reason.to_owned(),
            HookDenialReason::ExtensionBlocked,
        ));
    }
    let confirm = object.get("confirm")?;
    let Some(prompt) = confirm.get("prompt").and_then(Value::as_str) else {
        owner
            .facts
            .diagnostic(hook_ref, HookDiagnosticKind::InvalidOutput);
        return None;
    };
    let reason = confirm
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("confirmation denied")
        .to_owned();
    match ToolBeforeContext::new(call, owner.interaction.as_ref()).confirm(prompt) {
        ToolBeforeConfirmation::Confirmed => None,
        ToolBeforeConfirmation::Denied => {
            Some((hook_ref.to_owned(), reason, HookDenialReason::UserDenied))
        }
        ToolBeforeConfirmation::HeadlessDenied => Some((
            hook_ref.to_owned(),
            reason,
            HookDenialReason::HeadlessConfirmationDenied,
        )),
    }
}
