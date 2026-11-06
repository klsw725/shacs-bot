use crate::permission_user_rules::{self, AutoModeConfig, AutoModeProblem, PermissionRuleLists};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use std::time::{SystemTime, UNIX_EPOCH};

const ALL_SAFETY_CAPABILITIES: &[SafetyCapability] = &[
    SafetyCapability::FsRead,
    SafetyCapability::FsWrite,
    SafetyCapability::ProcExec,
    SafetyCapability::NetOutbound,
    SafetyCapability::SecretRead,
    SafetyCapability::ExternalDelivery,
    SafetyCapability::AutomationSchedule,
    SafetyCapability::AppInstall,
    SafetyCapability::RuntimeConfigWrite,
    SafetyCapability::SelfModification,
];

const READ_ONLY_CAPABILITIES: &[SafetyCapability] = &[SafetyCapability::FsRead];
const EDIT_CAPABILITIES: &[SafetyCapability] =
    &[SafetyCapability::FsRead, SafetyCapability::FsWrite];
const EXPLICIT_RULE_CAPABILITIES: &[SafetyCapability] = &[];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    Plan,
    #[default]
    Default,
    AcceptEdits,
    Auto,
    DontAsk,
    BypassPermissions,
}

impl PermissionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Default => "default",
            Self::AcceptEdits => "accept_edits",
            Self::Auto => "auto",
            Self::DontAsk => "dont_ask",
            Self::BypassPermissions => "bypass_permissions",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "plan" => Some(Self::Plan),
            "default" => Some(Self::Default),
            "accept_edits" | "accept-edits" => Some(Self::AcceptEdits),
            "auto" => Some(Self::Auto),
            "dont_ask" | "dont-ask" => Some(Self::DontAsk),
            "bypass_permissions" | "bypass-permissions" => Some(Self::BypassPermissions),
            _ => None,
        }
    }

    pub fn baseline_candidate_capabilities(self) -> &'static [SafetyCapability] {
        match self {
            Self::Plan | Self::Default => READ_ONLY_CAPABILITIES,
            Self::AcceptEdits => EDIT_CAPABILITIES,
            Self::Auto | Self::DontAsk => EXPLICIT_RULE_CAPABILITIES,
            Self::BypassPermissions => ALL_SAFETY_CAPABILITIES,
        }
    }
}

impl<'de> Deserialize<'de> for PermissionMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        Ok(parse_permission_mode(&value).unwrap_or_default())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionModeSource {
    UserLocalConfig,
    WorkspaceConfig,
    CliFlag,
    LocalApiRequest,
    SessionCommand,
    /// The user-local config writes no mode and the run is interactive, so
    /// it starts in auto (Spec 048 PRD 050 scope 2).
    InteractiveDefault,
    DefaultFallback,
}

