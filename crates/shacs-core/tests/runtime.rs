// allow: SIZE_OK — preexisting runtime integration suite; Spec034 diff is one test-fixture deadline field hook
use serde_json::{json, Value};
use shacs_config::AutoApprovalConfig;
#[cfg(unix)]
use shacs_core::runtime::trusted_runtime::{
    LocalSpec030ProjectionProvider, Spec030FactStore, WorkspaceTrustObservation,
};
use shacs_core::runtime::{
    dispatch_bridge_tool_call, dispatch_bridge_tool_calls, ActionNormalizationError,
    ActionNormalizationState, ApprovalActor, ApprovalCacheEntry, ApprovalCorrelationError,
    ApprovalDecision, ApprovalDecisionKind, CancellationToken, ContainerNetworkMode,
    ContainerRuntimeKind, ContainmentSnapshotRef, DockerContainmentSnapshot,
    PermissionCeilingSnapshot, PermissionMode, PermissionModeSnapshot, PermissionRuleInput,
    PermissionedActionOrigin, ProcExecSummary, RuntimeBoundaryOrigin, RuntimeContextTools,
    RuntimeInterrupt, RuntimeToolCall, RuntimeToolExecutor, SafetyCapability, ToolExecutionContext,
};
use shacs_core::tools::{
    AskUserTool, CronTool, DeferredToolCatalog, DeferredToolCatalogEntry, ExecTool, JsonMap,
    MessageTool, OutboundMessage, SchemaFragment, SpawnRequest, SpawnTool, StringSchema, Tool,
    ToolParameters, ToolRegistry, ToolResult,
};
use shacs_cron::InMemoryCronService;
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

struct RepeatTool;

impl Tool for RepeatTool {
    fn name(&self) -> &str {
        "repeat"
    }

    fn description(&self) -> &str {
        "Repeat text."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("text", StringSchema::new("Text"))
            .property(
                "times",
                shacs_core::tools::IntegerSchema::new("Repeat count").minimum(1),
            )
            .required(["text", "times"])
            .to_json_schema()
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let text = params
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let times = params.get("times").and_then(Value::as_u64).unwrap_or(1);
        ToolResult::Text(text.repeat(times as usize))
    }
}

struct CountingTool {
    calls: Arc<AtomicUsize>,
}

impl Tool for CountingTool {
    fn name(&self) -> &str {
        "count"
    }

    fn description(&self) -> &str {
        "Count executions."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new().to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        "counted".into()
    }
}

struct WriteFileCountingTool {
    calls: Arc<AtomicUsize>,
}

impl Tool for WriteFileCountingTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write a file."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("path", StringSchema::new("Path"))
            .property("content", StringSchema::new("Content"))
            .required(["path", "content"])
            .to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        "written".into()
    }
}

struct ProcExecCountingTool {
    calls: Arc<AtomicUsize>,
}

impl Tool for ProcExecCountingTool {
    fn name(&self) -> &str {
        "exec"
    }

    fn description(&self) -> &str {
        "Count proc exec attempts."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("command", StringSchema::new("Command"))
            .required(["command"])
            .to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        "executed".into()
    }
}

struct JsonTool;

impl Tool for JsonTool {
    fn name(&self) -> &str {
        "mcp_json_tool"
    }

    fn description(&self) -> &str {
        "Return JSON."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new().to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        ToolResult::Json(json!({ "ok": true }))
    }
}

struct ErrorTool;

impl Tool for ErrorTool {
    fn name(&self) -> &str {
        "mcp_error_tool"
    }

    fn description(&self) -> &str {
        "Return a tool error."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new().to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        "Error: simulated failure".into()
    }
}

struct DelayTool {
    name: &'static str,
    read_only: bool,
    active: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
}

struct NamedRepeatTool(&'static str);

struct NamedCountingTool {
    name: &'static str,
    calls: Arc<AtomicUsize>,
}

impl Tool for NamedRepeatTool {
    fn name(&self) -> &str {
        self.0
    }

    fn description(&self) -> &str {
        "Repeat text."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("text", StringSchema::new("Text"))
            .property(
                "times",
                shacs_core::tools::IntegerSchema::new("Repeat count").minimum(1),
            )
            .required(["text", "times"])
            .to_json_schema()
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let text = params
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let times = params.get("times").and_then(Value::as_u64).unwrap_or(1);
        ToolResult::Text(text.repeat(times as usize))
    }
}

impl Tool for NamedCountingTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        "Count executions."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new().to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        "counted".into()
    }
}

struct DeferredAskTool;

impl Tool for DeferredAskTool {
    fn name(&self) -> &str {
        "mcp_ask_user"
    }

    fn description(&self) -> &str {
        "Ask the user from a deferred tool."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("question", StringSchema::new("Question"))
            .required(["question"])
            .to_json_schema()
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        ToolResult::AskUserInterrupt {
            question: params
                .get("question")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            options: vec!["Yes".to_owned(), "No".to_owned()],
        }
    }
}

impl DelayTool {
    fn new(
        name: &'static str,
        read_only: bool,
        active: Arc<AtomicUsize>,
        max_active: Arc<AtomicUsize>,
        calls: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            name,
            read_only,
            active,
            max_active,
            calls,
        }
    }
}

impl Tool for DelayTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        "Delay and record concurrency."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new().to_json_schema()
    }

    fn read_only(&self) -> bool {
        self.read_only
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        record_max(&self.max_active, active);
        thread::sleep(Duration::from_millis(25));
        self.active.fetch_sub(1, Ordering::SeqCst);
        self.name.into()
    }
}

fn confirmed_containment_ref() -> ContainmentSnapshotRef {
    ContainmentSnapshotRef {
        contained: Some(true),
        backend: None,
        digest: Some("test-contained".to_owned()),
        summary: Some("non-privileged test containment".to_owned()),
    }
}

fn confirmed_containment() -> DockerContainmentSnapshot {
    DockerContainmentSnapshot {
        contained: Some(true),
        runtime: ContainerRuntimeKind::Docker,
        root_user: Some(false),
        privileged: Some(false),
        host_mounts_summary: Vec::new(),
        network_mode: ContainerNetworkMode::None,
        digest: Some("test-contained".to_owned()),
        summary: Some("non-privileged test containment".to_owned()),
    }
}

fn safe_proc_exec_rule_input() -> PermissionRuleInput {
    PermissionRuleInput {
        containment: confirmed_containment(),
        protected_targets: Vec::new(),
        proc_exec_summary: Some(ProcExecSummary {
            command_family: "test".to_owned(),
            target_refs: Vec::new(),
            destructive: false,
            network: false,
            secret_exposure: false,
            summary_available: true,
        }),
    }
}

fn safe_mcp_tool_context() -> ToolExecutionContext {
    ToolExecutionContext {
        containment_snapshot: Some(confirmed_containment_ref()),
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::BypassPermissions,
            source: Some("runtime_test".to_owned()),
            scope_ref: None,
        },
        permission_rule_input: safe_proc_exec_rule_input(),
        ..ToolExecutionContext::default()
    }
}

fn permissive_local_tool_context() -> ToolExecutionContext {
    ToolExecutionContext {
        containment_snapshot: Some(confirmed_containment_ref()),
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::BypassPermissions,
            source: Some("runtime_test".to_owned()),
            scope_ref: None,
        },
        permission_rule_input: safe_proc_exec_rule_input(),
        ..ToolExecutionContext::default()
    }
}

fn restrictive_read_only_ceiling() -> PermissionCeilingSnapshot {
    PermissionCeilingSnapshot {
        parent_mode: PermissionMode::Default,
        capability_ceiling: vec![SafetyCapability::FsRead],
        approved_scope_refs: Vec::new(),
        origin: RuntimeBoundaryOrigin::Subagent {
            subagent_id: Some("child-1".to_owned()),
        },
    }
}

fn record_max(target: &AtomicUsize, value: usize) {
    let mut observed = target.load(Ordering::SeqCst);
    while observed < value {
        match target.compare_exchange(observed, value, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return,
            Err(next) => observed = next,
        }
    }
}

