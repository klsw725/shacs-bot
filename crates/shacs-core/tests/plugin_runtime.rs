use serde_json::{json, Map};
use shacs_core::runtime::{
    build_plugin_runtime_snapshot, build_plugin_surface_projection,
    containment_permission_proof_for_process_gate, plugin_runtime_commands,
    register_plugin_runtime_tools, AgentHook, AgentHookContext, AgentLoop, AgentLoopCommandResult,
    AgentLoopConfig, ContainerNetworkMode, ContainerRuntimeKind, ContextBuilder, DiscoveredPlugin,
    DockerContainmentSnapshot, InheritedPermissionContext, MessageBus, PermissionCeilingSnapshot,
    PermissionMode, PermissionModeSnapshot, PermissionRuleInput, PermissionedAction,
    PermissionedActionOrigin, PluginBlockReason, PluginCommandDispatcher, PluginExecutableCommand,
    PluginHookCallbackResult, PluginHookCommandExecutor, PluginHookCommandInvocation,
    PluginHookDispatchMode, PluginHookDispatchStatus, PluginHookEvent, PluginManifest,
    PluginManifestSource, PluginProcessPermissionContext, PluginRuntimeHook,
    PluginRuntimeHookAgentHook, PluginRuntimePlugin, PluginRuntimeSnapshot, PluginState,
    PolicySafetyDigest, PolicySafetySnapshotId, PolicySafetySnapshotRef,
    PolicySafetySnapshotSchemaId, ProcExecSummary, ProcessAdapterKind,
    ProcessContainmentProofCandidate, ProcessExecutionEnvelope, ProcessExecutionEnvelopeInput,
    ProcessGateInput, ProcessGateTerminalPrecondition, ProcessIdentity,
    ProcessPluginHookCommandExecutor, ProcessRedactedCommand, RedactedPolicySafetySummary,
    RuntimeBoundaryOrigin, RuntimeToolCall, RuntimeToolExecutor, SafetyCapability, SessionManager,
    ToolExecutionContext,
};
use shacs_core::tools::{
    AskUserTool, JsonMap, SchemaFragment, Tool, ToolParameters, ToolRegistry, ToolResult,
};
use shacs_providers::{
    LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest, ToolCallRequest,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex, MutexGuard};

static PROCESS_EXECUTOR_TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn spec025_runtime_snapshot_includes_enabled_plugins_only() {
    let enabled = enabled_plugin(
        "enabled",
        json!({"hooks": ["runtime:start"]}),
        json!({"hooks": {"runtime:start": {"command": "bin/hook"}}}),
    );
    let disabled = plugin_with_state("disabled", PluginState::Disabled);
    let not_enabled = plugin_with_state("waiting", PluginState::NotEnabled);
    let blocked = plugin_with_state("blocked", PluginState::Blocked);

    let snapshot = build_plugin_runtime_snapshot(&[blocked, not_enabled, disabled, enabled]);

    assert_eq!(snapshot.plugins.len(), 1);
    assert_eq!(snapshot.plugins[0].id, "enabled");
    assert_eq!(
        snapshot.plugins[0].manifest_digest,
        Some("sha256:enabled".to_owned())
    );
    assert_eq!(snapshot.plugins[0].source, PluginManifestSource::UserData);
    assert_eq!(snapshot.plugins[0].hooks.len(), 1);
    assert!(snapshot.diagnostics.is_empty());
}

#[test]
fn spec025_runtime_snapshot_parses_typed_hook_commands() {
    let plugin = enabled_plugin(
        "argv",
        json!({"hooks": ["runtime:start", "tool:before"]}),
        json!({
            "hooks": {
                "tool:before": {"command": ["bin/tool-hook", "--flag"], "timeoutMs": 250},
                "runtime:start": {"command": "hook", "args": ["--boot"]}
            }
        }),
    );

    let snapshot = build_plugin_runtime_snapshot(&[plugin]);
    let hooks = &snapshot.plugins[0].hooks;

    assert_eq!(hooks.len(), 2);
    assert_eq!(hooks[0].event, PluginHookEvent::RuntimeStart);
    assert_eq!(hooks[0].event_name, "runtime:start");
    assert_eq!(
        hooks[0].command.command_path,
        PathBuf::from("/tmp/argv/hook")
    );
    assert_eq!(hooks[0].command.args, ["--boot"]);
    assert_eq!(hooks[0].command.timeout_ms, 1_000);
    assert_eq!(hooks[1].event, PluginHookEvent::ToolBefore);
    assert_eq!(
        hooks[1].command.command_path,
        PathBuf::from("/tmp/argv/bin/tool-hook")
    );
    assert_eq!(hooks[1].command.args, ["--flag"]);
    assert_eq!(hooks[1].command.timeout_ms, 250);
    assert!(snapshot.diagnostics.is_empty());
}

#[test]
fn spec025_runtime_snapshot_reports_invalid_hooks_without_blocking_valid_hooks() {
    let plugin = enabled_plugin(
        "mixed",
        json!({"hooks": ["runtime:start", "llm:after", "tool:before", "tool:after"]}),
        json!({
            "hooks": {
                "runtime:start": {"command": "bin/start"},
                "llm:after": "node hook.js --token sk-secret-token",
                "tool:before": {"command": "../sk-secret-token"},
                "tool:after": {"command": "bin/after", "timeoutMs": 0},
                "unknown:event": {"command": "bin/unknown"}
            }
        }),
    );

    let snapshot = build_plugin_runtime_snapshot(&[plugin]);

    assert_eq!(snapshot.plugins.len(), 1);
    assert_eq!(snapshot.plugins[0].hooks.len(), 1);
    assert_eq!(
        snapshot.plugins[0].hooks[0].event,
        PluginHookEvent::RuntimeStart
    );
    assert_eq!(snapshot.diagnostics.len(), 4);
    assert!(snapshot.diagnostics.iter().any(|diagnostic| {
        diagnostic.event.as_deref() == Some("llm:after")
            && diagnostic.code == "unsupported_hook_entrypoint"
    }));
    assert!(snapshot.diagnostics.iter().any(|diagnostic| {
        diagnostic.event.as_deref() == Some("tool:before")
            && diagnostic.code == "unsafe_hook_command"
            && !diagnostic.message.contains("sk-secret-token")
    }));
    assert!(snapshot.diagnostics.iter().any(|diagnostic| {
        diagnostic.event.as_deref() == Some("tool:after")
            && diagnostic.code == "invalid_hook_timeout"
    }));
    assert!(snapshot.diagnostics.iter().any(|diagnostic| {
        diagnostic.event.as_deref() == Some("unknown:event")
            && diagnostic.code == "unsupported_hook_event"
    }));
    assert!(!serde_json::to_string(&snapshot)
        .unwrap_or_else(|error| error.to_string())
        .contains("sk-secret-token"));
}

#[test]
fn spec025_runtime_snapshot_reports_missing_hook_entrypoints() {
    let plugin = enabled_plugin("missing", json!({"hooks": ["runtime:start"]}), json!({}));

    let snapshot = build_plugin_runtime_snapshot(&[plugin]);

    assert_eq!(snapshot.plugins.len(), 1);
    assert!(snapshot.plugins[0].hooks.is_empty());
    assert_eq!(snapshot.diagnostics.len(), 1);
    assert_eq!(snapshot.diagnostics[0].code, "missing_hook_entrypoint");
    assert_eq!(
        snapshot.diagnostics[0].event.as_deref(),
        Some("runtime:start")
    );
}

#[test]
fn spec025_runtime_snapshot_rejects_hook_entrypoints_not_declared_in_surface() {
    let plugin = enabled_plugin(
        "hidden",
        json!({"hooks": []}),
        json!({"hooks": {"llm:after": {"command": "bin/hidden"}}}),
    );

    let snapshot = build_plugin_runtime_snapshot(&[plugin]);

    assert_eq!(snapshot.plugins.len(), 1);
    assert!(snapshot.plugins[0].hooks.is_empty());
    assert_eq!(snapshot.diagnostics.len(), 1);
    assert_eq!(snapshot.diagnostics[0].code, "undeclared_hook_entrypoint");
    assert_eq!(snapshot.diagnostics[0].event.as_deref(), Some("llm:after"));
}

