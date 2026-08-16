mod diagnostics_release;
/// Spec031 envelopes must be parsed through the bounded public parse API.
///
/// ```compile_fail
/// use shacs_projection::Spec031Envelope;
///
/// let _ = serde_json::from_str::<Spec031Envelope>("{}");
/// ```
mod projection;
mod release_evidence;
mod remembered_permissions;
pub mod spec030;
pub mod spec031;
mod spec031_extension;
mod spec033;
mod spec034;
mod spec035;

pub use diagnostics_release::{
    build_spec018_diagnostics_manifest, build_spec018_ledger_inspect_result,
    context_prd005_release_evidence_checklist, evaluate_spec018_release_gate,
    spec024_release_evidence_checklist, tool_search_prd005_release_evidence_checklist,
    tool_search_prd006_release_evidence_checklist, ContextReleaseEvidence,
    ContextReleaseEvidenceBucket, ContextReleaseEvidenceChecklist,
    RuntimeSpec018DiagnosticsManifestInput, RuntimeSpec018LedgerInspectInput,
    RuntimeSpec018ReleaseGateInput, Spec024ReleaseEvidence, Spec024ReleaseEvidenceBucket,
    Spec024ReleaseEvidenceChecklist, ToolSearchReleaseEvidence, ToolSearchReleaseEvidenceBucket,
    ToolSearchReleaseEvidenceChecklist,
};
pub use projection::{
    build_spec018_projection, build_spec024_projection, runtime_spec018_channel_projection,
    runtime_spec018_local_api_projection, runtime_spec024_channel_projection,
    runtime_spec024_local_api_projection, RuntimeSpec018ProjectionInput, RuntimeSpec024Projection,
    RuntimeSpec024ProjectionInput,
};
pub use remembered_permissions::{
    build_remembered_permission_projection, format_remembered_permission_projection,
    format_remembered_permission_rule, normalize_remembered_permission_rule_prefix,
    project_remembered_permission_rule, project_remembered_permission_rule_by_prefix,
    project_removed_remembered_permission_rule, RememberedPermissionProjection,
    RememberedPermissionProjectionInput, RememberedPermissionRulePrefixError,
    RememberedPermissionRuleProjection, RememberedPermissionStoreHealthInput,
};
pub use spec030::*;
pub use spec031::*;
pub use spec031_extension::{
    spec031_extension_catalog, spec031_extension_diagnostic, Spec031ExtensionCatalogProjection,
    Spec031ExtensionDiagnostic, Spec031ExtensionDiagnosticSeverity, Spec031ExtensionEnabledState,
    Spec031ExtensionProjection, Spec031ExtensionReadiness, Spec031ExtensionReason,
    Spec031ExtensionSurfaceKind, Spec031ExtensionSurfaceProjection,
    SPEC031_EXTENSION_SCHEMA_VERSION,
};
pub use spec033::*;
pub use spec034::*;
pub use spec035::*;
