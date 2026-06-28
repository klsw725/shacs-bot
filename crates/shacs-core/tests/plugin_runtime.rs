use serde_json::json;
use shacs_core::runtime::{
    build_plugin_runtime_snapshot, build_plugin_surface_projection, AgentHook, AgentHookContext,
    DiscoveredPlugin, PluginBlockReason, PluginExecutableCommand, PluginHookCallbackResult,
    PluginHookCommandExecutor, PluginHookCommandInvocation, PluginHookDispatchMode,
    PluginHookDispatchStatus, PluginHookEvent, PluginManifest, PluginManifestSource,
    PluginRuntimeHook, PluginRuntimeHookAgentHook, PluginRuntimePlugin, PluginRuntimeSnapshot,
    PluginState, ProcessPluginHookCommandExecutor, RuntimeToolCall,
};
use shacs_providers::LlmResponse;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

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
                    command_path: hook_path,
                    args: Vec::new(),
                    timeout_ms: 1_000,
                },
            }],
        }],
        diagnostics: Vec::new(),
    };
    let hook = PluginRuntimeHookAgentHook::with_executor(
        snapshot,
        PluginHookDispatchMode::LiveDiagnostics,
        Arc::new(ProcessPluginHookCommandExecutor),
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
    let snapshot = process_snapshot("env", tempdir.path().to_path_buf(), hook_path, 1_000);
    let hook = PluginRuntimeHookAgentHook::with_executor(
        snapshot,
        PluginHookDispatchMode::LiveDiagnostics,
        Arc::new(ProcessPluginHookCommandExecutor),
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
    let snapshot = process_snapshot("timeout", tempdir.path().to_path_buf(), hook_path, 50);
    let hook = PluginRuntimeHookAgentHook::with_executor(
        snapshot,
        PluginHookDispatchMode::LiveDiagnostics,
        Arc::new(ProcessPluginHookCommandExecutor),
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
        hook_path,
        1_000,
    );
    let hook = PluginRuntimeHookAgentHook::with_executor(
        snapshot,
        PluginHookDispatchMode::LiveDiagnostics,
        Arc::new(ProcessPluginHookCommandExecutor),
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
            command_path: hook_path,
            args: Vec::new(),
            timeout_ms: 50,
        },
        working_dir: bin_dir,
        stdin_payload: json!({}),
    };

    let started = std::time::Instant::now();
    let result = ProcessPluginHookCommandExecutor.execute(&invocation);

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
            command_path: hook_path,
            args: Vec::new(),
            timeout_ms: 50,
        },
        working_dir: bin_dir,
        stdin_payload: json!({"payload": "x".repeat(2 * 1024 * 1024)}),
    };

    let started = std::time::Instant::now();
    let result = ProcessPluginHookCommandExecutor.execute(&invocation);

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
        diagnostics: Vec::new(),
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
