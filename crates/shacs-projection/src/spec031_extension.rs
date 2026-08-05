use serde::{Deserialize, Serialize};
use shacs_redaction::redact_string;

pub const SPEC031_EXTENSION_SCHEMA_VERSION: &str = "spec031.extension.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031ExtensionEnabledState {
    Enabled,
    Disabled,
    NotEnabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031ExtensionReadiness {
    Ready,
    Degraded,
    Blocked,
    Unavailable,
}

impl Spec031ExtensionReadiness {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Degraded => "degraded",
            Self::Blocked => "blocked",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031ExtensionReason {
    Ready,
    Degraded,
    Blocked,
    Unavailable,
}

impl Spec031ExtensionReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Degraded => "degraded",
            Self::Blocked => "blocked",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031ExtensionDiagnosticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031ExtensionSurfaceKind {
    Tool,
    Hook,
    Skill,
    Command,
    Mcp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spec031ExtensionCatalogProjection {
    pub schema_version: String,
    pub extensions: Vec<Spec031ExtensionProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spec031ExtensionProjection {
    pub extension_ref: String,
    pub label: String,
    pub owner_source: String,
    pub enabled_state: Spec031ExtensionEnabledState,
    pub readiness: Spec031ExtensionReadiness,
    pub reason: Spec031ExtensionReason,
    pub diagnostics: Vec<Spec031ExtensionDiagnostic>,
    pub surfaces: Vec<Spec031ExtensionSurfaceProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spec031ExtensionDiagnostic {
    pub severity: Spec031ExtensionDiagnosticSeverity,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spec031ExtensionSurfaceProjection {
    pub kind: Spec031ExtensionSurfaceKind,
    pub name: String,
    pub execution_enabled: bool,
}

pub fn spec031_extension_catalog(
    extensions: Vec<Spec031ExtensionProjection>,
) -> Spec031ExtensionCatalogProjection {
    Spec031ExtensionCatalogProjection {
        schema_version: SPEC031_EXTENSION_SCHEMA_VERSION.to_owned(),
        extensions,
    }
}

pub fn spec031_extension_diagnostic(
    severity: Spec031ExtensionDiagnosticSeverity,
    code: &str,
    message: &str,
) -> Spec031ExtensionDiagnostic {
    Spec031ExtensionDiagnostic {
        severity,
        code: redact_string(code),
        message: redact_string(message),
    }
}
