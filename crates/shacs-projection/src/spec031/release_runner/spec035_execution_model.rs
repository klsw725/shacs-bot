use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Execution {
    pub(super) run_id: String,
    pub(super) source: SourceBinding,
    pub(super) inventory: FileRef,
    pub(super) requirements: Vec<RequirementEvidence>,
    pub(super) owners: Vec<OwnerEvidence>,
    pub(super) commands: Vec<CommandEvidence>,
    pub(super) gates: Vec<SubjectEvidence>,
    pub(super) resources: Vec<String>,
    pub(super) cleanup: FileRef,
    pub(super) incidents: Vec<IncidentEvidence>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct FileRef {
    pub(super) path: String,
    pub(super) sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SourceBinding {
    pub(super) before: FileRef,
    pub(super) after: FileRef,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RequirementEvidence {
    pub(super) authority: super::spec035_catalog::Requirement,
    pub(super) receipt: FileRef,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SubjectEvidence {
    pub(super) id: String,
    pub(super) receipt: FileRef,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct IncidentEvidence {
    pub(super) id: String,
    pub(super) receipt: FileRef,
    pub(super) disposition: Option<IncidentDisposition>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum IncidentDisposition {
    UserAcceptedRepairedBaseline,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct IncidentReceipt {
    pub(super) schema: IncidentSchema,
    pub(super) run_id: String,
    pub(super) source_sha256: String,
    pub(super) subject: String,
    pub(super) acceptance: FileRef,
}

#[derive(Deserialize)]
pub(super) enum IncidentSchema {
    #[serde(rename = "spec035.incident_disposition.v1")]
    V1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OwnerEvidence {
    pub(super) owner: String,
    pub(super) facts: Vec<SubjectEvidence>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CommandEvidence {
    pub(super) id: String,
    pub(super) kind: CommandKind,
    pub(super) run_id: String,
    pub(super) source_sha256: String,
    pub(super) argv: Vec<String>,
    pub(super) package: Option<String>,
    pub(super) filter: Option<String>,
    pub(super) tests: Option<super::model::Spec031ReleaseTestCounts>,
    pub(super) test_accounting: Option<FileRef>,
    pub(super) exit_code: i32,
    pub(super) stdout: FileRef,
    pub(super) stderr: FileRef,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum CommandKind {
    FocusedTest,
    WorkspaceTest,
    Format,
    Lint,
    Build,
    Surface,
    Review,
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum Verdict {
    Pass,
    Blocked,
    Failed,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    pub(super) run_id: String,
    pub(super) source_sha256: String,
    pub(super) subject: String,
    pub(super) verdict: Verdict,
    pub(super) checks: Vec<Check>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Check {
    pub(super) id: String,
    pub(super) verdict: Verdict,
    pub(super) producer: String,
    pub(super) commands: Vec<String>,
    pub(super) artifacts: Vec<FileRef>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Cleanup {
    pub(super) run_id: String,
    pub(super) source_sha256: String,
    pub(super) verdict: Verdict,
    pub(super) resources: Vec<Resource>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Resource {
    pub(super) id: String,
    pub(super) disposition: Disposition,
    pub(super) proof: FileRef,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Disposition {
    Removed,
    Stopped,
    Absent,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AbsenceProof {
    pub(super) run_id: String,
    pub(super) source_sha256: String,
    pub(super) resource_id: String,
    pub(super) absent: bool,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct ExecutionBinding {
    pub(super) run_id: String,
    pub(super) manifest_sha256: String,
    pub(super) source_sha256: String,
}
