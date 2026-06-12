use crate::runtime::tool_execution::permissioned_action_input_from_context;
use crate::runtime::{
    normalize_resolved_deferred_tool_call, PermissionedAction, RuntimeInterrupt, RuntimeToolCall,
    RuntimeToolExecutionReport, RuntimeToolExecutor, RuntimeToolMessage, ToolExecutionContext,
    ToolSearchMode, ToolSearchRuntimeInput,
};
use crate::tools::{
    bridge_tool_names, ActivationState, DeferredToolCatalog, ToolRegistry, ToolSurfaceAssembly,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use shacs_eval::evaluator::{EvidenceKind, EvidenceRef, RedactionStatus};
use shacs_utils::redaction::redact_string;
use std::collections::BTreeMap;
use std::fmt;

const TOOL_SEARCH: &str = "tool_search";
const TOOL_DESCRIBE: &str = "tool_describe";
const TOOL_CALL: &str = "tool_call";
const ERROR_HINT: &str = "\n\n[Analyze the error above and try a different approach.]";
const BRIDGE_MAPPING_EVIDENCE_OWNER_SPEC: &str = "020";
const BRIDGE_MAPPING_SUMMARY_MAX_CHARS: usize = 120;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSearchActivationReason {
    Off,
    Threshold,
    ForcedOn,
    NoDeferrableTools,
    BridgeCollision,
    UnknownContextWindow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSearchDiagnosticsSummary {
    pub mode: String,
    pub activated: bool,
    pub reason: ToolSearchActivationReason,
    pub visible_count: usize,
    pub deferred_count: usize,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub deferred_source_counts: BTreeMap<String, usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_digest: Option<String>,
}

impl ToolSearchDiagnosticsSummary {
    pub fn from_assembly(runtime: ToolSearchRuntimeInput, assembly: &ToolSurfaceAssembly) -> Self {
        let activated = matches!(assembly.activation_state, ActivationState::Activated);
        let reason = match &assembly.activation_state {
            ActivationState::Activated if runtime.config.enabled == ToolSearchMode::On => {
                ToolSearchActivationReason::ForcedOn
            }
            ActivationState::Activated => ToolSearchActivationReason::Threshold,
            ActivationState::ThresholdPassThrough { .. } => ToolSearchActivationReason::Threshold,
            ActivationState::CollisionPassThrough { .. } => {
                ToolSearchActivationReason::BridgeCollision
            }
            ActivationState::UnknownContextPassThrough { .. } => {
                ToolSearchActivationReason::UnknownContextWindow
            }
            ActivationState::PassThrough if runtime.config.enabled == ToolSearchMode::Off => {
                ToolSearchActivationReason::Off
            }
            ActivationState::PassThrough => ToolSearchActivationReason::NoDeferrableTools,
        };
        let deferred_count = assembly
            .catalog
            .as_ref()
            .map(|catalog| catalog.entries.len())
            .unwrap_or(0);
        let scope_digest = assembly
            .catalog
            .as_ref()
            .map(|catalog| catalog.scope_digest.clone());
        let deferred_source_counts = assembly
            .catalog
            .as_ref()
            .map(|catalog| catalog.source_kind_counts())
            .unwrap_or_default();

        Self {
            mode: tool_search_mode_label(runtime.config.enabled).to_owned(),
            activated,
            reason,
            visible_count: assembly.provider_tools.len(),
            deferred_count,
            deferred_source_counts,
            scope_digest,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSearchQueryEvidence {
    pub redacted_query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    pub matched_names: Vec<String>,
    pub scope_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDescribeEvidence {
    pub requested_name: String,
    pub found: bool,
    pub scope_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeUnderlyingMappingEvidence {
    pub bridge_call_id: String,
    pub bridge_name: String,
    pub underlying_name: String,
    pub scope_digest: String,
}

pub fn bridge_underlying_mapping_evidence_ref(
    evidence: &BridgeUnderlyingMappingEvidence,
) -> EvidenceRef {
    let sanitized = json!({
        "bridge_call_id": redacted_mapping_component(&evidence.bridge_call_id),
        "bridge_name": redacted_mapping_component(&evidence.bridge_name),
        "underlying_name": redacted_mapping_component(&evidence.underlying_name),
        "scope_digest": redacted_mapping_component(&evidence.scope_digest),
    });
    let digest = bridge_mapping_digest(&sanitized);
    let short_digest = digest.chars().take(16).collect::<String>();
    EvidenceRef {
        kind: EvidenceKind::ToolPayload,
        id: format!("tool-search-bridge-mapping-{short_digest}"),
        digest,
        summary: format!(
            "Tool Search bridge mapping {} to {}",
            redacted_mapping_component(&evidence.bridge_name),
            redacted_mapping_component(&evidence.underlying_name)
        ),
        redaction_status: RedactionStatus::Redacted,
        owner_spec: Some(BRIDGE_MAPPING_EVIDENCE_OWNER_SPEC.to_owned()),
        locator: Some(format!(
            "trajectory://tool-search/bridge-mapping/{short_digest}"
        )),
        retention_hint: Some("trajectory_tool_ref".to_owned()),
    }
}

fn bridge_mapping_digest(sanitized: &Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(sanitized.to_string().as_bytes());
    format!("{:x}", hasher.finalize())
}

fn redacted_mapping_component(value: &str) -> String {
    let redacted = redact_string(value.trim());
    if redacted.chars().count() <= BRIDGE_MAPPING_SUMMARY_MAX_CHARS {
        return redacted;
    }
    let mut bounded = redacted
        .chars()
        .take(BRIDGE_MAPPING_SUMMARY_MAX_CHARS)
        .collect::<String>();
    bounded.push_str("...");
    bounded
}

fn tool_search_mode_label(mode: ToolSearchMode) -> &'static str {
    match mode {
        ToolSearchMode::Off => "off",
        ToolSearchMode::On => "on",
        ToolSearchMode::Auto => "auto",
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BridgeToolCall {
    pub original_call_id: String,
    pub bridge_name: String,
    pub bridge_arguments: Value,
}

impl BridgeToolCall {
    pub fn from_runtime(call: &RuntimeToolCall) -> Self {
        Self {
            original_call_id: call.id.clone(),
            bridge_name: call.name.clone(),
            bridge_arguments: call.arguments.clone(),
        }
    }

    pub fn to_runtime_call(&self) -> RuntimeToolCall {
        RuntimeToolCall::new(
            self.original_call_id.clone(),
            self.bridge_name.clone(),
            self.bridge_arguments.clone(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedDeferredToolCall {
    pub original_call_id: String,
    pub bridge_name: String,
    pub underlying_name: String,
    pub underlying_arguments: Value,
    pub scope_digest: String,
}

impl ResolvedDeferredToolCall {
    pub fn to_runtime_call(&self) -> RuntimeToolCall {
        RuntimeToolCall::new(
            self.original_call_id.clone(),
            self.underlying_name.clone(),
            self.underlying_arguments.clone(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BridgeToolResult {
    pub message: RuntimeToolMessage,
    pub resolved_call: Option<ResolvedDeferredToolCall>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BridgeToolExecutionReport {
    pub results: Vec<BridgeToolResult>,
    pub interrupt: Option<RuntimeInterrupt>,
    pub skipped_tool_calls: Vec<RuntimeToolCall>,
    pub resolved_calls: Vec<ResolvedDeferredToolCall>,
    #[serde(default)]
    pub permissioned_actions: Vec<PermissionedAction>,
}

impl BridgeToolExecutionReport {
    pub fn messages(&self) -> Vec<RuntimeToolMessage> {
        self.results
            .iter()
            .map(|result| result.message.clone())
            .collect()
    }

    pub fn into_runtime_report(self) -> RuntimeToolExecutionReport {
        RuntimeToolExecutionReport {
            messages: self
                .results
                .into_iter()
                .map(|result| result.message)
                .collect(),
            interrupt: self.interrupt,
            skipped_tool_calls: self.skipped_tool_calls,
            permissioned_actions: self.permissioned_actions,
        }
    }

    fn new() -> Self {
        Self {
            results: Vec::new(),
            interrupt: None,
            skipped_tool_calls: Vec::new(),
            resolved_calls: Vec::new(),
            permissioned_actions: Vec::new(),
        }
    }

    fn push_message(&mut self, message: RuntimeToolMessage) {
        self.results.push(BridgeToolResult {
            message,
            resolved_call: None,
        });
    }

    fn push_resolved_message(
        &mut self,
        message: RuntimeToolMessage,
        resolved_call: ResolvedDeferredToolCall,
    ) {
        self.results.push(BridgeToolResult {
            message,
            resolved_call: Some(resolved_call),
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolCallScopeError {
    MissingCatalog,
    UnsupportedBridgeTool { name: String },
    MissingStringArgument { name: String },
    MissingArguments,
    InvalidArguments { detail: String },
    RecursiveBridgeCall { name: String },
    DirectCallRequired { name: String },
    UnknownName { name: String },
}

impl fmt::Display for ToolCallScopeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCatalog => write!(
                formatter,
                "Error: deferred tool catalog is not available for this bridge call"
            ),
            Self::UnsupportedBridgeTool { name } => {
                write!(
                    formatter,
                    "Error: '{name}' is not a Tool Search bridge tool"
                )
            }
            Self::MissingStringArgument { name } => write!(
                formatter,
                "Error: bridge argument '{name}' must be a non-empty string"
            ),
            Self::MissingArguments => write!(
                formatter,
                "Error: bridge argument 'arguments' must be a JSON object or JSON object string"
            ),
            Self::InvalidArguments { detail } => {
                write!(formatter, "Error: Invalid bridge arguments: {detail}")
            }
            Self::RecursiveBridgeCall { name } => write!(
                formatter,
                "Error: recursive bridge tool call rejected for '{name}'"
            ),
            Self::DirectCallRequired { name } => write!(
                formatter,
                "Error: tool '{name}' is not deferred in the current catalog; call it directly"
            ),
            Self::UnknownName { name } => write!(
                formatter,
                "Error: tool '{name}' is outside the current deferred tool catalog"
            ),
        }
    }
}

pub fn dispatch_bridge_tool_call(
    call: RuntimeToolCall,
    catalog: Option<&DeferredToolCatalog>,
    registry: &ToolRegistry,
    executor: &RuntimeToolExecutor<'_>,
    context: &ToolExecutionContext,
) -> BridgeToolExecutionReport {
    dispatch_bridge_tool_calls(vec![call], catalog, registry, executor, context, false)
}

pub fn dispatch_bridge_tool_calls(
    calls: Vec<RuntimeToolCall>,
    catalog: Option<&DeferredToolCatalog>,
    registry: &ToolRegistry,
    executor: &RuntimeToolExecutor<'_>,
    context: &ToolExecutionContext,
    concurrent_tools: bool,
) -> BridgeToolExecutionReport {
    let bridge_calls = calls
        .iter()
        .map(BridgeToolCall::from_runtime)
        .collect::<Vec<_>>();
    let mut report = BridgeToolExecutionReport::new();
    let mut pending = Vec::new();

    for (index, bridge_call) in bridge_calls.iter().enumerate() {
        match resolve_bridge_call(bridge_call, catalog, registry) {
            BridgeAction::Immediate(message) => {
                if flush_pending(
                    &mut pending,
                    &mut report,
                    executor,
                    context,
                    concurrent_tools,
                ) {
                    report.skipped_tool_calls.extend(
                        bridge_calls[index..]
                            .iter()
                            .map(BridgeToolCall::to_runtime_call),
                    );
                    return report;
                }
                report.push_message(message);
            }
            BridgeAction::Execute(resolved_call) => {
                pending.push(PendingResolvedCall {
                    bridge_call: bridge_call.clone(),
                    resolved_call,
                });
            }
            BridgeAction::Error(error) => {
                if flush_pending(
                    &mut pending,
                    &mut report,
                    executor,
                    context,
                    concurrent_tools,
                ) {
                    report.skipped_tool_calls.extend(
                        bridge_calls[index..]
                            .iter()
                            .map(BridgeToolCall::to_runtime_call),
                    );
                    return report;
                }
                report.push_message(error_message(bridge_call, error));
            }
        }
    }

    flush_pending(
        &mut pending,
        &mut report,
        executor,
        context,
        concurrent_tools,
    );
    report
}

#[derive(Debug, Clone)]
struct PendingResolvedCall {
    bridge_call: BridgeToolCall,
    resolved_call: ResolvedDeferredToolCall,
}

enum BridgeAction {
    Immediate(RuntimeToolMessage),
    Execute(ResolvedDeferredToolCall),
    Error(ToolCallScopeError),
}

fn resolve_bridge_call(
    call: &BridgeToolCall,
    catalog: Option<&DeferredToolCatalog>,
    registry: &ToolRegistry,
) -> BridgeAction {
    if !bridge_tool_names().contains(&call.bridge_name.as_str()) {
        return BridgeAction::Error(ToolCallScopeError::UnsupportedBridgeTool {
            name: call.bridge_name.clone(),
        });
    }
    let Some(catalog) = catalog else {
        return BridgeAction::Error(ToolCallScopeError::MissingCatalog);
    };

    match call.bridge_name.as_str() {
        TOOL_SEARCH => search_result(call, catalog),
        TOOL_DESCRIBE => describe_result(call, catalog, registry),
        TOOL_CALL => resolved_tool_call(call, catalog, registry),
        other => BridgeAction::Error(ToolCallScopeError::UnsupportedBridgeTool {
            name: other.to_owned(),
        }),
    }
}

fn search_result(call: &BridgeToolCall, catalog: &DeferredToolCatalog) -> BridgeAction {
    let arguments = match object_arguments(call) {
        Ok(arguments) => arguments,
        Err(error) => return BridgeAction::Error(error),
    };
    let query = match required_string(arguments, "query") {
        Ok(query) => query.trim().to_owned(),
        Err(error) => return BridgeAction::Error(error),
    };
    if query.is_empty() {
        return BridgeAction::Error(ToolCallScopeError::MissingStringArgument {
            name: "query".to_owned(),
        });
    }
    let limit = match search_limit(arguments.get("limit")) {
        Ok(limit) => limit,
        Err(error) => return BridgeAction::Error(error),
    };
    let matches = catalog.search(&query, limit);
    let matches = matches
        .iter()
        .map(|item| {
            json!({
                "name": item.name,
                "short_description": item.short_description,
                "source": {
                    "kind": item.source_kind,
                    "name": item.source_name,
                },
                "rank": item.rank,
                "score": item.score,
            })
        })
        .collect::<Vec<_>>();
    BridgeAction::Immediate(RuntimeToolMessage {
        tool_call_id: call.original_call_id.clone(),
        name: call.bridge_name.clone(),
        content: json!({
            "query": query,
            "matches": matches,
        })
        .to_string(),
    })
}

fn describe_result(
    call: &BridgeToolCall,
    catalog: &DeferredToolCatalog,
    registry: &ToolRegistry,
) -> BridgeAction {
    let arguments = match object_arguments(call) {
        Ok(arguments) => arguments,
        Err(error) => return BridgeAction::Error(error),
    };
    let name = match required_string(arguments, "name") {
        Ok(name) => name,
        Err(error) => return BridgeAction::Error(error),
    };
    let Some(entry) = catalog.entries.iter().find(|entry| entry.name == name) else {
        return BridgeAction::Error(out_of_catalog_error(name, registry));
    };
    BridgeAction::Immediate(RuntimeToolMessage {
        tool_call_id: call.original_call_id.clone(),
        name: call.bridge_name.clone(),
        content: json!({
            "name": entry.name,
            "description": entry.description,
            "source": {
                "kind": entry.source_kind,
                "name": entry.source_name,
            },
            "schema": entry.full_schema,
        })
        .to_string(),
    })
}

fn resolved_tool_call(
    call: &BridgeToolCall,
    catalog: &DeferredToolCatalog,
    registry: &ToolRegistry,
) -> BridgeAction {
    let arguments = match object_arguments(call) {
        Ok(arguments) => arguments,
        Err(error) => return BridgeAction::Error(error),
    };
    let name = match required_string(arguments, "name") {
        Ok(name) => name,
        Err(error) => return BridgeAction::Error(error),
    };
    if bridge_tool_names().contains(&name) {
        return BridgeAction::Error(ToolCallScopeError::RecursiveBridgeCall {
            name: name.to_owned(),
        });
    }
    if !catalog.entries.iter().any(|entry| entry.name == name) {
        return BridgeAction::Error(out_of_catalog_error(name, registry));
    }
    let Some(raw_arguments) = arguments.get("arguments") else {
        return BridgeAction::Error(ToolCallScopeError::MissingArguments);
    };
    let underlying_arguments = match normalize_underlying_arguments(raw_arguments) {
        Ok(arguments) => arguments,
        Err(error) => return BridgeAction::Error(error),
    };

    BridgeAction::Execute(ResolvedDeferredToolCall {
        original_call_id: call.original_call_id.clone(),
        bridge_name: call.bridge_name.clone(),
        underlying_name: name.to_owned(),
        underlying_arguments,
        scope_digest: catalog.scope_digest.clone(),
    })
}

fn flush_pending(
    pending: &mut Vec<PendingResolvedCall>,
    report: &mut BridgeToolExecutionReport,
    executor: &RuntimeToolExecutor<'_>,
    context: &ToolExecutionContext,
    concurrent_tools: bool,
) -> bool {
    if pending.is_empty() {
        return false;
    }
    let tool_calls = pending
        .iter()
        .map(|entry| entry.resolved_call.to_runtime_call())
        .collect::<Vec<_>>();
    report
        .permissioned_actions
        .extend(pending.iter().map(|entry| {
            normalize_resolved_deferred_tool_call(
                executor.registry(),
                &entry.resolved_call,
                permissioned_action_input_from_context(context),
            )
        }));
    let runtime_report = if concurrent_tools {
        executor.execute_tool_calls_concurrent(tool_calls, context)
    } else {
        executor.execute_tool_calls(tool_calls, context)
    };

    report
        .resolved_calls
        .extend(pending.iter().map(|entry| entry.resolved_call.clone()));
    for message in runtime_report.messages {
        let Some(pending_call) = pending_call_by_id(pending, &message.tool_call_id) else {
            report.push_message(message);
            continue;
        };
        report.push_resolved_message(
            RuntimeToolMessage {
                tool_call_id: message.tool_call_id,
                name: pending_call.bridge_call.bridge_name.clone(),
                content: message.content,
            },
            pending_call.resolved_call.clone(),
        );
    }
    for skipped in runtime_report.skipped_tool_calls {
        if let Some(pending_call) = pending_call_by_id(pending, &skipped.id) {
            report
                .skipped_tool_calls
                .push(pending_call.bridge_call.to_runtime_call());
        } else {
            report.skipped_tool_calls.push(skipped);
        }
    }
    report.interrupt = runtime_report.interrupt;
    pending.clear();
    report.interrupt.is_some()
}

fn pending_call_by_id<'a>(
    pending: &'a [PendingResolvedCall],
    tool_call_id: &str,
) -> Option<&'a PendingResolvedCall> {
    pending
        .iter()
        .find(|entry| entry.resolved_call.original_call_id == tool_call_id)
}

fn object_arguments(call: &BridgeToolCall) -> Result<&Map<String, Value>, ToolCallScopeError> {
    call.bridge_arguments
        .as_object()
        .ok_or_else(|| ToolCallScopeError::InvalidArguments {
            detail: "bridge call arguments must be a JSON object".to_owned(),
        })
}

fn required_string<'a>(
    arguments: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a str, ToolCallScopeError> {
    let Some(value) = arguments.get(name).and_then(Value::as_str) else {
        return Err(ToolCallScopeError::MissingStringArgument {
            name: name.to_owned(),
        });
    };
    if value.is_empty() {
        return Err(ToolCallScopeError::MissingStringArgument {
            name: name.to_owned(),
        });
    }
    Ok(value)
}

fn search_limit(value: Option<&Value>) -> Result<Option<usize>, ToolCallScopeError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(raw_limit) = value.as_u64() else {
        return Err(ToolCallScopeError::InvalidArguments {
            detail: "limit must be a positive integer".to_owned(),
        });
    };
    match usize::try_from(raw_limit) {
        Ok(limit) => Ok(Some(limit.max(1))),
        Err(_) => Ok(Some(usize::MAX)),
    }
}

fn normalize_underlying_arguments(value: &Value) -> Result<Value, ToolCallScopeError> {
    match value {
        Value::Object(arguments) => Ok(Value::Object(arguments.clone())),
        Value::String(text) => match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(arguments)) => Ok(Value::Object(arguments)),
            Ok(_) => Err(ToolCallScopeError::InvalidArguments {
                detail: "arguments JSON string must decode to an object".to_owned(),
            }),
            Err(error) => Err(ToolCallScopeError::InvalidArguments {
                detail: format!("arguments JSON string is invalid: {error}"),
            }),
        },
        _ => Err(ToolCallScopeError::InvalidArguments {
            detail: "arguments must be a JSON object or JSON object string".to_owned(),
        }),
    }
}

fn out_of_catalog_error(name: &str, registry: &ToolRegistry) -> ToolCallScopeError {
    if registry.has(name) {
        ToolCallScopeError::DirectCallRequired {
            name: name.to_owned(),
        }
    } else {
        ToolCallScopeError::UnknownName {
            name: name.to_owned(),
        }
    }
}

fn error_message(call: &BridgeToolCall, error: ToolCallScopeError) -> RuntimeToolMessage {
    RuntimeToolMessage {
        tool_call_id: call.original_call_id.clone(),
        name: call.bridge_name.clone(),
        content: format!("{error}{ERROR_HINT}"),
    }
}