#[test]
fn spec025_descriptor_only_surface_remains_non_executable_with_runtime_snapshot() {
    let plugin = enabled_plugin(
        "compat",
        json!({"hooks": ["runtime:start"]}),
        json!({"hooks": {"runtime:start": {"command": "bin/start"}}}),
    );

    let projection = build_plugin_surface_projection(std::slice::from_ref(&plugin));
    let snapshot = build_plugin_runtime_snapshot(&[plugin]);

    assert_eq!(projection.hooks.len(), 1);
    assert!(!projection.hooks[0].execution_enabled);
    assert_eq!(snapshot.plugins[0].hooks.len(), 1);
}

#[test]
fn spec025_s3_live_diagnostics_dispatch_executes_matching_hooks_and_records_summary() {
    let plugin = enabled_plugin(
        "observer",
        json!({"hooks": ["llm:after"]}),
        json!({"hooks": {"llm:after": {"command": "bin/observe"}}}),
    );
    let snapshot = build_plugin_runtime_snapshot(&[plugin]);
    let executor = Arc::new(FakeExecutor::new(PluginHookCallbackResult::Output(
        json!({"diagnostic": {"message": "observed"}}),
    )));
    let summaries = Arc::new(Mutex::new(Vec::new()));
    let hook = PluginRuntimeHookAgentHook::with_executor(
        snapshot,
        PluginHookDispatchMode::LiveDiagnostics,
        executor.clone(),
    )
    .with_sink(recording_sink(summaries.clone()));
    let context = AgentHookContext {
        iteration: 2,
        messages: vec![json!({"role": "user", "content": "hello sk-secret-token"})],
    };
    let response = LlmResponse {
        content: Some("assistant text".to_owned()),
        ..LlmResponse::default()
    };

    hook.after_response(&context, &response);

    assert_eq!(executor.count(), 1);
    let invocations = executor.invocations();
    assert_eq!(invocations.len(), 1);
    assert_eq!(invocations[0].event, PluginHookEvent::LlmAfter);
    assert_eq!(invocations[0].stdin_payload["event"], "llm:after");
    assert_eq!(invocations[0].stdin_payload["plugin_id"], "observer");
    assert_eq!(recorded_summaries_len(&summaries), 1);
    let summary = recorded_summary(&summaries, 0);
    assert_eq!(summary.dispatch_count, 1);
    assert_eq!(summary.success_count, 1);
    assert_eq!(summary.observed_count, 1);
    assert_eq!(summary.output_evidence.len(), 1);
}

#[test]
fn spec025_s3_hook_outputs_do_not_mutate_tool_calls_or_llm_response_content() {
    let plugin = enabled_plugin(
        "diagnostic-only",
        json!({"hooks": ["llm:after", "tool:before"]}),
        json!({"hooks": {
            "llm:after": {"command": "bin/llm"},
            "tool:before": {"command": "bin/tool"}
        }}),
    );
    let snapshot = build_plugin_runtime_snapshot(&[plugin]);
    let executor = Arc::new(FakeExecutor::new(PluginHookCallbackResult::Output(json!({
        "diagnostic": {"replacementText": "must not apply"},
        "block": "must not block"
    }))));
    let hook = PluginRuntimeHookAgentHook::with_executor(
        snapshot,
        PluginHookDispatchMode::LiveDiagnostics,
        executor,
    );
    let context = AgentHookContext {
        iteration: 0,
        messages: vec![json!({"role": "assistant", "content": "pending"})],
    };
    let calls = vec![RuntimeToolCall::new(
        "call-1",
        "read_file",
        json!({"path": "/tmp/sk-secret-token"}),
    )];
    let original_calls = calls.clone();
    let response = LlmResponse {
        content: Some("original response".to_owned()),
        ..LlmResponse::default()
    };

    hook.before_execute_tools(&context, &calls);
    hook.after_response(&context, &response);
    let finalized = hook.finalize_content(&context, "final content".to_owned());

    assert_eq!(calls, original_calls);
    assert_eq!(response.content.as_deref(), Some("original response"));
    assert_eq!(finalized, "final content");
}

#[test]
fn spec025_s3_invalid_executor_output_is_redacted_and_non_fatal() {
    let plugin = enabled_plugin(
        "broken",
        json!({"hooks": ["llm:after"]}),
        json!({"hooks": {"llm:after": {"command": "bin/broken"}}}),
    );
    let snapshot = build_plugin_runtime_snapshot(&[plugin]);
    let executor = Arc::new(FakeExecutor::new(PluginHookCallbackResult::Error(
        "process exited 1 with token sk-secret-token".to_owned(),
    )));
    let hook = PluginRuntimeHookAgentHook::with_executor(
        snapshot,
        PluginHookDispatchMode::LiveDiagnostics,
        executor,
    );
    let context = AgentHookContext {
        iteration: 0,
        messages: Vec::new(),
    };

    let summary = hook
        .dispatch_llm_after(&context, &LlmResponse::default())
        .unwrap_or_else(|| panic!("expected plugin hook dispatch summary"));

    assert_eq!(summary.dispatch_count, 1);
    assert_eq!(summary.error_count, 1);
    assert_eq!(summary.records[0].status, PluginHookDispatchStatus::Failed);
    let message = summary.records[0]
        .error
        .as_ref()
        .map(|error| error.message.as_str())
        .unwrap_or_else(|| panic!("expected redacted error diagnostic"));
    assert!(message.contains("[REDACTED]"));
    assert!(!message.contains("sk-secret-token"));
}

#[test]
fn spec025_s3_process_executor_runs_argv_without_shell_and_parses_json_stdout() {
    let _guard = process_executor_test_guard();
    let tempdir = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("failed to create temporary plugin root: {error}"));
    let bin_dir = tempdir.path().join("bin");
    fs::create_dir(&bin_dir).unwrap_or_else(|error| panic!("failed to create bin dir: {error}"));
    let hook_path = bin_dir.join("hook");
    fs::write(
        &hook_path,
        "#!/bin/sh\nprintf '{\"diagnostic\":{\"message\":\"argv-ok\"}}'\n",
    )
    .unwrap_or_else(|error| panic!("failed to write hook script: {error}"));
    let mut permissions = fs::metadata(&hook_path)
        .unwrap_or_else(|error| panic!("failed to read hook metadata: {error}"))
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&hook_path, permissions)
        .unwrap_or_else(|error| panic!("failed to make hook executable: {error}"));
    let snapshot = PluginRuntimeSnapshot {
        plugins: vec![PluginRuntimePlugin {
            id: "process".to_owned(),
            root: tempdir.path().to_path_buf(),
            manifest_digest: None,
            source: PluginManifestSource::UserData,
            hooks: vec![PluginRuntimeHook {
                plugin_id: "process".to_owned(),
                event: PluginHookEvent::LlmAfter,
                event_name: "llm:after".to_owned(),
                command: PluginExecutableCommand {
                    command_path: hook_path.clone(),
                    args: Vec::new(),
                    timeout_ms: 1_000,
                },
            }],
        }],
        commands: Vec::new(),
        diagnostics: Vec::new(),
    };
    let hook = PluginRuntimeHookAgentHook::with_executor(
        snapshot,
        PluginHookDispatchMode::LiveDiagnostics,
        Arc::new(ProcessPluginHookCommandExecutor::with_process_gate_input(
            plugin_process_gate_input(
                ProcessAdapterKind::PluginHook,
                "process",
                "llm:after",
                &PluginExecutableCommand {
                    command_path: hook_path,
                    args: Vec::new(),
                    timeout_ms: 1_000,
                },
            ),
        )),
    );
    let context = AgentHookContext {
        iteration: 0,
        messages: Vec::new(),
    };

    let summary = hook
        .dispatch_llm_after(&context, &LlmResponse::default())
        .unwrap_or_else(|| panic!("expected process executor summary"));

    assert_eq!(summary.dispatch_count, 1);
    assert_eq!(summary.success_count, 1);
    assert_eq!(summary.observed_count, 1);
    assert!(summary.output_evidence[0]
        .redacted_preview
        .contains("argv-ok"));
}

