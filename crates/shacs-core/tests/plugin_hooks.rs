use serde_json::json;
use shacs_core::runtime::{
    plugin_hook_catalog, plugin_hook_error_diagnostic, plugin_hook_timeout_diagnostic,
    summarize_plugin_hook_dispatch, validate_plugin_hook_output, PluginHookCallbackResult,
    PluginHookDispatchAttempt, PluginHookDispatchEffect, PluginHookDispatchStatus, PluginHookEvent,
    PluginHookOutputPolicy,
};

#[test]
fn spec025_hook_catalog_policies_are_descriptor_only() {
    let catalog = plugin_hook_catalog();

    let tool_before = catalog
        .entries
        .iter()
        .find(|entry| entry.event == PluginHookEvent::ToolBefore)
        .ok_or("missing tool:before")
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        tool_before.output_policy,
        PluginHookOutputPolicy::BehaviorAffecting
    );
    assert!(!tool_before.can_request_permission_approval);
    assert!(catalog.entries.iter().all(|entry| entry.timeout_ms > 0));
}

#[test]
fn spec025_behavior_affecting_output_validation_accepts_safe_output() {
    let validation = validate_plugin_hook_output(
        "review",
        PluginHookEvent::LlmBefore,
        json!({"replacementText": "safe"}),
    );

    assert!(validation.accepted);
    assert_eq!(
        validation.effective_output,
        Some(json!({"replacementText": "safe"}))
    );
    assert!(validation.diagnostics.is_empty());
}

#[test]
fn spec025_tool_before_cannot_approve_permissions() {
    let validation = validate_plugin_hook_output(
        "review",
        PluginHookEvent::ToolBefore,
        json!({"approvePermissions": true}),
    );

    assert!(!validation.accepted);
    assert_eq!(validation.effective_output, None);
    assert_eq!(validation.diagnostics.len(), 1);
    assert!(validation.diagnostics[0]
        .message
        .contains("cannot approve or grant permissions"));
}

#[test]
fn spec025_observer_only_hooks_ignore_outputs() {
    let validation = validate_plugin_hook_output(
        "review",
        PluginHookEvent::SessionStart,
        json!({"replacementText": "ignored"}),
    );

    assert!(validation.accepted);
    assert_eq!(validation.effective_output, None);
}

#[test]
fn spec025_hook_diagnostics_are_redacted() {
    let timeout = plugin_hook_timeout_diagnostic(
        "review",
        PluginHookEvent::LlmAfter,
        1000,
        "token sk-test-secret-value timed out",
    );
    let error = plugin_hook_error_diagnostic(
        "review",
        PluginHookEvent::LlmAfter,
        "authorization: Bearer shacs_secret_123456789",
    );

    assert!(!timeout.message.contains("sk-test-secret-value"));
    assert!(!error.message.contains("shacs_secret_123456789"));
}

#[test]
fn spec025_hook_dispatch_summary_isolates_errors_timeouts_and_invalid_outputs() {
    let summary = summarize_plugin_hook_dispatch(
        PluginHookEvent::ToolBefore,
        vec![
            PluginHookDispatchAttempt {
                plugin_id: "review".to_owned(),
                event: PluginHookEvent::ToolBefore,
                timeout_ms: 1000,
                result: PluginHookCallbackResult::Output(json!({"block": "unsafe"})),
            },
            PluginHookDispatchAttempt {
                plugin_id: "approval".to_owned(),
                event: PluginHookEvent::ToolBefore,
                timeout_ms: 1000,
                result: PluginHookCallbackResult::Output(json!({"approvePermissions": true})),
            },
            PluginHookDispatchAttempt {
                plugin_id: "slow".to_owned(),
                event: PluginHookEvent::ToolBefore,
                timeout_ms: 250,
                result: PluginHookCallbackResult::Timeout(
                    "token sk-test-secret-value timed out".to_owned(),
                ),
            },
            PluginHookDispatchAttempt {
                plugin_id: "broken".to_owned(),
                event: PluginHookEvent::ToolBefore,
                timeout_ms: 1000,
                result: PluginHookCallbackResult::Error(
                    "authorization: Bearer shacs_secret_123456789".to_owned(),
                ),
            },
            PluginHookDispatchAttempt {
                plugin_id: "other-event".to_owned(),
                event: PluginHookEvent::LlmAfter,
                timeout_ms: 1000,
                result: PluginHookCallbackResult::Error("ignored".to_owned()),
            },
        ],
    );

    assert_eq!(summary.dispatch_count, 4);
    assert_eq!(summary.success_count, 1);
    assert_eq!(summary.invalid_output_count, 1);
    assert_eq!(summary.timeout_count, 1);
    assert_eq!(summary.error_count, 1);
    assert_eq!(summary.blocked_count, 1);
    assert_eq!(summary.output_evidence.len(), 1);
    assert!(summary.output_evidence[0].digest.starts_with("sha256:"));
    assert!(summary.output_evidence[0]
        .redacted_preview
        .contains("unsafe"));
    assert_eq!(summary.last_success_plugin_id, Some("review".to_owned()));
    assert!(summary
        .last_error
        .as_ref()
        .is_some_and(|error| error.message.contains("[REDACTED]")));
    assert!(summary.last_timeout.is_some());
    assert_eq!(
        summary.records[0].status,
        PluginHookDispatchStatus::Succeeded
    );
    assert_eq!(
        summary.records[0].effect,
        Some(PluginHookDispatchEffect::Blocked)
    );
    assert!(summary.records[0].output_evidence.is_some());
    assert_eq!(
        summary.records[1].status,
        PluginHookDispatchStatus::InvalidOutput
    );
    assert_eq!(
        summary.records[2].status,
        PluginHookDispatchStatus::TimedOut
    );
    assert_eq!(summary.records[3].status, PluginHookDispatchStatus::Failed);
    assert!(!summary.records[3]
        .error
        .as_ref()
        .is_some_and(|error| error.message.contains("shacs_secret_123456789")));
    assert!(!summary.records[2]
        .timeout
        .as_ref()
        .is_some_and(|timeout| timeout.message.contains("sk-test-secret-value")));
}

#[test]
fn spec025_hook_dispatch_rejects_live_replay_without_effective_output() {
    let summary = summarize_plugin_hook_dispatch(
        PluginHookEvent::LlmBefore,
        vec![PluginHookDispatchAttempt {
            plugin_id: "replay-plugin".to_owned(),
            event: PluginHookEvent::LlmBefore,
            timeout_ms: 1000,
            result: PluginHookCallbackResult::ReplayRejected(
                "authorization: Bearer shacs_secret_123456789".to_owned(),
            ),
        }],
    );

    assert_eq!(summary.dispatch_count, 1);
    assert_eq!(summary.replay_rejection_count, 1);
    assert!(summary.output_evidence.is_empty());
    assert_eq!(
        summary.records[0].status,
        PluginHookDispatchStatus::ReplayRejected
    );
    let message = &summary.records[0].error.as_ref().unwrap().message;
    assert!(message.contains("rejected during replay"));
    assert!(!message.contains("shacs_secret_123456789"));
}