impl PermissionModeSource {
    pub fn from_trusted_name(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "user_local_config" | "user-local-config" => Some(Self::UserLocalConfig),
            "workspace_config" | "workspace-config" => Some(Self::WorkspaceConfig),
            "cli_flag" | "cli-flag" => Some(Self::CliFlag),
            "local_api_request" | "local-api-request" => Some(Self::LocalApiRequest),
            "session_command" | "session-command" => Some(Self::SessionCommand),
            "interactive_default" | "interactive-default" => Some(Self::InteractiveDefault),
            "default_fallback" | "default-fallback" => Some(Self::DefaultFallback),
            _ => None,
        }
    }

    fn permits_auto(self, context: PermissionActivationContext) -> bool {
        match self {
            Self::UserLocalConfig
            | Self::CliFlag
            | Self::LocalApiRequest
            | Self::SessionCommand
            | Self::InteractiveDefault => true,
            Self::WorkspaceConfig => context.user_local_auto_opt_in,
            Self::DefaultFallback => false,
        }
    }

    fn permits_bypass_permissions(self) -> bool {
        matches!(self, Self::UserLocalConfig | Self::CliFlag)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyCapability {
    FsRead,
    FsWrite,
    ProcExec,
    NetOutbound,
    SecretRead,
    ExternalDelivery,
    AutomationSchedule,
    AppInstall,
    RuntimeConfigWrite,
    SelfModification,
}

impl SafetyCapability {
    pub fn all() -> &'static [Self] {
        ALL_SAFETY_CAPABILITIES
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "fs_read" => Some(Self::FsRead),
            "fs_write" => Some(Self::FsWrite),
            "proc_exec" => Some(Self::ProcExec),
            "net_outbound" => Some(Self::NetOutbound),
            "secret_read" => Some(Self::SecretRead),
            "external_delivery" => Some(Self::ExternalDelivery),
            "automation_schedule" => Some(Self::AutomationSchedule),
            "app_install" => Some(Self::AppInstall),
            "runtime_config_write" => Some(Self::RuntimeConfigWrite),
            "self_modification" => Some(Self::SelfModification),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PermissionActivationContext {
    pub user_local_auto_opt_in: bool,
    pub containment_precondition_met: bool,
    /// The run can ask the user (`permission_interactive`); a user-local
    /// config without a mode starts such a run in auto.
    pub interactive_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoApprovalConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub require_docker_containment_for_exec: bool,
    #[serde(default = "default_true")]
    pub allow_workspace_edits: bool,
    #[serde(default)]
    pub allow_proc_exec_verification: bool,
    #[serde(default)]
    pub protected_targets: Vec<String>,
}

impl Default for AutoApprovalConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            require_docker_containment_for_exec: true,
            allow_workspace_edits: true,
            allow_proc_exec_verification: false,
            protected_targets: Vec::new(),
        }
    }
}

/// The `permissions` section. Saving writes back what the user wrote, so a
/// save never adds a mode or repairs a field that could not be read (Spec
/// 048 PRD 050 scope 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionsConfig {
    mode: PermissionMode,
    /// `mode` as written, malformed or not.
    written_mode: Option<Value>,
    pub auto_approval: AutoApprovalConfig,
    /// `autoApproval` as written when it could not be read.
    unreadable_auto_approval: Option<Value>,
    /// The whole section as written when it is not an object.
    unreadable_section: Option<Value>,
    written_user_rules: Map<String, Value>,
    pub user_rules: PermissionRuleLists,
    pub auto_mode: AutoModeConfig,
    auto_mode_problem: Option<AutoModeProblem>,
    mode_configured: bool,
    diagnostics: PermissionConfigDiagnostics,
}

impl Default for PermissionsConfig {
    fn default() -> Self {
        Self {
            mode: PermissionMode::Default,
            written_mode: None,
            auto_approval: AutoApprovalConfig::default(),
            unreadable_auto_approval: None,
            unreadable_section: None,
            written_user_rules: Map::new(),
            user_rules: PermissionRuleLists::default(),
            auto_mode: AutoModeConfig::default(),
            auto_mode_problem: None,
            mode_configured: false,
            diagnostics: PermissionConfigDiagnostics::default(),
        }
    }
}