#[cfg(unix)]
#[test]
fn spec025_s3_process_executor_clears_parent_environment() {
    let _guard = process_executor_test_guard();
    let tempdir = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("failed to create temporary plugin root: {error}"));
    let bin_dir = tempdir.path().join("bin");
    fs::create_dir(&bin_dir).unwrap_or_else(|error| panic!("failed to create bin dir: {error}"));
    let hook_path = bin_dir.join("hook");
    fs::write(
        &hook_path,
        "#!/bin/sh\nif [ -n \"$HOME\" ]; then printf '{\"diagnostic\":{\"message\":\"env-leaked\"}}'; else printf '{\"diagnostic\":{\"message\":\"env-clear\"}}'; fi\n",
    )
    .unwrap_or_else(|error| panic!("failed to write hook script: {error}"));
    make_executable(&hook_path);
    let snapshot = process_snapshot(
        "env",
        tempdir.path().to_path_buf(),
        hook_path.clone(),
        1_000,
    );
    let hook = PluginRuntimeHookAgentHook::with_executor(
        snapshot,
        PluginHookDispatchMode::LiveDiagnostics,
        Arc::new(ProcessPluginHookCommandExecutor::with_process_gate_input(
            plugin_process_gate_input(
                ProcessAdapterKind::PluginHook,
                "env",
                "llm:after",
                &PluginExecutableCommand {
                    command_path: hook_path,
                    args: Vec::new(),
                    timeout_ms: 1_000,
                },
            ),
        )),
    );
    let context = AgentHookContext {
        iteration: 0,
        messages: Vec::new(),
    };

    let summary = hook
        .dispatch_llm_after(&context, &LlmResponse::default())
        .unwrap_or_else(|| panic!("expected process executor summary"));

    assert_eq!(summary.dispatch_count, 1);
    assert_eq!(summary.success_count, 1);
    assert!(summary.output_evidence[0]
        .redacted_preview
        .contains("env-clear"));
    assert!(!summary.output_evidence[0]
        .redacted_preview
        .contains("env-leaked"));
}

#[cfg(unix)]
#[test]
fn spec025_s3_process_executor_kills_process_group_on_timeout() {
    let _guard = process_executor_test_guard();
    let tempdir = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("failed to create temporary plugin root: {error}"));
    let bin_dir = tempdir.path().join("bin");
    fs::create_dir(&bin_dir).unwrap_or_else(|error| panic!("failed to create bin dir: {error}"));
    let marker_path = tempdir.path().join("leaked-child");
    let hook_path = bin_dir.join("hook");
    fs::write(
        &hook_path,
        format!(
            "#!/bin/sh\n(sleep 1; printf leaked > {}) &\nsleep 5\n",
            shell_quote(marker_path.to_string_lossy().as_ref())
        ),
    )
    .unwrap_or_else(|error| panic!("failed to write hook script: {error}"));
    make_executable(&hook_path);
    let snapshot = process_snapshot(
        "timeout",
        tempdir.path().to_path_buf(),
        hook_path.clone(),
        50,
    );
    let hook = PluginRuntimeHookAgentHook::with_executor(
        snapshot,
        PluginHookDispatchMode::LiveDiagnostics,
        Arc::new(ProcessPluginHookCommandExecutor::with_process_gate_input(
            plugin_process_gate_input(
                ProcessAdapterKind::PluginHook,
                "timeout",
                "llm:after",
                &PluginExecutableCommand {
                    command_path: hook_path,
                    args: Vec::new(),
                    timeout_ms: 50,
                },
            ),
        )),
    );
    let context = AgentHookContext {
        iteration: 0,
        messages: Vec::new(),
    };

    let summary = hook
        .dispatch_llm_after(&context, &LlmResponse::default())
        .unwrap_or_else(|| panic!("expected process executor summary"));
    std::thread::sleep(std::time::Duration::from_millis(1_500));

    assert_eq!(summary.dispatch_count, 1);
    assert_eq!(summary.timeout_count, 1);
    assert!(!marker_path.exists());
}

#[cfg(unix)]
#[test]
fn spec025_s3_process_executor_success_cleans_background_child_without_hanging() {
    let _guard = process_executor_test_guard();
    let tempdir = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("failed to create temporary plugin root: {error}"));
    let bin_dir = tempdir.path().join("bin");
    fs::create_dir(&bin_dir).unwrap_or_else(|error| panic!("failed to create bin dir: {error}"));
    let marker_path = tempdir.path().join("leaked-child");
    let hook_path = bin_dir.join("hook");
    fs::write(
        &hook_path,
        format!(
            "#!/bin/sh\n(sleep 1; printf leaked > {}) &\nprintf '{{\"diagnostic\":{{\"message\":\"background-clean\"}}}}'\n",
            shell_quote(marker_path.to_string_lossy().as_ref())
        ),
    )
    .unwrap_or_else(|error| panic!("failed to write hook script: {error}"));
    make_executable(&hook_path);
    let snapshot = process_snapshot(
        "background-child",
        tempdir.path().to_path_buf(),
        hook_path.clone(),
        1_000,
    );
    let hook = PluginRuntimeHookAgentHook::with_executor(
        snapshot,
        PluginHookDispatchMode::LiveDiagnostics,
        Arc::new(ProcessPluginHookCommandExecutor::with_process_gate_input(
            plugin_process_gate_input(
                ProcessAdapterKind::PluginHook,
                "background-child",
                "llm:after",
                &PluginExecutableCommand {
                    command_path: hook_path,
                    args: Vec::new(),
                    timeout_ms: 1_000,
                },
            ),
        )),
    );
    let context = AgentHookContext {
        iteration: 0,
        messages: Vec::new(),
    };

    let started = std::time::Instant::now();
    let summary = hook
        .dispatch_llm_after(&context, &LlmResponse::default())
        .unwrap_or_else(|| panic!("expected process executor summary"));
    let dispatch_elapsed = started.elapsed();
    std::thread::sleep(std::time::Duration::from_millis(1_500));

    assert!(dispatch_elapsed < std::time::Duration::from_secs(2));
    assert_eq!(summary.dispatch_count, 1);
    assert_eq!(summary.success_count, 1);
    assert!(summary.output_evidence[0]
        .redacted_preview
        .contains("background-clean"));
    assert!(!marker_path.exists());
}

#[cfg(unix)]
#[test]
fn spec025_s3_process_executor_timeout_is_not_blocked_by_noisy_stdout() {
    let _guard = process_executor_test_guard();
    let tempdir = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("failed to create temporary plugin root: {error}"));
    let bin_dir = tempdir.path().join("bin");
    fs::create_dir(&bin_dir).unwrap_or_else(|error| panic!("failed to create bin dir: {error}"));
    let hook_path = bin_dir.join("hook");
    fs::write(&hook_path, "#!/bin/sh\nwhile :; do printf x; done\n")
        .unwrap_or_else(|error| panic!("failed to write hook script: {error}"));
    make_executable(&hook_path);
    let invocation = PluginHookCommandInvocation {
        plugin_id: "noisy-stdout".to_owned(),
        event: PluginHookEvent::LlmAfter,
        event_name: "llm:after".to_owned(),
        command: PluginExecutableCommand {
            command_path: hook_path.clone(),
            args: Vec::new(),
            timeout_ms: 50,
        },
        working_dir: bin_dir,
        stdin_payload: json!({}),
    };

    let started = std::time::Instant::now();
    let result =
        ProcessPluginHookCommandExecutor::with_process_gate_input(plugin_process_gate_input(
            ProcessAdapterKind::PluginHook,
            "noisy-stdout",
            "llm:after",
            &PluginExecutableCommand {
                command_path: hook_path,
                args: Vec::new(),
                timeout_ms: 50,
            },
        ))
        .execute(&invocation);

    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    assert!(matches!(result, PluginHookCallbackResult::Timeout(_)));
}

#[cfg(unix)]
#[test]
fn spec025_s3_process_executor_timeout_is_not_blocked_by_large_stdin() {
    let _guard = process_executor_test_guard();
    let tempdir = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("failed to create temporary plugin root: {error}"));
    let bin_dir = tempdir.path().join("bin");
    fs::create_dir(&bin_dir).unwrap_or_else(|error| panic!("failed to create bin dir: {error}"));
    let hook_path = bin_dir.join("hook");
    fs::write(&hook_path, "#!/bin/sh\nsleep 5\n")
        .unwrap_or_else(|error| panic!("failed to write hook script: {error}"));
    make_executable(&hook_path);
    let invocation = PluginHookCommandInvocation {
        plugin_id: "large-stdin".to_owned(),
        event: PluginHookEvent::LlmAfter,
        event_name: "llm:after".to_owned(),
        command: PluginExecutableCommand {
            command_path: hook_path.clone(),
            args: Vec::new(),
            timeout_ms: 50,
        },
        working_dir: bin_dir,
        stdin_payload: json!({"payload": "x".repeat(2 * 1024 * 1024)}),
    };

    let started = std::time::Instant::now();
    let result =
        ProcessPluginHookCommandExecutor::with_process_gate_input(plugin_process_gate_input(
            ProcessAdapterKind::PluginHook,
            "large-stdin",
            "llm:after",
            &PluginExecutableCommand {
                command_path: hook_path,
                args: Vec::new(),
                timeout_ms: 50,
            },
        ))
        .execute(&invocation);

    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    assert!(matches!(result, PluginHookCallbackResult::Timeout(_)));
}