#[test]
fn runtime_denies_direct_proc_exec_without_executing_tool() -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let executor = RuntimeToolExecutor::new(&registry);
    let context = ToolExecutionContext {
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::Auto,
            source: Some("test".to_owned()),
            scope_ref: None,
        },
        ..ToolExecutionContext::default()
    };

    let report = executor.execute_tool_calls(
        vec![RuntimeToolCall::new(
            "exec-call",
            "exec",
            json!({ "command": "cargo test" }),
        )],
        &context,
    );

    let message = report
        .messages
        .first()
        .ok_or("missing permission message")?;
    if calls.load(Ordering::SeqCst) != 0
        || report.interrupt.is_some()
        || message.tool_call_id != "exec-call"
        || message.name != "exec"
        || report
            .permissioned_actions
            .first()
            .is_none_or(|action| action.tool_name != "exec" || action.action_digest.is_empty())
    {
        return Err(format!(
            "direct proc_exec should be blocked before execution: report={report:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn runtime_asks_before_interactive_proc_exec_without_executing_tool() -> Result<(), Box<dyn Error>>
{
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let executor = RuntimeToolExecutor::new(&registry);
    let context = ToolExecutionContext {
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::Auto,
            source: Some("test".to_owned()),
            scope_ref: Some("workspace".to_owned()),
        },
        permission_auto_approval: AutoApprovalConfig {
            enabled: true,
            ..AutoApprovalConfig::default()
        },
        permission_ceiling_snapshot: Some(PermissionCeilingSnapshot {
            parent_mode: PermissionMode::Auto,
            capability_ceiling: vec![SafetyCapability::ProcExec],
            approved_scope_refs: vec!["workspace".to_owned(), "".to_owned()],
            origin: RuntimeBoundaryOrigin::UserTurn,
        }),
        permission_interactive: true,
        ..ToolExecutionContext::default()
    };

    let report = executor.execute_tool_calls(
        vec![RuntimeToolCall::new(
            "exec-call",
            "exec",
            json!({ "command": "cargo test" }),
        )],
        &context,
    );

    match &report.interrupt {
        Some(RuntimeInterrupt::PermissionApproval {
            approval_request_id,
            approval_request,
            tool_call,
            options,
            ..
        }) if calls.load(Ordering::SeqCst) == 0
            && approval_request_id.starts_with("approval_")
            && approval_request.approval_request_id.as_str() == approval_request_id.as_str()
            && approval_request.requested_scope == "cli:direct"
            && !approval_request.action_digest.is_empty()
            && !approval_request.snapshot_digest.is_empty()
            && approval_request.allowed_decisions.as_slice()
                == [
                    ApprovalDecisionKind::Approved,
                    ApprovalDecisionKind::Denied,
                    ApprovalDecisionKind::ApprovedForSession,
                    ApprovalDecisionKind::ApprovedForProject,
                    ApprovalDecisionKind::DeniedForSession,
                    ApprovalDecisionKind::DeniedForProject,
                ]
            && tool_call.id == "exec-call"
            && tool_call.name == "exec"
            && options.as_slice()
                == [
                    "approve",
                    "deny",
                    "approve_session",
                    "approve_project",
                    "deny_session",
                    "deny_project",
                ] => Ok(()),
        other => Err(format!(
            "interactive proc_exec should ask before execution: interrupt={other:?} report={report:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into()),
    }
}

#[test]
fn runtime_auto_approval_executes_safe_workspace_edit_without_evaluator(
) -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(WriteFileCountingTool {
        calls: calls.clone(),
    });
    let executor = RuntimeToolExecutor::new(&registry);
    let context = ToolExecutionContext {
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::Auto,
            source: Some("test".to_owned()),
            scope_ref: None,
        },
        permission_auto_approval: AutoApprovalConfig {
            enabled: true,
            allow_workspace_edits: true,
            ..AutoApprovalConfig::default()
        },
        permission_interactive: true,
        ..ToolExecutionContext::default()
    };

    let report = executor.execute_tool_calls(
        vec![RuntimeToolCall::new(
            "write-call",
            "write_file",
            json!({ "path": "src/lib.rs", "content": "ok" }),
        )],
        &context,
    );

    if calls.load(Ordering::SeqCst) != 1
        || report.interrupt.is_some()
        || report.messages.len() != 1
        || report.messages[0].content != "written"
    {
        return Err(format!(
            "auto-approved workspace edit did not execute: report={report:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn runtime_auto_approval_executes_guarded_web_tools_without_evaluator() -> Result<(), Box<dyn Error>>
{
    for name in ["web_search", "web_fetch"] {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::new();
        registry.register(NamedCountingTool {
            name,
            calls: calls.clone(),
        });
        let executor = RuntimeToolExecutor::new(&registry);
        let context = ToolExecutionContext {
            permission_mode_snapshot: PermissionModeSnapshot {
                mode: PermissionMode::Auto,
                source: Some("test".to_owned()),
                scope_ref: None,
            },
            permission_auto_approval: AutoApprovalConfig {
                enabled: true,
                ..AutoApprovalConfig::default()
            },
            permission_interactive: true,
            ..ToolExecutionContext::default()
        };

        let report = executor.execute_tool_calls(
            vec![RuntimeToolCall::new(
                format!("{name}-call"),
                name,
                json!({}),
            )],
            &context,
        );

        if calls.load(Ordering::SeqCst) != 1 || report.interrupt.is_some() {
            return Err(format!(
                "guarded web tool was not auto-approved: name={name} report={report:?} calls={}",
                calls.load(Ordering::SeqCst)
            )
            .into());
        }
    }
    Ok(())
}

#[test]
fn runtime_auto_approval_keeps_image_generation_approval_gated() -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(NamedCountingTool {
        name: "image_generate",
        calls: calls.clone(),
    });
    let executor = RuntimeToolExecutor::new(&registry);
    let context = ToolExecutionContext {
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::Auto,
            source: Some("test".to_owned()),
            scope_ref: None,
        },
        permission_auto_approval: AutoApprovalConfig {
            enabled: true,
            ..AutoApprovalConfig::default()
        },
        permission_interactive: true,
        ..ToolExecutionContext::default()
    };

    let report = executor.execute_tool_calls(
        vec![RuntimeToolCall::new(
            "image-call",
            "image_generate",
            json!({}),
        )],
        &context,
    );

    if calls.load(Ordering::SeqCst) != 0
        || !matches!(
            report.interrupt,
            Some(RuntimeInterrupt::PermissionApproval { .. })
        )
    {
        return Err(format!(
            "image generation should remain approval-gated: report={report:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn runtime_auto_approval_executes_native_safe_verification() -> Result<(), Box<dyn Error>> {
    for command in ["pwd", "cargo fmt --check"] {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::new();
        registry.register(ProcExecCountingTool {
            calls: calls.clone(),
        });
        let executor = RuntimeToolExecutor::new(&registry);
        let context = ToolExecutionContext {
            permission_mode_snapshot: PermissionModeSnapshot {
                mode: PermissionMode::Auto,
                source: Some("test".to_owned()),
                scope_ref: None,
            },
            permission_auto_approval: AutoApprovalConfig {
                enabled: true,
                ..AutoApprovalConfig::default()
            },
            permission_interactive: true,
            ..ToolExecutionContext::default()
        };

        let report = executor.execute_tool_calls(
            vec![RuntimeToolCall::new(
                "exec-call",
                "exec",
                json!({ "command": command }),
            )],
            &context,
        );

        if calls.load(Ordering::SeqCst) != 1 || report.interrupt.is_some() {
            return Err(format!(
                "native-safe verification was not auto-approved: command={command} report={report:?} calls={}",
                calls.load(Ordering::SeqCst)
            )
            .into());
        }
    }
    Ok(())
}

#[test]
fn runtime_auto_approval_does_not_widen_disallowed_workspace_edits() -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(WriteFileCountingTool {
        calls: calls.clone(),
    });
    let executor = RuntimeToolExecutor::new(&registry);
    let context = ToolExecutionContext {
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::Auto,
            source: Some("test".to_owned()),
            scope_ref: None,
        },
        permission_auto_approval: AutoApprovalConfig {
            enabled: true,
            allow_workspace_edits: false,
            ..AutoApprovalConfig::default()
        },
        permission_interactive: true,
        ..ToolExecutionContext::default()
    };

    let report = executor.execute_tool_calls(
        vec![RuntimeToolCall::new(
            "write-call",
            "write_file",
            json!({ "path": "src/lib.rs", "content": "ok" }),
        )],
        &context,
    );

    if calls.load(Ordering::SeqCst) != 0
        || !matches!(
            report.interrupt,
            Some(RuntimeInterrupt::PermissionApproval { .. })
        )
    {
        return Err(format!(
            "disallowed workspace edit should require approval: report={report:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn runtime_auto_mode_asks_for_configured_protected_targets() -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(WriteFileCountingTool {
        calls: calls.clone(),
    });
    let executor = RuntimeToolExecutor::new(&registry);
    let context = ToolExecutionContext {
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::Auto,
            source: Some("test".to_owned()),
            scope_ref: None,
        },
        permission_auto_approval: AutoApprovalConfig {
            enabled: true,
            allow_workspace_edits: true,
            protected_targets: vec!["src".to_owned()],
            ..AutoApprovalConfig::default()
        },
        permission_interactive: true,
        ..ToolExecutionContext::default()
    };

    let report = executor.execute_tool_calls(
        vec![RuntimeToolCall::new(
            "write-call",
            "write_file",
            json!({ "path": "src/lib.rs", "content": "ok" }),
        )],
        &context,
    );

    if calls.load(Ordering::SeqCst) != 0
        || !matches!(
            report.interrupt,
            Some(RuntimeInterrupt::PermissionApproval { .. })
        )
    {
        return Err(format!(
            "protected target should ask before execution: report={report:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn runtime_auto_mode_does_not_execute_protected_target_after_user_approval(
) -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(WriteFileCountingTool {
        calls: calls.clone(),
    });
    let executor = RuntimeToolExecutor::new(&registry);
    let context = ToolExecutionContext {
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::Auto,
            source: Some("test".to_owned()),
            scope_ref: None,
        },
        permission_auto_approval: AutoApprovalConfig {
            enabled: true,
            allow_workspace_edits: true,
            protected_targets: vec!["src".to_owned()],
            ..AutoApprovalConfig::default()
        },
        permission_interactive: true,
        ..ToolExecutionContext::default()
    };
    let call = RuntimeToolCall::new(
        "write-call",
        "write_file",
        json!({ "path": "src/lib.rs", "content": "ok" }),
    );
    let approval_report = executor.execute_tool_calls(vec![call.clone()], &context);
    let approval_request = match approval_report.interrupt {
        Some(RuntimeInterrupt::PermissionApproval {
            approval_request, ..
        }) => approval_request,
        other => {
            return Err(format!("missing protected target approval request: {other:?}").into())
        }
    };
    let approved_at = approval_request.expires_at_unix_ms.saturating_sub(1);
    let policy_safety_snapshot_ref = approval_request.policy_safety_snapshot_ref.clone();
    let decision = ApprovalDecision {
        approval_request_id: approval_request.approval_request_id.clone(),
        action_digest: approval_request.action_digest.clone(),
        snapshot_digest: approval_request.snapshot_digest.clone(),
        decision: ApprovalDecisionKind::Approved,
        approved_scope: approval_request.requested_scope.clone(),
        actor: ApprovalActor::LocalUser,
        decided_at_unix_ms: approved_at,
        consumed: false,
        policy_safety_snapshot_ref,
        secret_ref_evidence: approval_request.secret_ref_evidence.clone(),
    };
    let approved_context = ToolExecutionContext {
        permission_approval_cache: Some(ApprovalCacheEntry {
            request: *approval_request,
            decision,
        }),
        ..context
    };

    let report = executor.execute_tool_calls(vec![call], &approved_context);

    if calls.load(Ordering::SeqCst) != 0
        || report.interrupt.is_some()
        || !report.messages.first().is_some_and(|message| {
            message.tool_call_id == "write-call"
                && message.name == "write_file"
                && message.content.contains("Permission denied")
                && message.content.contains("ProtectedTarget")
        })
    {
        return Err(format!(
            "approved protected target was not denied cleanly: report={report:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn bridge_permission_ask_interrupts_before_exec_without_executing() -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let catalog = bridge_catalog([("exec", "Deferred proc exec", ["command"])]);
    let context = ToolExecutionContext {
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::Auto,
            source: Some("test".to_owned()),
            scope_ref: None,
        },
        permission_rule_input: PermissionRuleInput {
            containment: confirmed_containment(),
            protected_targets: Vec::new(),
            proc_exec_summary: None,
        },
        permission_interactive: true,
        ..ToolExecutionContext::default()
    };

    let report = dispatch_bridge_tool_calls(
        vec![RuntimeToolCall::new(
            "exec-bridge",
            "tool_call",
            json!({ "name": "exec", "arguments": { "command": "cargo test" } }),
        )],
        Some(&catalog),
        &registry,
        &RuntimeToolExecutor::new(&registry),
        &context,
        false,
    );

    match &report.interrupt {
        Some(RuntimeInterrupt::PermissionApproval {
            approval_request_id,
            approval_request,
            tool_call,
            options,
            ..
        }) if calls.load(Ordering::SeqCst) == 0
            && report.messages().is_empty()
            && approval_request_id.starts_with("approval_")
            && approval_request.approval_request_id.as_str() == approval_request_id.as_str()
            && approval_request.requested_scope == "cli:direct"
            && !approval_request.action_digest.is_empty()
            && !approval_request.snapshot_digest.is_empty()
            && approval_request.allowed_decisions.as_slice()
                == [
                    ApprovalDecisionKind::Approved,
                    ApprovalDecisionKind::Denied,
                    ApprovalDecisionKind::ApprovedForSession,
                    ApprovalDecisionKind::ApprovedForProject,
                    ApprovalDecisionKind::DeniedForSession,
                    ApprovalDecisionKind::DeniedForProject,
                ]
            && tool_call.id == "exec-bridge"
            && tool_call.name == "tool_call"
            && options.as_slice()
                == [
                    "approve",
                    "deny",
                    "approve_session",
                    "approve_project",
                    "deny_session",
                    "deny_project",
                ] => Ok(()),
        other => Err(format!(
            "bridge exec should ask before execution: interrupt={other:?} report={report:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into()),
    }
}

#[test]
fn runtime_executes_proc_exec_after_permission_approval() -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let executor = RuntimeToolExecutor::new(&registry);
    let context = ToolExecutionContext {
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::Auto,
            source: Some("test".to_owned()),
            scope_ref: Some("workspace".to_owned()),
        },
        permission_rule_input: PermissionRuleInput {
            containment: confirmed_containment(),
            protected_targets: Vec::new(),
            proc_exec_summary: None,
        },
        permission_ceiling_snapshot: Some(PermissionCeilingSnapshot {
            parent_mode: PermissionMode::Auto,
            capability_ceiling: vec![SafetyCapability::ProcExec],
            approved_scope_refs: vec!["workspace".to_owned()],
            origin: RuntimeBoundaryOrigin::UserTurn,
        }),
        permission_interactive: true,
        ..ToolExecutionContext::default()
    };
    let approval_report = executor.execute_tool_calls(
        vec![RuntimeToolCall::new(
            "exec-call",
            "exec",
            json!({ "command": "cargo test" }),
        )],
        &context,
    );
    let approval_request = match approval_report.interrupt {
        Some(RuntimeInterrupt::PermissionApproval {
            approval_request, ..
        }) => approval_request,
        other => {
            return Err(format!("missing approval request before execution: {other:?}").into())
        }
    };
    let approved_at = approval_request.expires_at_unix_ms.saturating_sub(1);
    let approval_decision = ApprovalDecision {
        approval_request_id: approval_request.approval_request_id.clone(),
        action_digest: approval_request.action_digest.clone(),
        snapshot_digest: approval_request.snapshot_digest.clone(),
        decision: ApprovalDecisionKind::Approved,
        approved_scope: approval_request.requested_scope.clone(),
        actor: ApprovalActor::LocalUser,
        decided_at_unix_ms: approved_at,
        consumed: false,
        policy_safety_snapshot_ref: approval_request.policy_safety_snapshot_ref.clone(),
        secret_ref_evidence: approval_request.secret_ref_evidence.clone(),
    };
    let approved_context = ToolExecutionContext {
        permission_approval_cache: Some(ApprovalCacheEntry {
            request: *approval_request,
            decision: approval_decision,
        }),
        ..context
    };

    let report = executor.execute_tool_calls(
        vec![RuntimeToolCall::new(
            "exec-call",
            "exec",
            json!({ "command": "cargo test" }),
        )],
        &approved_context,
    );

    let message = report.messages.first().ok_or("missing exec result")?;
    if calls.load(Ordering::SeqCst) != 1
        || report.interrupt.is_some()
        || message.tool_call_id != "exec-call"
        || message.name != "exec"
        || !message.content.contains("executed")
    {
        return Err(format!(
            "approved proc_exec was not executed: report={report:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn oracle_runtime_approved_exec_uses_actual_process_context() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let mut registry = ToolRegistry::new();
    registry.register(ExecTool::with_workspace(temp.path()));
    let executor = RuntimeToolExecutor::new(&registry);
    let context = ToolExecutionContext {
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::Auto,
            source: Some("test".to_owned()),
            scope_ref: Some("workspace".to_owned()),
        },
        permission_ceiling_snapshot: Some(PermissionCeilingSnapshot {
            parent_mode: PermissionMode::Auto,
            capability_ceiling: vec![SafetyCapability::ProcExec],
            approved_scope_refs: vec!["workspace".to_owned()],
            origin: RuntimeBoundaryOrigin::UserTurn,
        }),
        permission_interactive: true,
        permission_rule_input: PermissionRuleInput {
            containment: confirmed_containment(),
            protected_targets: Vec::new(),
            proc_exec_summary: Some(ProcExecSummary {
                command_family: "pwd".to_owned(),
                target_refs: Vec::new(),
                destructive: false,
                network: false,
                secret_exposure: false,
                summary_available: true,
            }),
        },
        ..ToolExecutionContext::default()
    };
    let call = RuntimeToolCall::new("exec-call", "exec", json!({ "command": "pwd" }));
    let approval_report = executor.execute_tool_calls(vec![call.clone()], &context);
    let approval_request = match approval_report.interrupt {
        Some(RuntimeInterrupt::PermissionApproval {
            approval_request, ..
        }) => approval_request,
        other => return Err(format!("missing approval request before exec: {other:?}").into()),
    };
    let approved_at = approval_request.expires_at_unix_ms.saturating_sub(1);
    let approved_context = ToolExecutionContext {
        permission_approval_cache: Some(ApprovalCacheEntry {
            request: (*approval_request).clone(),
            decision: ApprovalDecision {
                approval_request_id: approval_request.approval_request_id.clone(),
                action_digest: approval_request.action_digest.clone(),
                snapshot_digest: approval_request.snapshot_digest.clone(),
                decision: ApprovalDecisionKind::Approved,
                approved_scope: approval_request.requested_scope.clone(),
                actor: ApprovalActor::LocalUser,
                decided_at_unix_ms: approved_at,
                consumed: false,
                policy_safety_snapshot_ref: approval_request.policy_safety_snapshot_ref.clone(),
                secret_ref_evidence: approval_request.secret_ref_evidence.clone(),
            },
        }),
        ..context
    };

    let report = executor.execute_tool_calls(vec![call], &approved_context);

    let message = report.messages.first().ok_or("missing exec output")?;
    if report.interrupt.is_some()
        || !message
            .content
            .contains(&temp.path().to_string_lossy().to_string())
        || !message.content.contains("Exit code: 0")
    {
        return Err(
            format!("approved runtime exec did not use process context: {report:?}").into(),
        );
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn spec030_process_runtime_cancellation_aborts_exec_and_descendants() -> Result<(), Box<dyn Error>>
{
    use shacs_projection::{
        ProcessAdapterKind, ProcessAdapterSupport, ProcessControlReason, ProcessControlScope,
        ProcessTerminalOutcome, Spec030ProjectionProvider,
    };

    let temp = tempfile::tempdir()?;
    let facts = Spec030FactStore::new(WorkspaceTrustObservation::Trusted);
    let mut registry = ToolRegistry::new();
    registry.register(ExecTool::with_workspace(temp.path()).with_spec030_fact_store(facts.clone()));
    let executor = RuntimeToolExecutor::new(&registry);
    let cancellation = CancellationToken::new();
    let context = ToolExecutionContext {
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::Auto,
            source: Some("test".to_owned()),
            scope_ref: Some("workspace".to_owned()),
        },
        permission_ceiling_snapshot: Some(PermissionCeilingSnapshot {
            parent_mode: PermissionMode::Auto,
            capability_ceiling: vec![SafetyCapability::ProcExec],
            approved_scope_refs: vec!["workspace".to_owned()],
            origin: RuntimeBoundaryOrigin::UserTurn,
        }),
        permission_interactive: true,
        permission_rule_input: safe_proc_exec_rule_input(),
        ..ToolExecutionContext::default()
    };
    let script = "trap '' TERM; sh -c 'trap \"\" TERM; echo $$ > descendant.pid; exec sleep 30' & touch started; wait";
    let call = RuntimeToolCall::new(
        "cancel-exec",
        "exec",
        json!({ "command": script, "timeout": 30 }),
    );
    let approval = executor.execute_tool_calls(vec![call.clone()], &context);
    let approval_request = match approval.interrupt {
        Some(RuntimeInterrupt::PermissionApproval {
            approval_request, ..
        }) => approval_request,
        other => return Err(format!("missing approval before cancellable exec: {other:?}").into()),
    };
    let approved_at = approval_request.expires_at_unix_ms.saturating_sub(1);
    let approved_context = ToolExecutionContext {
        permission_approval_cache: Some(ApprovalCacheEntry {
            request: (*approval_request).clone(),
            decision: ApprovalDecision {
                approval_request_id: approval_request.approval_request_id.clone(),
                action_digest: approval_request.action_digest.clone(),
                snapshot_digest: approval_request.snapshot_digest.clone(),
                decision: ApprovalDecisionKind::Approved,
                approved_scope: approval_request.requested_scope.clone(),
                actor: ApprovalActor::LocalUser,
                decided_at_unix_ms: approved_at,
                consumed: false,
                policy_safety_snapshot_ref: approval_request.policy_safety_snapshot_ref.clone(),
                secret_ref_evidence: approval_request.secret_ref_evidence.clone(),
            },
        }),
        cancellation_token: Some(cancellation.clone()),
        ..context
    };

    let report = thread::scope(|scope| -> Result<_, Box<dyn Error>> {
        let execution = scope.spawn(|| executor.execute_tool_calls(vec![call], &approved_context));
        let ready = wait_for_runtime_path(&temp.path().join("started"));
        cancellation.cancel();
        let report = execution
            .join()
            .map_err(|_| "runtime exec thread panicked")?;
        if let Err(error) = ready {
            return Err(format!("{error}; report={report:?}").into());
        }
        Ok(report)
    })?;

    assert!(report.messages[0].content.contains("Command aborted"));
    let pid = std::fs::read_to_string(temp.path().join("descendant.pid"))?
        .trim()
        .parse::<i32>()?;
    wait_for_runtime_process_exit(pid)?;
    let projection = LocalSpec030ProjectionProvider::new(facts).projection();
    let bash = projection
        .process_adapters()
        .iter()
        .find(|row| row.adapter == ProcessAdapterKind::Bash)
        .ok_or("missing Bash fact")?;
    assert_eq!(bash.support, ProcessAdapterSupport::Supported);
    assert_eq!(bash.control_scope, ProcessControlScope::ControlledChild);
    assert_eq!(
        bash.reason,
        ProcessControlReason::ControlledChildObservedNoRollback
    );
    assert_eq!(
        bash.recent_outcomes[0].outcome,
        ProcessTerminalOutcome::Aborted
    );
    Ok(())
}

#[cfg(unix)]
fn wait_for_runtime_path(path: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !path.exists() {
        if Instant::now() >= deadline {
            return Err(format!("timed out waiting for {}", path.display()).into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

#[cfg(unix)]
fn wait_for_runtime_process_exit(pid: i32) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(3);
    while nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok() {
        if Instant::now() >= deadline {
            return Err(format!("descendant {pid} survived runtime cancellation").into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

#[test]
fn runtime_asks_again_for_mismatched_permission_approval_cache() -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let executor = RuntimeToolExecutor::new(&registry);
    let context = ToolExecutionContext {
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::Auto,
            source: Some("test".to_owned()),
            scope_ref: None,
        },
        permission_rule_input: PermissionRuleInput {
            containment: confirmed_containment(),
            protected_targets: Vec::new(),
            proc_exec_summary: None,
        },
        permission_interactive: true,
        ..ToolExecutionContext::default()
    };
    let approval_report = executor.execute_tool_calls(
        vec![RuntimeToolCall::new(
            "exec-call",
            "exec",
            json!({ "command": "cargo test" }),
        )],
        &context,
    );
    let approval_request = match approval_report.interrupt {
        Some(RuntimeInterrupt::PermissionApproval {
            approval_request, ..
        }) => approval_request,
        other => {
            return Err(format!("missing approval request before execution: {other:?}").into())
        }
    };
    let approved_at = approval_request.expires_at_unix_ms.saturating_sub(1);
    let approval_decision = ApprovalDecision {
        approval_request_id: approval_request.approval_request_id.clone(),
        action_digest: "different-action".to_owned(),
        snapshot_digest: approval_request.snapshot_digest.clone(),
        decision: ApprovalDecisionKind::Approved,
        approved_scope: approval_request.requested_scope.clone(),
        actor: ApprovalActor::LocalUser,
        decided_at_unix_ms: approved_at,
        consumed: false,
        policy_safety_snapshot_ref: approval_request.policy_safety_snapshot_ref.clone(),
        secret_ref_evidence: approval_request.secret_ref_evidence.clone(),
    };
    let approved_context = ToolExecutionContext {
        permission_approval_cache: Some(ApprovalCacheEntry {
            request: *approval_request,
            decision: approval_decision,
        }),
        ..context
    };

    let report = executor.execute_tool_calls(
        vec![RuntimeToolCall::new(
            "exec-call",
            "exec",
            json!({ "command": "cargo test" }),
        )],
        &approved_context,
    );

    if calls.load(Ordering::SeqCst) != 0
        || !matches!(
            report.interrupt,
            Some(RuntimeInterrupt::PermissionApproval { .. })
        )
        || !report
            .permissioned_actions
            .first()
            .is_some_and(|action| action.tool_name == "exec")
        || !matches!(
            shacs_core::runtime::correlate_approval(
                &approved_context
                    .permission_approval_cache
                    .as_ref()
                    .ok_or("missing approval cache")?
                    .request,
                &approved_context
                    .permission_approval_cache
                    .as_ref()
                    .ok_or("missing approval cache")?
                    .decision,
                approved_at,
            )
            .error,
            Some(ApprovalCorrelationError::ActionMismatch)
        )
    {
        return Err(format!(
            "mismatched cached approval should ask again without executing: report={report:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn runtime_asks_again_for_changed_policy_safety_snapshot_ref_cache() -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let executor = RuntimeToolExecutor::new(&registry);
    let context = ToolExecutionContext {
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::Auto,
            source: Some("test".to_owned()),
            scope_ref: None,
        },
        permission_interactive: true,
        ..ToolExecutionContext::default()
    };
    let approval_report = executor.execute_tool_calls(
        vec![RuntimeToolCall::new(
            "exec-call",
            "exec",
            json!({ "command": "cargo test" }),
        )],
        &context,
    );
    let approval_request = match approval_report.interrupt {
        Some(RuntimeInterrupt::PermissionApproval {
            approval_request, ..
        }) => approval_request,
        other => {
            return Err(format!("missing approval request before execution: {other:?}").into())
        }
    };
    let approved_at = approval_request.expires_at_unix_ms.saturating_sub(1);
    let mut stale_request = (*approval_request).clone();
    let mut stale_ref = stale_request
        .policy_safety_snapshot_ref
        .clone()
        .ok_or("approval request must carry policy safety snapshot ref")?;
    stale_ref.created_at_unix_ms = stale_ref.created_at_unix_ms.saturating_add(1);
    stale_request.policy_safety_snapshot_ref = Some(stale_ref);
    let approval_decision = ApprovalDecision {
        approval_request_id: stale_request.approval_request_id.clone(),
        action_digest: stale_request.action_digest.clone(),
        snapshot_digest: stale_request.snapshot_digest.clone(),
        decision: ApprovalDecisionKind::Approved,
        approved_scope: stale_request.requested_scope.clone(),
        actor: ApprovalActor::LocalUser,
        decided_at_unix_ms: approved_at,
        consumed: false,
        policy_safety_snapshot_ref: stale_request.policy_safety_snapshot_ref.clone(),
        secret_ref_evidence: stale_request.secret_ref_evidence.clone(),
    };
    let approved_context = ToolExecutionContext {
        permission_approval_cache: Some(ApprovalCacheEntry {
            request: stale_request,
            decision: approval_decision,
        }),
        ..context
    };

    let report = executor.execute_tool_calls(
        vec![RuntimeToolCall::new(
            "exec-call",
            "exec",
            json!({ "command": "cargo test" }),
        )],
        &approved_context,
    );

    if calls.load(Ordering::SeqCst) != 0
        || !matches!(
            report.interrupt,
            Some(RuntimeInterrupt::PermissionApproval { .. })
        )
    {
        return Err(format!(
            "changed policy safety snapshot ref should ask again without executing: report={report:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn runtime_enforces_inherited_ceiling_before_direct_tool_execution() -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let executor = RuntimeToolExecutor::new(&registry);
    let mut context = safe_mcp_tool_context();
    context.permission_ceiling_snapshot = Some(restrictive_read_only_ceiling());

    let report = executor.execute_tool_calls(
        vec![RuntimeToolCall::new(
            "exec-call",
            "exec",
            json!({ "command": "cargo test" }),
        )],
        &context,
    );

    let message = report
        .messages
        .first()
        .ok_or("missing inherited ceiling denial")?;
    if calls.load(Ordering::SeqCst) != 0
        || message.tool_call_id != "exec-call"
        || !message.content.contains("CeilingViolation")
        || !report
            .permissioned_actions
            .first()
            .is_some_and(|action| action.tool_name == "exec")
    {
        return Err(format!(
            "inherited ceiling did not block direct exec: report={report:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn bridge_denies_deferred_proc_exec_without_executing_underlying_tool() -> Result<(), Box<dyn Error>>
{
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let executor = RuntimeToolExecutor::new(&registry);
    let exec_schema = registry
        .definitions()
        .into_iter()
        .next()
        .ok_or("missing exec schema")?;
    let catalog = DeferredToolCatalog::new(
        vec![DeferredToolCatalogEntry {
            name: "exec".to_owned(),
            description: "Deferred proc exec".to_owned(),
            parameter_names: vec!["command".to_owned()],
            full_schema: exec_schema,
            source_kind: "parent_only".to_owned(),
            source_name: "runtime".to_owned(),
        }],
        2,
        4,
    );

    let report = dispatch_bridge_tool_call(
        RuntimeToolCall::new(
            "bridge-exec",
            "tool_call",
            json!({ "name": "exec", "arguments": { "command": "cargo test" } }),
        ),
        Some(&catalog),
        &registry,
        &executor,
        &ToolExecutionContext::default(),
    );

    let messages = report.messages();
    let message = messages
        .first()
        .ok_or("missing bridge permission message")?;
    if calls.load(Ordering::SeqCst) != 0
        || report.interrupt.is_some()
        || message.tool_call_id != "bridge-exec"
        || message.name != "tool_call"
        || report
            .permissioned_actions
            .first()
            .map(|action| &action.origin)
            .is_none()
    {
        return Err(format!(
            "deferred proc_exec should be blocked before execution: report={report:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    match &report.permissioned_actions[0].origin {
        PermissionedActionOrigin::DeferredBridge { bridge_name, .. }
            if bridge_name == "tool_call" => {}
        other => return Err(format!("deferred policy evaluated wrong origin: {other:?}").into()),
    }
    Ok(())
}

#[test]
fn runtime_enforces_inherited_ceiling_before_deferred_tool_execution() -> Result<(), Box<dyn Error>>
{
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let executor = RuntimeToolExecutor::new(&registry);
    let exec_schema = registry
        .definitions()
        .into_iter()
        .next()
        .ok_or("missing exec schema")?;
    let catalog = DeferredToolCatalog::new(
        vec![DeferredToolCatalogEntry {
            name: "exec".to_owned(),
            description: "Deferred proc exec".to_owned(),
            parameter_names: vec!["command".to_owned()],
            full_schema: exec_schema,
            source_kind: "parent_only".to_owned(),
            source_name: "runtime".to_owned(),
        }],
        2,
        4,
    );
    let mut context = safe_mcp_tool_context();
    context.permission_ceiling_snapshot = Some(restrictive_read_only_ceiling());

    let report = dispatch_bridge_tool_call(
        RuntimeToolCall::new(
            "bridge-exec",
            "tool_call",
            json!({ "name": "exec", "arguments": { "command": "cargo test" } }),
        ),
        Some(&catalog),
        &registry,
        &executor,
        &context,
    );

    let message = report
        .messages()
        .first()
        .ok_or("missing deferred inherited ceiling denial")?
        .clone();
    if calls.load(Ordering::SeqCst) != 0
        || message.tool_call_id != "bridge-exec"
        || !message.content.contains("CeilingViolation")
        || !report
            .permissioned_actions
            .first()
            .is_some_and(|action| action.tool_name == "exec")
    {
        return Err(format!(
            "inherited ceiling did not block deferred exec: report={report:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn runtime_executes_tool_calls_and_maps_result_messages() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(NamedRepeatTool("mcp_repeat"));
    registry.register(JsonTool);

    let executor = RuntimeToolExecutor::new(&registry);
    let context = permissive_local_tool_context();
    let report = executor.execute_tool_calls(
        vec![
            RuntimeToolCall::new(
                "call_repeat",
                "mcp_repeat",
                json!({ "text": 42, "times": "2" }),
            ),
            RuntimeToolCall::new("call_json", "mcp_json_tool", json!({})),
        ],
        &context,
    );

    if report.interrupt.is_some() || !report.skipped_tool_calls.is_empty() {
        return Err(format!("unexpected runtime stop: {report:?}").into());
    }
    if report.messages.len() != 2
        || report.messages[0].tool_call_id != "call_repeat"
        || report.messages[0].content != "4242"
        || report.messages[1].to_json()
            != json!({
                "role": "tool",
                "tool_call_id": "call_json",
                "name": "mcp_json_tool",
                "content": "{\"ok\":true}"
            })
    {
        return Err(format!("unexpected runtime messages: {report:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_assistant_tool_call_message_uses_openai_argument_string() -> Result<(), Box<dyn Error>> {
    let message = shacs_core::runtime::RuntimeAssistantToolCallMessage::new(
        Some("using tools".to_owned()),
        vec![RuntimeToolCall::new(
            "call_1",
            "repeat",
            json!({ "text": "ha", "times": 2 }),
        )],
    );

    if message.to_json()
        != json!({
            "role": "assistant",
            "content": "using tools",
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": "repeat",
                    "arguments": "{\"text\":\"ha\",\"times\":2}"
                }
            }]
        })
    {
        return Err(format!(
            "assistant tool call JSON shape drifted: {}",
            message.to_json()
        )
        .into());
    }
    Ok(())
}

#[test]
fn runtime_returns_tool_errors_without_stopping_batch() -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(NamedCountingTool {
        name: "mcp_count",
        calls: calls.clone(),
    });

    let executor = RuntimeToolExecutor::new(&registry);
    let context = permissive_local_tool_context();
    let report = executor.execute_tool_calls(
        vec![
            RuntimeToolCall::new("missing", "missing_tool", json!({})),
            RuntimeToolCall::new("count", "mcp_count", json!({})),
        ],
        &context,
    );

    if report.messages.len() != 2
        || !report.messages[0].content.contains("Permission denied")
        || !report.messages[0].content.contains("StaticDeny")
        || report.messages[1].content != "counted"
        || calls.load(Ordering::SeqCst) != 1
    {
        return Err(format!("runtime did not preserve fail-closed behavior: {report:?}").into());
    }
    if report.permissioned_actions.len() != 2
        || report.permissioned_actions[0].tool_name != "missing_tool"
        || report.permissioned_actions[0].normalization_state
            != ActionNormalizationState::DenyCandidate
        || !report.permissioned_actions[0]
            .normalization_errors
            .contains(&ActionNormalizationError::UnknownTool {
                tool_name: "missing_tool".to_owned(),
            })
        || report.permissioned_actions[1].tool_name != "mcp_count"
        || report.permissioned_actions[1].normalization_state != ActionNormalizationState::Ready
    {
        return Err(format!("runtime did not report pre-execution actions: {report:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_appends_retry_hint_to_executed_tool_errors() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(ErrorTool);

    let executor = RuntimeToolExecutor::new(&registry);
    let context = permissive_local_tool_context();
    let report = executor.execute_tool_calls(
        vec![RuntimeToolCall::new("error", "mcp_error_tool", json!({}))],
        &context,
    );

    if report.messages.len() != 1
        || report.messages[0].content
            != "Error: simulated failure\n\n[Analyze the error above and try a different approach.]"
    {
        return Err(format!("runtime did not append tool error hint: {report:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_batches_only_concurrency_safe_tools_when_enabled() -> Result<(), Box<dyn Error>> {
    let sequential_active = Arc::new(AtomicUsize::new(0));
    let sequential_max = Arc::new(AtomicUsize::new(0));
    let sequential_calls = Arc::new(AtomicUsize::new(0));
    let mut sequential_registry = ToolRegistry::new();
    sequential_registry.register(DelayTool::new(
        "mcp_safe_a",
        true,
        sequential_active.clone(),
        sequential_max.clone(),
        sequential_calls.clone(),
    ));
    sequential_registry.register(DelayTool::new(
        "mcp_safe_b",
        true,
        sequential_active,
        sequential_max.clone(),
        sequential_calls.clone(),
    ));
    let context = permissive_local_tool_context();
    let sequential = RuntimeToolExecutor::new(&sequential_registry).execute_tool_calls(
        vec![
            RuntimeToolCall::new("safe-a", "mcp_safe_a", json!({})),
            RuntimeToolCall::new("safe-b", "mcp_safe_b", json!({})),
        ],
        &context,
    );
    if sequential.messages.len() != 2
        || sequential_max.load(Ordering::SeqCst) != 1
        || sequential_calls.load(Ordering::SeqCst) != 2
    {
        return Err(format!("sequential execution drifted: {sequential:?}").into());
    }

    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(DelayTool::new(
        "mcp_safe_a",
        true,
        active.clone(),
        max_active.clone(),
        calls.clone(),
    ));
    registry.register(DelayTool::new(
        "mcp_safe_b",
        true,
        active.clone(),
        max_active.clone(),
        calls.clone(),
    ));
    registry.register(DelayTool::new(
        "mcp_unsafe_tool",
        false,
        active.clone(),
        max_active.clone(),
        calls.clone(),
    ));
    registry.register(DelayTool::new(
        "mcp_safe_c",
        true,
        active,
        max_active.clone(),
        calls.clone(),
    ));

    let report = RuntimeToolExecutor::new(&registry).execute_tool_calls_concurrent(
        vec![
            RuntimeToolCall::new("safe-a", "mcp_safe_a", json!({})),
            RuntimeToolCall::new("safe-b", "mcp_safe_b", json!({})),
            RuntimeToolCall::new("unsafe", "mcp_unsafe_tool", json!({})),
            RuntimeToolCall::new("safe-c", "mcp_safe_c", json!({})),
        ],
        &context,
    );
    let contents = report
        .messages
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>();
    if contents != ["mcp_safe_a", "mcp_safe_b", "mcp_unsafe_tool", "mcp_safe_c"]
        || max_active.load(Ordering::SeqCst) != 2
        || calls.load(Ordering::SeqCst) != 4
    {
        return Err(format!("concurrent batching drifted: {report:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_concurrent_execution_preserves_ask_user_interrupt_boundary() -> Result<(), Box<dyn Error>>
{
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let before_calls = Arc::new(AtomicUsize::new(0));
    let after_calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(DelayTool::new(
        "mcp_safe_before",
        true,
        active.clone(),
        max_active.clone(),
        before_calls.clone(),
    ));
    registry.register(AskUserTool::new());
    registry.register(DelayTool::new(
        "mcp_safe_after",
        true,
        active,
        max_active,
        after_calls.clone(),
    ));

    let context = permissive_local_tool_context();
    let report = RuntimeToolExecutor::new(&registry).execute_tool_calls_concurrent(
        vec![
            RuntimeToolCall::new("before", "mcp_safe_before", json!({})),
            RuntimeToolCall::new(
                "ask",
                "ask_user",
                json!({ "question": "Continue?", "options": ["Yes", "No"] }),
            ),
            RuntimeToolCall::new("after", "mcp_safe_after", json!({})),
        ],
        &context,
    );
    if report.messages.len() != 1
        || report.messages[0].tool_call_id != "before"
        || before_calls.load(Ordering::SeqCst) != 1
        || after_calls.load(Ordering::SeqCst) != 0
        || report.skipped_tool_calls.len() != 1
        || report.skipped_tool_calls[0].id != "after"
    {
        return Err(format!("ask_user boundary drifted under concurrency: {report:?}").into());
    }
    match report.interrupt {
        Some(RuntimeInterrupt::AskUser { tool_call_id, .. }) if tool_call_id == "ask" => Ok(()),
        other => Err(format!("unexpected concurrent ask interrupt: {other:?}").into()),
    }
}

#[test]
fn runtime_preserves_ask_user_interrupt_and_skips_later_tools() -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(AskUserTool::new());
    registry.register(CountingTool {
        calls: calls.clone(),
    });

    let executor = RuntimeToolExecutor::new(&registry);
    let report = executor.execute_tool_calls(
        vec![
            RuntimeToolCall::new(
                "ask",
                "ask_user",
                json!({ "question": "Continue?", "options": ["Yes", "No"] }),
            ),
            RuntimeToolCall::new("count", "count", json!({})),
        ],
        &ToolExecutionContext::default(),
    );

    if !report.messages.is_empty()
        || report.skipped_tool_calls.len() != 1
        || calls.load(Ordering::SeqCst) != 0
    {
        return Err(format!("ask_user did not stop later tools: {report:?}").into());
    }
    match report.interrupt {
        Some(RuntimeInterrupt::AskUser {
            tool_call_id,
            name,
            question,
            options,
        }) if tool_call_id == "ask"
            && name == "ask_user"
            && question == "Continue?"
            && options == ["Yes", "No"] =>
        {
            Ok(())
        }
        other => Err(format!("unexpected ask_user interrupt: {other:?}").into()),
    }
}

#[test]
fn runtime_ask_user_skips_later_denied_tool_without_permission_message(
) -> Result<(), Box<dyn Error>> {
    let exec_calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(AskUserTool::new());
    registry.register(ProcExecCountingTool {
        calls: exec_calls.clone(),
    });

    let report = RuntimeToolExecutor::new(&registry).execute_tool_calls(
        vec![
            RuntimeToolCall::new(
                "ask",
                "ask_user",
                json!({ "question": "Continue?", "options": ["Yes", "No"] }),
            ),
            RuntimeToolCall::new("exec-after", "exec", json!({ "command": "cargo test" })),
        ],
        &ToolExecutionContext::default(),
    );

    if !report.messages.is_empty()
        || report.skipped_tool_calls.len() != 1
        || report.skipped_tool_calls[0].id != "exec-after"
        || report.skipped_tool_calls[0].name != "exec"
        || exec_calls.load(Ordering::SeqCst) != 0
        || report
            .permissioned_actions
            .iter()
            .any(|action| action.tool_name == "exec")
    {
        return Err(format!(
            "ask_user should skip later denied tool without pre-emitting permission message: {report:?}"
        )
        .into());
    }
    match report.interrupt {
        Some(RuntimeInterrupt::AskUser { tool_call_id, .. }) if tool_call_id == "ask" => Ok(()),
        other => Err(format!("unexpected ask_user interrupt: {other:?}").into()),
    }
}

#[test]
fn bridge_dispatcher_search_and_describe_use_current_catalog() -> Result<(), Box<dyn Error>> {
    let registry = ToolRegistry::new();
    let executor = RuntimeToolExecutor::new(&registry);
    let catalog = bridge_catalog([
        (
            "mcp_github_search_repositories",
            "Find GitHub repositories",
            ["query"],
        ),
        ("mcp_slack_post_message", "Post Slack messages", ["channel"]),
    ]);

    let search = dispatch_bridge_tool_call(
        RuntimeToolCall::new(
            "search-call",
            "tool_search",
            json!({ "query": "github", "limit": 1 }),
        ),
        Some(&catalog),
        &registry,
        &executor,
        &ToolExecutionContext::default(),
    );
    let search_messages = search.messages();
    let search_message = search_messages.first().ok_or("missing search message")?;
    let search_content = parse_json_content(&search_message.content)?;
    if search_message.tool_call_id != "search-call"
        || search_message.name != "tool_search"
        || search_content["matches"].as_array().map(Vec::len) != Some(1)
        || search_content["matches"][0]["name"] != "mcp_github_search_repositories"
        || search_content["matches"][0].get("schema").is_some()
    {
        return Err(format!("unexpected search result: {search:?}").into());
    }

    let describe = dispatch_bridge_tool_call(
        RuntimeToolCall::new(
            "describe-call",
            "tool_describe",
            json!({ "name": "mcp_github_search_repositories" }),
        ),
        Some(&catalog),
        &registry,
        &executor,
        &ToolExecutionContext::default(),
    );
    let describe_messages = describe.messages();
    let describe_message = describe_messages
        .first()
        .ok_or("missing describe message")?;
    let describe_content = parse_json_content(&describe_message.content)?;
    if describe_message.tool_call_id != "describe-call"
        || describe_message.name != "tool_describe"
        || describe_content["name"] != "mcp_github_search_repositories"
        || describe_content["schema"]["function"]["parameters"]["properties"]
            .get("query")
            .is_none()
    {
        return Err(format!("unexpected describe result: {describe:?}").into());
    }
    Ok(())
}

#[test]
fn bridge_dispatcher_fails_closed_without_catalog() -> Result<(), Box<dyn Error>> {
    let registry = ToolRegistry::new();
    let executor = RuntimeToolExecutor::new(&registry);
    let report = dispatch_bridge_tool_call(
        RuntimeToolCall::new(
            "missing-catalog",
            "tool_search",
            json!({ "query": "github" }),
        ),
        None,
        &registry,
        &executor,
        &ToolExecutionContext::default(),
    );
    let messages = report.messages();
    let message = messages.first().ok_or("missing error message")?;
    if message.tool_call_id != "missing-catalog"
        || !message
            .content
            .contains("deferred tool catalog is not available")
        || !message
            .content
            .contains("[Analyze the error above and try a different approach.]")
    {
        return Err(format!("missing catalog was not fail-closed: {report:?}").into());
    }
    Ok(())
}

#[test]
fn bridge_dispatcher_rejects_recursive_core_and_out_of_scope_calls() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(RepeatTool);
    let executor = RuntimeToolExecutor::new(&registry);
    let catalog = bridge_catalog([("mcp_repeat", "Repeat deferred text", ["text"])]);
    let cases = [
        (
            "recursive",
            json!({ "name": "tool_search", "arguments": {} }),
            "recursive bridge tool call rejected",
        ),
        (
            "core",
            json!({ "name": "repeat", "arguments": {} }),
            "call it directly",
        ),
        (
            "unknown",
            json!({ "name": "mcp_missing", "arguments": {} }),
            "outside the current deferred tool catalog",
        ),
    ];

    for (call_id, arguments, expected) in cases {
        let report = dispatch_bridge_tool_call(
            RuntimeToolCall::new(call_id, "tool_call", arguments),
            Some(&catalog),
            &registry,
            &executor,
            &ToolExecutionContext::default(),
        );
        let messages = report.messages();
        let message = messages.first().ok_or("missing rejection message")?;
        if message.tool_call_id != call_id || !message.content.contains(expected) {
            return Err(format!("unexpected rejection for {call_id}: {report:?}").into());
        }
    }
    Ok(())
}

#[test]
fn bridge_dispatcher_accepts_object_and_json_string_arguments() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(NamedRepeatTool("mcp_repeat"));
    let executor = RuntimeToolExecutor::new(&registry);
    let catalog = bridge_catalog([("mcp_repeat", "Repeat deferred text", ["text"])]);
    let context = safe_mcp_tool_context();
    let report = dispatch_bridge_tool_calls(
        vec![
            RuntimeToolCall::new(
                "object-call",
                "tool_call",
                json!({ "name": "mcp_repeat", "arguments": { "text": "ha", "times": 2 } }),
            ),
            RuntimeToolCall::new(
                "string-call",
                "tool_call",
                json!({ "name": "mcp_repeat", "arguments": "{\"text\":\"yo\",\"times\":3}" }),
            ),
        ],
        Some(&catalog),
        &registry,
        &executor,
        &context,
        false,
    );

    let messages = report.messages();
    let contents = messages
        .iter()
        .map(|message| {
            (
                message.tool_call_id.as_str(),
                message.name.as_str(),
                message.content.as_str(),
            )
        })
        .collect::<Vec<_>>();
    if contents
        != [
            ("object-call", "tool_call", "haha"),
            ("string-call", "tool_call", "yoyoyo"),
        ]
        || report.resolved_calls.len() != 2
        || report.resolved_calls[0].underlying_name != "mcp_repeat"
    {
        return Err(format!("bridge did not execute normalized arguments: {report:?}").into());
    }
    if report.permissioned_actions.len() != 2
        || report.permissioned_actions[0].tool_name != "mcp_repeat"
        || report.permissioned_actions[0].normalization_state != ActionNormalizationState::Ready
    {
        return Err(
            format!("bridge did not report deferred permissioned actions: {report:?}").into(),
        );
    }
    match &report.permissioned_actions[0].origin {
        PermissionedActionOrigin::DeferredBridge {
            bridge_name,
            parent_origin,
            ..
        } if bridge_name == "tool_call"
            && matches!(**parent_origin, PermissionedActionOrigin::UserTurn) => {}
        other => return Err(format!("bridge origin did not preserve parent: {other:?}").into()),
    }
    Ok(())
}

#[test]
fn runtime_permission_actions_include_context_containment() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(RepeatTool);
    registry.register(NamedRepeatTool("mcp_repeat"));
    let executor = RuntimeToolExecutor::new(&registry);
    let containment = ContainmentSnapshotRef {
        contained: Some(true),
        backend: Some("backend token=sk-backend-secret".to_owned()),
        digest: Some("container token=sk-container-secret".to_owned()),
        summary: Some("sandbox bearer sk-summary-secret".to_owned()),
    };
    let context = ToolExecutionContext {
        containment_snapshot: Some(containment),
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::Auto,
            source: Some("user_local_config".to_owned()),
            scope_ref: Some("workspace".to_owned()),
        },
        ..ToolExecutionContext::default()
    };

    let direct_report = executor.execute_tool_calls(
        vec![RuntimeToolCall::new(
            "direct-repeat",
            "repeat",
            json!({ "text": "ok", "times": 1 }),
        )],
        &context,
    );
    let direct_action = direct_report
        .permissioned_actions
        .first()
        .ok_or("missing direct permissioned action")?;
    assert_safe_containment_snapshot(direct_action)?;
    assert_non_default_permission_snapshot(direct_action)?;

    let catalog = bridge_catalog([("mcp_repeat", "Repeat deferred text", ["text"])]);
    let bridge_report = dispatch_bridge_tool_call(
        RuntimeToolCall::new(
            "bridge-repeat",
            "tool_call",
            json!({ "name": "mcp_repeat", "arguments": { "text": "ha", "times": 2 } }),
        ),
        Some(&catalog),
        &registry,
        &executor,
        &context,
    );
    let bridge_action = bridge_report
        .permissioned_actions
        .first()
        .ok_or("missing bridge permissioned action")?;
    assert_safe_containment_snapshot(bridge_action)?;
    assert_non_default_permission_snapshot(bridge_action)?;
    Ok(())
}

fn assert_non_default_permission_snapshot(
    action: &shacs_core::runtime::PermissionedAction,
) -> Result<(), Box<dyn Error>> {
    let snapshot = &action.permission_mode_snapshot;
    if snapshot.mode != PermissionMode::Auto
        || snapshot.source.as_deref() != Some("user_local_config")
        || snapshot.scope_ref.as_deref() != Some("workspace")
    {
        return Err(format!("permission snapshot was not propagated: {action:?}").into());
    }
    Ok(())
}

fn assert_safe_containment_snapshot(
    action: &shacs_core::runtime::PermissionedAction,
) -> Result<(), Box<dyn Error>> {
    let snapshot = action
        .containment_snapshot
        .as_ref()
        .ok_or("missing containment snapshot")?;
    let serialized = serde_json::to_string(action)?;
    if snapshot.contained != Some(true)
        || serialized.contains("sk-container-secret")
        || serialized.contains("sk-summary-secret")
        || snapshot
            .digest
            .as_deref()
            .is_some_and(|value| value.contains("sk-container-secret"))
        || snapshot
            .summary
            .as_deref()
            .is_some_and(|value| value.contains("sk-summary-secret"))
    {
        return Err(format!("unsafe containment snapshot in action: {serialized}").into());
    }
    Ok(())
}

#[test]
fn bridge_dispatcher_rejects_invalid_arguments_before_execution() -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(CountingTool {
        calls: calls.clone(),
    });
    let executor = RuntimeToolExecutor::new(&registry);
    let catalog = bridge_catalog([("count", "Count executions", ["unused"])]);
    let cases = [
        ("array-args", json!({ "name": "count", "arguments": [] })),
        ("bad-json", json!({ "name": "count", "arguments": "{" })),
        ("scalar-json", json!({ "name": "count", "arguments": "[]" })),
    ];

    for (call_id, arguments) in cases {
        let report = dispatch_bridge_tool_call(
            RuntimeToolCall::new(call_id, "tool_call", arguments),
            Some(&catalog),
            &registry,
            &executor,
            &ToolExecutionContext::default(),
        );
        let messages = report.messages();
        let message = messages.first().ok_or("missing invalid argument message")?;
        if !message.content.contains("Invalid bridge arguments") {
            return Err(format!("invalid arguments were not rejected: {report:?}").into());
        }
    }
    if calls.load(Ordering::SeqCst) != 0 {
        return Err("invalid bridge arguments reached underlying executor".into());
    }
    Ok(())
}

#[test]
fn bridge_dispatcher_preserves_underlying_validation_error_shape() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(NamedRepeatTool("mcp_repeat"));
    let executor = RuntimeToolExecutor::new(&registry);
    let catalog = bridge_catalog([("mcp_repeat", "Repeat deferred text", ["text"])]);
    let report = dispatch_bridge_tool_call(
        RuntimeToolCall::new(
            "invalid-repeat",
            "tool_call",
            json!({ "name": "mcp_repeat", "arguments": {} }),
        ),
        Some(&catalog),
        &registry,
        &executor,
        &ToolExecutionContext::default(),
    );
    let messages = report.messages();
    let message = messages.first().ok_or("missing validation error message")?;
    if message.tool_call_id != "invalid-repeat"
        || message.name != "tool_call"
        || !message.content.contains("Permission denied")
        || !message.content.contains("StaticDeny")
        || !report.permissioned_actions.first().is_some_and(|action| {
            action
                .normalization_errors
                .iter()
                .any(|error| matches!(error, ActionNormalizationError::InvalidArguments { .. }))
        })
    {
        return Err(format!("validation denial shape drifted: {report:?}").into());
    }
    Ok(())
}

#[test]
fn bridge_ask_user_skips_later_denied_tool_without_permission_message() -> Result<(), Box<dyn Error>>
{
    let exec_calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(DeferredAskTool);
    registry.register(ProcExecCountingTool {
        calls: exec_calls.clone(),
    });
    let exec_schema = registry
        .definitions()
        .into_iter()
        .find(|schema| {
            schema
                .get("function")
                .and_then(Value::as_object)
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
                == Some("exec")
        })
        .ok_or("missing exec schema")?;
    let catalog = DeferredToolCatalog::new(
        vec![
            DeferredToolCatalogEntry {
                name: "mcp_ask_user".to_owned(),
                description: "Ask deferred question".to_owned(),
                parameter_names: vec!["question".to_owned()],
                full_schema: registry
                    .definitions()
                    .into_iter()
                    .find(|schema| {
                        schema
                            .get("function")
                            .and_then(Value::as_object)
                            .and_then(|function| function.get("name"))
                            .and_then(Value::as_str)
                            == Some("mcp_ask_user")
                    })
                    .ok_or("missing mcp_ask_user schema")?,
                source_kind: "mcp_tool".to_owned(),
                source_name: "ask".to_owned(),
            },
            DeferredToolCatalogEntry {
                name: "exec".to_owned(),
                description: "Deferred proc exec".to_owned(),
                parameter_names: vec!["command".to_owned()],
                full_schema: exec_schema,
                source_kind: "parent_only".to_owned(),
                source_name: "runtime".to_owned(),
            },
        ],
        2,
        4,
    );

    let report = dispatch_bridge_tool_calls(
        vec![
            RuntimeToolCall::new(
                "ask-bridge",
                "tool_call",
                json!({ "name": "mcp_ask_user", "arguments": { "question": "Continue?" } }),
            ),
            RuntimeToolCall::new(
                "exec-bridge",
                "tool_call",
                json!({ "name": "exec", "arguments": { "command": "cargo test" } }),
            ),
        ],
        Some(&catalog),
        &registry,
        &RuntimeToolExecutor::new(&registry),
        &safe_mcp_tool_context(),
        false,
    );

    if !report.messages().is_empty()
        || report.skipped_tool_calls.len() != 1
        || report.skipped_tool_calls[0].id != "exec-bridge"
        || report.skipped_tool_calls[0].name != "tool_call"
        || exec_calls.load(Ordering::SeqCst) != 0
        || report
            .permissioned_actions
            .iter()
            .any(|action| action.tool_name == "exec")
    {
        return Err(format!(
            "bridge ask_user should skip later denied tool without permission message: {report:?}"
        )
        .into());
    }
    match report.interrupt {
        Some(RuntimeInterrupt::AskUser {
            tool_call_id, name, ..
        }) if tool_call_id == "ask-bridge" && name == "mcp_ask_user" => Ok(()),
        other => Err(format!("unexpected bridge ask_user interrupt: {other:?}").into()),
    }
}

#[test]
fn bridge_dispatcher_propagates_ask_user_interrupt() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(DeferredAskTool);
    registry.register(CountingTool {
        calls: Arc::new(AtomicUsize::new(0)),
    });
    let executor = RuntimeToolExecutor::new(&registry);
    let catalog = bridge_catalog([
        ("mcp_ask_user", "Ask deferred question", ["question"]),
        ("count", "Count executions", ["unused"]),
    ]);
    let report = dispatch_bridge_tool_calls(
        vec![
            RuntimeToolCall::new(
                "ask-bridge",
                "tool_call",
                json!({ "name": "mcp_ask_user", "arguments": { "question": "Continue?" } }),
            ),
            RuntimeToolCall::new(
                "after-bridge",
                "tool_call",
                json!({ "name": "count", "arguments": {} }),
            ),
        ],
        Some(&catalog),
        &registry,
        &executor,
        &safe_mcp_tool_context(),
        false,
    );
    if !report.messages().is_empty()
        || report.skipped_tool_calls.len() != 1
        || report.skipped_tool_calls[0].id != "after-bridge"
        || report.skipped_tool_calls[0].name != "tool_call"
    {
        return Err(format!("ask_user bridge boundary drifted: {report:?}").into());
    }
    match report.interrupt {
        Some(RuntimeInterrupt::AskUser {
            tool_call_id,
            name,
            question,
            options,
        }) if tool_call_id == "ask-bridge"
            && name == "mcp_ask_user"
            && question == "Continue?"
            && options == ["Yes", "No"] =>
        {
            Ok(())
        }
        other => Err(format!("unexpected bridge interrupt: {other:?}").into()),
    }
}

#[test]
fn bridge_mcp_ask_user_requires_proc_exec_permission_before_interrupt() -> Result<(), Box<dyn Error>>
{
    let mut registry = ToolRegistry::new();
    registry.register(DeferredAskTool);
    let executor = RuntimeToolExecutor::new(&registry);
    let catalog = bridge_catalog([("mcp_ask_user", "Ask deferred question", ["question"])]);
    let report = dispatch_bridge_tool_call(
        RuntimeToolCall::new(
            "ask-bridge",
            "tool_call",
            json!({ "name": "mcp_ask_user", "arguments": { "question": "Continue?" } }),
        ),
        Some(&catalog),
        &registry,
        &executor,
        &ToolExecutionContext::default(),
    );
    let messages = report.messages();
    let message = messages.first().ok_or("missing permission denial")?;
    let action = report
        .permissioned_actions
        .first()
        .ok_or("missing permissioned action")?;

    if report.interrupt.is_some()
        || message.tool_call_id != "ask-bridge"
        || message.name != "tool_call"
        || !message.content.contains("Permission denied")
        || !message.content.contains("StaticAskRequired")
        || action.tool_name != "mcp_ask_user"
        || action.capabilities != vec![shacs_core::runtime::SafetyCapability::ProcExec]
    {
        return Err(format!(
            "mcp_ask_user reached interrupt without proc_exec permission: {report:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn bridge_dispatcher_concurrency_uses_underlying_tool_metadata() -> Result<(), Box<dyn Error>> {
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(DelayTool::new(
        "mcp_safe_a",
        true,
        active.clone(),
        max_active.clone(),
        calls.clone(),
    ));
    registry.register(DelayTool::new(
        "mcp_safe_b",
        true,
        active,
        max_active.clone(),
        calls.clone(),
    ));
    let executor = RuntimeToolExecutor::new(&registry);
    let catalog = bridge_catalog([
        ("mcp_safe_a", "Safe deferred A", ["unused"]),
        ("mcp_safe_b", "Safe deferred B", ["unused"]),
    ]);
    let context = safe_mcp_tool_context();
    let report = dispatch_bridge_tool_calls(
        vec![
            RuntimeToolCall::new(
                "safe-a",
                "tool_call",
                json!({ "name": "mcp_safe_a", "arguments": {} }),
            ),
            RuntimeToolCall::new(
                "safe-b",
                "tool_call",
                json!({ "name": "mcp_safe_b", "arguments": {} }),
            ),
        ],
        Some(&catalog),
        &registry,
        &executor,
        &context,
        true,
    );
    let contents = report
        .messages()
        .iter()
        .map(|message| {
            (
                message.tool_call_id.clone(),
                message.name.clone(),
                message.content.clone(),
            )
        })
        .collect::<Vec<_>>();
    if contents
        != [
            (
                "safe-a".to_owned(),
                "tool_call".to_owned(),
                "mcp_safe_a".to_owned(),
            ),
            (
                "safe-b".to_owned(),
                "tool_call".to_owned(),
                "mcp_safe_b".to_owned(),
            ),
        ]
        || max_active.load(Ordering::SeqCst) != 2
        || calls.load(Ordering::SeqCst) != 2
    {
        return Err(
            format!("bridge concurrency did not use underlying metadata: {report:?}").into(),
        );
    }
    Ok(())
}

#[test]
fn runtime_applies_message_and_spawn_context() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let sent = Arc::new(Mutex::new(Vec::<OutboundMessage>::new()));
    let sent_capture = sent.clone();
    let message_tool = MessageTool::with_sender(
        workspace.path(),
        Arc::new(move |message: OutboundMessage| {
            sent_capture
                .lock()
                .map_err(|error| error.to_string())?
                .push(message);
            Ok(())
        }),
        "",
        "",
        None,
    );

    let spawned = Arc::new(Mutex::new(Vec::<SpawnRequest>::new()));
    let spawned_capture = spawned.clone();
    let spawn_tool = SpawnTool::new(Arc::new(move |request: SpawnRequest| {
        spawned_capture
            .lock()
            .map_err(|error| error.to_string())?
            .push(request);
        Ok("spawned".to_owned())
    }));

    let mut registry = ToolRegistry::new();
    registry.register(message_tool.clone());
    registry.register(spawn_tool.clone());
    let executor = RuntimeToolExecutor::with_context_tools(
        &registry,
        RuntimeContextTools::new()
            .with_message(message_tool.clone())
            .with_spawn(spawn_tool),
    );
    let context = ToolExecutionContext {
        channel: "telegram".to_owned(),
        chat_id: "chat-1".to_owned(),
        message_id: Some("msg-1".to_owned()),
        metadata: json!({ "thread": "alpha" }),
        session_key: Some("session-1".to_owned()),
        containment_snapshot: Some(confirmed_containment_ref()),
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::BypassPermissions,
            source: Some("test".to_owned()),
            scope_ref: None,
        },
        permission_rule_input: safe_proc_exec_rule_input(),
        permission_auto_approval: AutoApprovalConfig::default(),
        permission_ceiling_snapshot: None,
        permission_evaluator: None,
        permission_interactive: false,
        permission_approval_cache: None,
        permission_session_approval_cache: Vec::new(),
        permission_session_remembered_rules: Vec::new(),
        project_permission_store: None,
        active_workspace: None,
        in_cron_context: false,
        record_channel_delivery: true,
        cancellation_token: None,
        deadline: None,
    };

    let report = executor.execute_tool_calls(
        vec![
            RuntimeToolCall::new("message", "message", json!({ "content": "hello" })),
            RuntimeToolCall::new("spawn", "spawn", json!({ "task": "check status" })),
        ],
        &context,
    );
    if report.messages.len() != 2 || report.messages[1].content != "spawned" {
        return Err(format!("unexpected context tool report: {report:?}").into());
    }

    let sent = sent.lock().map_err(|error| error.to_string())?;
    let Some(message) = sent.first() else {
        return Err("message tool did not send outbound message".into());
    };
    if message.channel != "telegram"
        || message.chat_id != "chat-1"
        || message.metadata["message_id"] != "msg-1"
        || message.metadata["thread"] != "alpha"
        || message.metadata["_record_channel_delivery"] != true
        || !message_tool.sent_in_turn()
    {
        return Err(format!("message context was not applied: {message:?}").into());
    }

    {
        let spawned = spawned.lock().map_err(|error| error.to_string())?;
        let Some(request) = spawned.first() else {
            return Err("spawn tool did not capture request".into());
        };
        if request.origin_channel != "telegram"
            || request.origin_chat_id != "chat-1"
            || request.session_key != "session-1"
        {
            return Err(format!("spawn context was not applied: {request:?}").into());
        }
    }

    let plain_executor = RuntimeToolExecutor::new(&registry);
    plain_executor.execute_tool_calls(
        vec![RuntimeToolCall::new(
            "spawn-default",
            "spawn",
            json!({ "task": "default context" }),
        )],
        &ToolExecutionContext {
            containment_snapshot: Some(confirmed_containment_ref()),
            permission_mode_snapshot: PermissionModeSnapshot {
                mode: PermissionMode::BypassPermissions,
                source: Some("test".to_owned()),
                scope_ref: None,
            },
            permission_rule_input: safe_proc_exec_rule_input(),
            ..ToolExecutionContext::default()
        },
    );
    let spawned = spawned.lock().map_err(|error| error.to_string())?;
    let Some(default_request) = spawned.get(1) else {
        return Err("spawn tool did not capture default-context request".into());
    };
    if default_request.origin_channel != "cli"
        || default_request.origin_chat_id != "direct"
        || default_request.session_key != "cli:direct"
    {
        return Err(format!("spawn context leaked after execution: {default_request:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_restores_cron_context_guard_after_execution() -> Result<(), Box<dyn Error>> {
    let cron_tool = CronTool::new(Arc::new(InMemoryCronService::new()));
    let mut registry = ToolRegistry::new();
    registry.register(cron_tool.clone());
    cron_tool.set_cron_context(true);

    let executor = RuntimeToolExecutor::with_context_tools(
        &registry,
        RuntimeContextTools::new().with_cron(cron_tool.clone()),
    );
    let report = executor.execute_tool_calls(
        vec![RuntimeToolCall::new(
            "cron",
            "cron",
            json!({ "action": "add", "message": "stretch", "every_seconds": 60 }),
        )],
        &ToolExecutionContext {
            channel: "cli".to_owned(),
            chat_id: "direct".to_owned(),
            session_key: Some("cli:direct".to_owned()),
            containment_snapshot: Some(confirmed_containment_ref()),
            permission_mode_snapshot: PermissionModeSnapshot {
                mode: PermissionMode::BypassPermissions,
                source: Some("test".to_owned()),
                scope_ref: None,
            },
            permission_rule_input: PermissionRuleInput {
                containment: confirmed_containment(),
                ..PermissionRuleInput::default()
            },
            ..ToolExecutionContext::default()
        },
    );
    if report.messages.len() != 1 || !report.messages[0].content.contains("Created job") {
        return Err(
            format!("runtime cron execution did not temporarily clear guard: {report:?}").into(),
        );
    }

    let direct = cron_tool
        .execute(json_map(json!({
            "action": "add",
            "message": "should fail",
            "every_seconds": 60
        }))?)
        .into_text();
    cron_tool.reset_cron_context(false);
    if direct != "Error: cannot schedule new jobs from within a cron job execution" {
        return Err(format!("cron context guard was not restored: {direct}").into());
    }
    Ok(())
}

fn json_map(value: Value) -> Result<JsonMap, Box<dyn Error>> {
    match value {
        Value::Object(map) => Ok(map),
        other => Err(format!("expected object, got {other}").into()),
    }
}

fn parse_json_content(content: &str) -> Result<Value, Box<dyn Error>> {
    serde_json::from_str(content).map_err(Into::into)
}

fn bridge_catalog<const COUNT: usize, const PARAMETER_COUNT: usize>(
    entries: [(&str, &str, [&str; PARAMETER_COUNT]); COUNT],
) -> DeferredToolCatalog {
    DeferredToolCatalog::new(
        entries
            .into_iter()
            .map(
                |(name, description, parameter_names)| DeferredToolCatalogEntry {
                    name: name.to_owned(),
                    description: description.to_owned(),
                    parameter_names: parameter_names.into_iter().map(str::to_owned).collect(),
                    full_schema: runtime_tool_schema(name, description, parameter_names),
                    source_kind: "mcp_tool".to_owned(),
                    source_name: "test".to_owned(),
                },
            )
            .collect(),
        2,
        3,
    )
}

fn runtime_tool_schema<const PARAMETER_COUNT: usize>(
    name: &str,
    description: &str,
    parameter_names: [&str; PARAMETER_COUNT],
) -> Value {
    let mut properties = serde_json::Map::new();
    for parameter_name in parameter_names {
        properties.insert(
            parameter_name.to_owned(),
            json!({ "type": "string", "description": parameter_name }),
        );
    }
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": {
                "type": "object",
                "properties": properties,
                "required": []
            }
        }
    })
}
