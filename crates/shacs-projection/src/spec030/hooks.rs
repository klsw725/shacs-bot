use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HookRuntimeProjection {
    pub availability: Spec030Availability,
    pub status: HookRuntimeStatus,
    pub registered_handlers: u32,
    pub diagnostics: Vec<HookDiagnosticProjection>,
    pub recent_denials: Vec<HookDenialProjection>,
}

impl HookRuntimeProjection {
    pub(super) const fn unavailable() -> Self {
        Self {
            availability: Spec030Availability::Unavailable,
            status: HookRuntimeStatus::Unavailable,
            registered_handlers: 0,
            diagnostics: Vec::new(),
            recent_denials: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HookDiagnosticProjection {
    pub hook_ref: String,
    pub kind: HookDiagnosticKind,
    pub behavior: HookFailureBehavior,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HookDenialProjection {
    pub hook_ref: String,
    pub call_ref: String,
    pub reason: HookDenialReason,
}