#[test]
fn spec025_s3_replay_mode_rejects_live_dispatch_without_executor_invocation() {
    let plugin = enabled_plugin(
        "replay",
        json!({"hooks": ["tool:before"]}),
        json!({"hooks": {"tool:before": {"command": "bin/tool"}}}),
    );
    let snapshot = build_plugin_runtime_snapshot(&[plugin]);
    let executor = Arc::new(FakeExecutor::new(PluginHookCallbackResult::Output(json!({
        "diagnostic": {"message": "should not run"}
    }))));
    let hook = PluginRuntimeHookAgentHook::with_executor(
        snapshot,
        PluginHookDispatchMode::Replay,
        executor.clone(),
    );
    let context = AgentHookContext {
        iteration: 0,
        messages: Vec::new(),
    };
    let calls = vec![RuntimeToolCall::new("call-1", "read_file", json!({}))];

    let summary = hook
        .dispatch_tool_before(&context, &calls)
        .unwrap_or_else(|| panic!("expected replay rejection summary"));

    assert_eq!(executor.count(), 0);
    assert_eq!(summary.dispatch_count, 1);
    assert_eq!(summary.replay_rejection_count, 1);
    assert_eq!(
        summary.records[0].status,
        PluginHookDispatchStatus::ReplayRejected
    );
}

#[test]
fn spec025_replay_mode_does_not_apply_tool_before_blocks() {
    let plugin = enabled_plugin(
        "replay-block",
        json!({"hooks": ["tool:before"]}),
        json!({"hooks": {"tool:before": {"command": "bin/tool"}}}),
    );
    let snapshot = build_plugin_runtime_snapshot(&[plugin]);
    let executor = Arc::new(FakeExecutor::new(PluginHookCallbackResult::Output(json!({
        "block": {"reason": "should not apply"}
    }))));
    let hook = PluginRuntimeHookAgentHook::with_executor(
        snapshot,
        PluginHookDispatchMode::Replay,
        executor.clone(),
    );
    let context = AgentHookContext {
        iteration: 0,
        messages: Vec::new(),
    };
    let calls = vec![RuntimeToolCall::new("call-1", "read_file", json!({}))];

    let blocked = hook.blocked_tool_messages(&context, &calls);

    assert_eq!(executor.count(), 0);
    assert!(blocked.is_empty());
}

#[test]
fn spec025_tool_before_block_skips_tool_without_permission_approval() {
    let plugin = enabled_plugin(
        "blocker",
        json!({"hooks": ["tool:before"]}),
        json!({"hooks": {"tool:before": {"command": "bin/block"}}}),
    );
    let snapshot = build_plugin_runtime_snapshot(&[plugin]);
    let executor = Arc::new(FakeExecutor::new(PluginHookCallbackResult::Output(json!({
        "block": {"reason": "policy denied"}
    }))));
    let hook = Arc::new(PluginRuntimeHookAgentHook::with_executor(
        snapshot,
        PluginHookDispatchMode::LiveDiagnostics,
        executor.clone(),
    ));
    let mut registry = ToolRegistry::new();
    registry.register(PanicTool);
    let runner = shacs_core::runtime::AgentRunner::new();
    let client = QueueProviderClient::new(vec![
        Ok(LlmResponse {
            content: Some("call tool".to_owned()),
            tool_calls: vec![ToolCallRequest {
                id: "call-1".to_owned(),
                name: "panic_tool".to_owned(),
                arguments: serde_json::Map::new(),
                extra_content: None,
                provider_specific_fields: None,
                function_provider_specific_fields: None,
            }],
            finish_reason: "tool_calls".to_owned(),
            ..LlmResponse::default()
        }),
        Ok(LlmResponse {
            content: Some("blocked handled".to_owned()),
            finish_reason: "stop".to_owned(),
            ..LlmResponse::default()
        }),
    ]);
    let mut spec = shacs_core::runtime::AgentRunSpec::new(
        vec![json!({"role": "user", "content": "run"})],
        &registry,
        &client,
        "fake-model",
    );
    spec.max_iterations = 2;
    spec.agent_hook = Some(hook);

    let result = runner
        .run(spec)
        .unwrap_or_else(|error| panic!("agent run failed: {error}"));

    assert_eq!(executor.count(), 1);
    assert!(result.interrupt.is_none());
    let tool_message = result
        .messages
        .iter()
        .find(|message| message.get("role").and_then(serde_json::Value::as_str) == Some("tool"))
        .unwrap_or_else(|| panic!("missing blocked tool message: {:?}", result.messages));
    let content = tool_message
        .get("content")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    assert!(content.contains("Error: Tool `panic_tool` blocked by plugin hook `blocker`"));
    assert!(content.contains("policy denied"));
}

#[cfg(unix)]
#[test]
fn spec025_command_backed_plugin_tool_registers_and_executes_without_shell_env() {
    let _guard = process_executor_test_guard();
    let tempdir = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("failed to create temporary plugin root: {error}"));
    let bin_dir = tempdir.path().join("bin");
    fs::create_dir(&bin_dir).unwrap_or_else(|error| panic!("failed to create bin dir: {error}"));
    let tool_path = bin_dir.join("tool");
    fs::write(
        &tool_path,
        "#!/bin/sh\nif [ -n \"$HOME\" ]; then printf 'env leaked'; exit 2; fi\ncat >/dev/null\nprintf '{\"ok\":true,\"message\":\"plugin tool ran\"}'\n",
    )
    .unwrap_or_else(|error| panic!("failed to write tool script: {error}"));
    make_executable(&tool_path);
    let plugin = plugin_with_root(
        "tool-plugin",
        tempdir.path().to_path_buf(),
        json!({"tools": ["plugin_tool_probe"]}),
        json!({"tools": {"plugin_tool_probe": {
            "command": "bin/tool",
            "description": "Probe plugin tool",
            "parameters": {"type": "object", "properties": {"input": {"type": "string"}}}
        }}}),
    );
    let mut registry = ToolRegistry::new();

    let diagnostics = register_plugin_runtime_tools(&mut registry, &[plugin]);
    let executor = RuntimeToolExecutor::new(&registry);
    let report = executor.execute_tool_calls(
        vec![RuntimeToolCall::new(
            "plugin-tool-call",
            "plugin_tool_probe",
            json!({"input": "hello"}),
        )],
        &plugin_tool_context(),
    );
    let output = report.messages[0].content.clone();
    let definitions = registry.definitions();

    assert!(diagnostics.is_empty());
    assert!(registry.has("plugin_tool_probe"));
    assert!(output.contains("plugin tool ran"));
    assert!(definitions.iter().any(|definition| {
        definition
            .get("function")
            .and_then(serde_json::Value::as_object)
            .and_then(|function| function.get("x-shacs-source-kind"))
            .and_then(serde_json::Value::as_str)
            == Some("plugin_tool")
    }));
}

