use super::*;

pub(super) fn dispatch_event(
    owner: &PluginRuntimeHookAgentHook,
    event: PluginHookEvent,
    context_payload: Value,
) -> Option<PluginHookDispatchSummary> {
    let mut attempts = Vec::new();
    for plugin in &owner.snapshot.plugins {
        for hook in plugin.hooks.iter().filter(|hook| hook.event == event) {
            let result = match owner.mode {
                PluginHookDispatchMode::LiveDiagnostics => owner
                    .executor
                    .execute(&hook_invocation(plugin, hook, context_payload.clone())),
                PluginHookDispatchMode::Replay => PluginHookCallbackResult::ReplayRejected(
                    "runtime replay does not execute live plugin hook commands".to_owned(),
                ),
            };
            attempts.push(PluginHookDispatchAttempt {
                plugin_id: hook.plugin_id.clone(),
                event,
                timeout_ms: hook.command.timeout_ms,
                result,
            });
        }
    }
    let summary = (!attempts.is_empty()).then(|| summarize_plugin_hook_dispatch(event, attempts));
    if let (Some(sink), Some(summary)) = (&owner.sink, &summary) {
        sink(summary.clone());
    }
    summary
}
