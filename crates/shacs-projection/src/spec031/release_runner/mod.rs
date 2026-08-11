mod audit;
mod command;
mod command_process;
mod command_validate;
mod coverage;
mod coverage_audit_validate;
mod coverage_ids;
mod coverage_matrix;
mod coverage_matrix_rows;
mod coverage_owner;
mod coverage_provenance;
mod coverage_requirement_rows;
mod coverage_validate;
mod coverage_validate_artifact;
mod current_commands;
mod external_audit_facts;
mod fixture;
mod model;
mod receipts;
mod runner;
mod runner_outputs;
mod validate;
mod writer;

pub(crate) use command::execute_spec031_release_command_with;
pub use command::{execute_spec031_release_command, parse_cargo_test_counts};
pub use coverage::{
    Spec031ArtifactMediaType, Spec031CoverageEvidenceKind, Spec031CoverageRequirementKind,
    Spec031CoverageStatus, Spec031ExternalAuditRow, Spec031ExternalAuditStatus,
    Spec031ExternalOwnerId, Spec031ReleaseCoverageEntry, Spec031TypedEvidenceClass,
};
pub use model::{
    Spec031CommandProcessReceipt, Spec031ReleaseArtifactError, Spec031ReleaseCommandRecord,
    Spec031ReleaseCommandSpec, Spec031ReleaseCommandStatus, Spec031ReleaseGateKind,
    Spec031ReleaseRunArtifacts, Spec031ReleaseRunId, Spec031ReleaseRunnerConfig,
    Spec031ReleaseRunnerMode, Spec031ReleaseTestCounts, SPEC031_RELEASE_RUNNER_SCHEMA,
};
pub use runner::run_spec031_release_runner;
pub use validate::{
    validate_spec031_release_artifacts, validate_spec031_release_artifacts_with_repo_root,
};
pub use writer::write_spec031_release_artifacts;

const REQUIRED_ARTIFACTS: [&str; 5] = [
    "manifest.json",
    "coverage-matrix.json",
    "results.json",
    "failure-triage.json",
    "summary.md",
];