#[cfg(unix)]
#[test]
fn spec025_plugin_tool_name_conflict_does_not_override_existing_tool() {
    let _guard = process_executor_test_guard();
    let tempdir = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("failed to create temporary plugin root: {error}"));
    let bin_dir = tempdir.path().join("bin");
    fs::create_dir(&bin_dir).unwrap_or_else(|error| panic!("failed to create bin dir: {error}"));
    let marker_path = tempdir.path().join("plugin-tool-ran");
    let tool_path = bin_dir.join("tool");
    fs::write(
        &tool_path,
        format!(
            "#!/bin/sh\nprintf ran > {}\nprintf '{{\"ok\":true,\"message\":\"plugin tool ran\"}}'\n",
            shell_quote(marker_path.to_string_lossy().as_ref())
        ),
    )
    .unwrap_or_else(|error| panic!("failed to write tool script: {error}"));
    make_executable(&tool_path);
    let plugin = plugin_with_root(
        "tool-plugin",
        tempdir.path().to_path_buf(),
        json!({"tools": ["exec"]}),
        json!({"tools": {"exec": {
            "command": "bin/tool",
            "description": "Conflicting plugin tool",
            "parameters": {"type": "object", "properties": {"command": {"type": "string"}}}
        }}}),
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(CountingExecTool {
        calls: calls.clone(),
    });

    let diagnostics = register_plugin_runtime_tools(&mut registry, &[plugin]);
    let output = registry
        .execute("exec", json!({"command": "cargo test"}))
        .into_text();

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(output, "exec-output");
    assert!(!marker_path.exists());
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "tool_name_conflict"
            && diagnostic.plugin_id == "tool-plugin"
            && diagnostic.event.as_deref() == Some("exec")
    }));
}

#[cfg(unix)]
#[test]
fn spec025_plugin_command_dispatcher_routes_and_executes_without_shell_env() {
    let _guard = process_executor_test_guard();
    let tempdir = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("failed to create temporary plugin root: {error}"));
    let bin_dir = tempdir.path().join("bin");
    fs::create_dir(&bin_dir).unwrap_or_else(|error| panic!("failed to create bin dir: {error}"));
    let command_path = bin_dir.join("review");
    fs::write(
        &command_path,
        "#!/bin/sh\nif [ -n \"$HOME\" ]; then printf 'env leaked'; exit 2; fi\ninput=$(cat)\ncase \"$input\" in *'\"args\":\"today\"'*) printf '{\"ok\":true,\"message\":\"plugin command ran\"}' ;; *) printf 'bad stdin'; exit 3 ;; esac\n",
    )
    .unwrap_or_else(|error| panic!("failed to write command script: {error}"));
    make_executable(&command_path);
    let enabled = plugin_with_root(
        "command-plugin",
        tempdir.path().to_path_buf(),
        json!({"commands": ["review", "status"]}),
        json!({"commands": {
            "review": {"command": "bin/review", "description": "Run review"},
            "status": {"command": "bin/review"}
        }}),
    );
    let disabled = plugin_with_state("disabled-command", PluginState::Disabled);
    let mut diagnostics = Vec::new();

    let commands = plugin_runtime_commands(&[enabled, disabled], &mut diagnostics);
    let dispatcher = PluginCommandDispatcher::with_process_gate_input(
        commands,
        plugin_process_gate_input(
            ProcessAdapterKind::PluginCommand,
            "command-plugin",
            "review",
            &PluginExecutableCommand {
                command_path: command_path.clone(),
                args: Vec::new(),
                timeout_ms: 1_000,
            },
        ),
    );
    let execution = dispatcher
        .dispatch_text("/review today")
        .unwrap_or_else(|error| panic!("plugin command dispatch failed: {error:?}"));
    let output = execution.output.into_text();

    assert_eq!(dispatcher.commands().len(), 1);
    assert_eq!(execution.plugin_id, "command-plugin");
    assert_eq!(execution.command_name, "review");
    assert!(output.contains("plugin command ran"), "{output}");
    assert!(dispatcher.dispatch_text("/status").is_err());
    assert!(dispatcher.dispatch_text("/disabled-command").is_err());
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "builtin_command_conflict"
            && diagnostic.event.as_deref() == Some("status")
    }));
}

#[cfg(unix)]
#[test]
fn plugin_command_dispatcher_fails_closed_without_process_gate_context() {
    let _guard = process_executor_test_guard();
    let tempdir = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("failed to create temporary plugin root: {error}"));
    let bin_dir = tempdir.path().join("bin");
    fs::create_dir(&bin_dir).unwrap_or_else(|error| panic!("failed to create bin dir: {error}"));
    let marker_path = tempdir.path().join("plugin-command-ran");
    let command_path = bin_dir.join("review");
    fs::write(
        &command_path,
        format!(
            "#!/bin/sh\nprintf ran > {}\nprintf '{{\"ok\":true,\"message\":\"plugin command ran\"}}'\n",
            shell_quote(marker_path.to_string_lossy().as_ref())
        ),
    )
    .unwrap_or_else(|error| panic!("failed to write command script: {error}"));
    make_executable(&command_path);
    let plugin = plugin_with_root(
        "command-plugin",
        tempdir.path().to_path_buf(),
        json!({"commands": ["review"]}),
        json!({"commands": {
            "review": {"command": "bin/review", "description": "Run review", "timeoutMs": 5000}
        }}),
    );
    let mut diagnostics = Vec::new();
    let dispatcher =
        PluginCommandDispatcher::new(plugin_runtime_commands(&[plugin], &mut diagnostics));

    let execution = dispatcher
        .dispatch_text("/review today")
        .unwrap_or_else(|error| panic!("plugin command dispatch failed: {error:?}"));
    let output = execution.output.into_text();

    assert!(diagnostics.is_empty());
    assert!(output.contains("process gate rejected before spawn"));
    assert!(output.contains("Denied"));
    assert!(!marker_path.exists());
}

#[cfg(unix)]
#[test]
fn plugin_command_dispatcher_uses_live_permission_context() {
    let _guard = process_executor_test_guard();
    let tempdir = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("failed to create temporary plugin root: {error}"));
    let bin_dir = tempdir.path().join("bin");
    fs::create_dir(&bin_dir).unwrap_or_else(|error| panic!("failed to create bin dir: {error}"));
    let command_path = bin_dir.join("review");
    fs::write(
        &command_path,
        "#!/bin/sh\nprintf '{\"ok\":true,\"message\":\"plugin command ran\"}'\n",
    )
    .unwrap_or_else(|error| panic!("failed to write command script: {error}"));
    make_executable(&command_path);
    let plugin = plugin_with_root(
        "command-plugin",
        tempdir.path().to_path_buf(),
        json!({"commands": ["review"]}),
        json!({"commands": {
            "review": {"command": "bin/review", "description": "Run review"}
        }}),
    );
    let mut diagnostics = Vec::new();
    let dispatcher = PluginCommandDispatcher::with_permission_context(
        plugin_runtime_commands(&[plugin], &mut diagnostics),
        PluginProcessPermissionContext {
            permission_mode: PermissionMode::BypassPermissions,
            permission_rules: confirmed_permission_rules("review"),
            inherited_context: Some(InheritedPermissionContext {
                ceiling: PermissionCeilingSnapshot {
                    parent_mode: PermissionMode::BypassPermissions,
                    capability_ceiling: vec![SafetyCapability::ProcExec],
                    approved_scope_refs: vec!["plugin-process".to_owned()],
                    origin: RuntimeBoundaryOrigin::UserTurn,
                },
                requested_mode: PermissionMode::BypassPermissions,
                requested_capabilities: vec![SafetyCapability::ProcExec],
                per_action_evaluation_required: true,
            }),
        },
    );

    let execution = dispatcher
        .dispatch_text("/review today")
        .unwrap_or_else(|error| panic!("plugin command dispatch failed: {error:?}"));
    let output = execution.output.into_text();

    assert!(diagnostics.is_empty());
    assert!(output.contains("plugin command ran"), "{output}");
}

#[cfg(unix)]
#[test]
fn plugin_command_process_closes_stdin_after_payload_write() {
    let _guard = process_executor_test_guard();
    let tempdir = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("failed to create temporary plugin root: {error}"));
    let bin_dir = tempdir.path().join("bin");
    fs::create_dir(&bin_dir).unwrap_or_else(|error| panic!("failed to create bin dir: {error}"));
    let command_path = bin_dir.join("review");
    fs::write(
        &command_path,
        "#!/bin/sh\ncat >/dev/null\nprintf '{\"ok\":true,\"message\":\"stdin-closed\"}'\n",
    )
    .unwrap_or_else(|error| panic!("failed to write command script: {error}"));
    make_executable(&command_path);
    let plugin = plugin_with_root(
        "command-plugin",
        tempdir.path().to_path_buf(),
        json!({"commands": ["review"]}),
        json!({"commands": {
            "review": {"command": "bin/review", "description": "Run review", "timeoutMs": 1000}
        }}),
    );
    let mut diagnostics = Vec::new();
    let dispatcher = PluginCommandDispatcher::with_process_gate_input(
        plugin_runtime_commands(&[plugin], &mut diagnostics),
        plugin_process_gate_input(
            ProcessAdapterKind::PluginCommand,
            "command-plugin",
            "review",
            &PluginExecutableCommand {
                command_path: command_path.clone(),
                args: Vec::new(),
                timeout_ms: 1_000,
            },
        ),
    );

    let execution = dispatcher
        .dispatch_text("/review today")
        .unwrap_or_else(|error| panic!("plugin command dispatch failed: {error:?}"));
    let output = execution.output.into_text();

    assert!(diagnostics.is_empty());
    assert!(output.contains("stdin-closed"));
    assert!(!output.contains("timed out"));
}