impl Serialize for PermissionsConfig {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Some(section) = &self.unreadable_section {
            return section.serialize(serializer);
        }
        let mut map = serializer.serialize_map(None)?;
        if let Some(mode) = &self.written_mode {
            map.serialize_entry("mode", mode)?;
        }
        match &self.unreadable_auto_approval {
            Some(written) => map.serialize_entry("autoApproval", written)?,
            None => map.serialize_entry("autoApproval", &self.auto_approval)?,
        }
        for (key, value) in &self.written_user_rules {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

impl PermissionsConfig {
    pub fn is_default(&self) -> bool {
        self.written_mode.is_none()
            && self.unreadable_section.is_none()
            && self.unreadable_auto_approval.is_none()
            && self.auto_approval == AutoApprovalConfig::default()
            && self.written_user_rules.is_empty()
            && self.diagnostics == PermissionConfigDiagnostics::default()
    }

    /// The mode read from `permissions.mode`; `default` when it is missing,
    /// malformed, or the section could not be read.
    pub fn mode(&self) -> PermissionMode {
        self.mode
    }

    pub fn written_mode(&self) -> Option<&Value> {
        self.written_mode.as_ref()
    }

    /// Writes `mode` as the user's own choice, keeping everything else as
    /// written, the way the `/permission` wizard writes it.
    pub fn set_mode(&mut self, mode: PermissionMode) {
        let mut section = match serde_json::to_value(&*self) {
            Ok(Value::Object(section)) => section,
            _ => Map::new(),
        };
        section.insert("mode".to_owned(), Value::String(mode.as_str().to_owned()));
        *self = Self::from_value(&Value::Object(section));
    }

    /// No mode is written and the rest of the section can be read, so an
    /// interactive run may start in auto by default.
    fn interactive_default_allowed(&self) -> bool {
        self.written_mode.is_none()
            && self.unreadable_section.is_none()
            && self.unreadable_auto_approval.is_none()
            && self.auto_mode_problem.is_none()
    }

    pub fn diagnostics(&self) -> &PermissionConfigDiagnostics {
        &self.diagnostics
    }

    /// Why `permissions.autoMode` could not be read; auto mode then starts as
    /// `default`.
    pub fn auto_mode_problem(&self) -> Option<&AutoModeProblem> {
        self.auto_mode_problem.as_ref()
    }

    pub fn normalized_snapshot(
        &self,
        requested_source: PermissionModeSource,
        context: PermissionActivationContext,
    ) -> PermissionConfigSnapshot {
        let source = if self.mode_configured {
            requested_source
        } else if requested_source == PermissionModeSource::UserLocalConfig
            && context.interactive_run
            && self.interactive_default_allowed()
        {
            PermissionModeSource::InteractiveDefault
        } else {
            PermissionModeSource::DefaultFallback
        };
        let mut mode = if source == PermissionModeSource::InteractiveDefault {
            PermissionMode::Auto
        } else {
            self.mode
        };
        let mut diagnostics = self.diagnostics.clone();
        diagnostics.source = source;
        diagnostics.normalized_mode = mode;

        let mut snapshot_source = source;
        if mode == PermissionMode::Auto && !source.permits_auto(context) {
            diagnostics.rejected_source = Some(source);
            diagnostics.safe_fallback_reason =
                Some("auto_requires_user_local_or_explicit_source".to_owned());
            diagnostics
                .warnings
                .push("auto mode activation was rejected".to_owned());
            mode = PermissionMode::Default;
            snapshot_source = PermissionModeSource::DefaultFallback;
        }
        if mode == PermissionMode::BypassPermissions {
            if !source.permits_bypass_permissions() {
                diagnostics.rejected_source = Some(source);
                diagnostics.safe_fallback_reason =
                    Some("bypass_permissions_requires_user_local_or_cli_source".to_owned());
                diagnostics
                    .warnings
                    .push("bypass_permissions source was rejected".to_owned());
                mode = PermissionMode::Default;
                snapshot_source = PermissionModeSource::DefaultFallback;
            } else if !context.containment_precondition_met {
                diagnostics.safe_fallback_reason =
                    Some("bypass_permissions_requires_containment".to_owned());
                diagnostics
                    .warnings
                    .push("bypass_permissions containment precondition was not met".to_owned());
                mode = PermissionMode::Default;
                snapshot_source = PermissionModeSource::DefaultFallback;
            }
        }
        diagnostics.normalized_mode = mode;

        let mut auto_approval = self.auto_approval.clone();
        auto_approval.enabled = mode == PermissionMode::Auto;

        PermissionConfigSnapshot {
            mode,
            source: snapshot_source,
            auto_approval,
            baseline_candidate_capabilities: mode.baseline_candidate_capabilities().to_vec(),
            diagnostics,
            generated_at_unix_ms: generated_at_unix_ms(),
        }
    }
}

impl<'de> Deserialize<'de> for PermissionsConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        Ok(Self::from_value(&value))
    }
}

