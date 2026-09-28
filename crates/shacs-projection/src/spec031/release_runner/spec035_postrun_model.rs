use super::spec035_execution_model::{
    CommandEvidence, ExecutionBinding, FileRef, SubjectEvidence, Verdict,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Postrun {
    pub(super) binding: ExecutionBinding,
    pub(super) runner_manifest: FileRef,
    pub(super) originals: Vec<FileRef>,
    pub(super) exit: FileRef,
    pub(super) review_command: CommandEvidence,
    pub(super) read_audit: FileRef,
    pub(super) requirements: Vec<SubjectEvidence>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExitObservation {
    pub(super) binding: ExecutionBinding,
    pub(super) runner_manifest: FileRef,
    pub(super) originals_sha256: String,
    pub(super) argv: Vec<String>,
    pub(super) exit_code: i32,
    pub(super) reaped: bool,
    pub(super) stdout: FileRef,
    pub(super) stderr: FileRef,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Seal {
    pub(super) binding: ExecutionBinding,
    pub(super) postrun: FileRef,
    pub(super) verdict: Verdict,
}

#[derive(Serialize)]
pub struct Spec035PreflightReport {
    pub status: &'static str,
    pub run_id: String,
    pub manifest_sha256: String,
    pub source_sha256: String,
    pub requirements: usize,
    pub passed: usize,
    pub blocked: Vec<String>,
}