#[cfg(unix)]
#[test]
fn plugin_hook_process_concurrent_stdin_consumers_complete_without_timeouts() {
    let _guard = process_executor_test_guard();
    let workers = 2;
    let stdin_payload = format!(
        "{}stdin-eof-sentinel",
        "x".repeat(70 * 1024 - "stdin-eof-sentinel".len())
    );
    let barrier = Arc::new(Barrier::new(workers));
    let mut handles = Vec::with_capacity(workers);

    for index in 0..workers {
        let barrier = barrier.clone();
        let stdin_payload = stdin_payload.clone();
        handles.push(std::thread::spawn(move || {
            let tempdir = tempfile::tempdir()
                .unwrap_or_else(|error| panic!("failed to create temporary plugin root: {error}"));
            let bin_dir = tempdir.path().join("bin");
            fs::create_dir(&bin_dir)
                .unwrap_or_else(|error| panic!("failed to create bin dir: {error}"));
            let hook_path = bin_dir.join("hook");
            fs::write(
                &hook_path,
                "#!/bin/sh\nIFS= read -r input || true\ncase \"$input\" in\n  *stdin-eof-sentinel*) printf '{\"diagnostic\":{\"message\":\"stdin-closed\"}}' ;;\n  *) printf 'missing stdin-eof-sentinel\\n' >&2; exit 7 ;;\nesac\n",
            )
            .unwrap_or_else(|error| panic!("failed to write hook script: {error}"));
            make_executable(&hook_path);
            let command = PluginExecutableCommand {
                command_path: hook_path.clone(),
                args: Vec::new(),
                timeout_ms: 1_000,
            };
            let invocation = PluginHookCommandInvocation {
                plugin_id: format!("stdin-consumer-{index}"),
                event: PluginHookEvent::LlmAfter,
                event_name: "llm:after".to_owned(),
                command: command.clone(),
                working_dir: bin_dir,
                stdin_payload: json!({"payload": stdin_payload}),
            };
            barrier.wait();

            ProcessPluginHookCommandExecutor::with_process_gate_input(plugin_process_gate_input(
                ProcessAdapterKind::PluginHook,
                &format!("stdin-consumer-{index}"),
                "llm:after",
                &command,
            ))
            .execute(&invocation)
        }));
    }

    for handle in handles {
        let result = handle
            .join()
            .unwrap_or_else(|error| panic!("worker thread panicked: {error:?}"));
        assert_eq!(
            result,
            PluginHookCallbackResult::Output(json!({
                "diagnostic": {"message": "stdin-closed"}
            }))
        );
    }
}

#[cfg(unix)]
#[test]
fn spec025_agent_loop_executes_enabled_plugin_commands_without_provider_call() {
    let _guard = process_executor_test_guard();
    let tempdir = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("failed to create temporary plugin root: {error}"));
    let workspace = tempdir.path().join("workspace");
    fs::create_dir(&workspace)
        .unwrap_or_else(|error| panic!("failed to create workspace: {error}"));
    let bin_dir = tempdir.path().join("bin");
    fs::create_dir(&bin_dir).unwrap_or_else(|error| panic!("failed to create bin dir: {error}"));
    let command_path = bin_dir.join("review");
    fs::write(
        &command_path,
        "#!/bin/sh\nif [ -n \"$HOME\" ]; then printf 'env leaked'; exit 2; fi\ninput=$(cat)\ncase \"$input\" in *'\"args\":\"today\"'*) printf '{\"ok\":true,\"message\":\"agent loop plugin command ran\"}' ;; *) printf 'bad stdin'; exit 3 ;; esac\n",
    )
    .unwrap_or_else(|error| panic!("failed to write command script: {error}"));
    make_executable(&command_path);
    let plugin = plugin_with_root(
        "agent-command-plugin",
        tempdir.path().to_path_buf(),
        json!({"commands": ["review", "status"]}),
        json!({"commands": {
            "review": {"command": "bin/review", "description": "Run review"},
            "status": {"command": "bin/review"}
        }}),
    );
    let snapshot = build_plugin_runtime_snapshot(&[plugin]);
    let registry = ToolRegistry::new();
    let client = PanicProviderClient;
    let mut loop_runtime = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(&workspace)
            .unwrap_or_else(|error| panic!("failed to create session manager: {error}")),
        ContextBuilder::new(&workspace),
        &registry,
        &client,
        AgentLoopConfig::new(&workspace, "test-model"),
    )
    .with_plugin_command_dispatcher(PluginCommandDispatcher::with_process_gate_input(
        snapshot.commands,
        plugin_process_gate_input(
            ProcessAdapterKind::PluginCommand,
            "agent-command-plugin",
            "review",
            &PluginExecutableCommand {
                command_path: command_path.clone(),
                args: Vec::new(),
                timeout_ms: 1_000,
            },
        ),
    ));

    let result = loop_runtime
        .process_direct("/review today", Some("plugin-command"))
        .unwrap_or_else(|error| panic!("plugin command turn failed: {error}"));
    let output = result.final_content.unwrap_or_default();

    assert_eq!(result.command, Some(AgentLoopCommandResult::PluginCommand));
    assert!(output.contains("agent loop plugin command ran"));
    let status = loop_runtime
        .process_direct("/status", Some("plugin-command"))
        .unwrap_or_else(|error| panic!("builtin status command failed: {error}"));
    assert_eq!(status.command, Some(AgentLoopCommandResult::Status));
}

#[cfg(unix)]
#[test]
fn spec025_agent_loop_does_not_run_plugin_command_while_ask_user_is_pending() {
    let _guard = process_executor_test_guard();
    let tempdir = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("failed to create temporary plugin root: {error}"));
    let workspace = tempdir.path().join("workspace");
    fs::create_dir(&workspace)
        .unwrap_or_else(|error| panic!("failed to create workspace: {error}"));
    let bin_dir = tempdir.path().join("bin");
    fs::create_dir(&bin_dir).unwrap_or_else(|error| panic!("failed to create bin dir: {error}"));
    let marker_path = tempdir.path().join("plugin-command-ran");
    let command_path = bin_dir.join("review");
    fs::write(
        &command_path,
        format!(
            "#!/bin/sh\nprintf ran > {}\nprintf '{{\"ok\":true,\"message\":\"plugin command ran\"}}'\n",
            shell_quote(marker_path.to_string_lossy().as_ref())
        ),
    )
    .unwrap_or_else(|error| panic!("failed to write command script: {error}"));
    make_executable(&command_path);
    let plugin = plugin_with_root(
        "agent-command-plugin",
        tempdir.path().to_path_buf(),
        json!({"commands": ["review"]}),
        json!({"commands": {
            "review": {"command": "bin/review", "description": "Run review"}
        }}),
    );
    let snapshot = build_plugin_runtime_snapshot(&[plugin]);
    let mut registry = ToolRegistry::new();
    registry.register(AskUserTool::new());
    let client = QueueProviderClient::new(vec![
        Ok(LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "ask-1",
                "ask_user",
                Map::from_iter([
                    ("question".to_owned(), json!("Continue?")),
                    ("options".to_owned(), json!(["Yes", "No"])),
                ]),
            )],
            ..LlmResponse::default()
        }),
        Ok(LlmResponse {
            content: Some("resumed after plugin-looking answer".to_owned()),
            ..LlmResponse::default()
        }),
    ]);
    let mut loop_runtime = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(&workspace)
            .unwrap_or_else(|error| panic!("failed to create session manager: {error}")),
        ContextBuilder::new(&workspace),
        &registry,
        &client,
        AgentLoopConfig::new(&workspace, "test-model"),
    )
    .with_plugin_command_dispatcher(PluginCommandDispatcher::new(snapshot.commands));

    let first = loop_runtime
        .process_direct("start", Some("plugin-command-pending-ask"))
        .unwrap_or_else(|error| panic!("ask_user turn failed: {error}"));
    let second = loop_runtime
        .process_direct("/review today", Some("plugin-command-pending-ask"))
        .unwrap_or_else(|error| panic!("ask_user resume turn failed: {error}"));

    assert_eq!(first.stop_reason, "ask_user");
    assert_eq!(first.ask_user_options, ["Yes", "No"]);
    assert_ne!(second.command, Some(AgentLoopCommandResult::PluginCommand));
    assert_eq!(
        second.final_content.as_deref(),
        Some("resumed after plugin-looking answer")
    );
    assert!(!marker_path.exists());
}