impl PermissionsConfig {
    fn from_value(value: &Value) -> Self {
        let Some(object) = value.as_object() else {
            let mut diagnostics = PermissionConfigDiagnostics::default();
            diagnostics.malformed_fields.push("permissions".to_owned());
            diagnostics.safe_fallback_reason = Some("permissions_must_be_object".to_owned());
            diagnostics
                .warnings
                .push("permissions config was ignored because it is not an object".to_owned());
            return Self {
                unreadable_section: Some(value.clone()),
                diagnostics,
                ..Self::default()
            };
        };

        let mut diagnostics = PermissionConfigDiagnostics::default();
        let written_mode = object.get("mode").cloned();
        let mut unreadable_auto_approval = None;
        let mut mode_configured = written_mode.is_some();
        let mut mode = object
            .get("mode")
            .map(|value| {
                parse_permission_mode(value).unwrap_or_else(|| {
                    mode_configured = false;
                    diagnostics
                        .malformed_fields
                        .push("permissions.mode".to_owned());
                    diagnostics.safe_fallback_reason = Some("malformed_permission_mode".to_owned());
                    diagnostics
                        .warnings
                        .push("permission mode fell back to default".to_owned());
                    PermissionMode::Default
                })
            })
            .unwrap_or_default();

        let auto_approval = object
            .get("autoApproval")
            .map(|value| {
                serde_json::from_value::<AutoApprovalConfig>(value.clone()).unwrap_or_else(|_| {
                    unreadable_auto_approval = Some(value.clone());
                    mode = PermissionMode::Default;
                    mode_configured = false;
                    diagnostics
                        .malformed_fields
                        .push("permissions.autoApproval".to_owned());
                    diagnostics.safe_fallback_reason =
                        Some("malformed_auto_approval_config".to_owned());
                    diagnostics
                        .warnings
                        .push("autoApproval config fell back to defaults".to_owned());
                    AutoApprovalConfig::default()
                })
            })
            .unwrap_or_default();

        let (user_rules, malformed_lists) = permission_user_rules::rule_lists(object);
        for name in malformed_lists {
            diagnostics
                .malformed_fields
                .push(format!("permissions.{name}"));
            diagnostics.warnings.push(format!(
                "permissions.{name} was ignored because it is not a list of strings"
            ));
        }
        let (auto_mode, auto_mode_problem) =
            match permission_user_rules::auto_mode(object.get("autoMode")) {
                Ok(auto_mode) => (auto_mode, None),
                Err(problem) => {
                    mode = PermissionMode::Default;
                    mode_configured = false;
                    diagnostics
                        .malformed_fields
                        .push("permissions.autoMode".to_owned());
                    diagnostics.safe_fallback_reason =
                        Some("malformed_auto_mode_config".to_owned());
                    diagnostics
                        .warnings
                        .push(format!("autoMode config was ignored: {problem}"));
                    (AutoModeConfig::default(), Some(problem))
                }
            };

        diagnostics.normalized_mode = mode;
        Self {
            mode,
            written_mode,
            auto_approval,
            unreadable_auto_approval,
            unreadable_section: None,
            written_user_rules: permission_user_rules::written_fields(object),
            user_rules,
            auto_mode,
            auto_mode_problem,
            mode_configured,
            diagnostics,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionConfigSnapshot {
    pub mode: PermissionMode,
    pub source: PermissionModeSource,
    pub auto_approval: AutoApprovalConfig,
    pub baseline_candidate_capabilities: Vec<SafetyCapability>,
    pub diagnostics: PermissionConfigDiagnostics,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_at_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionConfigDiagnostics {
    pub normalized_mode: PermissionMode,
    pub source: PermissionModeSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejected_source: Option<PermissionModeSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub malformed_fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_fallback_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl Default for PermissionConfigDiagnostics {
    fn default() -> Self {
        Self {
            normalized_mode: PermissionMode::Default,
            source: PermissionModeSource::DefaultFallback,
            rejected_source: None,
            malformed_fields: Vec::new(),
            safe_fallback_reason: None,
            warnings: Vec::new(),
        }
    }
}

fn parse_permission_mode(value: &Value) -> Option<PermissionMode> {
    value.as_str().and_then(PermissionMode::parse)
}

fn default_true() -> bool {
    true
}

fn generated_at_unix_ms() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn permission_mode_defaults_to_default_with_no_baseline_exec() {
        let snapshot = PermissionsConfig::default().normalized_snapshot(
            PermissionModeSource::UserLocalConfig,
            PermissionActivationContext::default(),
        );
        assert_eq!(snapshot.mode, PermissionMode::Default);
        assert_eq!(snapshot.source, PermissionModeSource::DefaultFallback);
        assert!(snapshot.generated_at_unix_ms.is_some());
        assert_eq!(
            snapshot.baseline_candidate_capabilities,
            vec![SafetyCapability::FsRead]
        );
        assert!(!snapshot
            .baseline_candidate_capabilities
            .contains(&SafetyCapability::ProcExec));
    }

    #[test]
    fn permission_modes_deserialize_from_config_strings() -> Result<(), String> {
        for (mode, expected) in [
            ("plan", PermissionMode::Plan),
            ("default", PermissionMode::Default),
            ("accept_edits", PermissionMode::AcceptEdits),
            ("auto", PermissionMode::Auto),
            ("dont_ask", PermissionMode::DontAsk),
            ("bypass_permissions", PermissionMode::BypassPermissions),
        ] {
            let config: PermissionsConfig =
                serde_json::from_value(json!({"mode": mode})).map_err(|error| error.to_string())?;
            assert_eq!(config.mode, expected);
        }
        Ok(())
    }

    #[test]
    fn malformed_permission_mode_safe_fallback_never_records_raw_value() -> Result<(), String> {
        let config: PermissionsConfig = serde_json::from_value(json!({
            "mode": "sk-or-secret-auto"
        }))
        .map_err(|error| error.to_string())?;
        let diagnostics =
            serde_json::to_string(config.diagnostics()).map_err(|error| error.to_string())?;
        assert_eq!(config.mode, PermissionMode::Default);
        assert!(config
            .diagnostics()
            .malformed_fields
            .contains(&"permissions.mode".to_owned()));
        assert!(!diagnostics.contains("sk-or-secret-auto"));
        let snapshot = config.normalized_snapshot(
            PermissionModeSource::UserLocalConfig,
            PermissionActivationContext::default(),
        );
        assert_eq!(snapshot.source, PermissionModeSource::DefaultFallback);
        Ok(())
    }

    #[test]
    fn malformed_auto_approval_safe_fallbacks_without_preserving_auto_source() -> Result<(), String>
    {
        let config: PermissionsConfig = serde_json::from_value(json!({
            "mode": "auto",
            "autoApproval": "bad"
        }))
        .map_err(|error| error.to_string())?;
        let snapshot = config.normalized_snapshot(
            PermissionModeSource::UserLocalConfig,
            PermissionActivationContext::default(),
        );
        assert_eq!(config.auto_approval, AutoApprovalConfig::default());
        assert_eq!(snapshot.mode, PermissionMode::Default);
        assert_eq!(snapshot.source, PermissionModeSource::DefaultFallback);
        assert_eq!(
            snapshot.diagnostics.safe_fallback_reason.as_deref(),
            Some("malformed_auto_approval_config")
        );
        Ok(())
    }

    #[test]
    fn workspace_auto_requires_user_local_opt_in() -> Result<(), String> {
        let config: PermissionsConfig =
            serde_json::from_value(json!({"mode": "auto"})).map_err(|error| error.to_string())?;
        let user_local = config.normalized_snapshot(
            PermissionModeSource::UserLocalConfig,
            PermissionActivationContext::default(),
        );
        assert_eq!(user_local.mode, PermissionMode::Auto);
        assert!(user_local.auto_approval.enabled);

        let rejected = config.normalized_snapshot(
            PermissionModeSource::WorkspaceConfig,
            PermissionActivationContext::default(),
        );
        assert_eq!(rejected.mode, PermissionMode::Default);
        assert_eq!(rejected.source, PermissionModeSource::DefaultFallback);
        assert!(!rejected.auto_approval.enabled);
        assert_eq!(
            rejected.diagnostics.rejected_source,
            Some(PermissionModeSource::WorkspaceConfig)
        );

        let accepted = config.normalized_snapshot(
            PermissionModeSource::WorkspaceConfig,
            PermissionActivationContext {
                user_local_auto_opt_in: true,
                containment_precondition_met: false,
                interactive_run: false,
            },
        );
        assert_eq!(accepted.mode, PermissionMode::Auto);
        assert_eq!(accepted.source, PermissionModeSource::WorkspaceConfig);
        assert!(accepted.auto_approval.enabled);
        Ok(())
    }

    #[test]
    fn bypass_permissions_requires_explicit_source_and_containment() -> Result<(), String> {
        let config: PermissionsConfig = serde_json::from_value(json!({
            "mode": "bypass_permissions"
        }))
        .map_err(|error| error.to_string())?;
        let workspace = config.normalized_snapshot(
            PermissionModeSource::WorkspaceConfig,
            PermissionActivationContext {
                user_local_auto_opt_in: true,
                containment_precondition_met: true,
                interactive_run: false,
            },
        );
        assert_eq!(workspace.mode, PermissionMode::Default);
        assert_eq!(
            workspace.diagnostics.safe_fallback_reason.as_deref(),
            Some("bypass_permissions_requires_user_local_or_cli_source")
        );

        let uncontained = config.normalized_snapshot(
            PermissionModeSource::UserLocalConfig,
            PermissionActivationContext::default(),
        );
        assert_eq!(uncontained.mode, PermissionMode::Default);
        assert_eq!(
            uncontained.diagnostics.safe_fallback_reason.as_deref(),
            Some("bypass_permissions_requires_containment")
        );

        let contained = config.normalized_snapshot(
            PermissionModeSource::CliFlag,
            PermissionActivationContext {
                user_local_auto_opt_in: false,
                containment_precondition_met: true,
                interactive_run: false,
            },
        );
        assert_eq!(contained.mode, PermissionMode::BypassPermissions);
        assert_eq!(contained.source, PermissionModeSource::CliFlag);
        Ok(())
    }

    #[test]
    fn prompt_skill_manifest_memory_and_tool_result_are_not_mode_sources() {
        for source in [
            "prompt",
            "skill_instruction",
            "app_manifest",
            "session_memory",
            "tool_result",
        ] {
            assert!(PermissionModeSource::from_trusted_name(source).is_none());
        }
    }

    #[test]
    fn safety_capability_taxonomy_is_canonical_and_unknown_is_not_known() {
        assert_eq!(SafetyCapability::all().len(), 10);
        for capability in [
            "fs_read",
            "fs_write",
            "proc_exec",
            "net_outbound",
            "secret_read",
            "external_delivery",
            "automation_schedule",
            "app_install",
            "runtime_config_write",
            "self_modification",
        ] {
            assert!(SafetyCapability::parse(capability).is_some());
        }
        assert!(SafetyCapability::parse("unknown_capability").is_none());
    }

    #[test]
    fn accept_edits_does_not_make_proc_exec_a_baseline_candidate() -> Result<(), String> {
        let config: PermissionsConfig = serde_json::from_value(json!({"mode": "accept_edits"}))
            .map_err(|error| error.to_string())?;
        let snapshot = config.normalized_snapshot(
            PermissionModeSource::UserLocalConfig,
            PermissionActivationContext::default(),
        );
        assert!(snapshot
            .baseline_candidate_capabilities
            .contains(&SafetyCapability::FsWrite));
        assert!(!snapshot
            .baseline_candidate_capabilities
            .contains(&SafetyCapability::ProcExec));
        Ok(())
    }

    #[test]
    fn a_missing_mode_starts_only_interactive_user_local_runs_in_auto() -> Result<(), String> {
        let interactive = PermissionActivationContext {
            interactive_run: true,
            ..PermissionActivationContext::default()
        };
        let unset: PermissionsConfig = serde_json::from_value(json!({"allow": ["read"]}))
            .map_err(|error| error.to_string())?;
        let auto = unset.normalized_snapshot(PermissionModeSource::UserLocalConfig, interactive);
        assert_eq!(auto.mode, PermissionMode::Auto);
        assert_eq!(auto.source, PermissionModeSource::InteractiveDefault);
        assert_eq!(
            auto.diagnostics.source,
            PermissionModeSource::InteractiveDefault
        );
        assert!(auto.auto_approval.enabled);
        for (source, context) in [
            (
                PermissionModeSource::UserLocalConfig,
                PermissionActivationContext::default(),
            ),
            (PermissionModeSource::WorkspaceConfig, interactive),
        ] {
            let snapshot = unset.normalized_snapshot(source, context);
            assert_eq!(snapshot.mode, PermissionMode::Default, "{source:?}");
            assert_eq!(snapshot.source, PermissionModeSource::DefaultFallback);
        }
        for written in [
            json!({"mode": "default"}),
            json!({"mode": "defualt"}),
            json!({"autoApproval": "bad"}),
            json!({"autoMode": "on"}),
            json!("not an object"),
        ] {
            let config: PermissionsConfig =
                serde_json::from_value(written.clone()).map_err(|error| error.to_string())?;
            let snapshot =
                config.normalized_snapshot(PermissionModeSource::UserLocalConfig, interactive);
            assert_eq!(snapshot.mode, PermissionMode::Default, "{written}");
        }
        Ok(())
    }

    #[test]
    fn saving_writes_back_only_what_was_written() -> Result<(), String> {
        for (written, saved_mode) in [
            (json!({"allow": ["read"]}), None),
            (json!({"mode": "default"}), Some(json!("default"))),
            (json!({"mode": "auto"}), Some(json!("auto"))),
            (json!({"mode": "defualt"}), Some(json!("defualt"))),
        ] {
            let config: PermissionsConfig =
                serde_json::from_value(written.clone()).map_err(|error| error.to_string())?;
            let saved = serde_json::to_value(&config).map_err(|error| error.to_string())?;
            assert_eq!(saved.get("mode").cloned(), saved_mode, "{written}");
        }
        for unreadable in [json!({"autoApproval": "bad"}), json!("off")] {
            let config: PermissionsConfig =
                serde_json::from_value(unreadable.clone()).map_err(|error| error.to_string())?;
            let saved = serde_json::to_value(&config).map_err(|error| error.to_string())?;
            assert_eq!(saved, unreadable);
        }
        let mut chosen: PermissionsConfig =
            serde_json::from_value(json!({"deny": ["exec"]})).map_err(|error| error.to_string())?;
        chosen.set_mode(PermissionMode::Auto);
        assert_eq!(chosen.mode(), PermissionMode::Auto);
        let saved = serde_json::to_value(&chosen).map_err(|error| error.to_string())?;
        assert_eq!(saved["mode"], json!("auto"));
        assert_eq!(saved["deny"], json!(["exec"]));
        Ok(())
    }
}
