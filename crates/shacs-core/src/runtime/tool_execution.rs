use crate::runtime::permission_pattern::{
    session_approval_reuse_matches, session_approval_reuse_pattern,
};
use crate::runtime::{
    correlate_approval, decide_permission, evaluate_static_rules, normalize_runtime_tool_call,
    ApprovalCacheEntry, ApprovalCorrelation, ApprovalCorrelationError, ApprovalDecisionKind,
    ApprovalRequest, AutoEvaluatorVerdict, AutoEvaluatorVerdictKind, ContainmentSnapshotRef,
    EvaluatorConfidence, EvaluatorScopeMatch, InheritedPermissionContext,
    PermissionCeilingSnapshot, PermissionMode, PermissionModeSnapshot, PermissionPolicyDecision,
    PermissionPolicyDecisionKind, PermissionPolicyInput, PermissionRuleInput, PermissionedAction,
    PermissionedActionInput, PermissionedActionOrigin, ProcExecSummary, RecentAutoModeDenial,
    RecentAutoModeRetryToken, SafetyCapability, SessionApprovalCacheEntry, StaticRuleDecision,
    StaticRuleDecisionKind,
};
use crate::tools::{CronTool, MessageTool, SpawnTool, ToolRegistry, ToolResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use shacs_config::AutoApprovalConfig;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

const ERROR_HINT: &str = "\n\n[Analyze the error above and try a different approach.]";
const PERMISSION_APPROVAL_TTL_MS: u64 = 10 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

impl RuntimeToolCall {
    pub fn new(id: impl Into<String>, name: impl Into<String>, arguments: Value) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            arguments,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeToolMessage {
    pub tool_call_id: String,
    pub name: String,
    pub content: String,
}

impl RuntimeToolMessage {
    pub fn to_json(&self) -> Value {
        json!({
            "role": "tool",
            "tool_call_id": self.tool_call_id,
            "name": self.name,
            "content": self.content,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAssistantToolCallMessage {
    pub content: Option<String>,
    pub tool_calls: Vec<RuntimeToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_blocks: Option<Vec<Value>>,
}

impl RuntimeAssistantToolCallMessage {
    pub fn new(content: Option<String>, tool_calls: Vec<RuntimeToolCall>) -> Self {
        Self {
            content,
            tool_calls,
            reasoning_content: None,
            thinking_blocks: None,
        }
    }

    pub fn with_reasoning_content(mut self, reasoning_content: Option<String>) -> Self {
        self.reasoning_content = reasoning_content;
        self
    }

    pub fn with_thinking_blocks(mut self, thinking_blocks: Option<Vec<Value>>) -> Self {
        self.thinking_blocks = thinking_blocks;
        self
    }

    pub fn to_json(&self) -> Value {
        let tool_calls = self
            .tool_calls
            .iter()
            .map(|call| {
                json!({
                    "id": call.id,
                    "type": "function",
                    "function": {
                        "name": call.name,
                        "arguments": call.arguments.to_string(),
                    }
                })
            })
            .collect::<Vec<_>>();
        let mut message = Map::from_iter([
            ("role".to_owned(), Value::String("assistant".to_owned())),
            (
                "content".to_owned(),
                self.content
                    .as_ref()
                    .map_or(Value::Null, |content| Value::String(content.clone())),
            ),
            ("tool_calls".to_owned(), Value::Array(tool_calls)),
        ]);
        if let Some(reasoning_content) = &self.reasoning_content {
            message.insert(
                "reasoning_content".to_owned(),
                Value::String(reasoning_content.clone()),
            );
        }
        if let Some(thinking_blocks) = &self.thinking_blocks {
            message.insert(
                "thinking_blocks".to_owned(),
                Value::Array(thinking_blocks.clone()),
            );
        }
        Value::Object(message)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RuntimeInterrupt {
    AskUser {
        tool_call_id: String,
        name: String,
        question: String,
        options: Vec<String>,
    },
    PermissionApproval {
        approval_request_id: String,
        approval_request: Box<ApprovalRequest>,
        tool_call: RuntimeToolCall,
        question: String,
        options: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeToolExecutionReport {
    pub messages: Vec<RuntimeToolMessage>,
    pub interrupt: Option<RuntimeInterrupt>,
    pub skipped_tool_calls: Vec<RuntimeToolCall>,
    #[serde(default)]
    pub permissioned_actions: Vec<PermissionedAction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_auto_mode_denials: Vec<RecentAutoModeDenial>,
    #[serde(skip, default)]
    pub recent_auto_mode_retry_tokens: Vec<RecentAutoModeRetryToken>,
}

impl RuntimeToolExecutionReport {
    pub fn completed(messages: Vec<RuntimeToolMessage>) -> Self {
        Self {
            messages,
            interrupt: None,
            skipped_tool_calls: Vec::new(),
            permissioned_actions: Vec::new(),
            recent_auto_mode_denials: Vec::new(),
            recent_auto_mode_retry_tokens: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolExecutionContext {
    pub channel: String,
    pub chat_id: String,
    pub message_id: Option<String>,
    pub metadata: Value,
    pub session_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub containment_snapshot: Option<ContainmentSnapshotRef>,
    #[serde(default)]
    pub permission_mode_snapshot: PermissionModeSnapshot,
    #[serde(default)]
    pub permission_rule_input: PermissionRuleInput,
    #[serde(default)]
    pub permission_auto_approval: AutoApprovalConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_ceiling_snapshot: Option<PermissionCeilingSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_evaluator: Option<AutoEvaluatorVerdict>,
    #[serde(default)]
    pub permission_interactive: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_approval_cache: Option<ApprovalCacheEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permission_session_approval_cache: Vec<SessionApprovalCacheEntry>,
    pub in_cron_context: bool,
    pub record_channel_delivery: bool,
}

impl Default for ToolExecutionContext {
    fn default() -> Self {
        Self {
            channel: String::new(),
            chat_id: String::new(),
            message_id: None,
            metadata: Value::Object(Map::new()),
            session_key: None,
            containment_snapshot: None,
            permission_mode_snapshot: PermissionModeSnapshot::default(),
            permission_rule_input: PermissionRuleInput::default(),
            permission_auto_approval: AutoApprovalConfig::default(),
            permission_ceiling_snapshot: None,
            permission_evaluator: None,
            permission_interactive: false,
            permission_approval_cache: None,
            permission_session_approval_cache: Vec::new(),
            in_cron_context: false,
            record_channel_delivery: false,
        }
    }
}

#[derive(Default, Clone)]
pub struct RuntimeContextTools {
    pub message: Option<MessageTool>,
    pub cron: Option<CronTool>,
    pub spawn: Option<SpawnTool>,
}

impl RuntimeContextTools {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_message(mut self, tool: MessageTool) -> Self {
        self.message = Some(tool);
        self
    }

    pub fn with_cron(mut self, tool: CronTool) -> Self {
        self.cron = Some(tool);
        self
    }

    pub fn with_spawn(mut self, tool: SpawnTool) -> Self {
        self.spawn = Some(tool);
        self
    }
}

pub struct RuntimeToolExecutor<'a> {
    registry: &'a ToolRegistry,
    context_tools: RuntimeContextTools,
}

impl<'a> RuntimeToolExecutor<'a> {
    pub fn new(registry: &'a ToolRegistry) -> Self {
        Self {
            registry,
            context_tools: RuntimeContextTools::new(),
        }
    }

    pub fn with_context_tools(
        registry: &'a ToolRegistry,
        context_tools: RuntimeContextTools,
    ) -> Self {
        Self {
            registry,
            context_tools,
        }
    }

    pub(crate) fn registry(&self) -> &ToolRegistry {
        self.registry
    }

    pub fn execute_tool_calls(
        &self,
        tool_calls: Vec<RuntimeToolCall>,
        context: &ToolExecutionContext,
    ) -> RuntimeToolExecutionReport {
        self.execute_tool_calls_with_mode(tool_calls, context, false)
    }

    pub fn execute_tool_calls_concurrent(
        &self,
        tool_calls: Vec<RuntimeToolCall>,
        context: &ToolExecutionContext,
    ) -> RuntimeToolExecutionReport {
        self.execute_tool_calls_with_mode(tool_calls, context, true)
    }

    fn execute_tool_calls_with_mode(
        &self,
        tool_calls: Vec<RuntimeToolCall>,
        context: &ToolExecutionContext,
        concurrent_tools: bool,
    ) -> RuntimeToolExecutionReport {
        let _guard = AppliedToolContext::apply(&self.context_tools, context);
        let mut messages = Vec::new();
        let all_calls = tool_calls.clone();
        let mut pending_batch = Vec::new();
        let mut permissioned_actions = Vec::new();

        for (original_index, call) in tool_calls.into_iter().enumerate() {
            let action = normalize_runtime_tool_call(
                self.registry,
                &call,
                permissioned_action_input_from_context(context),
            );
            let decision = permission_decision_for_action(&action, context);
            permissioned_actions.push(action.clone());
            if !decision.can_handoff_to_tool_runtime {
                if let Some(report) = flush_allowed_batch(
                    self.registry,
                    &mut pending_batch,
                    concurrent_tools,
                    &mut messages,
                    &all_calls,
                    &permissioned_actions,
                ) {
                    return report;
                }
                if decision.kind == PermissionPolicyDecisionKind::Ask {
                    return RuntimeToolExecutionReport {
                        messages,
                        interrupt: Some(permission_approval_interrupt(call, &action)),
                        skipped_tool_calls: all_calls[original_index + 1..].to_vec(),
                        permissioned_actions,
                        recent_auto_mode_denials: Vec::new(),
                        recent_auto_mode_retry_tokens: Vec::new(),
                    };
                }
                messages.push(permission_block_message(&call.id, &call.name, &decision));
                continue;
            }

            let entry = IndexedToolCall {
                original_index,
                call,
            };
            if concurrent_tools && can_batch_concurrently(self.registry, &entry.call) {
                pending_batch.push(entry);
                continue;
            }
            if let Some(report) = flush_allowed_batch(
                self.registry,
                &mut pending_batch,
                concurrent_tools,
                &mut messages,
                &all_calls,
                &permissioned_actions,
            ) {
                return report;
            }
            pending_batch.push(entry);
            if let Some(report) = flush_allowed_batch(
                self.registry,
                &mut pending_batch,
                concurrent_tools,
                &mut messages,
                &all_calls,
                &permissioned_actions,
            ) {
                return report;
            }
        }

        if let Some(report) = flush_allowed_batch(
            self.registry,
            &mut pending_batch,
            concurrent_tools,
            &mut messages,
            &all_calls,
            &permissioned_actions,
        ) {
            return report;
        }

        RuntimeToolExecutionReport {
            messages,
            interrupt: None,
            skipped_tool_calls: Vec::new(),
            permissioned_actions,
            recent_auto_mode_denials: Vec::new(),
            recent_auto_mode_retry_tokens: Vec::new(),
        }
    }
}

pub(crate) fn permissioned_action_input_from_context(
    context: &ToolExecutionContext,
) -> PermissionedActionInput {
    let channel = non_empty_or(&context.channel, "cli");
    let chat_id = non_empty_or(&context.chat_id, "direct");
    let session_id = context
        .session_key
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("{channel}:{chat_id}"));
    let turn_id = context
        .message_id
        .clone()
        .or_else(|| {
            context
                .metadata
                .get("message_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| format!("turn:{session_id}"));
    let subagent_id = context
        .metadata
        .get("subagent_task_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned);
    let origin = if subagent_id.is_some() {
        PermissionedActionOrigin::Subagent { subagent_id }
    } else if context.in_cron_context {
        PermissionedActionOrigin::CronWake { job_id: None }
    } else if context.channel.trim().is_empty() {
        PermissionedActionOrigin::UserTurn
    } else {
        PermissionedActionOrigin::ChannelInbound {
            channel,
            message_id: context.message_id.clone(),
        }
    };

    PermissionedActionInput {
        session_id,
        turn_id,
        origin,
        permission_mode_snapshot: context.permission_mode_snapshot.clone(),
        containment_snapshot: context.containment_snapshot.clone(),
        intent_snapshot: None,
    }
}

pub(crate) fn permission_decision_for_action(
    action: &PermissionedAction,
    context: &ToolExecutionContext,
) -> PermissionPolicyDecision {
    let rule_input = effective_permission_rule_input(action, context);
    let static_rule_decision = evaluate_static_rules(action, &rule_input);
    let evaluator = context.permission_evaluator.clone().or_else(|| {
        auto_approval_evaluator_for_action(action, &static_rule_decision, &rule_input, context)
    });
    decide_permission(PermissionPolicyInput {
        action: action.clone(),
        static_rule_decision,
        evaluator,
        approval: permission_approval_for_action(action, context),
        inherited_context: inherited_context_for_action(action, context),
        interactive: context.permission_interactive,
    })
}

pub(crate) fn effective_permission_rule_input(
    action: &PermissionedAction,
    context: &ToolExecutionContext,
) -> PermissionRuleInput {
    let mut input = context.permission_rule_input.clone();
    for protected_target in &context.permission_auto_approval.protected_targets {
        if !input.protected_targets.contains(protected_target) {
            input.protected_targets.push(protected_target.clone());
        }
    }
    if input.proc_exec_summary.is_none()
        && action.capabilities.contains(&SafetyCapability::ProcExec)
    {
        input.proc_exec_summary = proc_exec_summary_for_action(action);
    }
    input
}

fn proc_exec_summary_for_action(action: &PermissionedAction) -> Option<ProcExecSummary> {
    if action.tool_name != "exec" {
        return None;
    }
    let command = action
        .redacted_arguments
        .get("command")
        .and_then(Value::as_str)?;
    let tokens = simple_verification_command_tokens(command)?;
    let family = proc_exec_command_family(&tokens)?;
    Some(ProcExecSummary {
        command_family: family,
        target_refs: Vec::new(),
        destructive: false,
        network: false,
        secret_exposure: false,
        summary_available: true,
    })
}

fn simple_verification_command_tokens(command: &str) -> Option<Vec<&str>> {
    if command.is_empty()
        || command.len() > 200
        || command.chars().any(|character| {
            matches!(
                character,
                '\n' | '\r' | ';' | '&' | '|' | '<' | '>' | '`' | '$' | '(' | ')'
            )
        })
    {
        return None;
    }
    let command = command.trim();
    if command.is_empty() {
        return None;
    }
    let tokens = command.split_whitespace().collect::<Vec<_>>();
    (!tokens.is_empty()).then_some(tokens)
}

fn proc_exec_command_family(tokens: &[&str]) -> Option<String> {
    match tokens.first().copied()? {
        "pwd" if tokens.len() == 1 => Some("pwd".to_owned()),
        "cargo" => cargo_verification_family(tokens),
        _ => None,
    }
}

fn cargo_verification_family(tokens: &[&str]) -> Option<String> {
    let subcommand = tokens.get(1).copied()?;
    let allowed = matches!(tokens, ["cargo", "check" | "test" | "clippy" | "build"])
        || matches!(tokens, ["cargo", "fmt", "--check"]);
    allowed.then(|| format!("cargo {subcommand}"))
}

fn auto_approval_evaluator_for_action(
    action: &PermissionedAction,
    static_rule_decision: &StaticRuleDecision,
    rule_input: &PermissionRuleInput,
    context: &ToolExecutionContext,
) -> Option<AutoEvaluatorVerdict> {
    let config = &context.permission_auto_approval;
    if !config.enabled
        || action.permission_mode_snapshot.mode != PermissionMode::Auto
        || static_rule_decision.kind != StaticRuleDecisionKind::AllowCandidate
        || !auto_approval_allows_capabilities(action, config, rule_input)
    {
        return None;
    }

    Some(AutoEvaluatorVerdict {
        verdict: AutoEvaluatorVerdictKind::AllowCandidate,
        confidence: EvaluatorConfidence::High,
        scope_match: EvaluatorScopeMatch::Requested,
        risk_summary: "allowed by local autoApproval static rules".to_owned(),
        evidence_refs: vec!["permissions.autoApproval".to_owned()],
        expires_at_unix_ms: now_unix_ms().saturating_add(PERMISSION_APPROVAL_TTL_MS),
        evaluator_ref: Some("local-auto-approval".to_owned()),
        prompt_injection_signals: Vec::new(),
    })
}

fn auto_approval_allows_capabilities(
    action: &PermissionedAction,
    config: &AutoApprovalConfig,
    rule_input: &PermissionRuleInput,
) -> bool {
    !action.capabilities.is_empty()
        && action
            .capabilities
            .iter()
            .all(|capability| match capability {
                SafetyCapability::FsRead => true,
                SafetyCapability::FsWrite => config.allow_workspace_edits,
                SafetyCapability::ProcExec => {
                    config.allow_proc_exec_verification
                        && (!config.require_docker_containment_for_exec
                            || rule_input.containment.confirmed_non_privileged())
                }
                SafetyCapability::NetOutbound
                | SafetyCapability::SecretRead
                | SafetyCapability::ExternalDelivery
                | SafetyCapability::AutomationSchedule
                | SafetyCapability::AppInstall
                | SafetyCapability::RuntimeConfigWrite
                | SafetyCapability::SelfModification => false,
            })
}

pub(crate) fn session_approval_context_digest(action: &PermissionedAction) -> String {
    digest_json(&json!({
        "permission_mode_snapshot": &action.permission_mode_snapshot,
        "containment_snapshot": &action.containment_snapshot,
        "intent_snapshot": &action.intent_snapshot,
        "session_id": &action.session_id,
        "origin": stable_session_approval_origin(&action.origin),
    }))
}

fn stable_session_approval_origin(origin: &PermissionedActionOrigin) -> Value {
    match origin {
        PermissionedActionOrigin::UserTurn => json!({ "kind": "user_turn" }),
        PermissionedActionOrigin::Subagent { subagent_id } => {
            json!({ "kind": "subagent", "subagent_id": subagent_id })
        }
        PermissionedActionOrigin::CronWake { job_id } => {
            json!({ "kind": "cron_wake", "job_id": job_id })
        }
        PermissionedActionOrigin::AppTask { app_id, task_id } => {
            json!({ "kind": "app_task", "app_id": app_id, "task_id": task_id })
        }
        PermissionedActionOrigin::LocalApi { request_id } => {
            json!({ "kind": "local_api", "request_id": request_id })
        }
        PermissionedActionOrigin::ChannelInbound { channel, .. } => {
            json!({ "kind": "channel_inbound", "channel": channel })
        }
        PermissionedActionOrigin::DeferredBridge {
            bridge_name,
            scope_digest,
            parent_origin,
            ..
        } => json!({
            "kind": "deferred_bridge",
            "bridge_name": bridge_name,
            "scope_digest": scope_digest,
            "parent_origin": stable_session_approval_origin(parent_origin),
        }),
    }
}

pub(crate) fn permission_approval_interrupt(
    call: RuntimeToolCall,
    action: &PermissionedAction,
) -> RuntimeInterrupt {
    let now = now_unix_ms();
    let approval_request_id = format!(
        "approval_{}",
        action.action_id.chars().take(16).collect::<String>()
    );
    let expires_at_unix_ms = now.saturating_add(PERMISSION_APPROVAL_TTL_MS);
    let risk_summary = format!("Run tool `{}`", action.tool_name);
    let approval_request = ApprovalRequest {
        approval_request_id: approval_request_id.clone(),
        action_digest: action.action_digest.clone(),
        snapshot_digest: action.snapshot_digest.clone(),
        requested_scope: action.session_id.clone(),
        risk_summary: risk_summary.clone(),
        allowed_decisions: vec![
            ApprovalDecisionKind::Approved,
            ApprovalDecisionKind::ApprovedForSession,
            ApprovalDecisionKind::Denied,
        ],
        expires_at_unix_ms,
    };
    let arguments = action.redacted_arguments.to_string();
    let target_summary = approval_target_summary(action);
    let session_reuse_summary =
        session_approval_reuse_pattern(action).unwrap_or_else(|| "exact action only".to_owned());
    RuntimeInterrupt::PermissionApproval {
        approval_request_id: approval_request_id.clone(),
        approval_request: Box::new(approval_request),
        tool_call: call,
        question: format!(
            "Permission approval required before running tool `{}`.\n\nRisk: {}\nTarget: {}\nScope: {}\nSession reuse: {}\nExpires at (unix ms): {}\nArguments: `{}`\n\nReply with `1` or `approve` to run it once, `3` or `approve_session` to approve matching actions in this session, or `2` or `deny` to cancel.\n\nApproval id: `{}`",
            action.tool_name,
            risk_summary,
            target_summary,
            action.session_id,
            session_reuse_summary,
            expires_at_unix_ms,
            arguments,
            approval_request_id
        ),
        options: vec![
            "approve".to_owned(),
            "deny".to_owned(),
            "approve_session".to_owned(),
        ],
    }
}

fn approval_target_summary(action: &PermissionedAction) -> String {
    if action.target_refs.is_empty() {
        return "none".to_owned();
    }
    action
        .target_refs
        .iter()
        .map(|target| format!("{}={}", target.kind, target.redacted_value))
        .collect::<Vec<_>>()
        .join(", ")
}

fn permission_approval_for_action(
    action: &PermissionedAction,
    context: &ToolExecutionContext,
) -> Option<ApprovalCorrelation> {
    if let Some(entry) = &context.permission_approval_cache {
        return Some(approval_cache_correlation(entry, action));
    }
    for entry in &context.permission_session_approval_cache {
        let Some(session_key) = context.session_key.as_deref() else {
            continue;
        };
        if entry.session_key != session_key {
            continue;
        }
        if entry.approval.decision.decision != ApprovalDecisionKind::ApprovedForSession {
            continue;
        }
        let correlation = session_approval_cache_correlation(entry, action);
        if let Some(correlation) = correlation {
            return Some(correlation);
        }
    }
    None
}

fn session_approval_cache_correlation(
    entry: &SessionApprovalCacheEntry,
    action: &PermissionedAction,
) -> Option<ApprovalCorrelation> {
    let approval = &entry.approval;
    if !session_approval_reuse_matches(&entry.reuse_match, &approval.request.action_digest, action)
    {
        return None;
    }
    if approval.request.requested_scope != action.session_id {
        return None;
    }
    if entry.approval_context_digest != session_approval_context_digest(action) {
        return None;
    }
    if approval.request.approval_request_id != approval.decision.approval_request_id
        || approval.request.action_digest != approval.decision.action_digest
        || approval.request.snapshot_digest != approval.decision.snapshot_digest
        || approval.request.requested_scope != approval.decision.approved_scope
        || approval.decision.consumed
        || !approval
            .request
            .allowed_decisions
            .contains(&approval.decision.decision)
        || approval.decision.decision != ApprovalDecisionKind::ApprovedForSession
        || now_unix_ms() > approval.request.expires_at_unix_ms
        || approval.decision.decided_at_unix_ms > approval.request.expires_at_unix_ms
    {
        return None;
    }
    Some(ApprovalCorrelation::approved(
        approval.request.approval_request_id.clone(),
    ))
}

fn approval_cache_correlation(
    entry: &ApprovalCacheEntry,
    action: &PermissionedAction,
) -> ApprovalCorrelation {
    let correlation = correlate_approval(&entry.request, &entry.decision, now_unix_ms());
    if !correlation.is_approved() {
        return correlation;
    }
    if entry.request.action_digest != action.action_digest {
        return ApprovalCorrelation::rejected(ApprovalCorrelationError::ActionMismatch);
    }
    if entry.request.snapshot_digest != action.snapshot_digest {
        return ApprovalCorrelation::rejected(ApprovalCorrelationError::SnapshotMismatch);
    }
    if entry.request.requested_scope != action.session_id {
        return ApprovalCorrelation::rejected(ApprovalCorrelationError::ScopeMismatch);
    }
    correlation
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn digest_json(value: &Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.to_string().as_bytes());
    format!("{:x}", hasher.finalize())
}

fn inherited_context_for_action(
    action: &PermissionedAction,
    context: &ToolExecutionContext,
) -> Option<InheritedPermissionContext> {
    context
        .permission_ceiling_snapshot
        .clone()
        .map(|ceiling| InheritedPermissionContext {
            ceiling,
            requested_mode: action.permission_mode_snapshot.mode,
            requested_capabilities: action.capabilities.clone(),
            per_action_evaluation_required: true,
        })
}

pub(crate) fn permission_block_message(
    tool_call_id: &str,
    name: &str,
    decision: &PermissionPolicyDecision,
) -> RuntimeToolMessage {
    let label = match decision.kind {
        PermissionPolicyDecisionKind::Allow => "Permission allowed",
        PermissionPolicyDecisionKind::Ask => "Permission approval required",
        PermissionPolicyDecisionKind::Deny => "Permission denied",
    };
    RuntimeToolMessage {
        tool_call_id: tool_call_id.to_owned(),
        name: name.to_owned(),
        content: format!(
            "{label}: tool call was not executed (reason: {:?}).",
            decision.reason
        ),
    }
}

fn non_empty_or(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_owned()
    } else {
        trimmed.to_owned()
    }
}

#[derive(Debug, Clone)]
struct IndexedToolCall {
    original_index: usize,
    call: RuntimeToolCall,
}

struct ToolCallOutcome {
    original_index: usize,
    call: RuntimeToolCall,
    outcome: ToolResult,
}

fn flush_allowed_batch(
    registry: &ToolRegistry,
    batch: &mut Vec<IndexedToolCall>,
    concurrent_tools: bool,
    messages: &mut Vec<RuntimeToolMessage>,
    all_calls: &[RuntimeToolCall],
    permissioned_actions: &[PermissionedAction],
) -> Option<RuntimeToolExecutionReport> {
    if batch.is_empty() {
        return None;
    }
    let results = if concurrent_tools && batch.len() > 1 {
        execute_concurrent_batch(registry, batch)
    } else {
        execute_sequential_batch(registry, batch)
    };
    batch.clear();
    for result in results {
        match result.outcome {
            ToolResult::AskUserInterrupt { question, options } => {
                return Some(RuntimeToolExecutionReport {
                    messages: std::mem::take(messages),
                    interrupt: Some(RuntimeInterrupt::AskUser {
                        tool_call_id: result.call.id,
                        name: result.call.name,
                        question,
                        options,
                    }),
                    skipped_tool_calls: all_calls[result.original_index + 1..].to_vec(),
                    permissioned_actions: permissioned_actions.to_vec(),
                    recent_auto_mode_denials: Vec::new(),
                    recent_auto_mode_retry_tokens: Vec::new(),
                });
            }
            ToolResult::Text(content) => messages.push(RuntimeToolMessage {
                tool_call_id: result.call.id,
                name: result.call.name,
                content: append_error_hint(content),
            }),
            ToolResult::Json(value) => messages.push(RuntimeToolMessage {
                tool_call_id: result.call.id,
                name: result.call.name,
                content: value.to_string(),
            }),
        }
    }
    None
}

fn can_batch_concurrently(registry: &ToolRegistry, call: &RuntimeToolCall) -> bool {
    registry
        .get(&call.name)
        .is_some_and(|tool| tool.concurrency_safe())
}

fn execute_sequential_batch(
    registry: &ToolRegistry,
    batch: &[IndexedToolCall],
) -> Vec<ToolCallOutcome> {
    let mut outcomes = Vec::new();
    for entry in batch {
        let outcome = execute_one_tool(registry, &entry.call);
        let is_interrupt = matches!(outcome, ToolResult::AskUserInterrupt { .. });
        outcomes.push(ToolCallOutcome {
            original_index: entry.original_index,
            call: entry.call.clone(),
            outcome,
        });
        if is_interrupt {
            break;
        }
    }
    outcomes
}

fn execute_concurrent_batch(
    registry: &ToolRegistry,
    batch: &[IndexedToolCall],
) -> Vec<ToolCallOutcome> {
    let handles = batch
        .iter()
        .map(|entry| {
            let original_index = entry.original_index;
            let fallback_call = entry.call.clone();
            let call = entry.call.clone();
            let prepared = registry.prepare_call(&call.name, call.arguments.clone());
            let handle = thread::spawn(move || {
                let outcome = match prepared {
                    Ok(prepared) => prepared.tool.execute(prepared.params),
                    Err(error) => ToolResult::Text(format!("{error}{ERROR_HINT}")),
                };
                ToolCallOutcome {
                    original_index,
                    call,
                    outcome,
                }
            });
            (original_index, fallback_call, handle)
        })
        .collect::<Vec<_>>();

    handles
        .into_iter()
        .map(
            |(original_index, fallback_call, handle)| match handle.join() {
                Ok(outcome) => outcome,
                Err(_) => ToolCallOutcome {
                    original_index,
                    call: fallback_call,
                    outcome: ToolResult::Text(format!("Error: tool thread panicked{ERROR_HINT}")),
                },
            },
        )
        .collect()
}

fn execute_one_tool(registry: &ToolRegistry, call: &RuntimeToolCall) -> ToolResult {
    match registry.prepare_call(&call.name, call.arguments.clone()) {
        Ok(prepared) => prepared.tool.execute(prepared.params),
        Err(error) => ToolResult::Text(format!("{error}{ERROR_HINT}")),
    }
}

fn append_error_hint(content: String) -> String {
    if content.starts_with("Error") {
        format!("{content}{ERROR_HINT}")
    } else {
        content
    }
}

struct AppliedToolContext<'a> {
    message: Option<(&'a MessageTool, bool)>,
    cron: Option<(&'a CronTool, bool)>,
    spawn: Option<&'a SpawnTool>,
}

impl<'a> AppliedToolContext<'a> {
    fn apply(tools: &'a RuntimeContextTools, context: &ToolExecutionContext) -> Self {
        let message = tools.message.as_ref().map(|tool| {
            tool.set_context(
                context.channel.clone(),
                context.chat_id.clone(),
                context.message_id.clone(),
                Some(context.metadata.clone()),
            );
            let previous = tool.set_record_channel_delivery(context.record_channel_delivery);
            (tool, previous)
        });

        let cron = tools.cron.as_ref().map(|tool| {
            tool.set_context(
                context.channel.clone(),
                context.chat_id.clone(),
                Some(context.metadata.clone()),
                context.session_key.clone(),
            );
            let previous = tool.set_cron_context(context.in_cron_context);
            (tool, previous)
        });

        let spawn = tools.spawn.as_ref().map(|tool| {
            tool.set_context(
                context.channel.clone(),
                context.chat_id.clone(),
                context.session_key.clone(),
            );
            tool
        });

        Self {
            message,
            cron,
            spawn,
        }
    }
}

impl Drop for AppliedToolContext<'_> {
    fn drop(&mut self) {
        if let Some((tool, previous)) = self.message {
            tool.reset_record_channel_delivery(previous);
        }
        if let Some((tool, previous)) = self.cron {
            tool.reset_cron_context(previous);
        }
        if let Some(tool) = self.spawn {
            tool.clear_context();
        }
    }
}
