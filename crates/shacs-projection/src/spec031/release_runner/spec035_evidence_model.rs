use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Manifest {
    pub(super) verdict: String,
    pub(super) source_binding: ManifestSourceBinding,
    pub(super) registries: Registries,
    pub(super) summary: Summary,
    pub(super) requirements: Vec<Requirement>,
    pub(super) gates: Vec<Gate>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ManifestSourceBinding {
    pub(super) final_source_files: String,
    pub(super) drift: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Registries {
    pub(super) commands: String,
    pub(super) external_owners: String,
    pub(super) visual_review: String,
    pub(super) cleanup: String,
    pub(super) artifact_hashes: String,
    pub(super) workspace_failure_triage: String,
    pub(super) correction_commands: String,
    pub(super) incidents: String,
    pub(super) correction_source_binding: String,
    pub(super) corrected_closure_mapping: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Summary {
    pub(super) requirements: usize,
    pub(super) passed: usize,
    pub(super) blocked: usize,
    pub(super) focused_tests_failed: u64,
    pub(super) workspace_tests_failed: u64,
}

#[derive(Deserialize)]
pub(super) struct Requirement {
    pub(super) id: String,
    pub(super) status: String,
    pub(super) evidence: Vec<String>,
}

#[derive(Deserialize)]
pub(super) struct Gate {
    pub(super) id: String,
    pub(super) status: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CommandRegistry {
    pub(super) commands: Vec<CommandRecord>,
    pub(super) test_summary: TestSummary,
    pub(super) workspace_gate: WorkspaceGate,
    pub(super) result: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CommandRecord {
    pub(super) id: String,
    pub(super) transcript: String,
    pub(super) exit_code: i32,
    pub(super) kind: Option<String>,
    pub(super) passed: Option<u64>,
    pub(super) failed: Option<u64>,
}

#[derive(Deserialize)]
pub(super) struct TestSummary {
    pub(super) passed: u64,
    pub(super) failed: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WorkspaceGate {
    pub(super) exit_code: i32,
    pub(super) status: String,
    pub(super) failed: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WorkspaceDocument {
    pub(super) workspace_result: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SourceBindingDocument {
    pub(super) files: usize,
    pub(super) final_source_list: String,
    pub(super) pre_sha256: String,
    pub(super) final_sha256: String,
    pub(super) drift: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ExternalOwnerRegistry {
    pub(super) schema: String,
    pub(super) owners: Vec<ExternalOwnerAudit>,
    pub(super) result: String,
}

#[derive(Deserialize)]
pub(super) struct ExternalOwnerAudit {
    pub(super) spec: String,
    pub(super) status: String,
    pub(super) tests: Vec<String>,
    pub(super) passed: u64,
    pub(super) failed: u64,
}

#[derive(Deserialize)]
pub(super) struct CleanupRegistry {
    pub(super) schema: String,
    pub(super) items: Vec<CleanupItem>,
    pub(super) result: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CleanupItem {
    pub(super) id: String,
    pub(super) disposition: String,
    pub(super) absence_proof: String,
}

#[derive(Deserialize)]
pub(super) struct IncidentRegistry {
    pub(super) schema: String,
    pub(super) incidents: Vec<Incident>,
    pub(super) result: String,
}

#[derive(Deserialize)]
pub(super) struct Incident {
    pub(super) id: String,
    pub(super) status: String,
}