#[cfg(unix)]
#[test]
fn spec025_agent_loop_does_not_run_plugin_command_while_permission_approval_is_pending() {
    let _guard = process_executor_test_guard();
    let tempdir = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("failed to create temporary plugin root: {error}"));
    let workspace = tempdir.path().join("workspace");
    fs::create_dir(&workspace)
        .unwrap_or_else(|error| panic!("failed to create workspace: {error}"));
    let bin_dir = tempdir.path().join("bin");
    fs::create_dir(&bin_dir).unwrap_or_else(|error| panic!("failed to create bin dir: {error}"));
    let marker_path = tempdir.path().join("plugin-command-ran");
    let command_path = bin_dir.join("review");
    fs::write(
        &command_path,
        format!(
            "#!/bin/sh\nprintf ran > {}\nprintf '{{\"ok\":true,\"message\":\"plugin command ran\"}}'\n",
            shell_quote(marker_path.to_string_lossy().as_ref())
        ),
    )
    .unwrap_or_else(|error| panic!("failed to write command script: {error}"));
    make_executable(&command_path);
    let plugin = plugin_with_root(
        "agent-command-plugin",
        tempdir.path().to_path_buf(),
        json!({"commands": ["review"]}),
        json!({"commands": {
            "review": {"command": "bin/review", "description": "Run review"}
        }}),
    );
    let snapshot = build_plugin_runtime_snapshot(&[plugin]);
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(CountingExecTool {
        calls: calls.clone(),
    });
    let client = QueueProviderClient::new(vec![Ok(LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![ToolCallRequest::new(
            "exec-1",
            "exec",
            Map::from_iter([("command".to_owned(), json!("cargo test"))]),
        )],
        ..LlmResponse::default()
    })]);
    let mut config = AgentLoopConfig::new(&workspace, "test-model");
    config.permission_mode_snapshot = PermissionModeSnapshot {
        mode: PermissionMode::Auto,
        source: Some("test".to_owned()),
        scope_ref: None,
    };
    config.permission_interactive = true;
    let mut loop_runtime = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(&workspace)
            .unwrap_or_else(|error| panic!("failed to create session manager: {error}")),
        ContextBuilder::new(&workspace),
        &registry,
        &client,
        config,
    )
    .with_plugin_command_dispatcher(PluginCommandDispatcher::new(snapshot.commands));

    let first = loop_runtime
        .process_direct("start", Some("plugin-command-pending-approval"))
        .unwrap_or_else(|error| panic!("approval turn failed: {error}"));
    let second = loop_runtime
        .process_direct("/review today", Some("plugin-command-pending-approval"))
        .unwrap_or_else(|error| panic!("pending approval reply failed: {error}"));

    assert_eq!(first.stop_reason, "ask_user");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_ne!(second.command, Some(AgentLoopCommandResult::PluginCommand));
    assert_eq!(second.stop_reason, "permission_approval_pending");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(!marker_path.exists());
}

#[test]
fn spec025_s3_descriptor_projection_stays_non_executable_after_live_hook_adapter() {
    let plugin = enabled_plugin(
        "descriptor-only",
        json!({"hooks": ["llm:after"]}),
        json!({"hooks": {"llm:after": {"command": "bin/observe"}}}),
    );
    let projection = build_plugin_surface_projection(std::slice::from_ref(&plugin));
    let snapshot = build_plugin_runtime_snapshot(&[plugin]);
    let _hook = PluginRuntimeHookAgentHook::new(snapshot);

    assert_eq!(projection.hooks.len(), 1);
    assert!(!projection.hooks[0].execution_enabled);
}

#[derive(Clone)]
struct FakeExecutor {
    result: PluginHookCallbackResult,
    invocations: Arc<Mutex<Vec<PluginHookCommandInvocation>>>,
}

struct PanicTool;

struct CountingExecTool {
    calls: Arc<AtomicUsize>,
}

impl Tool for PanicTool {
    fn name(&self) -> &str {
        "panic_tool"
    }

    fn description(&self) -> &str {
        "Panics if executed."
    }

    fn parameters(&self) -> serde_json::Value {
        ToolParameters::new().to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        panic!("panic_tool should have been blocked before execution")
    }
}

impl Tool for CountingExecTool {
    fn name(&self) -> &str {
        "exec"
    }

    fn description(&self) -> &str {
        "Count exec attempts."
    }

    fn parameters(&self) -> serde_json::Value {
        ToolParameters::new()
            .property("command", shacs_core::tools::StringSchema::new("Command"))
            .required(["command"])
            .to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        "exec-output".into()
    }
}

struct QueueProviderClient {
    responses: Mutex<Vec<Result<LlmResponse, ProviderError>>>,
}

struct PanicProviderClient;

