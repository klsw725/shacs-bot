use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResourceKind {
    Skill,
    Extension,
    Prompt,
    Context,
    Package,
}

impl ResourceKind {
    pub(super) const fn is_executable(self) -> bool {
        matches!(self, Self::Skill | Self::Extension | Self::Package)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResourceSource {
    Explicit,
    Project,
    User,
    Package,
    Builtin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResourcePrecedence {
    Explicit,
    ProjectConfigured,
    TrustedProjectAuto,
    UserConfigured,
    UserAuto,
    Package,
    Builtin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResourceCollisionStatus {
    None,
    Winner,
    Loser,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResourceLoadStatus {
    Candidate,
    Loaded,
    Rejected,
    ParseFailed,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResourceActivation {
    Explicit,
    TrustedWorkspace,
    Inactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrustedCodeDisclosure {
    NotExecutable,
    Required,
    Shown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceCandidateProjection {
    pub resource_ref: String,
    pub kind: ResourceKind,
    pub source: ResourceSource,
    pub precedence: ResourcePrecedence,
    pub canonical_path: String,
    #[serde(deserialize_with = "required_content_sha256")]
    pub content_sha256: Option<String>,
    pub collision: ResourceCollisionStatus,
    pub load_status: ResourceLoadStatus,
    pub activation: ResourceActivation,
    pub trusted_code_disclosure: TrustedCodeDisclosure,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<ResourceDiagnosticProjection>,
}

fn required_content_sha256<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceDiagnosticProjection {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub reason: String,
}
