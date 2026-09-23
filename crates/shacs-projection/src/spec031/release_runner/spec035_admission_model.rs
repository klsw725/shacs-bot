use super::spec035_admission_incident::AcceptedIncident;
use super::spec035_admission_source::SourceCorrespondence;
use super::spec035_execution_model::FileRef;
use super::spec035_test_counts::WorkspaceCounts;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Admission {
    pub(super) schema: AdmissionSchema,
    pub(super) run_id: String,
    pub(super) original_head: String,
    pub(super) target_commit: String,
    pub(super) started_at: String,
    pub(super) finished_at: String,
    pub(super) artifacts: Vec<FileRef>,
    pub(super) config_acceptance: FileRef,
}

#[derive(Deserialize)]
pub(super) enum AdmissionSchema {
    #[serde(rename = "spec035.receipt_admission.v1")]
    V1,
}

#[derive(Debug, Serialize)]
pub struct Spec035ReceiptAdmissionReport {
    pub schema: &'static str,
    pub run_id: String,
    pub adapter_sha256: String,
    pub workspace: WorkspaceCounts,
    pub regular_targets: usize,
    pub doc_targets: usize,
    pub artifacts_verified: usize,
    pub source: SourceCorrespondence,
    pub config_incident: AcceptedIncident,
    pub semantic_closure_evaluated: bool,
    pub release_verdict: ReleaseDisposition,
    pub multi_run_composition: CompositionDisposition,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReleaseDisposition {
    Blocked,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositionDisposition {
    NotAdmitted,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WorkspaceReceipt {
    pub(super) args: Vec<String>,
    pub(super) started_at: String,
    pub(super) finished_at: String,
    pub(super) timed_out: bool,
    pub(super) code: i32,
    pub(super) signal: Option<String>,
    pub(super) stdout_digest: String,
    pub(super) stderr_digest: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WorkspaceAudit {
    pub(super) head: String,
    pub(super) source_digest: String,
    pub(super) lockfile_digest: String,
    pub(super) command: Vec<String>,
    pub(super) executions: u64,
    pub(super) code: i32,
    pub(super) timed_out: bool,
    pub(super) started_at: String,
    pub(super) finished_at: String,
    pub(super) top_level_targets: usize,
    pub(super) doc_tests: DocTests,
    pub(super) combined_totals: Totals,
    pub(super) nested_totals_excluded: Totals,
}

#[derive(Deserialize)]
pub(super) struct DocTests {
    pub(super) targets: usize,
}

#[derive(Deserialize)]
pub(super) struct Totals {
    pub(super) passed: u64,
    pub(super) failed: u64,
    pub(super) ignored: u64,
}

impl Totals {
    pub(super) fn matches(&self, actual: &super::spec035_test_counts::HarnessTotals) -> bool {
        self.passed == actual.passed
            && self.failed == actual.failed
            && self.ignored == actual.ignored
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OriginalTarget {
    pub(super) target: String,
    pub(super) stdout_line: usize,
    pub(super) nested: bool,
    pub(super) passed: u64,
    pub(super) failed: u64,
    pub(super) ignored: u64,
}