impl ProviderClient for PanicProviderClient {
    fn chat(&self, _request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        panic!("provider should not be called for plugin commands")
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        _on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

impl QueueProviderClient {
    fn new(mut responses: Vec<Result<LlmResponse, ProviderError>>) -> Self {
        responses.reverse();
        Self {
            responses: Mutex::new(responses),
        }
    }
}

impl ProviderClient for QueueProviderClient {
    fn chat(&self, _request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        match self.responses.lock() {
            Ok(mut responses) => responses
                .pop()
                .unwrap_or_else(|| Ok(LlmResponse::default())),
            Err(error) => panic!("provider response lock poisoned: {error}"),
        }
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        _on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

impl FakeExecutor {
    fn new(result: PluginHookCallbackResult) -> Self {
        Self {
            result,
            invocations: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn count(&self) -> usize {
        self.invocations().len()
    }

    fn invocations(&self) -> Vec<PluginHookCommandInvocation> {
        match self.invocations.lock() {
            Ok(invocations) => invocations.clone(),
            Err(error) => panic!("fake executor invocation lock poisoned: {error}"),
        }
    }
}

impl PluginHookCommandExecutor for FakeExecutor {
    fn execute(&self, invocation: &PluginHookCommandInvocation) -> PluginHookCallbackResult {
        match self.invocations.lock() {
            Ok(mut invocations) => invocations.push(invocation.clone()),
            Err(error) => panic!("fake executor invocation lock poisoned: {error}"),
        }
        self.result.clone()
    }
}

fn recording_sink(
    summaries: Arc<Mutex<Vec<shacs_core::runtime::PluginHookDispatchSummary>>>,
) -> shacs_core::runtime::PluginHookDispatchSink {
    Arc::new(move |summary| match summaries.lock() {
        Ok(mut summaries) => summaries.push(summary),
        Err(error) => panic!("summary lock poisoned: {error}"),
    })
}

fn recorded_summaries_len(
    summaries: &Arc<Mutex<Vec<shacs_core::runtime::PluginHookDispatchSummary>>>,
) -> usize {
    match summaries.lock() {
        Ok(summaries) => summaries.len(),
        Err(error) => panic!("summary lock poisoned: {error}"),
    }
}

fn recorded_summary(
    summaries: &Arc<Mutex<Vec<shacs_core::runtime::PluginHookDispatchSummary>>>,
    index: usize,
) -> shacs_core::runtime::PluginHookDispatchSummary {
    match summaries.lock() {
        Ok(summaries) => summaries
            .get(index)
            .cloned()
            .unwrap_or_else(|| panic!("missing summary at index {index}")),
        Err(error) => panic!("summary lock poisoned: {error}"),
    }
}

fn process_executor_test_guard() -> MutexGuard<'static, ()> {
    match PROCESS_EXECUTOR_TEST_LOCK.lock() {
        Ok(guard) => guard,
        Err(error) => panic!("process executor test lock poisoned: {error}"),
    }
}

fn process_snapshot(
    plugin_id: &str,
    root: PathBuf,
    hook_path: PathBuf,
    timeout_ms: u64,
) -> PluginRuntimeSnapshot {
    PluginRuntimeSnapshot {
        plugins: vec![PluginRuntimePlugin {
            id: plugin_id.to_owned(),
            root,
            manifest_digest: None,
            source: PluginManifestSource::UserData,
            hooks: vec![PluginRuntimeHook {
                plugin_id: plugin_id.to_owned(),
                event: PluginHookEvent::LlmAfter,
                event_name: "llm:after".to_owned(),
                command: PluginExecutableCommand {
                    command_path: hook_path,
                    args: Vec::new(),
                    timeout_ms,
                },
            }],
        }],
        commands: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn plugin_tool_context() -> ToolExecutionContext {
    ToolExecutionContext {
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::BypassPermissions,
            source: Some("plugin-runtime-test".to_owned()),
            scope_ref: Some("plugin-process".to_owned()),
        },
        permission_rule_input: confirmed_permission_rules("plugin-tool"),
        permission_ceiling_snapshot: Some(PermissionCeilingSnapshot {
            parent_mode: PermissionMode::BypassPermissions,
            capability_ceiling: vec![SafetyCapability::ProcExec],
            approved_scope_refs: vec!["plugin-process".to_owned()],
            origin: RuntimeBoundaryOrigin::UserTurn,
        }),
        ..ToolExecutionContext::default()
    }
}

fn plugin_process_gate_input(
    adapter: ProcessAdapterKind,
    plugin_id: &str,
    process_name: &str,
    command: &PluginExecutableCommand,
) -> ProcessGateInput {
    let action = PermissionedAction {
        action_id: format!("test-plugin-process:{plugin_id}:{process_name}"),
        provider_tool_call_id: None,
        session_id: "test-plugin-session".to_owned(),
        turn_id: "test-plugin-turn".to_owned(),
        tool_name: format!("plugin:{plugin_id}:{process_name}"),
        capabilities: vec![SafetyCapability::ProcExec],
        target_refs: Vec::new(),
        action_digest: format!("test-action:{plugin_id}:{process_name}"),
        argument_digest: format!("test-arguments:{plugin_id}:{process_name}"),
        snapshot_digest: format!("test-snapshot:{plugin_id}:{process_name}"),
        policy_safety_snapshot_ref: Some(test_policy_ref()),
        origin: PermissionedActionOrigin::UserTurn,
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::BypassPermissions,
            source: Some("plugin-runtime-test".to_owned()),
            scope_ref: Some("plugin-process".to_owned()),
        },
        containment_snapshot: None,
        intent_snapshot: None,
        redacted_arguments: json!({
            "plugin_id": plugin_id,
            "process": process_name,
        }),
        secret_ref_evidence: Vec::new(),
        normalization_state: shacs_core::runtime::ActionNormalizationState::Ready,
        normalization_errors: Vec::new(),
    };
    let envelope = ProcessExecutionEnvelope::try_from_input(ProcessExecutionEnvelopeInput {
        identity: ProcessIdentity::new(
            format!("plugin:{plugin_id}:{process_name}"),
            "test-plugin-session",
            "test-plugin-turn",
        ),
        adapter,
        action,
        required_secret_ref_count: 0,
        redacted_command: ProcessRedactedCommand {
            command_family: command
                .command_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("plugin-process")
                .to_owned(),
            redacted_summary: format!("plugin process {plugin_id}:{process_name}"),
            redacted_targets: Vec::new(),
        },
    })
    .unwrap_or_else(|error| panic!("failed to build plugin process envelope: {error}"));
    let permission_rules = confirmed_permission_rules(
        command
            .command_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("plugin-process"),
    );
    let inherited_context = Some(InheritedPermissionContext {
        ceiling: PermissionCeilingSnapshot {
            parent_mode: PermissionMode::BypassPermissions,
            capability_ceiling: vec![SafetyCapability::ProcExec],
            approved_scope_refs: vec!["plugin-process".to_owned()],
            origin: RuntimeBoundaryOrigin::UserTurn,
        },
        requested_mode: PermissionMode::BypassPermissions,
        requested_capabilities: vec![SafetyCapability::ProcExec],
        per_action_evaluation_required: true,
    });
    let containment_proof = containment_permission_proof_for_process_gate(
        &envelope,
        &permission_rules,
        inherited_context.as_ref(),
        1,
    )
    .expect("plugin process containment proof fixture should be valid");
    ProcessGateInput {
        envelope,
        permission_rules,
        inherited_context,
        evaluator: None,
        approval: None,
        containment_proof: ProcessContainmentProofCandidate::Proof(Box::new(containment_proof)),
        interactive: false,
        terminal_precondition: ProcessGateTerminalPrecondition::Ready,
        now_unix_ms: 1,
    }
}

fn confirmed_permission_rules(command_family: &str) -> PermissionRuleInput {
    PermissionRuleInput {
        containment: DockerContainmentSnapshot {
            contained: Some(true),
            runtime: ContainerRuntimeKind::Docker,
            root_user: Some(false),
            privileged: Some(false),
            host_mounts_summary: vec!["plugin-runtime-test".to_owned()],
            network_mode: ContainerNetworkMode::Bridge,
            digest: Some("plugin-runtime-test".to_owned()),
            summary: Some("test supplied non-privileged containment".to_owned()),
        },
        protected_targets: Vec::new(),
        proc_exec_summary: Some(ProcExecSummary {
            command_family: command_family.to_owned(),
            target_refs: Vec::new(),
            destructive: false,
            network: false,
            secret_exposure: false,
            summary_available: true,
        }),
    }
}

fn test_policy_ref() -> PolicySafetySnapshotRef {
    PolicySafetySnapshotRef {
        schema_id: PolicySafetySnapshotSchemaId::V1,
        snapshot_id: PolicySafetySnapshotId("plugin-runtime-test".to_owned()),
        policy_safety_digest: PolicySafetyDigest(
            "3333333333333333333333333333333333333333333333333333333333333333".to_owned(),
        ),
        created_at_unix_ms: 0,
        expires_at_unix_ms: None,
        redacted_summary: RedactedPolicySafetySummary {
            permission_mode: "bypass_permissions".to_owned(),
            capability_count: 1,
            containment_digest: Some("plugin-runtime-test".to_owned()),
            source_ref_count: 1,
            provenance_ref_count: 1,
        },
    }
}

#[cfg(unix)]
fn make_executable(path: &std::path::Path) {
    let mut permissions = fs::metadata(path)
        .unwrap_or_else(|error| panic!("failed to read hook metadata: {error}"))
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
        .unwrap_or_else(|error| panic!("failed to make hook executable: {error}"));
}

#[cfg(unix)]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn plugin_with_state(id: &str, state: PluginState) -> DiscoveredPlugin {
    let mut plugin = enabled_plugin(
        id,
        json!({"hooks": ["runtime:start"]}),
        json!({"hooks": {"runtime:start": {"command": "bin/hook"}}}),
    );
    plugin.state = state;
    if state == PluginState::Blocked {
        plugin.block_reasons = vec![PluginBlockReason::UntrustedWorkspace];
    }
    plugin
}

fn enabled_plugin(
    id: &str,
    surfaces: serde_json::Value,
    entrypoints: serde_json::Value,
) -> DiscoveredPlugin {
    DiscoveredPlugin {
        id: id.to_owned(),
        state: PluginState::Enabled,
        source: PluginManifestSource::UserData,
        root: PathBuf::from(format!("/tmp/{id}")),
        manifest_path: PathBuf::from(format!("/tmp/{id}/plugin.json")),
        digest: Some(format!("sha256:{id}")),
        manifest: Some(PluginManifest {
            schema_version: 1,
            name: id.to_owned(),
            version: "0.1.0".to_owned(),
            description: None,
            surfaces,
            requires_env: Vec::new(),
            requires_config: Vec::new(),
            permissions: json!({}),
            entrypoints,
            assets: json!({}),
        }),
        missing_env: Vec::new(),
        missing_config: Vec::new(),
        block_reasons: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn plugin_with_root(
    id: &str,
    root: PathBuf,
    surfaces: serde_json::Value,
    entrypoints: serde_json::Value,
) -> DiscoveredPlugin {
    let mut plugin = enabled_plugin(id, surfaces, entrypoints);
    plugin.root = root.clone();
    plugin.manifest_path = root.join("plugin.json");
    plugin
}
