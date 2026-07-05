use crate::runtime::{PermissionMode, PermissionedAction, SafetyCapability, TargetRef};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use shacs_redaction::REDACTED;

pub const PERMISSION_STATIC_RULE_VERSION: &str = "spec022-static-rules-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerRuntimeKind {
    Docker,
    Podman,
    Devcontainer,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerNetworkMode {
    None,
    Bridge,
    Host,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerContainmentSnapshot {
    pub contained: Option<bool>,
    pub runtime: ContainerRuntimeKind,
    pub root_user: Option<bool>,
    pub privileged: Option<bool>,
    pub host_mounts_summary: Vec<String>,
    pub network_mode: ContainerNetworkMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

impl DockerContainmentSnapshot {
    pub fn unknown() -> Self {
        Self {
            contained: None,
            runtime: ContainerRuntimeKind::Unknown,
            root_user: None,
            privileged: None,
            host_mounts_summary: Vec::new(),
            network_mode: ContainerNetworkMode::Unknown,
            digest: None,
            summary: None,
        }
    }

    pub fn confirmed_non_privileged(&self) -> bool {
        self.contained == Some(true)
            && self.runtime != ContainerRuntimeKind::Unknown
            && self.root_user == Some(false)
            && self.privileged == Some(false)
    }

    pub fn is_unknown(&self) -> bool {
        self.contained.is_none()
            || self.runtime == ContainerRuntimeKind::Unknown
            || self.root_user.is_none()
            || self.privileged.is_none()
            || self.network_mode == ContainerNetworkMode::Unknown
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRuleInput {
    pub containment: DockerContainmentSnapshot,
    #[serde(default)]
    pub protected_targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proc_exec_summary: Option<ProcExecSummary>,
}

impl Default for PermissionRuleInput {
    fn default() -> Self {
        Self {
            containment: DockerContainmentSnapshot::unknown(),
            protected_targets: Vec::new(),
            proc_exec_summary: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcExecSummary {
    pub command_family: String,
    #[serde(default)]
    pub target_refs: Vec<String>,
    pub destructive: bool,
    pub network: bool,
    pub secret_exposure: bool,
    pub summary_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtectedTargetClass {
    GitState,
    AuthStore,
    RuntimeConfig,
    AppRegistry,
    HostMountRoot,
    StartupHook,
    PackageLifecycleScript,
    RawCredential,
    CustomProtectedTarget,
    UnknownTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetClassification {
    pub target_kind: String,
    pub redacted_value: Value,
    pub protected_class: Option<ProtectedTargetClass>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityClassification {
    pub action_id: String,
    pub capabilities: Vec<SafetyCapability>,
    pub target_classes: Vec<TargetClassification>,
    pub classification_known: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaticRuleDecisionKind {
    AllowCandidate,
    AskRequired,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaticRuleReason {
    NoStaticMatch,
    NormalizationError,
    ProtectedTarget,
    UnknownTargetClassification,
    SecretRead,
    RawAuthExport,
    ProcExecSummaryUnavailable,
    DangerousProcExec,
    ContainmentUnknown,
    BypassContainmentNotConfirmed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaticRuleDecision {
    pub kind: StaticRuleDecisionKind,
    pub reason: StaticRuleReason,
    pub diagnostics: RuleDiagnostics,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleDiagnostics {
    pub rule_version: String,
    pub matched_rules: Vec<String>,
    pub protected_targets: Vec<ProtectedTargetClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub containment_warning: Option<String>,
    pub unknown_classification: bool,
}

pub fn classify_permission_action(
    action: &PermissionedAction,
    input: &PermissionRuleInput,
) -> CapabilityClassification {
    let target_classes = action
        .target_refs
        .iter()
        .map(|target| classify_target(target, &input.protected_targets))
        .collect::<Vec<_>>();
    let classification_known = !target_classes
        .iter()
        .any(|target| target.protected_class == Some(ProtectedTargetClass::UnknownTarget));

    CapabilityClassification {
        action_id: action.action_id.clone(),
        capabilities: action.capabilities.clone(),
        target_classes,
        classification_known,
    }
}

pub fn evaluate_static_rules(
    action: &PermissionedAction,
    input: &PermissionRuleInput,
) -> StaticRuleDecision {
    let classification = classify_permission_action(action, input);
    let mut diagnostics = RuleDiagnostics {
        rule_version: PERMISSION_STATIC_RULE_VERSION.to_owned(),
        matched_rules: Vec::new(),
        protected_targets: classification
            .target_classes
            .iter()
            .filter_map(|target| target.protected_class.clone())
            .collect(),
        containment_warning: None,
        unknown_classification: !classification.classification_known,
    };

    if action.normalization_state != crate::runtime::ActionNormalizationState::Ready
        || action.normalization_errors.iter().any(|error| {
            matches!(
                error,
                crate::runtime::ActionNormalizationError::UnknownTool { .. }
                    | crate::runtime::ActionNormalizationError::InvalidArguments { .. }
                    | crate::runtime::ActionNormalizationError::UnsafeRawSecret { .. }
                    | crate::runtime::ActionNormalizationError::RedactionFailed { .. }
            )
        })
    {
        diagnostics
            .matched_rules
            .push("normalization_error".to_owned());
        return static_decision(
            StaticRuleDecisionKind::Deny,
            StaticRuleReason::NormalizationError,
            diagnostics,
        );
    }

    if action.capabilities.contains(&SafetyCapability::SecretRead) {
        diagnostics.matched_rules.push("secret_read".to_owned());
        return static_decision(
            StaticRuleDecisionKind::Deny,
            StaticRuleReason::SecretRead,
            diagnostics,
        );
    }

    if diagnostics.unknown_classification {
        diagnostics
            .matched_rules
            .push("unknown_target_classification".to_owned());
        return static_decision(
            StaticRuleDecisionKind::Deny,
            StaticRuleReason::UnknownTargetClassification,
            diagnostics,
        );
    }

    if diagnostics.protected_targets.iter().any(|class| {
        *class == ProtectedTargetClass::AuthStore || *class == ProtectedTargetClass::RawCredential
    }) {
        diagnostics.matched_rules.push("raw_auth_export".to_owned());
        return static_decision(
            StaticRuleDecisionKind::Deny,
            StaticRuleReason::RawAuthExport,
            diagnostics,
        );
    }

    if !diagnostics.protected_targets.is_empty() {
        diagnostics
            .matched_rules
            .push("protected_target".to_owned());
        return static_decision(
            StaticRuleDecisionKind::Deny,
            StaticRuleReason::ProtectedTarget,
            diagnostics,
        );
    }

    if action.permission_mode_snapshot.mode == PermissionMode::BypassPermissions
        && !input.containment.confirmed_non_privileged()
    {
        diagnostics
            .matched_rules
            .push("bypass_containment_not_confirmed".to_owned());
        diagnostics.containment_warning =
            Some("bypass_permissions requires non-privileged containment".to_owned());
        return static_decision(
            StaticRuleDecisionKind::Deny,
            StaticRuleReason::BypassContainmentNotConfirmed,
            diagnostics,
        );
    }

    if action.capabilities.contains(&SafetyCapability::ProcExec) {
        let Some(summary) = &input.proc_exec_summary else {
            diagnostics
                .matched_rules
                .push("proc_exec_summary_unavailable".to_owned());
            return static_decision(
                StaticRuleDecisionKind::AskRequired,
                StaticRuleReason::ProcExecSummaryUnavailable,
                diagnostics,
            );
        };
        if !summary.summary_available {
            diagnostics
                .matched_rules
                .push("proc_exec_summary_unavailable".to_owned());
            return static_decision(
                StaticRuleDecisionKind::AskRequired,
                StaticRuleReason::ProcExecSummaryUnavailable,
                diagnostics,
            );
        }
        if summary.destructive || summary.network || summary.secret_exposure {
            diagnostics
                .matched_rules
                .push("dangerous_proc_exec".to_owned());
            return static_decision(
                StaticRuleDecisionKind::Deny,
                StaticRuleReason::DangerousProcExec,
                diagnostics,
            );
        }
        if !input.containment.confirmed_non_privileged() {
            diagnostics
                .matched_rules
                .push("containment_unknown".to_owned());
            diagnostics.containment_warning =
                Some("proc_exec requires non-privileged containment".to_owned());
            return static_decision(
                StaticRuleDecisionKind::AskRequired,
                StaticRuleReason::ContainmentUnknown,
                diagnostics,
            );
        }
    }

    static_decision(
        StaticRuleDecisionKind::AllowCandidate,
        StaticRuleReason::NoStaticMatch,
        diagnostics,
    )
}

fn static_decision(
    kind: StaticRuleDecisionKind,
    reason: StaticRuleReason,
    diagnostics: RuleDiagnostics,
) -> StaticRuleDecision {
    StaticRuleDecision {
        kind,
        reason,
        diagnostics,
    }
}

fn classify_target(target: &TargetRef, configured_targets: &[String]) -> TargetClassification {
    let protected_class = target_value(target)
        .map(|value| classify_target_value(&value, configured_targets))
        .unwrap_or(Some(ProtectedTargetClass::UnknownTarget));
    TargetClassification {
        target_kind: target.kind.clone(),
        redacted_value: target.redacted_value.clone(),
        protected_class,
    }
}

fn target_value(target: &TargetRef) -> Option<String> {
    match &target.redacted_value {
        Value::String(value) if is_classifiable_target_value(value) => Some(value.clone()),
        _ => None,
    }
}

fn is_classifiable_target_value(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty() && trimmed != REDACTED
}

fn classify_target_value(
    value: &str,
    configured_targets: &[String],
) -> Option<ProtectedTargetClass> {
    let normalized = normalize_target_path(value);
    let path_parts = normalized.split('/').collect::<Vec<_>>();
    if normalized == ".git" || normalized.starts_with(".git/") || path_parts.contains(&".git") {
        return Some(ProtectedTargetClass::GitState);
    }
    if normalized.contains(".shacs-bot/auth.json")
        || normalized.contains("auth.json")
        || normalized.contains("credentials")
        || normalized.contains("token")
    {
        return Some(ProtectedTargetClass::AuthStore);
    }
    if normalized.contains("permissions") && normalized.ends_with("config.json") {
        return Some(ProtectedTargetClass::RuntimeConfig);
    }
    if normalized.contains("app-registry") || normalized.contains("apps/registry") {
        return Some(ProtectedTargetClass::AppRegistry);
    }
    if normalized.ends_with(".bashrc")
        || normalized.ends_with(".zshrc")
        || normalized.ends_with(".profile")
        || normalized.contains("/hooks/")
    {
        return Some(ProtectedTargetClass::StartupHook);
    }
    if normalized.ends_with("package.json") && normalized.contains("scripts") {
        return Some(ProtectedTargetClass::PackageLifecycleScript);
    }
    if normalized == "/" || normalized == "/home" || normalized == "/workspace" {
        return Some(ProtectedTargetClass::HostMountRoot);
    }
    if configured_targets
        .iter()
        .any(|configured| target_matches(&normalized, configured))
    {
        return Some(ProtectedTargetClass::CustomProtectedTarget);
    }
    None
}

fn target_matches(normalized: &str, configured: &str) -> bool {
    let configured = normalize_target_path(configured);
    if configured.is_empty() {
        return false;
    }
    normalized == configured
        || normalized.starts_with(&format!("{configured}/"))
        || (!configured.starts_with('/')
            && (normalized.ends_with(&format!("/{configured}"))
                || normalized.contains(&format!("/{configured}/"))))
}

fn normalize_target_path(value: &str) -> String {
    let replaced = value.replace('\\', "/").to_ascii_lowercase();
    let absolute = replaced.starts_with('/');
    let mut parts = Vec::new();
    for part in replaced.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if !parts.is_empty() {
                    parts.pop();
                }
            }
            part => parts.push(part),
        }
    }
    let joined = parts.join("/");
    if absolute {
        format!("/{joined}")
    } else {
        joined
    }
}
