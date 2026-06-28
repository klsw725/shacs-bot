use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use shacs_redaction::redact_string;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginHookEvent {
    #[serde(rename = "runtime:start")]
    RuntimeStart,
    #[serde(rename = "runtime:stop")]
    RuntimeStop,
    #[serde(rename = "session:start")]
    SessionStart,
    #[serde(rename = "session:end")]
    SessionEnd,
    #[serde(rename = "command:before")]
    CommandBefore,
    #[serde(rename = "llm:before")]
    LlmBefore,
    #[serde(rename = "llm:after")]
    LlmAfter,
    #[serde(rename = "tool:before")]
    ToolBefore,
    #[serde(rename = "tool:after")]
    ToolAfter,
    #[serde(rename = "tool:transform_result")]
    ToolTransformResult,
    #[serde(rename = "subagent:end")]
    SubagentEnd,
    #[serde(rename = "channel:inbound")]
    ChannelInbound,
}

impl PluginHookEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeStart => "runtime:start",
            Self::RuntimeStop => "runtime:stop",
            Self::SessionStart => "session:start",
            Self::SessionEnd => "session:end",
            Self::CommandBefore => "command:before",
            Self::LlmBefore => "llm:before",
            Self::LlmAfter => "llm:after",
            Self::ToolBefore => "tool:before",
            Self::ToolAfter => "tool:after",
            Self::ToolTransformResult => "tool:transform_result",
            Self::SubagentEnd => "subagent:end",
            Self::ChannelInbound => "channel:inbound",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginHookOutputPolicy {
    Ignored,
    DiagnosticOnly,
    BehaviorAffecting,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginHookCatalogEntry {
    pub event: PluginHookEvent,
    pub output_policy: PluginHookOutputPolicy,
    pub timeout_ms: u64,
    pub can_request_permission_approval: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginHookCatalog {
    pub entries: Vec<PluginHookCatalogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginHookTimeoutDiagnostic {
    pub plugin_id: String,
    pub event: PluginHookEvent,
    pub timeout_ms: u64,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginHookErrorDiagnostic {
    pub plugin_id: String,
    pub event: PluginHookEvent,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginHookOutputValidation {
    pub accepted: bool,
    pub effective_output: Option<Value>,
    pub diagnostics: Vec<PluginHookErrorDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginHookDispatchAttempt {
    pub plugin_id: String,
    pub event: PluginHookEvent,
    pub timeout_ms: u64,
    pub result: PluginHookCallbackResult,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginHookCallbackResult {
    Output(Value),
    Error(String),
    Timeout(String),
    ReplayRejected(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginHookDispatchStatus {
    Succeeded,
    InvalidOutput,
    Failed,
    TimedOut,
    ReplayRejected,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginHookDispatchRecord {
    pub plugin_id: String,
    pub event: PluginHookEvent,
    pub status: PluginHookDispatchStatus,
    pub effect: Option<PluginHookDispatchEffect>,
    pub output_evidence: Option<PluginHookOutputEvidence>,
    pub error: Option<PluginHookErrorDiagnostic>,
    pub timeout: Option<PluginHookTimeoutDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginHookDispatchEffect {
    Observed,
    Blocked,
    Rewritten,
    InjectedContext,
    Transformed,
    UnknownBehavior,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginHookOutputEvidence {
    pub digest: String,
    pub redacted_preview: String,
    pub redacted_byte_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginHookDispatchSummary {
    pub event: PluginHookEvent,
    pub dispatch_count: usize,
    pub success_count: usize,
    pub error_count: usize,
    pub timeout_count: usize,
    pub invalid_output_count: usize,
    pub replay_rejection_count: usize,
    pub observed_count: usize,
    pub blocked_count: usize,
    pub rewritten_count: usize,
    pub injected_context_count: usize,
    pub transformed_count: usize,
    pub unknown_behavior_count: usize,
    pub output_evidence: Vec<PluginHookOutputEvidence>,
    pub last_success_plugin_id: Option<String>,
    pub last_error: Option<PluginHookErrorDiagnostic>,
    pub last_timeout: Option<PluginHookTimeoutDiagnostic>,
    pub records: Vec<PluginHookDispatchRecord>,
}

pub fn plugin_hook_catalog() -> PluginHookCatalog {
    PluginHookCatalog {
        entries: vec![
            entry(
                PluginHookEvent::RuntimeStart,
                PluginHookOutputPolicy::Ignored,
            ),
            entry(
                PluginHookEvent::RuntimeStop,
                PluginHookOutputPolicy::Ignored,
            ),
            entry(
                PluginHookEvent::SessionStart,
                PluginHookOutputPolicy::Ignored,
            ),
            entry(PluginHookEvent::SessionEnd, PluginHookOutputPolicy::Ignored),
            entry(
                PluginHookEvent::CommandBefore,
                PluginHookOutputPolicy::BehaviorAffecting,
            ),
            entry(
                PluginHookEvent::LlmBefore,
                PluginHookOutputPolicy::BehaviorAffecting,
            ),
            entry(
                PluginHookEvent::LlmAfter,
                PluginHookOutputPolicy::DiagnosticOnly,
            ),
            entry(
                PluginHookEvent::ToolBefore,
                PluginHookOutputPolicy::BehaviorAffecting,
            ),
            entry(
                PluginHookEvent::ToolAfter,
                PluginHookOutputPolicy::DiagnosticOnly,
            ),
            entry(
                PluginHookEvent::ToolTransformResult,
                PluginHookOutputPolicy::BehaviorAffecting,
            ),
            entry(
                PluginHookEvent::SubagentEnd,
                PluginHookOutputPolicy::DiagnosticOnly,
            ),
            entry(
                PluginHookEvent::ChannelInbound,
                PluginHookOutputPolicy::BehaviorAffecting,
            ),
        ],
    }
}

pub fn plugin_hook_output_policy(event: PluginHookEvent) -> PluginHookOutputPolicy {
    plugin_hook_catalog()
        .entries
        .into_iter()
        .find(|entry| entry.event == event)
        .map(|entry| entry.output_policy)
        .unwrap_or(PluginHookOutputPolicy::Ignored)
}

pub fn validate_plugin_hook_output(
    plugin_id: &str,
    event: PluginHookEvent,
    output: Value,
) -> PluginHookOutputValidation {
    match plugin_hook_output_policy(event) {
        PluginHookOutputPolicy::Ignored => PluginHookOutputValidation {
            accepted: true,
            effective_output: None,
            diagnostics: Vec::new(),
        },
        PluginHookOutputPolicy::DiagnosticOnly => PluginHookOutputValidation {
            accepted: true,
            effective_output: diagnostic_only_output(output),
            diagnostics: Vec::new(),
        },
        PluginHookOutputPolicy::BehaviorAffecting => {
            validate_behavior_output(plugin_id, event, output)
        }
    }
}

pub fn summarize_plugin_hook_dispatch(
    event: PluginHookEvent,
    attempts: Vec<PluginHookDispatchAttempt>,
) -> PluginHookDispatchSummary {
    let mut summary = PluginHookDispatchSummary {
        event,
        dispatch_count: 0,
        success_count: 0,
        error_count: 0,
        timeout_count: 0,
        invalid_output_count: 0,
        replay_rejection_count: 0,
        observed_count: 0,
        blocked_count: 0,
        rewritten_count: 0,
        injected_context_count: 0,
        transformed_count: 0,
        unknown_behavior_count: 0,
        output_evidence: Vec::new(),
        last_success_plugin_id: None,
        last_error: None,
        last_timeout: None,
        records: Vec::new(),
    };

    for attempt in attempts {
        if attempt.event != event {
            continue;
        }
        summary.dispatch_count += 1;
        let record = plugin_hook_dispatch_record(attempt);
        match record.status {
            PluginHookDispatchStatus::Succeeded => {
                summary.success_count += 1;
                summary.last_success_plugin_id = Some(record.plugin_id.clone());
                if let Some(evidence) = record.output_evidence.clone() {
                    summary.output_evidence.push(evidence);
                }
                match record.effect {
                    Some(PluginHookDispatchEffect::Observed) => summary.observed_count += 1,
                    Some(PluginHookDispatchEffect::Blocked) => summary.blocked_count += 1,
                    Some(PluginHookDispatchEffect::Rewritten) => summary.rewritten_count += 1,
                    Some(PluginHookDispatchEffect::InjectedContext) => {
                        summary.injected_context_count += 1;
                    }
                    Some(PluginHookDispatchEffect::Transformed) => summary.transformed_count += 1,
                    Some(PluginHookDispatchEffect::UnknownBehavior) => {
                        summary.unknown_behavior_count += 1;
                    }
                    None => {}
                }
            }
            PluginHookDispatchStatus::InvalidOutput => {
                summary.invalid_output_count += 1;
                summary.last_error = record.error.clone();
            }
            PluginHookDispatchStatus::Failed => {
                summary.error_count += 1;
                summary.last_error = record.error.clone();
            }
            PluginHookDispatchStatus::TimedOut => {
                summary.timeout_count += 1;
                summary.last_timeout = record.timeout.clone();
            }
            PluginHookDispatchStatus::ReplayRejected => {
                summary.replay_rejection_count += 1;
                summary.last_error = record.error.clone();
            }
        }
        summary.records.push(record);
    }

    summary
}

pub fn plugin_hook_timeout_diagnostic(
    plugin_id: &str,
    event: PluginHookEvent,
    timeout_ms: u64,
    detail: &str,
) -> PluginHookTimeoutDiagnostic {
    PluginHookTimeoutDiagnostic {
        plugin_id: plugin_id.to_owned(),
        event,
        timeout_ms,
        message: redact_string(detail),
    }
}

pub fn plugin_hook_error_diagnostic(
    plugin_id: &str,
    event: PluginHookEvent,
    detail: &str,
) -> PluginHookErrorDiagnostic {
    PluginHookErrorDiagnostic {
        plugin_id: plugin_id.to_owned(),
        event,
        message: redact_string(detail),
    }
}

fn plugin_hook_dispatch_record(attempt: PluginHookDispatchAttempt) -> PluginHookDispatchRecord {
    match attempt.result {
        PluginHookCallbackResult::Output(output) => {
            let validation = validate_plugin_hook_output(&attempt.plugin_id, attempt.event, output);
            if validation.accepted {
                let effect = plugin_hook_dispatch_effect(
                    attempt.event,
                    validation.effective_output.as_ref(),
                );
                let output_evidence = validation
                    .effective_output
                    .as_ref()
                    .map(plugin_hook_output_evidence);
                return PluginHookDispatchRecord {
                    plugin_id: attempt.plugin_id,
                    event: attempt.event,
                    status: PluginHookDispatchStatus::Succeeded,
                    effect,
                    output_evidence,
                    error: None,
                    timeout: None,
                };
            }
            PluginHookDispatchRecord {
                plugin_id: attempt.plugin_id,
                event: attempt.event,
                status: PluginHookDispatchStatus::InvalidOutput,
                effect: None,
                output_evidence: None,
                error: validation.diagnostics.into_iter().next(),
                timeout: None,
            }
        }
        PluginHookCallbackResult::Error(detail) => PluginHookDispatchRecord {
            error: Some(plugin_hook_error_diagnostic(
                &attempt.plugin_id,
                attempt.event,
                &detail,
            )),
            plugin_id: attempt.plugin_id,
            event: attempt.event,
            status: PluginHookDispatchStatus::Failed,
            effect: None,
            output_evidence: None,
            timeout: None,
        },
        PluginHookCallbackResult::Timeout(detail) => PluginHookDispatchRecord {
            timeout: Some(plugin_hook_timeout_diagnostic(
                &attempt.plugin_id,
                attempt.event,
                attempt.timeout_ms,
                &detail,
            )),
            plugin_id: attempt.plugin_id,
            event: attempt.event,
            status: PluginHookDispatchStatus::TimedOut,
            effect: None,
            output_evidence: None,
            error: None,
        },
        PluginHookCallbackResult::ReplayRejected(reason) => PluginHookDispatchRecord {
            error: Some(plugin_hook_error_diagnostic(
                &attempt.plugin_id,
                attempt.event,
                &format!("plugin hook live dispatch rejected during replay: {reason}"),
            )),
            plugin_id: attempt.plugin_id,
            event: attempt.event,
            status: PluginHookDispatchStatus::ReplayRejected,
            effect: None,
            output_evidence: None,
            timeout: None,
        },
    }
}

fn plugin_hook_dispatch_effect(
    event: PluginHookEvent,
    effective_output: Option<&Value>,
) -> Option<PluginHookDispatchEffect> {
    let Some(output) = effective_output else {
        return Some(PluginHookDispatchEffect::Observed);
    };
    match plugin_hook_output_policy(event) {
        PluginHookOutputPolicy::Ignored | PluginHookOutputPolicy::DiagnosticOnly => {
            Some(PluginHookDispatchEffect::Observed)
        }
        PluginHookOutputPolicy::BehaviorAffecting => Some(behavior_effect(output)),
    }
}

fn behavior_effect(output: &Value) -> PluginHookDispatchEffect {
    let Some(object) = output.as_object() else {
        return PluginHookDispatchEffect::UnknownBehavior;
    };
    if object.get("block").is_some() || object.get("skip").is_some() {
        return PluginHookDispatchEffect::Blocked;
    }
    if object.get("rewrite").is_some() || object.get("replacementText").is_some() {
        return PluginHookDispatchEffect::Rewritten;
    }
    if object.get("context").is_some() || object.get("injectedContext").is_some() {
        return PluginHookDispatchEffect::InjectedContext;
    }
    if object.get("transform").is_some() || object.get("transformedText").is_some() {
        return PluginHookDispatchEffect::Transformed;
    }
    PluginHookDispatchEffect::UnknownBehavior
}

fn plugin_hook_output_evidence(output: &Value) -> PluginHookOutputEvidence {
    let serialized = match serde_json::to_string(output) {
        Ok(serialized) => serialized,
        Err(error) => format!("plugin hook output serialization failed: {error}"),
    };
    let redacted = redact_string(&serialized);
    PluginHookOutputEvidence {
        digest: format!("sha256:{}", sha256_hex(redacted.as_bytes())),
        redacted_preview: redacted.chars().take(160).collect(),
        redacted_byte_count: redacted.len(),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn entry(event: PluginHookEvent, output_policy: PluginHookOutputPolicy) -> PluginHookCatalogEntry {
    PluginHookCatalogEntry {
        event,
        output_policy,
        timeout_ms: 1_000,
        can_request_permission_approval: false,
    }
}

fn diagnostic_only_output(output: Value) -> Option<Value> {
    output
        .as_object()
        .and_then(|object| object.get("diagnostic"))
        .cloned()
}

fn validate_behavior_output(
    plugin_id: &str,
    event: PluginHookEvent,
    output: Value,
) -> PluginHookOutputValidation {
    if requests_permission_approval(&output) {
        return PluginHookOutputValidation {
            accepted: false,
            effective_output: None,
            diagnostics: vec![plugin_hook_error_diagnostic(
                plugin_id,
                event,
                "plugin hook output cannot approve or grant permissions",
            )],
        };
    }
    PluginHookOutputValidation {
        accepted: true,
        effective_output: Some(output),
        diagnostics: Vec::new(),
    }
}

fn requests_permission_approval(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        object
            .get("approvePermissions")
            .or_else(|| object.get("permissionApproval"))
            .or_else(|| object.get("grantPermissions"))
            .is_some()
    })
}
