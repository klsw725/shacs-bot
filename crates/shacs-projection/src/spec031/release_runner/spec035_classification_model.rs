use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
#[serde(tag = "schema")]
pub(super) enum ClosureDocument {
    #[serde(rename = "spec035.prd000_009_closure_manifest.v1")]
    ExecutionV1(Box<super::spec035_evidence_model::Manifest>),
    #[serde(rename = "spec035.prd000_009_closure_classification.v2")]
    ClassificationV2(Box<Classification>),
    #[serde(rename = "spec035.prd000_009_closure_execution.v1")]
    CurrentExecution(Box<super::spec035_execution_model::Execution>),
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum Status {
    Pass,
    Blocked,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Classification {
    pub(super) verdict: Status,
    pub(super) final_committed_seal: bool,
    pub(super) all_requirements_approved: bool,
    pub(super) summary: Summary,
    pub(super) provenance: Provenance,
    pub(super) authority_root: String,
    pub(super) authorities: HashMap<String, Authority>,
    pub(super) evidence_path_base: String,
    pub(super) evidence_sets: HashMap<String, Vec<String>>,
    pub(super) requirements: Vec<Requirement>,
    pub(super) gates: Vec<super::spec035_evidence_model::Gate>,
    pub(super) registries: Registries,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Summary {
    pub(super) requirements: usize,
    pub(super) implementation_closure_evidence: usize,
    pub(super) prd007_final_conditions: usize,
    pub(super) passed: usize,
    pub(super) blocked: usize,
    pub(super) unmapped: usize,
    pub(super) external_owners: usize,
    pub(super) external_owners_passed: usize,
    pub(super) external_owners_blocked: usize,
    pub(super) commands_executed_for_this_correction: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Provenance {
    pub(super) classification_artifact_inventory: String,
    pub(super) historical_source_binding_status: String,
    pub(super) observations_are_execution_binding: bool,
}

#[derive(Deserialize)]
pub(super) struct Authority {
    pub(super) file: String,
    pub(super) section: String,
    pub(super) count: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Requirement {
    pub(super) id: String,
    pub(super) authority: String,
    pub(super) item: usize,
    pub(super) line: usize,
    pub(super) status: Status,
    pub(super) evidence_sets: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Registries {
    pub(super) external_owners: String,
}

#[derive(Deserialize)]
pub(super) struct Owners {
    pub(super) schema: String,
    pub(super) result: Status,
    pub(super) summary: OwnerSummary,
    pub(super) owners: Vec<Owner>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OwnerSummary {
    pub(super) owners: usize,
    pub(super) passed: usize,
    pub(super) blocked: usize,
    pub(super) historical_tests_passed: u64,
    pub(super) historical_tests_failed: u64,
    pub(super) new_tests_executed: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Owner {
    pub(super) spec: String,
    pub(super) status: Status,
    pub(super) historical_passed: u64,
    pub(super) historical_failed: u64,
    pub(super) facts: Vec<Fact>,
}

#[derive(Deserialize)]
pub(super) struct Fact {
    pub(super) status: Status,
}
